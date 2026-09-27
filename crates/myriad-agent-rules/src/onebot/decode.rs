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
    let segments = match event.message.as_ref() {
        Some(WireMessage::Array(segments)) => segments.as_slice(),
        _ => &[],
    };
    Some(InboundC2cText {
        msg_id: event.message_id.map_or(String::new(), |id| id.to_string()),
        user_openid: user_id.to_string(),
        content: truncate_qq_text(&decode_segments_to_text(segments)),
        images: decode_images(segments),
    })
}

/// 群里的一行。`addressed` 为真表示 @ 了她，或回复了她的消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneBotGroupLine {
    pub group_id: String,
    pub message_id: String,
    pub user_id: String,
    pub display_name: String,
    pub text: String,
    pub addressed: bool,
    pub reply_to: Option<QuotedLine>,
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
    if text.is_empty() {
        return None;
    }
    let reply_to = group_reply(segments, self_id);
    let addressed = addressed || reply_to.as_ref().is_some_and(|line| line.hers);
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
            .unwrap_or_else(|| "photo.png".to_string());
        let size = segment
            .i64_field("file_size")
            .filter(|value| *value >= 0)
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(0);
        images.push(ChannelImageRef {
            url,
            name,
            mime: "image/png".to_string(),
            size,
        });
    }
    images
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
