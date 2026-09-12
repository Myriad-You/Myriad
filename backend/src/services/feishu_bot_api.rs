//! Feishu OpenAPI send / upload / download. Token stays on the outbound path.

use std::time::Duration;

use myriad_agent_rules::channel::{
    feishu_photo_messages, feishu_token_needs_refresh, parse_feishu_api_code, truncate_feishu_text,
    ConnectFailureKind,
};
use myriad_error::redact_secrets;
use serde_json::Value;
use tracing::warn;

use crate::services::http_client;
use crate::GLOBAL_DYNAMIC_CONFIG;

const API_BASE: &str = "https://open.feishu.cn/open-apis";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn send_message(chat_id: &str, text: &str) -> Result<(), ConnectFailureKind> {
    send_outbound(chat_id, text, None).await
}

pub async fn send_outbound(
    chat_id: &str,
    text: &str,
    reply_markup: Option<Value>,
) -> Result<(), ConnectFailureKind> {
    if chat_id.is_empty() || (text.is_empty() && reply_markup.is_none()) {
        return Ok(());
    }
    if !bot_enabled().await {
        return Ok(());
    }
    if let Some(markup) = reply_markup {
        let mut content = markup;
        if !text.trim().is_empty() {
            let mut elements = vec![serde_json::json!({
                "tag": "div",
                "text": { "tag": "plain_text", "content": truncate_feishu_text(text) },
            })];
            if let Some(existing) = content.get("elements").and_then(|v| v.as_array()) {
                elements.extend(existing.iter().cloned());
            }
            content["elements"] = Value::Array(elements);
        }
        return with_auth(|auth| {
            let chat = chat_id.to_string();
            let body = content.to_string();
            async move { post_message(&auth, &chat, "interactive", &body).await }
        })
        .await;
    }
    let content = serde_json::json!({ "text": truncate_feishu_text(text) }).to_string();
    with_auth(|auth| {
        let chat = chat_id.to_string();
        let body = content.clone();
        async move { post_message(&auth, &chat, "text", &body).await }
    })
    .await
}

pub async fn send_photo(
    chat_id: &str,
    bytes: &[u8],
    mime: &str,
    reply_markup: Option<Value>,
) -> Result<(), ConnectFailureKind> {
    if chat_id.is_empty() || bytes.is_empty() {
        return Ok(());
    }
    if !bot_enabled().await {
        return Ok(());
    }
    let image_key = with_auth(|auth| {
        let data = bytes.to_vec();
        let mime = mime.to_string();
        async move { upload_image(&auth, &data, &mime).await }
    })
    .await?;
    for (msg_type, content) in feishu_photo_messages(&image_key, reply_markup) {
        let body = content.to_string();
        with_auth(|auth| {
            let chat = chat_id.to_string();
            let kind = msg_type.clone();
            let body = body.clone();
            async move { post_message(&auth, &chat, &kind, &body).await }
        })
        .await?;
    }
    Ok(())
}

pub async fn download_image_bytes(
    message_id: &str,
    image_key: &str,
) -> Result<(Vec<u8>, String), String> {
    if message_id.is_empty() || image_key.is_empty() {
        return Err("empty image_key".into());
    }
    with_auth(|auth| {
        let key = image_key.to_string();
        let message_id = message_id.to_string();
        async move { download_image_raw(&auth, &message_id, &key).await }
    })
    .await
    .map_err(|err| format!("{err:?}"))
}

async fn with_auth<T, F, Fut>(f: F) -> Result<T, ConnectFailureKind>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, ConnectFailureKind>>,
{
    let auth = crate::services::feishu_bot::cached_auth_header().await?;
    match f(auth).await {
        Ok(value) => Ok(value),
        Err(ConnectFailureKind::Transient) => {
            let auth = crate::services::feishu_bot::refresh_auth_header().await?;
            f(auth).await
        }
        Err(kind) => Err(kind),
    }
}

