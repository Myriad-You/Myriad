//! OneBot 私聊与群事件 → Myriad 入站。解析失败一律 `None`，不 panic。
use crate::channel::{
    CHANNEL_IMAGE_LIMIT, ChannelImageRef, InboundC2cText, QQ_TEXT_LIMIT, QuotedLine,
    split_channel_text,
};
use crate::onebot::wire::{RawEventJson, WireMessage, WireSegment};

/// 只保留私聊文本消息。群、群临时会话、机器人自己发出的事件都丢掉。
pub fn decode_private_inbound(raw: &str) -> Option<InboundC2cText> {
    let event: RawEventJson = serde_json::from_str(raw).ok()?;
    if event.post_type.as_deref() != Some("message") {
        return None;
    }
    if event.message_type.as_deref() != Some("private") {
        return None;
    }
    match event.sub_type.as_deref() {
        Some("friend" | "other") => {}
        _ => return None,
    }
    let user_id = event.user_id?;
    // CQ strings are not a private inbound. The worker warns once instead of
    // answering every such message with "no text or image".
    let Some(WireMessage::Array(segments)) = event.message.as_ref() else {
        return None;
    };
    Some(InboundC2cText {
        msg_id: event.message_id.map_or(String::new(), |id| id.to_string()),
        user_openid: user_id.to_string(),
        content: truncate_qq_text(&decode_segments_to_text(segments)),
        images: decode_images(segments),
    })
}

/// `Some(true)` when the payload is a private message reported as a CQ string.
pub fn private_message_is_cq_string(raw: &str) -> bool {
    let Ok(event) = serde_json::from_str::<RawEventJson>(raw) else {
        return false;
    };
    event.post_type.as_deref() == Some("message")
        && event.message_type.as_deref() == Some("private")
        && matches!(event.message, Some(WireMessage::Cq(_)))
}

/// 群里的一行。`addressed` 为真表示 @ 了她。回复她的消息不算：那可能是在跟别人说，
/// 由她和别的话一起读、自己判断（`reply_to.hers` 说明被回复的是她）。
///
/// NapCat 的回复段只有 `{id}`，解码时认不出被回复的是不是她。这时 `reply_to` 为空、
/// `reply_id` 带着那条消息的 id，由工人用 `get_msg` 反查后交给 [`decode_replied_message`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneBotGroupLine {
    pub group_id: String,
    pub message_id: String,
    pub user_id: String,
    pub display_name: String,
    pub text: String,
    pub addressed: bool,
    pub reply_to: Option<QuotedLine>,
    pub reply_id: Option<String>,
    /// Pictures in it: images and QQ stickers.
    pub images: Vec<crate::channel::GroupImage>,
}

/// 她在一个群里被禁言或解禁（`notice_type: group_ban`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneBotGroupBan {
    pub group_id: String,
    /// 谁动的手；拿不到就是空串。
    pub operator_id: String,
    pub muted: Muted,
    /// 全员禁言，不只是她。
    pub everyone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Muted {
    /// 禁言这么多秒。
    For(u64),
    /// 没说多久：直到解禁（全员禁言就是这样）。
    UntilLifted,
    Lifted,
}

/// 群禁言通知里和她有关的：禁她、解她，或全员禁言与解除（`user_id` 为 0）。别人被禁言不算。
pub fn decode_group_ban(raw: &str, self_id: i64) -> Option<OneBotGroupBan> {
    let event: RawEventJson = serde_json::from_str(raw).ok()?;
    if event.post_type.as_deref() != Some("notice")
        || event.notice_type.as_deref() != Some("group_ban")
    {
        return None;
    }
    let group_id = event.group_id?;
    let user_id = event.user_id.unwrap_or(0);
    if user_id != self_id && user_id != 0 {
        return None;
    }
    let duration = event.duration.unwrap_or(0);
    let muted = match event.sub_type.as_deref() {
        Some("lift_ban") => Muted::Lifted,
        Some("ban") if duration > 0 => Muted::For(duration as u64),
        Some("ban") => Muted::UntilLifted,
        _ => return None,
    };
    Some(OneBotGroupBan {
        group_id: group_id.to_string(),
        operator_id: event
            .operator_id
            .filter(|id| *id != 0)
            .map(|id| id.to_string())
            .unwrap_or_default(),
        muted,
        everyone: user_id == 0,
    })
}

