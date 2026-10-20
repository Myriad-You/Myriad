//! OneBot 私聊动作出站编码。只产出请求体，不带 `echo`，无 I/O。
use crate::channel::CHANNEL_IMAGE_LIMIT;
use serde_json::{Value, json};

/// `{"type":"text","data":{"text":…}}`
pub fn encode_text_segment(text: &str) -> Value {
    json!({"type": "text", "data": {"text": text}})
}

/// `{"type":"image","data":{"file":…}}`。`file` 接受 URL 或 `base64://…`，不走 `data.url`。
/// SnowLuma 优先读 `file`。站内图片它取不到，调用方应先读出字节再以 `base64://` 发。
pub fn encode_image_segment(url: &str) -> Value {
    json!({"type": "image", "data": {"file": url}})
}

/// `{"type":"reply","data":{"id":…}}`, quoting a message. SnowLuma ids are
/// signed int32 hashes, often negative; `0` and a non-number quote nothing.
pub fn encode_reply_segment(message_id: &str) -> Option<Value> {
    let id = message_id
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|id| *id != 0)?;
    Some(json!({"type": "reply", "data": {"id": id.to_string()}}))
}

/// `send_private_msg`。`user_id` 按 i64 写入，不经浮点；空段或非法 id 返回 `None`。
pub fn encode_private_message(user_id: &str, segments: &[Value]) -> Option<Value> {
    if segments.is_empty() {
        return None;
    }
    let user_id = user_id.parse::<i64>().ok()?;
    Some(json!({
        "action": "send_private_msg",
        "params": {
            "user_id": user_id,
            "message": segments,
        }
    }))
}

/// `send_group_msg`。`group_id` 按 i64 写入；空段或非法 id 返回 `None`。
pub fn encode_group_message(group_id: &str, segments: &[Value]) -> Option<Value> {
    if segments.is_empty() {
        return None;
    }
    let group_id = group_id.parse::<i64>().ok()?;
    Some(json!({
        "action": "send_group_msg",
        "params": {
            "group_id": group_id,
            "message": segments,
        }
    }))
}

/// `set_input_status`，只发正在输入。
///
/// SnowLuma 把 `event_type` 原样放进 OIDB 0xcd4，只支持好友会话。它缺省按 `0` 处理、
/// 注释称 `0` 为取消，但取消的实际效果未经验证（uncertain），
/// 所以 `typing == false` 仍返回 `None`，只发 `1`。
pub fn encode_typing(user_id: &str, typing: bool) -> Option<Value> {
    if !typing {
        return None;
    }
    let user_id = user_id.parse::<i64>().ok()?;
    Some(json!({
        "action": "set_input_status",
        "params": {
            "user_id": user_id,
            "event_type": 1,
        }
    }))
}

/// 文本在前，图片随后，最多 [`CHANNEL_IMAGE_LIMIT`] 张。全空则 `None`。
///
/// 只发 text 段：SnowLuma 的 markdown 只能作为闪传卡片发，不能当普通消息段。
pub fn plan_private_delivery(user_id: &str, text: &str, images: &[String]) -> Option<Value> {
    let text = text.trim();
    let mut segments = Vec::new();
    if !text.is_empty() {
        segments.push(encode_text_segment(text));
    }
    for image in images.iter().take(CHANNEL_IMAGE_LIMIT) {
        segments.push(encode_image_segment(image));
    }
    encode_private_message(user_id, &segments)
}

/// Route a `send_private_msg` into the temp session (临时会话) opened from
/// `group_id`, for someone who is not her friend. SnowLuma takes it only when
/// that person has opened the session; otherwise QQ refuses the send.
/// `None`, an invalid id, or another action leaves `action` as it was.
pub fn via_temp_session(mut action: Value, group_id: Option<&str>) -> Value {
    let Some(group_id) = group_id
        .and_then(|id| id.trim().parse::<i64>().ok())
        .filter(|id| *id > 0)
    else {
        return action;
    };
    if action["action"] == "send_private_msg"
        && let Some(params) = action["params"].as_object_mut()
    {
        params.insert("group_id".into(), json!(group_id));
    }
    action
}

