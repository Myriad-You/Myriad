//! Tapp 权限下放服务
//!
//! 基于 Tapp 系统的权限等级（PermissionLevel）管理权限下放。
//!
//! ## 权限层级（来自 Tapp 系统）
//! - **basic**: check 默认 true；游客另拒 `requires_authenticated_subject()`
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
//! - brew:write (authenticated), brew:read (guest-safe)
//! - report:read (authenticated), storage:read (guest-safe)
//! - ui:notification (authenticated), ui:fullscreen, ui:theme, ui:confirm, ui:openUrl
//! - media:read, media:control, media:audio, event:subscribe
//! - federation:read
//! - federation:message, federation:files (guest-excluded)
//! - federation:interact (authenticated), federation:ring (authenticated)
//! - game:session (guest-excluded)
//!
//! ### Elevated - 可配置下放
//! - ai:generate, ai:analyze, ai:chat, ai:image, ai:search, 3d:generate
//! - network:fetch, storage:write, component:theme (authenticated)
//! - shortcut:register (authenticated), event:publish
//! - scheduler:register, speech:tts, speech:asr (all authenticated)
//! - federation:post, federation:channel, federation:room
//! - brew:commentWrite (authenticated)
//!
//! ### Privileged - 仅管理员
//! - widget:register, platform:write, platform:register, component:agent
//! - tappList:manage, brew:manage, federation:trust
//! - **report:write**（数据报告生成，不可下放）

use crate::config::DynamicConfig;
use serde::{Deserialize, Serialize};

pub use myriad_tapp_contract::permission::{
    tapp_permission_replacement_hint, PermissionLevel, TappPermission, UnknownTappPermission,
    UserRole, UNKNOWN_TAPP_PERMISSION_CODE,
};

/// `is_admin` → Admin；`user_id <= 0` → Guest；否则 User。
pub fn role_from_user_id(user_id: i32, is_admin: bool) -> UserRole {
    if is_admin {
        UserRole::Admin
    } else if user_id < 0 {
        UserRole::Guest
    } else if user_id > 0 {
        UserRole::User
    } else {
        UserRole::Guest
    }
}

/// Tapp 权限检查服务
pub struct TappPermissionService;

impl TappPermissionService {
    /// Filter requested permission ids to the granted set for this role.
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