async fn download_image_raw(
    auth_header: &str,
    message_id: &str,
    image_key: &str,
) -> Result<(Vec<u8>, String), ConnectFailureKind> {
    let client = http_client::get_global_client().await;
    let url = message_resource_url(message_id, image_key)?;
    let resp = client
        .get(url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|err| {
            warn!(
                error = %redact_secrets(&err.to_string()),
                "Feishu image download request failed"
            );
            ConnectFailureKind::Transient
        })?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        parse_feishu_openapi(status.as_u16(), &text)?;
        return Err(ConnectFailureKind::Transient);
    }
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = resp.bytes().await.map_err(|err| {
        warn!(
            error = %redact_secrets(&err.to_string()),
            "Feishu image download body failed"
        );
        ConnectFailureKind::Transient
    })?;
    Ok((bytes.to_vec(), mime))
}

async fn upload_image(
    auth_header: &str,
    bytes: &[u8],
    mime: &str,
) -> Result<String, ConnectFailureKind> {
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}/im/v1/images");
    let mime_type = if mime.starts_with("image/") {
        mime
    } else {
        "image/jpeg"
    };
    let part = reqwest::multipart::Part::bytes(bytes.to_vec())
        .file_name("image.jpg")
        .mime_str(mime_type)
        .map_err(|_| ConnectFailureKind::Transient)?;
    let form = reqwest::multipart::Form::new()
        .part("image", part)
        .text("image_type", "message");
    let resp = client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", auth_header)
        .multipart(form)
        .send()
        .await
        .map_err(|err| {
            warn!(
                error = %redact_secrets(&err.to_string()),
                "Feishu image upload request failed"
            );
            ConnectFailureKind::Transient
        })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let data = parse_feishu_openapi(status, &text)?;
    data.get("data")
        .and_then(|v| v.get("image_key"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or(ConnectFailureKind::Transient)
}

async fn post_message(
    auth_header: &str,
    chat_id: &str,
    msg_type: &str,
    content: &str,
) -> Result<(), ConnectFailureKind> {
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}/im/v1/messages?receive_id_type=chat_id");
    let body = serde_json::json!({
        "receive_id": chat_id,
        "msg_type": msg_type,
        "content": content,
    });
    let resp = client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", auth_header)
        .json(&body)
        .send()
        .await
        .map_err(|err| {
            warn!(
                error = %redact_secrets(&err.to_string()),
                "Feishu send message request failed"
            );
            ConnectFailureKind::Transient
        })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    parse_feishu_openapi(status, &text).map(|_| ())
}

fn parse_feishu_openapi(status: u16, body: &str) -> Result<Value, ConnectFailureKind> {
    match parse_feishu_api_code(status, body) {
        Ok(value) => Ok(value),
        Err(kind) => {
            if let Ok(data) = serde_json::from_str::<Value>(body) {
                if let Some(code) = data.get("code").and_then(|v| v.as_i64()) {
                    if feishu_token_needs_refresh(code) {
                        warn!(code, "Feishu OpenAPI token expired");
                        return Err(ConnectFailureKind::Transient);
                    }
                }
            }
            warn!(
                status,
                body = %redact_secrets(body),
                "Feishu OpenAPI call failed"
            );
            Err(kind)
        }
    }
}

async fn bot_enabled() -> bool {
    GLOBAL_DYNAMIC_CONFIG.read().await.feishu_bot_enabled
}

fn message_resource_url(
    message_id: &str,
    image_key: &str,
) -> Result<reqwest::Url, ConnectFailureKind> {
    let mut url = reqwest::Url::parse(&format!("{API_BASE}/im/v1/messages/"))
        .map_err(|_| ConnectFailureKind::Permanent)?;
    url.path_segments_mut()
        .map_err(|_| ConnectFailureKind::Permanent)?
        .pop_if_empty()
        .push(message_id)
        .push("resources")
        .push(image_key);
    url.query_pairs_mut().append_pair("type", "image");
    Ok(url)
}

#[cfg(test)]
mod resource_tests {
    #[test]
    fn downloads_message_resources_and_escapes_ids() {
        let url = super::message_resource_url("om_123", "img/a?b").unwrap();
        assert_eq!(url.as_str(), "https://open.feishu.cn/open-apis/im/v1/messages/om_123/resources/img%2Fa%3Fb?type=image");
    }
}