/// 群消息。频道、`message_sent`、没有文字的行丢掉。`self_id` 用来判断 @ 和回复是不是她。
pub fn decode_group_inbound(raw: &str, self_id: i64) -> Option<OneBotGroupLine> {
    let event: RawEventJson = serde_json::from_str(raw).ok()?;
    if event.post_type.as_deref() != Some("message") {
        return None;
    }
    if event.message_type.as_deref() != Some("group") {
        return None;
    }
    let group_id = event.group_id?;
    let user_id = event.user_id?;
    if user_id == self_id {
        return None;
    }
    let segments = match event.message.as_ref() {
        Some(WireMessage::Array(segments)) => segments.as_slice(),
        _ => return None,
    };
    let (text, addressed) = group_text(segments, self_id);
    let text = truncate_qq_text(&text);
    let images = group_images(segments);
    if text.is_empty() && images.is_empty() {
        return None;
    }
    let reply_to = group_reply(segments, self_id);
    let reply_id = reply_to
        .is_none()
        .then(|| {
            segments
                .iter()
                .find(|segment| segment.kind == "reply")
                .and_then(|segment| segment.str_field("id"))
        })
        .flatten()
        .filter(|id| !id.trim().is_empty());
    let name = event
        .sender
        .as_ref()
        .and_then(|sender| {
            sender
                .card
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or(sender.nickname.as_deref())
        })
        .unwrap_or("")
        .trim();
    Some(OneBotGroupLine {
        group_id: group_id.to_string(),
        message_id: event.message_id.map_or(String::new(), |id| id.to_string()),
        user_id: user_id.to_string(),
        display_name: name.chars().take(40).collect(),
        text,
        addressed,
        reply_to,
        reply_id,
        images,
    })
}

/// A group message from `get_group_msg_history`: when it was said (unix
/// seconds), whether it was hers, and the line.
#[derive(Debug, Clone, PartialEq)]
pub struct PastGroupLine {
    pub at: i64,
    pub hers: bool,
    pub line: OneBotGroupLine,
}

/// The messages of a `get_group_msg_history` response (`{"messages": [...]}`),
/// oldest first. Hers are kept too, marked hers; lines without text or
/// pictures are dropped, as live ones are.
pub fn decode_group_history(data: &serde_json::Value, self_id: i64) -> Vec<PastGroupLine> {
    let messages = data
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .or_else(|| data.as_array());
    let mut past: Vec<PastGroupLine> = messages
        .into_iter()
        .flatten()
        .filter_map(|message| {
            let mut message = message.clone();
            let object = message.as_object_mut()?;
            object.entry("post_type").or_insert("message".into());
            object.entry("message_type").or_insert("group".into());
            let at = object.get("time").and_then(serde_json::Value::as_i64)?;
            let hers = object.get("user_id").and_then(serde_json::Value::as_i64) == Some(self_id);
            let raw = serde_json::to_string(&message).ok()?;
            // Hers are read as anyone's: nothing in them addresses her.
            let line = decode_group_inbound(&raw, if hers { -1 } else { self_id })?;
            Some(PastGroupLine { at, hers, line })
        })
        .collect();
    past.sort_by_key(|line| line.at);
    past
}

/// Images (`image`) and QQ stickers (`mface`) in a group line. An image is
/// known again by its file name, which QQ derives from its content; its URL
/// is QQ's own download link.
fn group_images(segments: &[WireSegment]) -> Vec<crate::channel::GroupImage> {
    use crate::channel::{GROUP_IMAGES, GroupImage, ImageFetch};
    segments
        .iter()
        .filter_map(|segment| {
            let url = segment
                .str_field("url")
                .filter(|url| url.starts_with("https://") || url.starts_with("http://"))?;
            let summary = segment
                .str_field("summary")
                .map(|summary| summary.trim().chars().take(40).collect::<String>())
                .filter(|summary| !summary.is_empty());
            match segment.kind.as_str() {
                "image" => Some(GroupImage {
                    key: format!(
                        "qq:{}",
                        segment.str_field("file").unwrap_or_else(|| url.clone())
                    ),
                    sticker: segment.i64_field("sub_type") == Some(1)
                        || summary
                            .as_deref()
                            .is_some_and(|summary| summary.contains("表情")),
                    hint: summary,
                    fetch: ImageFetch::Url { url },
                }),
                "mface" => Some(GroupImage {
                    key: format!(
                        "qq-mface:{}",
                        segment.str_field("emoji_id").unwrap_or_else(|| url.clone())
                    ),
                    hint: summary,
                    sticker: true,
                    fetch: ImageFetch::Url { url },
                }),
                _ => None,
            }
        })
        .take(GROUP_IMAGES)
        .collect()
}