    /// 检查该角色是否被授予该权限
    pub fn check(config: &DynamicConfig, role: UserRole, permission: TappPermission) -> bool {
        // 出口地理位置闸门关闭时，联邦能力对**任何角色**都不授予——管理员也不例外，
        // 所以这一条必须排在下面的管理员短路之前。这只过滤授予权限：manifest 里的
        // 声明权限和安装时落库的批准权限都不动，闸门重新打开就自然恢复。
        if permission.is_federation() && !crate::services::federation_gate::federation_enabled() {
            return false;
        }

        // 闸门通过后，管理员短路为授予
        if role == UserRole::Admin {
            return true;
        }

        // 游客不授予 `requires_authenticated_subject` 的能力
        if role == UserRole::Guest && permission.requires_authenticated_subject() {
            return false;
        }

        // 游客额外拒绝 federation post/channel/room/message/files 与 game:session
        if role == UserRole::Guest
            && matches!(
                permission,
                TappPermission::FederationPost
                    | TappPermission::FederationChannel
                    | TappPermission::FederationRoom
                    | TappPermission::FederationMessage
                    | TappPermission::FederationFiles
                    | TappPermission::GameSession
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
            TappPermission::AiSearch => config.user_perm_ai_search,
            TappPermission::ThreeDGenerate => config.user_perm_3d_generate,
            TappPermission::NetworkFetch => config.user_perm_network_fetch,
            TappPermission::ComponentTheme => config.user_perm_component_theme,
            TappPermission::ShortcutRegister => config.user_perm_shortcut_register,
            TappPermission::EventPublish => config.user_perm_event_publish,
            TappPermission::SchedulerRegister => config.user_perm_scheduler_register,
            TappPermission::SpeechTts => config.user_perm_speech_tts,
            TappPermission::SpeechAsr => config.user_perm_speech_asr,
            TappPermission::StorageWrite => config.user_perm_storage_write,
            TappPermission::FederationPost => config.user_perm_federation_post,
            TappPermission::FederationChannel => config.user_perm_federation_channel,
            TappPermission::FederationRoom => config.user_perm_federation_room,
            TappPermission::BrewCommentWrite => config.user_perm_brew_comment_write,
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
            TappPermission::AiSearch => config.guest_perm_ai_search,
            TappPermission::ThreeDGenerate => config.guest_perm_3d_generate,
            TappPermission::NetworkFetch => config.guest_perm_network_fetch,
            TappPermission::ComponentTheme => false,
            TappPermission::ShortcutRegister => false,
            TappPermission::EventPublish => config.guest_perm_event_publish,
            TappPermission::SchedulerRegister => false,
            TappPermission::SpeechTts => false,
            TappPermission::SpeechAsr => false,
            TappPermission::StorageWrite => config.guest_perm_storage_write,
            // 游客写域固定 false（check() 排除块也会先返回）
            TappPermission::FederationPost => false,
            TappPermission::FederationChannel => false,
            TappPermission::FederationRoom => false,
            TappPermission::BrewCommentWrite => config.guest_perm_brew_comment_write,
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
                ai_search: config.user_perm_ai_search,
                three_d_generate: config.user_perm_3d_generate,
                report_write: false, // 不再下放
                network_fetch: config.user_perm_network_fetch,
                // media:control 为 basic，始终可用；字段保留供 API 兼容
                media_control: true,
                component_theme: config.user_perm_component_theme,
                shortcut_register: config.user_perm_shortcut_register,
                event_publish: config.user_perm_event_publish,
                scheduler_register: config.user_perm_scheduler_register,
                speech_tts: config.user_perm_speech_tts,
                speech_asr: config.user_perm_speech_asr,
                storage_write: config.user_perm_storage_write,
                federation_post: config.user_perm_federation_post,
                federation_channel: config.user_perm_federation_channel,
                federation_room: config.user_perm_federation_room,
                brew_comment_write: config.user_perm_brew_comment_write,
            },
            guest: ElevatedPermissions {
                ai_generate: config.guest_perm_ai_generate,
                ai_analyze: config.guest_perm_ai_analyze,
                ai_chat: config.guest_perm_ai_chat,
                ai_image: config.guest_perm_ai_image,
                ai_search: config.guest_perm_ai_search,
                three_d_generate: config.guest_perm_3d_generate,
                report_write: false, // 不再下放
                network_fetch: config.guest_perm_network_fetch,
                // media:control 为 basic，始终可用；字段保留供 API 兼容
                media_control: true,
                // These routes require a durable authenticated subject. Keep
                // config fields for schema compatibility; grant path is hardcoded false.
                component_theme: false,
                shortcut_register: false,
                event_publish: config.guest_perm_event_publish,
                scheduler_register: false,
                speech_tts: false,
                speech_asr: false,
                storage_write: config.guest_perm_storage_write,
                // federation 写域不向游客下放：固定 false
                federation_post: false,
                federation_channel: false,
                federation_room: false,
                // brew:commentWrite 路由要求持久登录主体，游客一律关闭
                brew_comment_write: false,
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

/// Elevated 级别权限配置（report:write 为 privileged，不可下放）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevatedPermissions {
    pub ai_generate: bool,
    pub ai_analyze: bool,
    pub ai_chat: bool,
    pub ai_image: bool,
    #[serde(default)]
    pub ai_search: bool,
    #[serde(default)]
    pub three_d_generate: bool,
    /// 兼容字段；`get_permission_config` 写 false
    #[serde(default)]
    pub report_write: bool,
    pub network_fetch: bool,
    /// 保留字段：media:control 为 basic，摘要中始终为 true
    #[serde(default)]
    pub media_control: bool,
    pub component_theme: bool,
    pub shortcut_register: bool,
    pub event_publish: bool,
    pub scheduler_register: bool,
    pub speech_tts: bool,
    pub speech_asr: bool,
    pub storage_write: bool,
    pub federation_post: bool,
    pub federation_channel: bool,
    pub federation_room: bool,
    /// brew:commentWrite - 写 Brew 评论（Elevated，需登录主体）
    pub brew_comment_write: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_from_user_id_maps_admin_user_and_guest() {
        assert_eq!(role_from_user_id(1, true), UserRole::Admin);
        assert_eq!(role_from_user_id(0, true), UserRole::Admin);
        assert_eq!(role_from_user_id(7, false), UserRole::User);
        assert_eq!(role_from_user_id(-1, false), UserRole::Guest);
        assert_eq!(role_from_user_id(0, false), UserRole::Guest);
    }

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
        assert_eq!(
            TappPermission::StorageWrite.level(),
            PermissionLevel::Elevated
        );
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
        // media:control is basic: always allowed regardless of user/guest_perm_media_control
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
            "brew:commentWrite".to_string(),
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
        // Still excluded: brew:write / brew:commentWrite,
        // report:read, notifications, speech, etc.
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
        assert!(!TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::ThreeDGenerate
        ));
        assert!(!TappPermissionService::check(
            &config,
            UserRole::Guest,
            TappPermission::ThreeDGenerate
        ));
    }

