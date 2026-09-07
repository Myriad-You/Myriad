//! Permission catalog shared by install validation, CLI export, and grant filtering.
//!
//! This module is the static directory: names, levels, replacement hints, and
//! which capabilities require an authenticated subject. Grant/delegation
//! (`TappPermissionService::check`) stays in the backend.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const UNKNOWN_TAPP_PERMISSION_CODE: &str = "UNKNOWN_TAPP_PERMISSION";

/// Retired names that still emit a replacement hint and never decode.
pub const RETIRED_TAPP_PERMISSIONS: &[&str] = &["storage", "federation:write", "brew:comment"];

/// Catalog name → level string, in sorted name order.
pub fn permission_levels() -> BTreeMap<&'static str, &'static str> {
    TappPermission::ALL
        .iter()
        .map(|permission| (permission.as_str(), permission.level().as_str()))
        .collect()
}

/// Retired name → replacement hint. Fail-closed: unknown names stay out.
pub fn replacement_hints() -> BTreeMap<&'static str, &'static str> {
    RETIRED_TAPP_PERMISSIONS
        .iter()
        .map(|name| {
            (
                *name,
                tapp_permission_replacement_hint(name)
                    .expect("retired permission names must have a replacement hint"),
            )
        })
        .collect()
}

/// Catalog names whose HTTP boundary requires a durable logged-in user.
pub fn requires_authenticated_subject_names() -> Vec<&'static str> {
    TappPermission::ALL
        .iter()
        .filter(|permission| permission.requires_authenticated_subject())
        .map(|permission| permission.as_str())
        .collect()
}

/// 已移除权限名的替代建议（仅用于错误提示，不构成兼容映射；
/// 未知名仍 fail-closed，绝不解码成新权限）。
/// 单一来源：permission-service、manifest 校验、声明式 API 与运行时签发共用。
pub fn tapp_permission_replacement_hint(permission: &str) -> Option<&'static str> {
    match permission {
        "storage" => Some(
            "use 'storage:read' or 'storage:write' instead; update the TAPP Manifest, then update or reinstall the app",
        ),
        "federation:write" => Some(
            "use 'federation:post', 'federation:interact', 'federation:channel', 'federation:room', or 'federation:ring' instead; update the TAPP Manifest, then update or reinstall the app",
        ),
        "brew:comment" => Some(
            "use 'brew:read' (read comments) or 'brew:commentWrite' (write comments) instead",
        ),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTappPermission {
    pub permission: String,
}

impl UnknownTappPermission {
    pub fn code(&self) -> &'static str {
        UNKNOWN_TAPP_PERMISSION_CODE
    }

    pub fn message(&self) -> String {
        match tapp_permission_replacement_hint(&self.permission) {
            Some(hint) => format!("Unknown Tapp permission '{}'; {}", self.permission, hint),
            None => format!("Unknown Tapp permission '{}'", self.permission),
        }
    }
}

impl std::fmt::Display for UnknownTappPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for UnknownTappPermission {}

/// 用户角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    User,
    Guest,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::Admin => "admin",
            UserRole::User => "user",
            UserRole::Guest => "guest",
        }
    }
}

impl From<&str> for UserRole {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "admin" => UserRole::Admin,
            "user" => UserRole::User,
            _ => UserRole::Guest,
        }
    }
}