/// The latest `count` messages of a group, for catching up on what was said
/// while she was not connected. SnowLuma answers `{"messages": [...]}`.
pub fn encode_get_group_msg_history(group_id: &str, count: u32) -> Option<Value> {
    let group_id = group_id.trim().parse::<i64>().ok()?;
    Some(json!({
        "action": "get_group_msg_history",
        "params": {"group_id": group_id, "count": count.clamp(1, 50)}
    }))
}

/// `get_group_member_info`: how someone shows in a group (see
/// [`crate::onebot::decode::decode_member_shown_as`]).
pub fn encode_get_group_member_info(group_id: &str, user_id: i64) -> Option<Value> {
    let group_id = group_id.trim().parse::<i64>().ok()?;
    Some(json!({
        "action": "get_group_member_info",
        "params": {"group_id": group_id, "user_id": user_id}
    }))
}

/// `get_msg`。SnowLuma 群消息里的回复段只有 `id`，被回复的是谁、说了什么要靠它反查。
/// 消息 id 是有符号 int32 哈希，常为负数。
pub fn encode_get_msg(message_id: &str) -> Option<Value> {
    let message_id = message_id.trim().parse::<i64>().ok()?;
    Some(json!({
        "action": "get_msg",
        "params": {"message_id": message_id}
    }))
}

/// `get_record` with a server-side transcode to WAV; SnowLuma answers with
/// `data.base64`. The transcode needs SnowLuma's ffmpeg support (uncertain on
/// any given deployment); a failure is answered `failed`.
pub fn encode_get_record_wav(file: &str) -> Option<Value> {
    let file = file.trim();
    if file.is_empty() {
        return None;
    }
    Some(json!({
        "action": "get_record",
        "params": {"file": file, "out_format": "wav"}
    }))
}

/// `get_private_file_url` (NapCat / SnowLuma extension, not OneBot v11) for a
/// private file whose segment came without a URL. SnowLuma needs the
/// `file_hash` from that segment; an empty one is left out.
pub fn encode_get_private_file_url(user_id: &str, file_id: &str, file_hash: &str) -> Option<Value> {
    let file_id = file_id.trim();
    if file_id.is_empty() {
        return None;
    }
    let mut params = serde_json::Map::new();
    if let Ok(user_id) = user_id.trim().parse::<i64>() {
        params.insert("user_id".into(), json!(user_id));
    }
    params.insert("file_id".into(), json!(file_id));
    if !file_hash.trim().is_empty() {
        params.insert("file_hash".into(), json!(file_hash.trim()));
    }
    Some(json!({"action": "get_private_file_url", "params": params}))
}

/// `fetch_ptt_text`: QQ's own voice-to-text for a received voice message,
/// answered `{text}`.
pub fn encode_fetch_ptt_text(message_id: &str) -> Option<Value> {
    let message_id = message_id.trim().parse::<i64>().ok()?;
    Some(json!({
        "action": "fetch_ptt_text",
        "params": {"message_id": message_id}
    }))
}

/// A reaction on a group message (SnowLuma `set_msg_emoji_like`, group only).
/// An emoji is sent as its decimal code point; SnowLuma treats a code longer
/// than three digits as a Unicode emoji rather than a QQ face id. Whether QQ
/// takes a given emoji is QQ's.
pub fn encode_set_msg_emoji_like(message_id: &str, emoji: &str) -> Option<Value> {
    let message_id = message_id.trim().parse::<i64>().ok()?;
    let mut chars = emoji.trim().trim_end_matches('\u{fe0f}').chars();
    let point = chars.next()?;
    if chars.next().is_some() || point.is_ascii() {
        return None;
    }
    Some(json!({
        "action": "set_msg_emoji_like",
        "params": {"message_id": message_id, "emoji_id": (point as u32).to_string(), "set": true}
    }))
}