    #[test]
    fn three_d_generate_round_trips_and_delegates_only_when_enabled() {
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

        let defaults = DynamicConfig::default();
        assert!(!defaults.user_perm_3d_generate);
        assert!(!defaults.guest_perm_3d_generate);
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::ThreeDGenerate
        ));

        let delegated = DynamicConfig {
            user_perm_3d_generate: true,
            guest_perm_3d_generate: true,
            ..DynamicConfig::default()
        };
        assert!(TappPermissionService::check(
            &delegated,
            UserRole::User,
            TappPermission::ThreeDGenerate
        ));
        assert!(TappPermissionService::check(
            &delegated,
            UserRole::Guest,
            TappPermission::ThreeDGenerate
        ));
        let effective = TappPermissionService::get_permission_config(&delegated);
        assert!(effective.user.three_d_generate);
        assert!(effective.guest.three_d_generate);
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
            "ui:theme".to_string(),
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
            Ok(vec!["storage:read".to_string(), "ui:theme".to_string()])
        );
        assert_eq!(
            TappPermissionService::filter_permissions_for_role(
                &config,
                UserRole::Guest,
                &requested,
            ),
            Ok(vec!["storage:read".to_string(), "ui:theme".to_string()])
        );
    }

    #[test]
    fn brew_permissions_keep_write_and_add_comment_write() {
        let defaults = DynamicConfig::default();

        // brew:write remains Basic and requires a durable login; commentWrite is Elevated.
        assert_eq!(TappPermission::BrewWrite.level(), PermissionLevel::Basic);
        assert_eq!(
            TappPermission::BrewCommentWrite.level(),
            PermissionLevel::Elevated
        );

        assert!(TappPermission::from_str("brew:write").is_some());
        assert!(TappPermission::from_str("brew:comment").is_none());

        // brew:write requires a durable login: user can, guest cannot.
        assert!(TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::BrewWrite
        ));
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::Guest,
            TappPermission::BrewWrite
        ));

        // commentWrite 默认不下放：user/guest 均不可
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::BrewCommentWrite
        ));
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::Guest,
            TappPermission::BrewCommentWrite
        ));

        // 显式下放后 user 可用；guest 仍受认证主体约束
        let delegated = DynamicConfig {
            user_perm_brew_comment_write: true,
            guest_perm_brew_comment_write: true,
            ..DynamicConfig::default()
        };
        assert!(TappPermissionService::check(
            &delegated,
            UserRole::User,
            TappPermission::BrewCommentWrite
        ));
        assert!(!TappPermissionService::check(
            &delegated,
            UserRole::Guest,
            TappPermission::BrewCommentWrite
        ));

        // 摘要中 guest 的 commentWrite 恒为关闭
        let effective = TappPermissionService::get_permission_config(&defaults);
        assert!(!effective.guest.brew_comment_write);
        let effective = TappPermissionService::get_permission_config(&delegated);
        assert!(effective.user.brew_comment_write);
        assert!(!effective.guest.brew_comment_write);
    }

    #[test]
    fn brew_write_and_comment_write_are_independent() {
        let config = DynamicConfig::default();
        let granted = TappPermissionService::filter_permissions_for_role(
            &config,
            UserRole::User,
            &["brew:write".to_string()],
        )
        .unwrap();
        assert_eq!(granted, vec!["brew:write"]);

        let granted = TappPermissionService::filter_permissions_for_role(
            &config,
            UserRole::User,
            &["brew:read".to_string()],
        )
        .unwrap();
        assert_eq!(granted, vec!["brew:read"]);
        assert!(!TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::BrewCommentWrite
        ));
    }

    #[test]
    fn brew_comment_write_requires_explicit_declaration_after_delegation() {
        // 即使 commentWrite 已下放，批准集里只有 brew:read 也不会进入授予集合
        let config = DynamicConfig {
            user_perm_brew_comment_write: true,
            ..DynamicConfig::default()
        };
        let granted = TappPermissionService::filter_permissions_for_role(
            &config,
            UserRole::User,
            &["brew:read".to_string()],
        )
        .unwrap();
        assert_eq!(granted, vec!["brew:read"]);

        // 批准集含 commentWrite 才进入授予集合
        let granted = TappPermissionService::filter_permissions_for_role(
            &config,
            UserRole::User,
            &["brew:read".to_string(), "brew:commentWrite".to_string()],
        )
        .unwrap();
        assert_eq!(granted, vec!["brew:read", "brew:commentWrite"]);
    }

    #[test]
    fn removed_brew_permission_names_are_rejected_explicitly() {
        let error = TappPermissionService::filter_permissions_for_role(
            &DynamicConfig::default(),
            UserRole::Admin,
            &["brew:comment".to_string()],
        )
        .unwrap_err();
        assert_eq!(error.permission, "brew:comment");
        assert_eq!(error.code(), UNKNOWN_TAPP_PERMISSION_CODE);
    }

    #[test]
    fn removed_brew_permission_rejections_list_replacements() {
        // brew:comment → brew:read + brew:commentWrite
        let error = TappPermissionService::filter_permissions_for_role(
            &DynamicConfig::default(),
            UserRole::Admin,
            &["brew:comment".to_string()],
        )
        .unwrap_err();
        let message = error.message();
        assert!(message.contains("'brew:comment'"), "{message}");
        assert!(message.contains("brew:read"), "{message}");
        assert!(message.contains("brew:commentWrite"), "{message}");

        // 未知名的提示不改变通用消息形态
        let generic = UnknownTappPermission {
            permission: "legacy:unknown".to_string(),
        };
        assert_eq!(
            generic.message(),
            "Unknown Tapp permission 'legacy:unknown'"
        );
    }

    #[test]
    fn test_federation_permissions_are_filtered_for_users() {
        let config = DynamicConfig::default();
        let requested = vec![
            "federation:read".to_string(),
            "federation:post".to_string(),
            "federation:interact".to_string(),
            "federation:channel".to_string(),
            "federation:room".to_string(),
            "federation:ring".to_string(),
            "federation:message".to_string(),
            "federation:files".to_string(),
            "federation:trust".to_string(),
        ];

        let granted =
            TappPermissionService::filter_permissions_for_role(&config, UserRole::User, &requested)
                .unwrap();

        // post/channel/room 是 Elevated，默认不授予普通用户；interact/ring 是
        // Basic 但要求持久登录主体（user 满足）。trust 是 Privileged。
        assert_eq!(
            granted,
            vec![
                "federation:read",
                "federation:interact",
                "federation:ring",
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
            "federation:post".to_string(),
            "federation:interact".to_string(),
            "federation:channel".to_string(),
            "federation:room".to_string(),
            "federation:ring".to_string(),
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

        // 游客严格只读：interact/ring 需持久登录主体；post/channel/room/message/
        // files 被 guest 排除块拦截；trust 仅管理员。
        assert_eq!(granted, vec!["federation:read"]);
    }

    #[test]
    fn test_federation_permissions_are_unfiltered_for_admin() {
        let config = DynamicConfig::default();
        let requested = vec![
            "federation:read".to_string(),
            "federation:post".to_string(),
            "federation:interact".to_string(),
            "federation:channel".to_string(),
            "federation:room".to_string(),
            "federation:ring".to_string(),
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
    fn federation_split_levels_and_delegation_defaults() {
        let defaults = DynamicConfig::default();
        // 写类 Elevated，互动/ring Basic。
        assert_eq!(
            TappPermission::FederationPost.level(),
            PermissionLevel::Elevated
        );
        assert_eq!(
            TappPermission::FederationChannel.level(),
            PermissionLevel::Elevated
        );
        assert_eq!(
            TappPermission::FederationRoom.level(),
            PermissionLevel::Elevated
        );
        assert_eq!(
            TappPermission::FederationInteract.level(),
            PermissionLevel::Basic
        );
        assert_eq!(
            TappPermission::FederationRing.level(),
            PermissionLevel::Basic
        );

        // `TappPermission::from_str("federation:write")` 为 `None`。
        assert!(TappPermission::from_str("federation:write").is_none());
        assert!(TappPermission::from_str("federation:post").is_some());
        assert!(TappPermission::from_str("federation:interact").is_some());
        assert!(TappPermission::from_str("federation:channel").is_some());
        assert!(TappPermission::from_str("federation:room").is_some());
        assert!(TappPermission::from_str("federation:ring").is_some());

        // 三个 Elevated 权限进入可配置下放集合。
        let elevated = TappPermission::all_elevated();
        assert!(elevated.contains(&TappPermission::FederationPost));
        assert!(elevated.contains(&TappPermission::FederationChannel));
        assert!(elevated.contains(&TappPermission::FederationRoom));
        assert!(!elevated.contains(&TappPermission::FederationInteract));
        assert!(!elevated.contains(&TappPermission::FederationRing));

        // 默认全部关闭：普通用户拿不到写类。
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::FederationPost
        ));
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::FederationChannel
        ));
        assert!(!TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::FederationRoom
        ));
        // Basic：普通用户默认持有 interact/ring。
        assert!(TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::FederationInteract
        ));
        assert!(TappPermissionService::check(
            &defaults,
            UserRole::User,
            TappPermission::FederationRing
        ));

        // 游客永不持有任何 federation 写类/互动权限（严格只读）。
        for perm in [
            TappPermission::FederationPost,
            TappPermission::FederationInteract,
            TappPermission::FederationChannel,
            TappPermission::FederationRoom,
            TappPermission::FederationRing,
        ] {
            assert!(!TappPermissionService::check(
                &defaults,
                UserRole::Guest,
                perm
            ));
        }
    }

    #[test]
    fn federation_elevated_delegation_grants_user_but_never_guest() {
        let delegated = DynamicConfig {
            user_perm_federation_post: true,
            user_perm_federation_channel: true,
            user_perm_federation_room: true,
            guest_perm_federation_post: true, // 配置即使开启也无效
            guest_perm_federation_channel: true,
            guest_perm_federation_room: true,
            ..DynamicConfig::default()
        };
        assert!(TappPermissionService::check(
            &delegated,
            UserRole::User,
            TappPermission::FederationPost
        ));
        assert!(TappPermissionService::check(
            &delegated,
            UserRole::User,
            TappPermission::FederationChannel
        ));
        assert!(TappPermissionService::check(
            &delegated,
            UserRole::User,
            TappPermission::FederationRoom
        ));
        // 游客严格只读：下放配置被 guest 排除块拦截。
        assert!(!TappPermissionService::check(
            &delegated,
            UserRole::Guest,
            TappPermission::FederationPost
        ));
        assert!(!TappPermissionService::check(
            &delegated,
            UserRole::Guest,
            TappPermission::FederationChannel
        ));
        assert!(!TappPermissionService::check(
            &delegated,
            UserRole::Guest,
            TappPermission::FederationRoom
        ));

        // 摘要带回 user 侧下放值；游客写域固定 false。
        let effective = TappPermissionService::get_permission_config(&delegated);
        assert!(effective.user.federation_post);
        assert!(effective.user.federation_channel);
        assert!(effective.user.federation_room);
        assert!(!effective.guest.federation_post);
        assert!(!effective.guest.federation_channel);
        assert!(!effective.guest.federation_room);
    }

    #[test]
    fn federation_basic_grants_cannot_reach_elevated_domains() {
        // 默认不下放时，请求里的 Elevated 联邦权限不会进入授予集。
        let config = DynamicConfig::default();
        let requested = vec![
            "federation:interact".to_string(),
            "federation:ring".to_string(),
            "federation:post".to_string(),
            "federation:channel".to_string(),
            "federation:room".to_string(),
        ];

        let granted =
            TappPermissionService::filter_permissions_for_role(&config, UserRole::User, &requested)
                .unwrap();
        assert!(granted.contains(&"federation:interact".to_string()));
        assert!(granted.contains(&"federation:ring".to_string()));
        assert!(!granted.contains(&"federation:post".to_string()));
        assert!(!granted.contains(&"federation:channel".to_string()));
        assert!(!granted.contains(&"federation:room".to_string()));

        // 过滤结果是请求与已下放授予的交集，不会补上未请求的 interact/ring。
        let elevated_config = DynamicConfig {
            user_perm_federation_post: true,
            user_perm_federation_channel: true,
            user_perm_federation_room: true,
            ..DynamicConfig::default()
        };
        let requested_elevated = vec![
            "federation:post".to_string(),
            "federation:channel".to_string(),
            "federation:room".to_string(),
        ];
        let granted_elevated = TappPermissionService::filter_permissions_for_role(
            &elevated_config,
            UserRole::User,
            &requested_elevated,
        )
        .unwrap();
        assert_eq!(
            granted_elevated,
            vec![
                "federation:post".to_string(),
                "federation:channel".to_string(),
                "federation:room".to_string()
            ]
        );
    }

    #[test]
    fn legacy_federation_write_fails_explicitly_with_replacements() {
        let requested = vec![
            "federation:read".to_string(),
            "federation:write".to_string(),
            "federation:message".to_string(),
        ];

        let error = TappPermissionService::filter_permissions_for_role(
            &DynamicConfig::default(),
            UserRole::Admin,
            &requested,
        )
        .unwrap_err();

        assert_eq!(error.permission, "federation:write");
        assert_eq!(error.code(), UNKNOWN_TAPP_PERMISSION_CODE);
        let message = error.message();
        for replacement in [
            "federation:post",
            "federation:interact",
            "federation:channel",
            "federation:room",
            "federation:ring",
        ] {
            assert!(message.contains(replacement), "{message}");
        }
    }

    #[test]
    fn filter_permissions_rejects_unknown_name_without_partial_result() {
        let requested = vec![
            "storage:read".to_string(),
            "legacy:unknown".to_string(),
            "ui:theme".to_string(),
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
