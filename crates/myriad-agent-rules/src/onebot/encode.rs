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

/// 从办事终态重建 Markdown。表格、图表、卡片不再走冒号降级。
///
/// 没有可渲染结构时返回 `None`，调用方改用已经降级的纯文本。
pub fn render_result_markdown(
    message: &str,
    data: Option<&Value>,
    data_display: Option<&Value>,
) -> Option<String> {
    let display = data_display?;
    let kind = display.get("type").and_then(Value::as_str).unwrap_or("");
    let body = match kind {
        "table" => render_markdown_table(display, data),
        "chart" => render_markdown_chart(display, data),
        "card_list" => render_markdown_cards(display, data),
        "markdown" => data
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        _ => None,
    }?;
    let message = message.trim();
    if message.is_empty() {
        Some(body)
    } else {
        Some(format!("{message}\n\n{body}"))
    }
}

fn render_markdown_table(display: &Value, data: Option<&Value>) -> Option<String> {
    let columns = display.get("columns")?.as_array()?;
    if columns.is_empty() {
        return None;
    }
    let headers: Vec<&str> = columns
        .iter()
        .map(|column| {
            column
                .get("title")
                .or_else(|| column.get("field"))
                .and_then(Value::as_str)
                .unwrap_or("列")
        })
        .collect();
    let rows = display_rows(data, display);
    if rows.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    lines.push(format!(
        "| {} |",
        headers
            .iter()
            .map(|cell| escape_cell(cell))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    lines.push(format!("|{}|", vec![" --- "; headers.len()].join("|")));
    for row in rows.iter().take(16) {
        let cells: Vec<String> = columns
            .iter()
            .map(|column| {
                let field = column.get("field").and_then(Value::as_str).unwrap_or("");
                escape_cell(&cell_text(row.get(field)))
            })
            .collect();
        lines.push(format!("| {} |", cells.join(" | ")));
    }
    Some(lines.join("\n"))
}

fn render_markdown_chart(display: &Value, data: Option<&Value>) -> Option<String> {
    let rows = display_rows(data, display);
    if rows.is_empty() {
        return None;
    }
    let chart_type = display
        .get("chartType")
        .or_else(|| display.get("chart_type"))
        .and_then(Value::as_str)
        .unwrap_or("chart");
    let x_field = display
        .get("xField")
        .or_else(|| display.get("x_field"))
        .and_then(Value::as_str)
        .unwrap_or("x");
    let y_field = display
        .get("yField")
        .or_else(|| display.get("y_field"))
        .and_then(Value::as_str)
        .unwrap_or("y");
    let mut lines = vec![
        format!("图表（{chart_type}）"),
        String::new(),
        format!("| {} | {} |", escape_cell(x_field), escape_cell(y_field)),
        "| --- | --- |".to_string(),
    ];
    for row in rows.iter().take(12) {
        lines.push(format!(
            "| {} | {} |",
            escape_cell(&cell_text(row.get(x_field))),
            escape_cell(&cell_text(row.get(y_field)))
        ));
    }
    Some(lines.join("\n"))
}

fn render_markdown_cards(display: &Value, data: Option<&Value>) -> Option<String> {
    let rows = display_rows(data, display);
    if rows.is_empty() {
        return None;
    }
    let title_field = display
        .get("titleField")
        .or_else(|| display.get("title_field"))
        .and_then(Value::as_str)
        .unwrap_or("title");
    let description_field = display
        .get("descriptionField")
        .or_else(|| display.get("description_field"))
        .and_then(Value::as_str);
    let mut lines = Vec::new();
    for (index, row) in rows.iter().take(12).enumerate() {
        let title = cell_text(row.get(title_field));
        if let Some(field) = description_field {
            let desc = cell_text(row.get(field));
            if desc.is_empty() {
                lines.push(format!("{}. {title}", index + 1));
            } else {
                lines.push(format!("{}. {title} — {desc}", index + 1));
            }
        } else {
            lines.push(format!("{}. {title}", index + 1));
        }
    }
    Some(lines.join("\n"))
}

fn display_rows<'a>(data: Option<&'a Value>, display: &Value) -> Vec<&'a Value> {
    let Some(data) = data else {
        return Vec::new();
    };
    if let Some(path) = display.get("dataPath").and_then(Value::as_str)
        && let Some(found) = json_path(data, path).and_then(Value::as_array)
    {
        return found.iter().collect();
    }
    match data {
        Value::Array(rows) => rows.iter().collect(),
        Value::Object(map) => {
            for key in ["items", "rows", "data", "list", "records"] {
                if let Some(Value::Array(rows)) = map.get(key) {
                    return rows.iter().collect();
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.').filter(|part| !part.is_empty()) {
        current = current.get(part)?;
    }
    Some(current)
}

fn cell_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "—".to_string(),
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Bool(flag)) => {
            if *flag {
                "是".to_string()
            } else {
                "否".to_string()
            }
        }
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Array(rows)) => format!("{} 项", rows.len()),
        Some(Value::Object(_)) => "…".to_string(),
    }
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
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
