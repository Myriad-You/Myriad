//! Tapp 权限下放服务
//!
//! 基于 Tapp 系统的权限等级（PermissionLevel）管理权限下放。
//!
//! ## 权限层级（来自 Tapp 系统）
//! - **public**: 无需权限，所有人可用
//! - **basic**: 基础权限，默认所有角色可用；需要持久身份的数据能力可排除游客
//! - **elevated**: 提升权限，默认仅管理员，可配置下放
//! - **privileged**: 特权权限，始终仅管理员可用
//!
//! ## 设计原则
//! - basic 级别权限默认开放，但真实路由要求持久登录主体的能力不向游客签发
//! - elevated 级别权限可由管理员选择性下放给普通用户或游客
//! - privileged 级别权限始终仅限管理员
//!
//! ## Tapp 权限完整列表
//!
//! ### Basic - 默认开放（标注 authenticated 的能力不向游客签发）
//! - platform:read, analytics:read, tappList:read, brew:read
//! - brew:write (authenticated), brew:comment (authenticated)
//! - report:read (authenticated), storage:read (guest-safe)
//! - ui:notification (authenticated), ui:fullscreen, ui:theme:read, ui:theme:subscribe, ui:confirm, ui:openUrl
//! - media:read, media:control, media:audio, event:subscribe
//! - federation:read, federation:write, federation:message, federation:files
//!
//! ### Elevated - 可配置下放
//! - ai:generate, ai:analyze, ai:chat, ai:image
//! - network:fetch, component:theme (authenticated)
//! - shortcut:register (authenticated), event:publish
//! - scheduler:register, speech:tts, speech:asr (all authenticated)
//!
//! ### Privileged - 仅管理员
//! - widget:register, platform:write, platform:register, component:agent
//! - tappList:manage, brew:manage, federation:trust
//! - **report:write**（数据报告生成，不可下放）

use crate::config::DynamicConfig;
use serde::{Deserialize, Serialize};

pub const UNKNOWN_TAPP_PERMISSION_CODE: &str = "UNKNOWN_TAPP_PERMISSION";