/// Tapp 权限（与前端 TappPermission 类型对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TappPermission {
    // Basic 级别
    #[serde(rename = "platform:read")]
    PlatformRead,
    /// First-party site analytics aggregates (no visitor hashes).
    #[serde(rename = "analytics:read")]
    AnalyticsRead,
    #[serde(rename = "tappList:read")]
    TappListRead,
    #[serde(rename = "brew:read")]
    BrewRead,
    /// 修改当前用户自己的阅读状态与收藏（Basic，需登录主体）。
    #[serde(rename = "brew:write")]
    BrewWrite,
    #[serde(rename = "report:read")]
    ReportRead,
    #[serde(rename = "storage:read")]
    StorageRead,
    #[serde(rename = "ui:notification")]
    UiNotification,
    #[serde(rename = "ui:fullscreen")]
    UiFullscreen,
    #[serde(rename = "ui:theme")]
    UiTheme,
    #[serde(rename = "ui:confirm")]
    UiConfirm,
    /// Open a host browser tab for a manifest-declared allowlisted link only.
    #[serde(rename = "ui:openUrl")]
    UiOpenUrl,
    #[serde(rename = "media:read")]
    MediaRead,
    /// Play package audio via blob/data URLs inside the sandbox.
    #[serde(rename = "media:audio")]
    MediaAudio,
    /// Control media playback (play/pause/skip). Basic — always available.
    #[serde(rename = "media:control")]
    MediaControl,
    #[serde(rename = "event:subscribe")]
    EventSubscribe,
    #[serde(rename = "federation:read")]
    FederationRead,
    #[serde(rename = "federation:message")]
    FederationMessage,
    #[serde(rename = "federation:files")]
    FederationFiles,
    #[serde(rename = "game:session")]
    GameSession,

    // Basic 级别（拆分自 federation:write，ADR 0013 / 0020）
    /// follow/unfollow, like/unlike, bookmark/unbookmark, announce/unannounce.
    /// Basic 但要求持久登录主体（游客无身份可绑定互动）。
    #[serde(rename = "federation:interact")]
    FederationInteract,
    /// Ring membership and peer/sync operations. Basic 但同样要求持久登录主体。
    #[serde(rename = "federation:ring")]
    FederationRing,

    // Elevated 级别（拆分自 federation:write，ADR 0013 / 0020）
    /// Publish/unpublish, create notes, uploads and other actions that create
    /// externally visible posts; includes signing-key rotation and outbound
    /// delivery-queue management (both change what peers receive).
    #[serde(rename = "federation:post")]
    FederationPost,
    /// Create, accept, close, delete, key setup and governance for Channels.
    #[serde(rename = "federation:channel")]
    FederationChannel,
    /// Create/update/delete/join/invite/governance/key/sticker/pin operations
    /// for Rooms.
    #[serde(rename = "federation:room")]
    FederationRoom,

    // Elevated 级别
    #[serde(rename = "ai:generate")]
    AiGenerate,
    #[serde(rename = "ai:analyze")]
    AiAnalyze,
    #[serde(rename = "ai:chat")]
    AiChat,
    #[serde(rename = "ai:image")]
    AiImage,
    #[serde(rename = "ai:search")]
    AiSearch,
    #[serde(rename = "3d:generate")]
    ThreeDGenerate,
    #[serde(rename = "report:write")]
    ReportWrite,
    #[serde(rename = "network:fetch")]
    NetworkFetch,
    #[serde(rename = "component:theme")]
    ComponentTheme,
    #[serde(rename = "shortcut:register")]
    ShortcutRegister,
    #[serde(rename = "event:publish")]
    EventPublish,
    #[serde(rename = "scheduler:register")]
    SchedulerRegister,
    #[serde(rename = "speech:tts")]
    SpeechTts,
    #[serde(rename = "speech:asr")]
    SpeechAsr,
    #[serde(rename = "storage:write")]
    StorageWrite,
    /// 创建/更新/删除 Brew 评论与回复（Elevated，需登录主体）。
    #[serde(rename = "brew:commentWrite")]
    BrewCommentWrite,

    // Privileged 级别
    #[serde(rename = "widget:register")]
    WidgetRegister,
    #[serde(rename = "platform:write")]
    PlatformWrite,
    #[serde(rename = "platform:register")]
    PlatformRegister,
    #[serde(rename = "component:agent")]
    ComponentAgent,
    #[serde(rename = "tappList:manage")]
    TappListManage,
    #[serde(rename = "brew:manage")]
    BrewManage,
    #[serde(rename = "federation:trust")]
    FederationTrust,
}