/// `get_msg` 的 `data` → 被回复的那一行。形状与消息事件相同；`hers` 看发送者是不是 `self_id`。
pub fn decode_replied_message(data: &serde_json::Value, self_id: i64) -> Option<QuotedLine> {
    let message: RawEventJson = serde_json::from_value(data.clone()).ok()?;
    let sender = message
        .user_id
        .or_else(|| message.sender.as_ref().and_then(|sender| sender.user_id))?;
    let text = match message.message.as_ref() {
        Some(WireMessage::Array(segments)) => decode_segments_to_text(segments),
        _ => message.raw_message.clone().unwrap_or_default(),
    };
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: String = text.chars().take(160).collect();
    if text.is_empty() {
        return None;
    }
    let name = message
        .sender
        .as_ref()
        .and_then(|sender| {
            sender
                .card
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or(sender.nickname.as_deref())
        })
        .unwrap_or("")
        .trim()
        .chars()
        .take(40)
        .collect();
    Some(QuotedLine {
        name,
        text,
        hers: sender == self_id,
    })
}

fn group_text(segments: &[WireSegment], self_id: i64) -> (String, bool) {
    let mut text = String::new();
    let mut addressed = false;
    for segment in segments {
        match segment.kind.as_str() {
            "text" => {
                if let Some(piece) = segment.str_field("text") {
                    text.push_str(&piece);
                }
            }
            "at" if segment.i64_field("qq") == Some(self_id) => {
                addressed = true;
            }
            // Someone else @-ed: kept, so who is talking to whom still reads.
            "at" => {
                let qq = segment.str_field("qq").unwrap_or_default();
                let name = segment
                    .str_field("name")
                    .map(|name| name.trim().trim_start_matches('@').to_string())
                    .filter(|name| !name.is_empty());
                let shown = match (qq.as_str(), name) {
                    ("all", _) => "全体成员".to_string(),
                    (_, Some(name)) => name.chars().take(40).collect(),
                    (qq, None) if !qq.is_empty() => qq.to_string(),
                    _ => continue,
                };
                text.push_str(&format!(" @{shown} "));
            }
            _ => {}
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (text, addressed)
}

fn group_reply(segments: &[WireSegment], self_id: i64) -> Option<QuotedLine> {
    let reply = segments.iter().find(|segment| segment.kind == "reply")?;
    let text = reply
        .str_field("text")
        .or_else(|| reply.str_field("message"))
        .unwrap_or_default();
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: String = text.chars().take(160).collect();
    if text.is_empty() {
        return None;
    }
    let hers = reply.i64_field("user_id") == Some(self_id);
    let name = reply.str_field("nickname").unwrap_or_default();
    let name: String = name.chars().take(40).collect();
    Some(QuotedLine { name, text, hers })
}

/// 把段数组里所有文本段的 `data.text` 按出现顺序拼起来。
pub fn decode_segments_to_text(segments: &[WireSegment]) -> String {
    let mut text = String::new();
    for segment in segments {
        if segment.kind != "text" {
            continue;
        }
        if let Some(piece) = segment.str_field("text") {
            text.push_str(&piece);
        }
    }
    text
}

fn decode_images(segments: &[WireSegment]) -> Vec<ChannelImageRef> {
    let mut images = Vec::new();
    for segment in segments {
        if images.len() >= CHANNEL_IMAGE_LIMIT {
            break;
        }
        if segment.kind != "image" {
            continue;
        }
        let Some(url) = http_url(segment) else {
            continue;
        };
        let name = segment
            .str_field("file")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "photo.jpg".to_string());
        let size = segment
            .i64_field("file_size")
            .filter(|value| *value >= 0)
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(0);
        let mime = image_mime(&name).to_string();
        images.push(ChannelImageRef {
            url,
            name,
            mime,
            size,
        });
    }
    images
}

/// QQ 图片多为 jpg；`file` 带扩展名时按它来，认不出就按 jpg。
fn image_mime(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

fn http_url(segment: &WireSegment) -> Option<String> {
    let url = segment.str_field("url")?;
    let url = url.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url.to_string())
    } else {
        None
    }
}

/// 截到 [`QQ_TEXT_LIMIT`] 个 Unicode 标量。`split_channel_text` 会丢掉尾部空白，
/// 纯空白因此变空串——入站文本没有保留它的必要。
fn truncate_qq_text(text: &str) -> String {
    split_channel_text(text, QQ_TEXT_LIMIT)
        .into_iter()
        .next()
        .unwrap_or_default()
}