/// 已移除权限名的替代建议（仅用于错误提示，不构成兼容映射；
/// 未知名仍 fail-closed，绝不解码成新权限）。
/// 单一来源：permission-service、声明式 API、运行时签发三处错误路径共用。
pub(crate) fn tapp_permission_replacement_hint(permission: &str) -> Option<&'static str> {
    match permission {
        "storage" => Some(
            "use 'storage:read' or 'storage:write' instead; update the TAPP Manifest, then update or reinstall the app",
        ),
        "ui:theme" => Some(
            "use 'ui:theme:read' to read the current theme and/or 'ui:theme:subscribe' to receive theme changes; update the TAPP Manifest, then update or reinstall the app",
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
    #[serde(rename = "brew:write")]
    BrewWrite,
    #[serde(rename = "brew:comment")]
    BrewComment,
    #[serde(rename = "report:read")]
    ReportRead,
    #[serde(rename = "storage:read")]
    StorageRead,
    #[serde(rename = "ui:notification")]
    UiNotification,
    #[serde(rename = "ui:fullscreen")]
    UiFullscreen,
    /// Read the current site theme and primary color once.
    #[serde(rename = "ui:theme:read")]
    UiThemeRead,
    /// Subscribe to future host theme / primary-color changes.
    #[serde(rename = "ui:theme:subscribe")]
    UiThemeSubscribe,
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
    #[serde(rename = "federation:write")]
    FederationWrite,
    #[serde(rename = "federation:message")]
    FederationMessage,
    #[serde(rename = "federation:files")]
    FederationFiles,

    // Elevated 级别
    #[serde(rename = "ai:generate")]
    AiGenerate,
    #[serde(rename = "ai:analyze")]
    AiAnalyze,
    #[serde(rename = "ai:chat")]
    AiChat,
    #[serde(rename = "ai:image")]
    AiImage,
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
    /// Capabilities whose real backend routes require an authenticated,
    /// durable application user. Guest Runtime Grants must not advertise these
    /// even when their broad level is basic/elevated.
    /// Capabilities whose HTTP boundary still requires a durable logged-in user.
    ///
    /// Guest-safe basic capabilities (private storage under signed guest session
    /// id, public platform cache reads, **reduced** `analytics:read` visitor-card
    /// aggregates) are intentionally **not** listed here so Runtime Grants can
    /// include them for guest widgets. Full admin analytics breakdowns stay
    /// server-gated on admin role inside the analytics handlers.
    fn requires_authenticated_subject(&self) -> bool {
        matches!(
            self,
            TappPermission::BrewWrite
                | TappPermission::BrewComment
                | TappPermission::ReportRead
                | TappPermission::UiNotification
                | TappPermission::ComponentTheme
                | TappPermission::ShortcutRegister
                | TappPermission::SchedulerRegister
                | TappPermission::SpeechTts
                | TappPermission::SpeechAsr
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
            | TappPermission::BrewComment
            | TappPermission::ReportRead
            | TappPermission::StorageRead
            | TappPermission::UiNotification
            | TappPermission::UiFullscreen
            | TappPermission::UiThemeRead
            | TappPermission::UiThemeSubscribe
            | TappPermission::UiConfirm
            | TappPermission::UiOpenUrl
            | TappPermission::MediaRead
            | TappPermission::MediaAudio
            | TappPermission::MediaControl
            | TappPermission::EventSubscribe
            | TappPermission::FederationRead
            | TappPermission::FederationWrite
            | TappPermission::FederationMessage
            | TappPermission::FederationFiles => PermissionLevel::Basic,

            // Elevated（可配置下放的集合见 all_elevated；brew:write 不在其中）
            TappPermission::AiGenerate
            | TappPermission::AiAnalyze
            | TappPermission::AiChat
            | TappPermission::AiImage
            | TappPermission::NetworkFetch
            | TappPermission::ComponentTheme
            | TappPermission::ShortcutRegister
            | TappPermission::EventPublish
            | TappPermission::SchedulerRegister
            | TappPermission::SpeechTts
            | TappPermission::SpeechAsr => PermissionLevel::Elevated,
            TappPermission::StorageWrite => PermissionLevel::Elevated,

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

    /// 获取权限的显示名称
    #[allow(dead_code)]
    pub fn display_name(&self) -> &'static str {
        match self {
            TappPermission::WidgetRegister => "注册小组件",
            TappPermission::PlatformRead => "读取平台数据",
            TappPermission::AnalyticsRead => "读取访问统计",
            TappPermission::TappListRead => "读取 Tapp 列表",
            TappPermission::BrewRead => "读取 Brew 内容",
            TappPermission::PlatformWrite => "写入平台数据",
            TappPermission::PlatformRegister => "注册新平台",
            TappPermission::AiGenerate => "AI 生成",
            TappPermission::AiAnalyze => "AI 分析",
            TappPermission::AiChat => "AI 对话",
            TappPermission::AiImage => "AI 图片生成",
            TappPermission::ReportRead => "读取报告",
            TappPermission::ReportWrite => "生成报告",
            TappPermission::BrewWrite => "编辑 Brew 内容",
            TappPermission::BrewComment => "Brew 评论",
            TappPermission::StorageRead => "读取本地存储",
            TappPermission::StorageWrite => "写入本地存储",
            TappPermission::UiNotification => "显示通知",
            TappPermission::UiFullscreen => "全屏模式",
            TappPermission::UiThemeRead => "读取主题",
            TappPermission::UiThemeSubscribe => "订阅主题变化",
            TappPermission::UiConfirm => "确认对话框",
            TappPermission::UiOpenUrl => "打开声明链接",
            TappPermission::NetworkFetch => "网络请求",
            TappPermission::MediaControl => "媒体控制",
            TappPermission::MediaRead => "读取媒体",
            TappPermission::MediaAudio => "播放音频",
            TappPermission::ComponentTheme => "注册主题",
            TappPermission::ComponentAgent => "注册 Agent",
            TappPermission::TappListManage => "管理 Tapp",
            TappPermission::BrewManage => "管理 Brew",
            TappPermission::ShortcutRegister => "注册快捷键",
            TappPermission::EventPublish => "发布事件",
            TappPermission::SchedulerRegister => "注册定时任务",
            TappPermission::EventSubscribe => "订阅事件",
            TappPermission::SpeechTts => "文本转语音",
            TappPermission::SpeechAsr => "语音转文本",
            TappPermission::FederationRead => "读取联邦数据",
            TappPermission::FederationWrite => "联邦个人操作",
            TappPermission::FederationMessage => "联邦消息",
            TappPermission::FederationFiles => "联邦文件传输",
            TappPermission::FederationTrust => "联邦信任管理",
        }
    }

    /// 获取所有可配置下放的 elevated 级别权限（不含 report:write / brew:write）
    pub fn all_elevated() -> Vec<TappPermission> {
        vec![
            TappPermission::AiGenerate,
            TappPermission::AiAnalyze,
            TappPermission::AiChat,
            TappPermission::AiImage,
            TappPermission::NetworkFetch,
            TappPermission::ComponentTheme,
            TappPermission::ShortcutRegister,
            TappPermission::EventPublish,
            TappPermission::SchedulerRegister,
            TappPermission::SpeechTts,
            TappPermission::SpeechAsr,
            TappPermission::StorageWrite,
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
            "platform:write" => Some(TappPermission::PlatformWrite),
            "platform:register" => Some(TappPermission::PlatformRegister),
            "report:read" => Some(TappPermission::ReportRead),
            "report:write" => Some(TappPermission::ReportWrite),
            "brew:write" => Some(TappPermission::BrewWrite),
            "brew:comment" => Some(TappPermission::BrewComment),
            "storage:read" => Some(TappPermission::StorageRead),
            "ui:notification" => Some(TappPermission::UiNotification),
            "ui:fullscreen" => Some(TappPermission::UiFullscreen),
            "ui:theme:read" => Some(TappPermission::UiThemeRead),
            "ui:theme:subscribe" => Some(TappPermission::UiThemeSubscribe),
            "ui:confirm" => Some(TappPermission::UiConfirm),
            "ui:openUrl" => Some(TappPermission::UiOpenUrl),
            "ai:generate" => Some(TappPermission::AiGenerate),
            "ai:analyze" => Some(TappPermission::AiAnalyze),
            "ai:chat" => Some(TappPermission::AiChat),
            "ai:image" => Some(TappPermission::AiImage),
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
            "federation:read" => Some(TappPermission::FederationRead),
            "federation:write" => Some(TappPermission::FederationWrite),
            "federation:message" => Some(TappPermission::FederationMessage),
            "federation:files" => Some(TappPermission::FederationFiles),
            "federation:trust" => Some(TappPermission::FederationTrust),
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
            TappPermission::PlatformWrite => "platform:write",
            TappPermission::PlatformRegister => "platform:register",
            TappPermission::ReportRead => "report:read",
            TappPermission::ReportWrite => "report:write",
            TappPermission::BrewWrite => "brew:write",
            TappPermission::BrewComment => "brew:comment",
            TappPermission::StorageRead => "storage:read",
            TappPermission::StorageWrite => "storage:write",
            TappPermission::UiNotification => "ui:notification",
            TappPermission::UiFullscreen => "ui:fullscreen",
            TappPermission::UiThemeRead => "ui:theme:read",
            TappPermission::UiThemeSubscribe => "ui:theme:subscribe",
            TappPermission::UiConfirm => "ui:confirm",
            TappPermission::UiOpenUrl => "ui:openUrl",
            TappPermission::AiGenerate => "ai:generate",
            TappPermission::AiAnalyze => "ai:analyze",
            TappPermission::AiChat => "ai:chat",
            TappPermission::AiImage => "ai:image",
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
            TappPermission::FederationWrite => "federation:write",
            TappPermission::FederationMessage => "federation:message",
            TappPermission::FederationFiles => "federation:files",
            TappPermission::FederationTrust => "federation:trust",
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

/// Tapp 权限检查服务
pub struct TappPermissionService;

impl TappPermissionService {
    /// Filter manifest/requested permissions by the user's current role.
    pub fn filter_permissions_for_role(
        config: &DynamicConfig,
        role: UserRole,
        permissions: &[String],
    ) -> Result<Vec<String>, UnknownTappPermission> {
        let parsed = permissions
            .iter()
            .map(|permission| {
                TappPermission::from_str(permission).ok_or_else(|| UnknownTappPermission {
                    permission: permission.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(parsed
            .into_iter()
            .filter(|permission| Self::check(config, role, *permission))
            .map(|permission| permission.as_str().to_string())
            .collect())
    }

    /// 检查用户是否拥有特定 Tapp 权限
    pub fn check(config: &DynamicConfig, role: UserRole, permission: TappPermission) -> bool {
        // 管理员拥有所有权限
        if role == UserRole::Admin {
            return true;
        }

        // Never issue a capability whose real route is behind mandatory auth;
        // Runtime Grant metadata must match the executable HTTP boundary.
        // (Storage + platform:read are guest-safe and use optional_auth.)
        if role == UserRole::Guest && permission.requires_authenticated_subject() {
            return false;
        }

        // 游客的 federation 能力严格只读：可以读取经过内容级过滤的公开
        // Feed，但不能关注、发布、通信或传输文件。
        if role == UserRole::Guest
            && matches!(
                permission,
                TappPermission::FederationWrite
                    | TappPermission::FederationMessage
                    | TappPermission::FederationFiles
            )
        {
            return false;
        }

        match permission.level() {
            PermissionLevel::Public => true,
            PermissionLevel::Basic => true,
            PermissionLevel::Elevated => Self::check_elevated(config, role, permission),
            PermissionLevel::Privileged => false, // 仅管理员
        }
    }

    /// 检查 elevated 级别权限
    fn check_elevated(config: &DynamicConfig, role: UserRole, permission: TappPermission) -> bool {
        match role {
            UserRole::Admin => true,
            UserRole::User => Self::check_user_elevated(config, permission),
            UserRole::Guest => Self::check_guest_elevated(config, permission),
        }
    }

    /// 检查普通用户的 elevated 权限
    fn check_user_elevated(config: &DynamicConfig, permission: TappPermission) -> bool {
        match permission {
            TappPermission::AiGenerate => config.user_perm_ai_generate,
            TappPermission::AiAnalyze => config.user_perm_ai_analyze,
            TappPermission::AiChat => config.user_perm_ai_chat,
            TappPermission::AiImage => config.user_perm_ai_image,
            // report:write 已升 privileged；media:control 已降 basic；brew:write 不开放下放
            TappPermission::NetworkFetch => config.user_perm_network_fetch,
            TappPermission::ComponentTheme => config.user_perm_component_theme,
            TappPermission::ShortcutRegister => config.user_perm_shortcut_register,
            TappPermission::EventPublish => config.user_perm_event_publish,
            TappPermission::SchedulerRegister => config.user_perm_scheduler_register,
            TappPermission::SpeechTts => config.user_perm_speech_tts,
            TappPermission::SpeechAsr => config.user_perm_speech_asr,
            TappPermission::StorageWrite => config.user_perm_storage_write,
            _ => false,
        }
    }

    /// 检查游客的 elevated 权限
    fn check_guest_elevated(config: &DynamicConfig, permission: TappPermission) -> bool {
        match permission {
            TappPermission::AiGenerate => config.guest_perm_ai_generate,
            TappPermission::AiAnalyze => config.guest_perm_ai_analyze,
            TappPermission::AiChat => config.guest_perm_ai_chat,
            TappPermission::AiImage => config.guest_perm_ai_image,
            TappPermission::NetworkFetch => config.guest_perm_network_fetch,
            TappPermission::ComponentTheme => false,
            TappPermission::ShortcutRegister => false,
            TappPermission::EventPublish => config.guest_perm_event_publish,
            TappPermission::SchedulerRegister => false,
            TappPermission::SpeechTts => false,
            TappPermission::SpeechAsr => false,
            TappPermission::StorageWrite => config.guest_perm_storage_write,
            _ => false,
        }
    }

    /// 获取用户可用的权限等级列表
    pub fn get_allowed_levels(config: &DynamicConfig, role: UserRole) -> Vec<PermissionLevel> {
        match role {
            UserRole::Admin => vec![
                PermissionLevel::Public,
                PermissionLevel::Basic,
                PermissionLevel::Elevated,
                PermissionLevel::Privileged,
            ],
            UserRole::User | UserRole::Guest => {
                let mut levels = vec![PermissionLevel::Public, PermissionLevel::Basic];
                // 如果有任何 elevated 权限被授予，添加 elevated 级别
                let has_elevated = TappPermission::all_elevated()
                    .into_iter()
                    .any(|p| Self::check(config, role, p));
                if has_elevated {
                    levels.push(PermissionLevel::Elevated);
                }
                levels
            }
        }
    }

    /// 获取权限配置摘要
    pub fn get_permission_config(config: &DynamicConfig) -> TappPermissionConfig {
        TappPermissionConfig {
            user: ElevatedPermissions {
                ai_generate: config.user_perm_ai_generate,
                ai_analyze: config.user_perm_ai_analyze,
                ai_chat: config.user_perm_ai_chat,
                ai_image: config.user_perm_ai_image,
                report_write: false, // 不再下放
                network_fetch: config.user_perm_network_fetch,
                // media:control 已降 basic，始终可用；字段保留供 API 兼容
                media_control: true,
                component_theme: config.user_perm_component_theme,
                shortcut_register: config.user_perm_shortcut_register,
                event_publish: config.user_perm_event_publish,
                scheduler_register: config.user_perm_scheduler_register,
                speech_tts: config.user_perm_speech_tts,
                speech_asr: config.user_perm_speech_asr,
                storage_write: config.user_perm_storage_write,
            },
            guest: ElevatedPermissions {
                ai_generate: config.guest_perm_ai_generate,
                ai_analyze: config.guest_perm_ai_analyze,
                ai_chat: config.guest_perm_ai_chat,
                ai_image: config.guest_perm_ai_image,
                report_write: false, // 不再下放
                network_fetch: config.guest_perm_network_fetch,
                // media:control 已降 basic，始终可用；字段保留供 API 兼容
                media_control: true,
                // These routes require a durable authenticated subject. Keep
                // legacy config fields for schema compatibility, but never
                // advertise them as effective guest delegation settings.
                component_theme: false,
                shortcut_register: false,
                event_publish: config.guest_perm_event_publish,
                scheduler_register: false,
                speech_tts: false,
                speech_asr: false,
                storage_write: config.guest_perm_storage_write,
            },
            user_ai_quota: AiQuotaConfig {
                daily_calls: config.user_ai_daily_calls,
                daily_tokens: config.user_ai_daily_tokens,
                cooldown_seconds: config.user_ai_cooldown_seconds,
            },
            guest_ai_quota: AiQuotaConfig {
                daily_calls: config.guest_ai_daily_calls,
                daily_tokens: config.guest_ai_daily_tokens,
                cooldown_seconds: config.guest_ai_cooldown_seconds,
            },
        }
    }
}

/// Tapp 权限配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TappPermissionConfig {
    pub user: ElevatedPermissions,
    pub guest: ElevatedPermissions,
    /// 普通用户 AI 使用限额配置
    pub user_ai_quota: AiQuotaConfig,
    /// 游客 AI 使用限额配置
    pub guest_ai_quota: AiQuotaConfig,
}

/// AI 限额配置（用于设置界面）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiQuotaConfig {
    /// 每日 AI 调用次数限制
    pub daily_calls: i32,
    /// 每日 AI Token 限制
    pub daily_tokens: i32,
    /// AI 调用冷却时间（秒）
    pub cooldown_seconds: i32,
}

/// Elevated 级别权限配置（report:write 已升 privileged，不可下放）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevatedPermissions {
    pub ai_generate: bool,
    pub ai_analyze: bool,
    pub ai_chat: bool,
    pub ai_image: bool,
    /// 保留字段：始终为 false，前端不再展示
    #[serde(default)]
    pub report_write: bool,
    pub network_fetch: bool,
    /// 保留字段：media:control 已降 basic，摘要中始终为 true
    #[serde(default)]
    pub media_control: bool,
    pub component_theme: bool,
    pub shortcut_register: bool,
    pub event_publish: bool,
    pub scheduler_register: bool,
    pub speech_tts: bool,
    pub speech_asr: bool,
    pub storage_write: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_has_all_permissions() {
        let config = DynamicConfig::default();
        assert!(TappPermissionService::check(
            &config,
            UserRole::Admin,
            TappPermission::ComponentAgent
        ));
        assert!(TappPermissionService::check(
            &config,
            UserRole::Admin,
            TappPermission::AiGenerate
        ));
    }

    #[test]
    fn test_user_has_basic_permissions() {
        let config = DynamicConfig::default();
        assert!(TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::PlatformRead
        ));
        assert!(TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::StorageRead
        ));
    }

    #[test]
    fn storage_permissions_split_read_from_delegated_write() {
        let defaults = DynamicConfig::default();
        assert_eq!(TappPermission::StorageRead.level(), PermissionLevel::Basic);
        assert_eq!(TappPermission::StorageWrite.level(), PermissionLevel::Elevated);
        assert!(TappPermission::from_str("storage").is_none());
        assert!(TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::StorageRead
        ));
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::StorageWrite
        ));
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::Guest,
            TappPermission::StorageWrite
        ));

        let delegated = DynamicConfig {
            user_perm_storage_write: true,
            guest_perm_storage_write: true,
            ..DynamicConfig::default()
        };
        assert!(TappPermissionService::check(
            &delegated,
            UserRole::User,
            TappPermission::StorageWrite
        ));
        assert!(TappPermissionService::check(
            &delegated,
            UserRole::Guest,
            TappPermission::StorageWrite
        ));
    }

    #[test]
    fn test_media_control_is_basic_for_all_roles() {
        // media:control is basic: always allowed regardless of legacy config flags
        let config = DynamicConfig {
            user_perm_media_control: false,
            guest_perm_media_control: false,
            ..DynamicConfig::default()
        };
        assert_eq!(TappPermission::MediaControl.level(), PermissionLevel::Basic);
        assert!(!TappPermission::all_elevated().contains(&TappPermission::MediaControl));
        assert!(TappPermissionService::check(
            &config,
            UserRole::Admin,
            TappPermission::MediaControl
        ));
        assert!(TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::MediaControl
        ));
        assert!(TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::MediaControl
        ));

        let effective = TappPermissionService::get_permission_config(&config);
        assert!(effective.user.media_control);
        assert!(effective.guest.media_control);
    }

    #[test]
    fn test_guest_runtime_grant_excludes_authenticated_subject_capabilities() {
        let config = DynamicConfig {
            guest_perm_component_theme: true,
            guest_perm_shortcut_register: true,
            guest_perm_scheduler_register: true,
            guest_perm_speech_tts: true,
            guest_perm_speech_asr: true,
            ..DynamicConfig::default()
        };
        let requested = vec![
            "platform:read".to_string(),
            "analytics:read".to_string(),
            "media:read".to_string(),
            "media:control".to_string(),
            "event:subscribe".to_string(),
            "widget:register".to_string(),
            "brew:write".to_string(),
            "brew:comment".to_string(),
            "report:read".to_string(),
            "storage:read".to_string(),
            "ui:notification".to_string(),
            "component:theme".to_string(),
            "shortcut:register".to_string(),
            "scheduler:register".to_string(),
            "speech:tts".to_string(),
            "speech:asr".to_string(),
            "tappList:read".to_string(),
            "brew:read".to_string(),
            "federation:read".to_string(),
        ];

        let granted = TappPermissionService::filter_permissions_for_role(
            &config,
            UserRole::Guest,
            &requested,
        )
        .unwrap();

        // Guest-safe: platform:read, analytics:read (visitor-card aggregates only;
        // full admin summary is role-gated in the handler) + storage.
        // Still excluded: brew:write, report:read, notifications, speech, etc.
        assert_eq!(
            granted,
            vec![
                "platform:read",
                "analytics:read",
                "media:read",
                "media:control",
                "event:subscribe",
                "storage:read",
                "tappList:read",
                "brew:read",
                "federation:read"
            ]
        );
        assert!(TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::AnalyticsRead
        ));
        assert!(TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::StorageRead
        ));
        assert!(TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::PlatformRead
        ));

        let effective = TappPermissionService::get_permission_config(&config);
        assert!(!effective.guest.component_theme);
        assert!(!effective.guest.shortcut_register);
        assert!(!effective.guest.scheduler_register);
        assert!(!effective.guest.speech_tts);
        assert!(!effective.guest.speech_asr);
    }

    #[test]
    fn test_user_no_elevated_by_default() {
        let config = DynamicConfig::default();
        assert!(!TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::AiGenerate
        ));
        assert!(!TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::NetworkFetch
        ));
    }

    #[test]
    fn test_configurable_elevated_permissions() {
        let config = DynamicConfig::default();

        // 默认用户没有 AI 生成权限
        assert!(!TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::AiGenerate
        ));

        // 启用后可以
        let config_enabled = DynamicConfig {
            user_perm_ai_generate: true,
            ..Default::default()
        };
        assert!(TappPermissionService::check(
            &config_enabled,
            UserRole::User,
            TappPermission::AiGenerate
        ));
    }

    #[test]
    fn test_privileged_only_admin() {
        let config = DynamicConfig {
            user_perm_ai_generate: true, // 即使开启 elevated
            ..Default::default()
        };

        assert!(!TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::ComponentAgent
        ));
        assert!(!TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::ComponentAgent
        ));
    }

    #[test]
    fn test_widget_registration_is_admin_only_without_blocking_other_user_permissions() {
        let config = DynamicConfig::default();
        let requested = vec![
            "widget:register".to_string(),
            "storage:read".to_string(),
            "ui:theme:read".to_string(),
        ];

        assert!(TappPermissionService::check(
            &config,
            UserRole::Admin,
            TappPermission::WidgetRegister
        ));
        assert!(!TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::WidgetRegister
        ));
        assert!(!TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::WidgetRegister
        ));
        assert_eq!(
            TappPermissionService::filter_permissions_for_role(&config, UserRole::User, &requested,),
            Ok(vec![
                "storage:read".to_string(),
                "ui:theme:read".to_string()
            ])
        );
        assert_eq!(
            TappPermissionService::filter_permissions_for_role(
                &config,
                UserRole::Guest,
                &requested,
            ),
            Ok(vec![
                "storage:read".to_string(),
                "ui:theme:read".to_string()
            ])
        );
    }

    #[test]
    fn theme_permissions_split_read_from_subscribe() {
        // ADR 0013 / 0020: `ui:theme` split into read + subscribe, both Basic.
        // The old coarse name must not parse and its error must name the replacements.
        assert!(TappPermission::from_str("ui:theme").is_none());
        assert_eq!(
            TappPermission::from_str("ui:theme:read"),
            Some(TappPermission::UiThemeRead)
        );
        assert_eq!(
            TappPermission::from_str("ui:theme:subscribe"),
            Some(TappPermission::UiThemeSubscribe)
        );
        assert_eq!(TappPermission::UiThemeRead.as_str(), "ui:theme:read");
        assert_eq!(
            TappPermission::UiThemeSubscribe.as_str(),
            "ui:theme:subscribe"
        );
        assert_eq!(TappPermission::UiThemeRead.level(), PermissionLevel::Basic);
        assert_eq!(
            TappPermission::UiThemeSubscribe.level(),
            PermissionLevel::Basic
        );

        let config = DynamicConfig::default();
        for role in [UserRole::Admin, UserRole::User, UserRole::Guest] {
            assert!(TappPermissionService::check(
                &config,
                role,
                TappPermission::UiThemeRead
            ));
            assert!(TappPermissionService::check(
                &config,
                role,
                TappPermission::UiThemeSubscribe
            ));
        }

        // Read-only grants do not imply subscription and vice versa: the
        // granted set is exactly what was declared (no alias expansion).
        let read_only =
            TappPermissionService::filter_permissions_for_role(&config, UserRole::User, &[
                "ui:theme:read".to_string(),
            ])
            .unwrap();
        assert_eq!(read_only, vec!["ui:theme:read"]);

        let subscribe_only =
            TappPermissionService::filter_permissions_for_role(&config, UserRole::User, &[
                "ui:theme:subscribe".to_string(),
            ])
            .unwrap();
        assert_eq!(subscribe_only, vec!["ui:theme:subscribe"]);

        // `component:theme` stays a separate elevated, owner-scoped permission.
        assert_eq!(TappPermission::ComponentTheme.level(), PermissionLevel::Elevated);
        assert!(TappPermissionService::check(
            &config,
            UserRole::Admin,
            TappPermission::ComponentTheme
        ));
        assert!(!TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::ComponentTheme
        ));
    }

    #[test]
    fn removed_theme_permission_error_names_replacement_permissions() {
        let error = TappPermissionService::filter_permissions_for_role(
            &DynamicConfig::default(),
            UserRole::Admin,
            &["ui:theme".to_string()],
        )
        .unwrap_err();
        assert_eq!(error.permission, "ui:theme");
        assert_eq!(error.code(), UNKNOWN_TAPP_PERMISSION_CODE);
        let message = error.message();
        assert!(message.contains("ui:theme:read"));
        assert!(message.contains("ui:theme:subscribe"));
    }

    #[test]
    fn test_brew_mutation_permissions_require_authenticated_subject() {
        let config = DynamicConfig::default();

        assert!(TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::BrewWrite
        ));
        assert!(TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::BrewComment
        ));
        assert!(!TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::BrewWrite
        ));
        assert!(!TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::BrewComment
        ));
    }

    #[test]
    fn test_federation_permissions_are_filtered_for_users() {
        let config = DynamicConfig::default();
        let requested = vec![
            "federation:read".to_string(),
            "federation:write".to_string(),
            "federation:message".to_string(),
            "federation:files".to_string(),
            "federation:trust".to_string(),
        ];

        let granted =
            TappPermissionService::filter_permissions_for_role(&config, UserRole::User, &requested)
                .unwrap();

        assert_eq!(
            granted,
            vec![
                "federation:read",
                "federation:write",
                "federation:message",
                "federation:files"
            ]
        );
    }

    #[test]
    fn test_guest_federation_permissions_are_public_read_only() {
        let config = DynamicConfig::default();
        let requested = vec![
            "federation:read".to_string(),
            "federation:write".to_string(),
            "federation:message".to_string(),
            "federation:files".to_string(),
            "federation:trust".to_string(),
        ];

        let granted = TappPermissionService::filter_permissions_for_role(
            &config,
            UserRole::Guest,
            &requested,
        )
        .unwrap();

        assert_eq!(granted, vec!["federation:read"]);
    }

    #[test]
    fn test_federation_permissions_are_unfiltered_for_admin() {
        let config = DynamicConfig::default();
        let requested = vec![
            "federation:read".to_string(),
            "federation:write".to_string(),
            "federation:message".to_string(),
            "federation:files".to_string(),
            "federation:trust".to_string(),
        ];

        let granted = TappPermissionService::filter_permissions_for_role(
            &config,
            UserRole::Admin,
            &requested,
        )
        .unwrap();

        assert_eq!(granted, requested);
    }

    #[test]
    fn filter_permissions_rejects_unknown_name_without_partial_result() {
        let requested = vec![
            "storage:read".to_string(),
            "legacy:unknown".to_string(),
            "ui:theme:read".to_string(),
        ];

        let error = TappPermissionService::filter_permissions_for_role(
            &DynamicConfig::default(),
            UserRole::Admin,
            &requested,
        )
        .unwrap_err();

        assert_eq!(error.permission, "legacy:unknown");
        assert_eq!(error.code(), UNKNOWN_TAPP_PERMISSION_CODE);
    }

    #[test]
    fn retired_storage_error_recommends_split_permissions_but_stays_fail_closed() {
        // storage 仍不可解析：拆分后的 storage:read / storage:write 是独立权限。
        assert!(TappPermission::from_str("storage").is_none());
        assert!(TappPermission::from_str("storage:read").is_some());
        assert!(TappPermission::from_str("storage:write").is_some());

        // 真实 permission-service 过滤路径：storage 被拒，且错误提示给出替代权限。
        let error = TappPermissionService::filter_permissions_for_role(
            &DynamicConfig::default(),
            UserRole::Admin,
            &["storage".to_string()],
        )
        .unwrap_err();
        let message = error.message();
        assert!(message.contains("'storage'"), "{message}");
        assert!(message.contains("storage:read"), "{message}");
        assert!(message.contains("storage:write"), "{message}");
        assert!(message.contains("update"), "{message}");
        assert!(message.contains("reinstall"), "{message}");

        // 失败仍是 fail-closed：不放行任何权限。
        assert!(TappPermissionService::filter_permissions_for_role(
            &DynamicConfig::default(),
            UserRole::Admin,
            &["storage".to_string()],
        )
        .is_err());

        // 任意未知名保持通用错误形态，不带替代建议。
        let generic = UnknownTappPermission {
            permission: "legacy:unknown".to_string(),
        };
        assert_eq!(
            generic.message(),
            "Unknown Tapp permission 'legacy:unknown'"
        );
    }
}
