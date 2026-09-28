//! OneBot 私聊通道的纯规则：开关、能力位、打字状态、握手失败归类。无 I/O。

use serde_json::Value;

use crate::channel::{ChannelCapabilities, ConnectFailureKind, WorkerIntent};

/// 开关打开、`ws_url` 去空白后非空、且已配置 token → 运行；否则停止。
pub fn onebot_worker_intent(enabled: bool, ws_url: &str, has_token: bool) -> WorkerIntent {
    if enabled && !ws_url.trim().is_empty() && has_token {
        WorkerIntent::Run
    } else {
        WorkerIntent::Stop
    }
}

/// OneBot 私聊：文本与媒体入站，最终文本与图片出站。
/// 没有按钮段，澄清靠编号文本；只有撤回，没有编辑。
/// 打字状态走 `set_input_status`，不占能力位，见 [`onebot_worker_supports_typing`]。
pub fn onebot_private_capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        inbound_text: true,
        inbound_media: true,
        inbound_callback: false,
        outbound_final_text: true,
        // Markdown segments cannot be sent directly on NapCat. Structured
        // results are rendered as readable text and sent as a text segment.
        outbound_markdown: false,
        outbound_image: true,
        outbound_edit: false,
        outbound_streaming_draft: false,
        interactive: true,
        frontend_action: false,
        performance: false,
        outfit: false,
    }
}

/// OneBot 有 `set_input_status`。既有通道的打字状态不在能力位里，这里是单一事实来源。
pub fn onebot_worker_supports_typing() -> bool {
    true
}

/// NapCat 鉴权不在握手阶段拒绝：先接受连接，再发 `retcode` 1403/1401，然后无关闭码断开。
/// OneBot 标准把 `1400`/`1401`/`1403`/`1404` 对应 HTTP 400/401/403/404。
/// 整段 1400..=1404 都是「配置或请求本身不对」，重试无用；其余 14xx 才是可重试的服务侧问题。
pub fn classify_onebot_handshake(status: Option<u16>, retcode: Option<i64>) -> ConnectFailureKind {
    if matches!(retcode, Some(1400..=1404)) {
        return ConnectFailureKind::Permanent;
    }
    match status {
        Some(400..=499) => ConnectFailureKind::Permanent,
        Some(500..=599) => ConnectFailureKind::Transient,
        // 其余 14xx，以及 status/retcode 都缺失：连接可重试。
        _ => ConnectFailureKind::Transient,
    }
}

/// 一帧 `failed` 响应没有可认领的 `echo`（缺失或 `null`），且 `retcode` 属于配置错误：连接被拒。
///
/// NapCat 鉴权失败时发 `OB11Response.res(null, 'failed', 1403, 'token验证失败')` 再断开，
/// 这个响应的 `echo` 是 `null` 而不是缺失，所以它按动作响应解出来，不会落到事件分支。
/// 本端发出的动作都带字符串 `echo`，被拒的单个动作由 echo 认领，不在这里。
pub fn onebot_frame_refuses_connection(echo: &Value, status: &str, retcode: i64) -> bool {
    !echo.is_string()
        && status == "failed"
        && classify_onebot_handshake(None, Some(retcode)) == ConnectFailureKind::Permanent
}

/// 群号白名单的分隔符：逗号、顿号、分号（全角半角都认）和空白。
fn group_allowlist_items(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(|c: char| matches!(c, ',' | '，' | '、' | ';' | '；') || c.is_whitespace())
        .filter(|item| !item.is_empty())
}

/// 把设置页填的群号名单规范化成升序、去重、逗号分隔。空名单是空串。
/// 有一项不是群号就整体拒绝，免得写错一个号却以为它在名单里。
pub fn normalize_onebot_group_allowlist(raw: &str) -> Result<String, String> {
    let mut ids = Vec::new();
    for item in group_allowlist_items(raw) {
        match item.parse::<u64>() {
            Ok(id) if id > 0 => ids.push(id),
            _ => return Err(format!("「{item}」不是群号")),
        }
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids.iter().map(u64::to_string).collect::<Vec<_>>().join(","))
}

/// 这个群的消息能不能进来。名单为空：不限群。名单解不开：一个群都不进——
/// 宁可漏听，也不听到没被允许的群。
pub fn onebot_group_allowed(allowlist: &str, group_id: &str) -> bool {
    let Ok(allowlist) = normalize_onebot_group_allowlist(allowlist) else {
        return false;
    };
    allowlist.is_empty() || allowlist.split(',').any(|id| id == group_id.trim())
}
