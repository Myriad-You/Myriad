//! OneBot 私聊动作出站编码。只产出请求体，不带 `echo`，无 I/O。
use crate::channel::CHANNEL_IMAGE_LIMIT;
use serde_json::{Value, json};

/// `{"type":"text","data":{"text":…}}`
pub fn encode_text_segment(text: &str) -> Value {
    json!({"type": "text", "data": {"text": text}})
}

/// `{"type":"image","data":{"file":…}}`。`file` 接受 URL，不走 `data.url`。
pub fn encode_image_segment(url: &str) -> Value {
    json!({"type": "image", "data": {"file": url}})
}

/// `{"type":"markdown","data":{"content":…}}`
pub fn encode_markdown_segment(content: &str) -> Value {
    json!({"type": "markdown", "data": {"content": content}})
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

/// `set_input_status`。`event_type`：`1` 正在输入，`0` 取消。
/// 这两个取值来自 NapCat 扩展文档的转述，仓库内没有对应源码，标 `uncertain`。
pub fn encode_typing(user_id: &str, typing: bool) -> Option<Value> {
    let user_id = user_id.parse::<i64>().ok()?;
    Some(json!({
        "action": "set_input_status",
        "params": {
            "user_id": user_id,
            "event_type": if typing { 1 } else { 0 },
        }
    }))
}

/// 文本（或 markdown）在前，图片随后，最多 [`CHANNEL_IMAGE_LIMIT`] 张。全空则 `None`。
pub fn plan_private_delivery(
    user_id: &str,
    text: &str,
    image_urls: &[String],
    use_markdown: bool,
) -> Option<Value> {
    let text = text.trim();
    let mut segments = Vec::new();
    if !text.is_empty() {
        segments.push(if use_markdown {
            encode_markdown_segment(text)
        } else {
            encode_text_segment(text)
        });
    }
    for url in image_urls.iter().take(CHANNEL_IMAGE_LIMIT) {
        segments.push(encode_image_segment(url));
    }
    encode_private_message(user_id, &segments)
}