impl TappPermission {
    /// Every catalog entry, in enum declaration order.
    pub const ALL: &'static [TappPermission] = &[
        TappPermission::PlatformRead,
        TappPermission::AnalyticsRead,
        TappPermission::TappListRead,
        TappPermission::BrewRead,
        TappPermission::BrewWrite,
        TappPermission::ReportRead,
        TappPermission::StorageRead,
        TappPermission::UiNotification,
        TappPermission::UiFullscreen,
        TappPermission::UiTheme,
        TappPermission::UiConfirm,
        TappPermission::UiOpenUrl,
        TappPermission::MediaRead,
        TappPermission::MediaAudio,
        TappPermission::MediaControl,
        TappPermission::EventSubscribe,
        TappPermission::FederationRead,
        TappPermission::FederationMessage,
        TappPermission::FederationFiles,
        TappPermission::GameSession,
        TappPermission::FederationInteract,
        TappPermission::FederationRing,
        TappPermission::FederationPost,
        TappPermission::FederationChannel,
        TappPermission::FederationRoom,
        TappPermission::AiGenerate,
        TappPermission::AiAnalyze,
        TappPermission::AiChat,
        TappPermission::AiImage,
        TappPermission::AiSearch,
        TappPermission::ThreeDGenerate,
        TappPermission::ReportWrite,
        TappPermission::NetworkFetch,
        TappPermission::ComponentTheme,
        TappPermission::ShortcutRegister,
        TappPermission::EventPublish,
        TappPermission::SchedulerRegister,
        TappPermission::SpeechTts,
        TappPermission::SpeechAsr,
        TappPermission::StorageWrite,
        TappPermission::BrewCommentWrite,
        TappPermission::WidgetRegister,
        TappPermission::PlatformWrite,
        TappPermission::PlatformRegister,
        TappPermission::ComponentAgent,
        TappPermission::TappListManage,
        TappPermission::BrewManage,
        TappPermission::FederationTrust,
    ];

    /// Capabilities whose HTTP boundary still requires a durable logged-in user.
    ///
    /// Guest-safe basic capabilities (private storage under signed guest session
    /// id, public platform cache reads, **reduced** `analytics:read` visitor-card
    /// aggregates) are intentionally **not** listed here so Runtime Grants can
    /// include them for guest widgets. Full admin analytics breakdowns stay
    /// server-gated on admin role inside the analytics handlers.
    pub fn requires_authenticated_subject(&self) -> bool {
        matches!(
            self,
            TappPermission::BrewWrite
                | TappPermission::BrewCommentWrite
                | TappPermission::ReportRead
                | TappPermission::UiNotification
                | TappPermission::ComponentTheme
                | TappPermission::ShortcutRegister
                | TappPermission::SchedulerRegister
                | TappPermission::SpeechTts
                | TappPermission::SpeechAsr
                | TappPermission::FederationInteract
                | TappPermission::FederationRing
        )
    }

    /// 获取权限等级
    pub fn level(&self) -> PermissionLevel {
        match self {
            // Basic
            TappPermission::PlatformRead
            | TappPermission::AnalyticsRead
            | TappPermission::TappListRead
            | TappPermission::BrewRead
            | TappPermission::BrewWrite
            | TappPermission::ReportRead
            | TappPermission::StorageRead
            | TappPermission::UiNotification
            | TappPermission::UiFullscreen
            | TappPermission::UiTheme
            | TappPermission::UiConfirm
            | TappPermission::UiOpenUrl
            | TappPermission::MediaRead
            | TappPermission::MediaAudio
            | TappPermission::MediaControl
            | TappPermission::EventSubscribe
            | TappPermission::FederationRead
            | TappPermission::FederationInteract
            | TappPermission::FederationRing
            | TappPermission::FederationMessage
            | TappPermission::FederationFiles
            | TappPermission::GameSession => PermissionLevel::Basic,

            // Elevated（可配置下放的集合见 all_elevated）
            TappPermission::AiGenerate
            | TappPermission::AiAnalyze
            | TappPermission::AiChat
            | TappPermission::AiImage
            | TappPermission::AiSearch
            | TappPermission::ThreeDGenerate
            | TappPermission::NetworkFetch
            | TappPermission::ComponentTheme
            | TappPermission::ShortcutRegister
            | TappPermission::EventPublish
            | TappPermission::SchedulerRegister
            | TappPermission::SpeechTts
            | TappPermission::SpeechAsr => PermissionLevel::Elevated,
            TappPermission::StorageWrite => PermissionLevel::Elevated,
            TappPermission::FederationPost
            | TappPermission::FederationChannel
            | TappPermission::FederationRoom => PermissionLevel::Elevated,
            TappPermission::BrewCommentWrite => PermissionLevel::Elevated,

            // Privileged
            TappPermission::WidgetRegister
            | TappPermission::PlatformWrite
            | TappPermission::PlatformRegister
            | TappPermission::ComponentAgent
            | TappPermission::TappListManage
            | TappPermission::BrewManage
            | TappPermission::FederationTrust
            | TappPermission::ReportWrite => PermissionLevel::Privileged,
        }
    }

    /// 获取所有可配置下放的 elevated 级别权限（不含 report:write）
    pub fn all_elevated() -> Vec<TappPermission> {
        vec![
            TappPermission::AiGenerate,
            TappPermission::AiAnalyze,
            TappPermission::AiChat,
            TappPermission::AiImage,
            TappPermission::AiSearch,
            TappPermission::ThreeDGenerate,
            TappPermission::NetworkFetch,
            TappPermission::ComponentTheme,
            TappPermission::ShortcutRegister,
            TappPermission::EventPublish,
            TappPermission::SchedulerRegister,
            TappPermission::SpeechTts,
            TappPermission::SpeechAsr,
            TappPermission::StorageWrite,
            TappPermission::FederationPost,
            TappPermission::FederationChannel,
            TappPermission::FederationRoom,
            TappPermission::BrewCommentWrite,
        ]
    }

    /// 从字符串解析权限
    pub fn from_str(s: &str) -> Option<TappPermission> {
        match s {
            "widget:register" => Some(TappPermission::WidgetRegister),
            "platform:read" => Some(TappPermission::PlatformRead),
            "analytics:read" => Some(TappPermission::AnalyticsRead),
            "tappList:read" => Some(TappPermission::TappListRead),
            "brew:read" => Some(TappPermission::BrewRead),
            "brew:write" => Some(TappPermission::BrewWrite),
            "platform:write" => Some(TappPermission::PlatformWrite),
            "platform:register" => Some(TappPermission::PlatformRegister),
            "report:read" => Some(TappPermission::ReportRead),
            "report:write" => Some(TappPermission::ReportWrite),
            "storage:read" => Some(TappPermission::StorageRead),
            "ui:notification" => Some(TappPermission::UiNotification),
            "ui:fullscreen" => Some(TappPermission::UiFullscreen),
            "ui:theme" => Some(TappPermission::UiTheme),
            "ui:confirm" => Some(TappPermission::UiConfirm),
            "ui:openUrl" => Some(TappPermission::UiOpenUrl),
            "ai:generate" => Some(TappPermission::AiGenerate),
            "ai:analyze" => Some(TappPermission::AiAnalyze),
            "ai:chat" => Some(TappPermission::AiChat),
            "ai:image" => Some(TappPermission::AiImage),
            "ai:search" => Some(TappPermission::AiSearch),
            "3d:generate" => Some(TappPermission::ThreeDGenerate),
            "network:fetch" => Some(TappPermission::NetworkFetch),
            "media:control" => Some(TappPermission::MediaControl),
            "media:read" => Some(TappPermission::MediaRead),
            "media:audio" => Some(TappPermission::MediaAudio),
            "component:theme" => Some(TappPermission::ComponentTheme),
            "component:agent" => Some(TappPermission::ComponentAgent),
            "tappList:manage" => Some(TappPermission::TappListManage),
            "brew:manage" => Some(TappPermission::BrewManage),
            "shortcut:register" => Some(TappPermission::ShortcutRegister),
            "event:publish" => Some(TappPermission::EventPublish),
            "event:subscribe" => Some(TappPermission::EventSubscribe),
            "scheduler:register" => Some(TappPermission::SchedulerRegister),
            "speech:tts" => Some(TappPermission::SpeechTts),
            "speech:asr" => Some(TappPermission::SpeechAsr),
            "storage:write" => Some(TappPermission::StorageWrite),
            "brew:commentWrite" => Some(TappPermission::BrewCommentWrite),
            "federation:read" => Some(TappPermission::FederationRead),
            "federation:post" => Some(TappPermission::FederationPost),
            "federation:interact" => Some(TappPermission::FederationInteract),
            "federation:channel" => Some(TappPermission::FederationChannel),
            "federation:room" => Some(TappPermission::FederationRoom),
            "federation:ring" => Some(TappPermission::FederationRing),
            "federation:message" => Some(TappPermission::FederationMessage),
            "federation:files" => Some(TappPermission::FederationFiles),
            "federation:trust" => Some(TappPermission::FederationTrust),
            "game:session" => Some(TappPermission::GameSession),
            _ => None,
        }
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            TappPermission::WidgetRegister => "widget:register",
            TappPermission::PlatformRead => "platform:read",
            TappPermission::AnalyticsRead => "analytics:read",
            TappPermission::TappListRead => "tappList:read",
            TappPermission::BrewRead => "brew:read",
            TappPermission::BrewWrite => "brew:write",
            TappPermission::PlatformWrite => "platform:write",
            TappPermission::PlatformRegister => "platform:register",
            TappPermission::ReportRead => "report:read",
            TappPermission::ReportWrite => "report:write",
            TappPermission::StorageRead => "storage:read",
            TappPermission::StorageWrite => "storage:write",
            TappPermission::BrewCommentWrite => "brew:commentWrite",
            TappPermission::UiNotification => "ui:notification",
            TappPermission::UiFullscreen => "ui:fullscreen",
            TappPermission::UiTheme => "ui:theme",
            TappPermission::UiConfirm => "ui:confirm",
            TappPermission::UiOpenUrl => "ui:openUrl",
            TappPermission::AiGenerate => "ai:generate",
            TappPermission::AiAnalyze => "ai:analyze",
            TappPermission::AiChat => "ai:chat",
            TappPermission::AiImage => "ai:image",
            TappPermission::AiSearch => "ai:search",
            TappPermission::ThreeDGenerate => "3d:generate",
            TappPermission::NetworkFetch => "network:fetch",
            TappPermission::MediaControl => "media:control",
            TappPermission::MediaRead => "media:read",
            TappPermission::MediaAudio => "media:audio",
            TappPermission::ComponentTheme => "component:theme",
            TappPermission::ComponentAgent => "component:agent",
            TappPermission::TappListManage => "tappList:manage",
            TappPermission::BrewManage => "brew:manage",
            TappPermission::ShortcutRegister => "shortcut:register",
            TappPermission::EventPublish => "event:publish",
            TappPermission::EventSubscribe => "event:subscribe",
            TappPermission::SchedulerRegister => "scheduler:register",
            TappPermission::SpeechTts => "speech:tts",
            TappPermission::SpeechAsr => "speech:asr",
            TappPermission::FederationRead => "federation:read",
            TappPermission::FederationPost => "federation:post",
            TappPermission::FederationInteract => "federation:interact",
            TappPermission::FederationChannel => "federation:channel",
            TappPermission::FederationRoom => "federation:room",
            TappPermission::FederationRing => "federation:ring",
            TappPermission::FederationMessage => "federation:message",
            TappPermission::FederationFiles => "federation:files",
            TappPermission::FederationTrust => "federation:trust",
            TappPermission::GameSession => "game:session",
        }
    }
}

