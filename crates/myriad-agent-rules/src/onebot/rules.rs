//! OneBot 私聊通道的纯规则：开关、能力位、打字状态、握手失败归类。无 I/O。

use crate::channel::{ChannelCapabilities, ConnectFailureKind, WorkerIntent};

/// 开关打开、`ws_url` 去空白后非空、且已配置 token → 运行；否则停止。
pub fn onebot_worker_intent(enabled: bool, ws_url: &str, has_token: bool) -> WorkerIntent {
    if enabled && !ws_url.trim().is_empty() && has_token {
        WorkerIntent::Run
    } else {
        WorkerIntent::Stop
    }
}

/// OneBot 私聊：文本与媒体入站，最终文本、markdown 与图片出站。
/// 没有按钮段，澄清靠编号文本；只有撤回，没有编辑。
/// 打字状态走 `set_input_status`，不占能力位，见 [`onebot_worker_supports_typing`]。
pub fn onebot_private_capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        inbound_text: true,
        inbound_media: true,
        inbound_callback: false,
        outbound_final_text: true,
        outbound_markdown: true,
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
