//! OneBot 私聊动作出站编码。只产出请求体，不带 `echo`，无 I/O。
use crate::channel::CHANNEL_IMAGE_LIMIT;
use serde_json::{Value, json};

/// `{"type":"text","data":{"text":…}}`
pub fn encode_text_segment(text: &str) -> Value {
    json!({"type": "text", "data": {"text": text}})
}

/// `{"type":"image","data":{"file":…}}`。`file` 接受 URL 或 `base64://…`，不走 `data.url`。
/// 站内图片 NapCat 取不到，调用方应先读出字节再以 `base64://` 发。
pub fn encode_image_segment(url: &str) -> Value {
    json!({"type": "image", "data": {"file": url}})
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
/// NapCat `SetInputStatus.ts` 的 `payloadExample` 把 `event_type` 写成 `1`，
/// `napcat-core` 再原样传给 `sendShowInputStatusReq`。源码没有取消取值，
/// `typing == false` 返回 `None`，不发明一个 `0`。
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
/// 只发 text 段：NapCat 的 markdown 段只能套在双层合并转发里，普通 QQ 号直接发不出去。
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

/// `get_msg`。NapCat 群消息里的回复段只有 `id`，被回复的是谁、说了什么要靠它反查。
/// The latest `count` messages of a group, for catching up on what was said
/// while she was not connected.
pub fn encode_get_group_msg_history(group_id: &str, count: u32) -> Option<Value> {
    let group_id = group_id.trim().parse::<i64>().ok()?;
    Some(json!({
        "action": "get_group_msg_history",
        "params": {"group_id": group_id, "count": count.clamp(1, 50)}
    }))
}

pub fn encode_get_msg(message_id: &str) -> Option<Value> {
    let message_id = message_id.trim().parse::<i64>().ok()?;
    Some(json!({
        "action": "get_msg",
        "params": {"message_id": message_id}
    }))
}