/// 权限等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    Public,
    Basic,
    Elevated,
    Privileged,
}

impl PermissionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionLevel::Public => "public",
            PermissionLevel::Basic => "basic",
            PermissionLevel::Elevated => "elevated",
            PermissionLevel::Privileged => "privileged",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_catalog_covers(permission: TappPermission) {
        match permission {
            TappPermission::PlatformRead
            | TappPermission::AnalyticsRead
            | TappPermission::TappListRead
            | TappPermission::BrewRead
            | TappPermission::BrewWrite
            | TappPermission::ReportRead
            | TappPermission::StorageRead
            | TappPermission::UiNotification
            | TappPermission::UiFullscreen
            | TappPermission::UiTheme
            | TappPermission::UiConfirm
            | TappPermission::UiOpenUrl
            | TappPermission::MediaRead
            | TappPermission::MediaAudio
            | TappPermission::MediaControl
            | TappPermission::EventSubscribe
            | TappPermission::FederationRead
            | TappPermission::FederationMessage
            | TappPermission::FederationFiles
            | TappPermission::GameSession
            | TappPermission::FederationInteract
            | TappPermission::FederationRing
            | TappPermission::FederationPost
            | TappPermission::FederationChannel
            | TappPermission::FederationRoom
            | TappPermission::AiGenerate
            | TappPermission::AiAnalyze
            | TappPermission::AiChat
            | TappPermission::AiImage
            | TappPermission::AiSearch
            | TappPermission::ThreeDGenerate
            | TappPermission::ReportWrite
            | TappPermission::NetworkFetch
            | TappPermission::ComponentTheme
            | TappPermission::ShortcutRegister
            | TappPermission::EventPublish
            | TappPermission::SchedulerRegister
            | TappPermission::SpeechTts
            | TappPermission::SpeechAsr
            | TappPermission::StorageWrite
            | TappPermission::BrewCommentWrite
            | TappPermission::WidgetRegister
            | TappPermission::PlatformWrite
            | TappPermission::PlatformRegister
            | TappPermission::ComponentAgent
            | TappPermission::TappListManage
            | TappPermission::BrewManage
            | TappPermission::FederationTrust => {}
        }
    }

    #[test]
    fn catalog_round_trips_every_entry() {
        for permission in TappPermission::ALL {
            assert_catalog_covers(*permission);
            assert_eq!(
                TappPermission::from_str(permission.as_str()),
                Some(*permission)
            );
        }
    }

    #[test]
    fn storage_split_is_fail_closed_with_replacement_hint() {
        assert!(TappPermission::from_str("storage").is_none());
        assert_eq!(
            TappPermission::from_str("storage:read"),
            Some(TappPermission::StorageRead)
        );
        assert_eq!(
            TappPermission::from_str("storage:write"),
            Some(TappPermission::StorageWrite)
        );
        assert_eq!(TappPermission::StorageRead.level(), PermissionLevel::Basic);
        assert_eq!(
            TappPermission::StorageWrite.level(),
            PermissionLevel::Elevated
        );
        assert_eq!(TappPermission::StorageRead.as_str(), "storage:read");
        assert_eq!(TappPermission::StorageWrite.as_str(), "storage:write");

        let hint = tapp_permission_replacement_hint("storage").expect("retired storage name");
        assert!(hint.contains("storage:read"), "{hint}");
        assert!(hint.contains("storage:write"), "{hint}");

        let unknown = UnknownTappPermission {
            permission: "storage".to_string(),
        };
        assert_eq!(unknown.code(), UNKNOWN_TAPP_PERMISSION_CODE);
        let message = unknown.message();
        assert!(message.contains("'storage'"), "{message}");
        assert!(message.contains("storage:read"), "{message}");
        assert!(message.contains("storage:write"), "{message}");
        assert!(TappPermission::from_str("storage").is_none());
    }

    #[test]
    fn three_d_generate_is_elevated_catalog_entry() {
        assert_eq!(
            TappPermission::from_str("3d:generate"),
            Some(TappPermission::ThreeDGenerate)
        );
        assert_eq!(TappPermission::ThreeDGenerate.as_str(), "3d:generate");
        assert_eq!(
            TappPermission::ThreeDGenerate.level(),
            PermissionLevel::Elevated
        );
        assert!(TappPermission::all_elevated().contains(&TappPermission::ThreeDGenerate));
        assert!(!TappPermission::ThreeDGenerate.requires_authenticated_subject());
    }

    #[test]
    fn authenticated_subject_set_matches_guest_grant_filter() {
        let required = [
            TappPermission::BrewWrite,
            TappPermission::BrewCommentWrite,
            TappPermission::ReportRead,
            TappPermission::UiNotification,
            TappPermission::ComponentTheme,
            TappPermission::ShortcutRegister,
            TappPermission::SchedulerRegister,
            TappPermission::SpeechTts,
            TappPermission::SpeechAsr,
            TappPermission::FederationInteract,
            TappPermission::FederationRing,
        ];
        for permission in required {
            assert!(
                permission.requires_authenticated_subject(),
                "{} must require an authenticated subject",
                permission.as_str()
            );
        }

        let guest_safe = [
            TappPermission::PlatformRead,
            TappPermission::AnalyticsRead,
            TappPermission::StorageRead,
            TappPermission::MediaRead,
            TappPermission::MediaControl,
            TappPermission::EventSubscribe,
            TappPermission::TappListRead,
            TappPermission::BrewRead,
            TappPermission::FederationRead,
        ];
        for permission in guest_safe {
            assert!(
                !permission.requires_authenticated_subject(),
                "{} must stay guest-safe at the catalog layer",
                permission.as_str()
            );
        }
    }

    #[test]
    fn all_elevated_excludes_privileged_and_basic() {
        let elevated = TappPermission::all_elevated();
        assert!(elevated.contains(&TappPermission::AiSearch));
        assert!(!elevated.contains(&TappPermission::ReportWrite));
        assert!(!elevated.contains(&TappPermission::MediaControl));
        assert!(!elevated.contains(&TappPermission::FederationInteract));
        for permission in &elevated {
            assert_eq!(permission.level(), PermissionLevel::Elevated);
        }
    }

    #[test]
    fn retired_names_hint_but_never_decode() {
        for retired in ["storage", "federation:write", "brew:comment"] {
            assert!(TappPermission::from_str(retired).is_none());
            assert!(tapp_permission_replacement_hint(retired).is_some());
            assert_eq!(
                UnknownTappPermission {
                    permission: retired.to_string(),
                }
                .code(),
                UNKNOWN_TAPP_PERMISSION_CODE
            );
        }
        assert!(tapp_permission_replacement_hint("legacy:unknown").is_none());
        assert_eq!(
            UnknownTappPermission {
                permission: "legacy:unknown".to_string(),
            }
            .message(),
            "Unknown Tapp permission 'legacy:unknown'"
        );
    }

    #[test]
    fn export_maps_call_shipped_catalog_functions() {
        let levels = permission_levels();
        assert_eq!(levels.len(), TappPermission::ALL.len());
        assert_eq!(levels.get("storage:read"), Some(&"basic"));
        assert_eq!(levels.get("storage:write"), Some(&"elevated"));
        assert_eq!(levels.get("3d:generate"), Some(&"elevated"));
        assert_eq!(levels.get("widget:register"), Some(&"privileged"));
        assert!(!levels.contains_key("storage"));

        for permission in TappPermission::ALL {
            assert_eq!(
                levels.get(permission.as_str()).copied(),
                Some(permission.level().as_str())
            );
        }

        let hints = replacement_hints();
        assert_eq!(
            hints.get("storage").copied(),
            tapp_permission_replacement_hint("storage")
        );
        assert_eq!(
            hints.get("federation:write").copied(),
            tapp_permission_replacement_hint("federation:write")
        );
        assert_eq!(
            hints.get("brew:comment").copied(),
            tapp_permission_replacement_hint("brew:comment")
        );

        let authenticated = requires_authenticated_subject_names();
        assert!(authenticated.contains(&"brew:write"));
        assert!(authenticated.contains(&"speech:tts"));
        assert!(!authenticated.contains(&"storage:read"));
        assert!(!authenticated.contains(&"platform:read"));
        for name in &authenticated {
            let permission = TappPermission::from_str(name).expect(name);
            assert!(permission.requires_authenticated_subject());
        }
    }
}
