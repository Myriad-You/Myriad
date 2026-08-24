//! 名称 / 简介（展示文案）来源 —— 与 [`crate::services::avatar`] 画像源**独立**。
//!
//! ## 为什么单独一套
//!
//! 产品规则「对齐，但是分开管理」：
//! - **对齐**：名称/简介按用户选定的文案来源解析，不再死用 `PLATFORM_ORDER` 第一个
//!   有数据的平台（否则脸换成 GitHub，文案仍是 B 站）。
//! - **分开管理**：`users.profile_text_source_*` 与 `users.avatar_source_*` 互不影响；
//!   可以脸用 GitHub、简介用 B 站，或脸用账号、名字用 GitHub 抓取。
//!
//! ## 存储
//!
//! | 列 | 含义 |
//! |----|------|
//! | `profile_text_source_kind` | `auto` / `account` / `identity` / `platform`（NULL→auto） |
//! | `profile_text_source_ref` | identity id 或平台键；auto/account 为空 |
//!
//! 不写解析快照：文案来自 JSON / 用户列，实时解析即可。

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value as SeaValue};
use serde_json::{json, Value};

use crate::services::avatar::{
    identity_provider_platform_key, owner_platform_profiles, platform_display_label,
    PlatformProfile, PLATFORM_ORDER,
};

const LAZY_BIO: &str = "这家伙很懒，没有介绍呢";

/// 文案来源类型。与画像源同形，但语义独立，切勿混用列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileTextSourceKind {
    Auto,
    Account,
    Identity,
    Platform,
}

impl ProfileTextSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Account => "account",
            Self::Identity => "identity",
            Self::Platform => "platform",
        }
    }

    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).unwrap_or_default() {
            "account" => Self::Account,
            "identity" => Self::Identity,
            "platform" => Self::Platform,
            _ => Self::Auto,
        }
    }
}

/// 解析后的展示文案（首页信息条 / SEO / user-info）。
#[derive(Debug, Clone)]
pub struct ResolvedProfileText {
    pub name: Option<String>,
    pub bio: String,
    /// 平台展示标签（"GitHub" / "Bilibili"…）；账号来源为 `None`。
    pub platform: Option<String>,
    /// 数据出处标记：`platform` / `account` / `identity` / `none`
    pub source: &'static str,
}

/// 选择器里的一行可选文案来源。
#[derive(Debug, Clone)]
pub struct ProfileTextSource {
    pub kind: ProfileTextSourceKind,
    pub source_ref: String,
    pub label: String,
    pub sublabel: Option<String>,
    /// 预览名（可选，给前端展示）
    pub preview_name: Option<String>,
    /// 预览简介截断前原文
    pub preview_bio: Option<String>,
    pub is_current: bool,
}

impl ProfileTextSource {
    pub fn to_json(&self) -> Value {
        json!({
            "kind": self.kind.as_str(),
            "ref": self.source_ref,
            "label": self.label,
            "sublabel": self.sublabel,
            "preview_name": self.preview_name,
            "preview_bio": self.preview_bio,
            "is_current": self.is_current,
        })
    }
}

struct UserTextRow {
    kind: ProfileTextSourceKind,
    source_ref: Option<String>,
    display_name: Option<String>,
    username: Option<String>,
    bio: Option<String>,
    is_owner: bool,
}

async fn load_user_text_row(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Option<UserTextRow>, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT profile_text_source_kind, profile_text_source_ref, \
                    display_name, username, bio, COALESCE(is_owner, false) AS is_owner \
             FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| format!("Failed to load profile text row: {e}"))?;

    Ok(row.map(|row| UserTextRow {
        kind: ProfileTextSourceKind::parse(
            row.try_get::<Option<String>>("", "profile_text_source_kind")
                .ok()
                .flatten()
                .as_deref(),
        ),
        source_ref: row
            .try_get::<Option<String>>("", "profile_text_source_ref")
            .ok()
            .flatten(),
        display_name: row
            .try_get::<Option<String>>("", "display_name")
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        username: row
            .try_get::<Option<String>>("", "username")
            .ok()
            .flatten()
            .or_else(|| row.try_get::<String>("", "username").ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        bio: row
            .try_get::<Option<String>>("", "bio")
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        is_owner: row.try_get::<bool>("", "is_owner").unwrap_or(false),
    }))
}

fn account_name(row: &UserTextRow) -> Option<String> {
    row.display_name.clone().or_else(|| row.username.clone())
}

fn account_bio(row: &UserTextRow) -> String {
    row.bio.clone().unwrap_or_else(|| LAZY_BIO.to_string())
}

fn account_resolved(row: &UserTextRow) -> ResolvedProfileText {
    ResolvedProfileText {
        name: account_name(row),
        bio: account_bio(row),
        platform: None,
        source: "account",
    }
}

fn platform_resolved(profile: &PlatformProfile) -> ResolvedProfileText {
    ResolvedProfileText {
        name: profile.name.clone(),
        bio: if profile.bio.trim().is_empty() {
            LAZY_BIO.to_string()
        } else {
            profile.bio.clone()
        },
        platform: Some(profile.platform.to_string()),
        source: "platform",
    }
}

/// auto：站长按 [`PLATFORM_ORDER`] 取第一个已解析出的平台（与历史 user-info 一致）；
/// 无平台数据或普通用户 → 账号 display_name/username + bio。
fn auto_resolved(row: &UserTextRow, profiles: &[(String, PlatformProfile)]) -> ResolvedProfileText {
    if row.is_owner {
        if let Some((_, profile)) = profiles.first() {
            return platform_resolved(profile);
        }
    }
    account_resolved(row)
}

struct IdentityText {
    id: i32,
    provider: String,
    username: Option<String>,
}

async fn load_identities(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<IdentityText>, String> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, provider, provider_username \
             FROM user_identities WHERE user_id = $1 ORDER BY linked_at ASC",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| format!("Failed to list identities: {e}"))?;

    let mut out = Vec::new();
    for row in rows {
        let provider = row.try_get::<String>("", "provider").unwrap_or_default();
        if provider.is_empty() {
            continue;
        }
        out.push(IdentityText {
            id: row.try_get::<i32>("", "id").unwrap_or(0),
            provider,
            username: row
                .try_get::<Option<String>>("", "provider_username")
                .ok()
                .flatten()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        });
    }
    Ok(out)
}

fn identity_resolved(identity: &IdentityText) -> ResolvedProfileText {
    ResolvedProfileText {
        name: identity.username.clone(),
        bio: LAZY_BIO.to_string(),
        platform: Some(platform_display_label(
            identity_provider_platform_key(&identity.provider)
                .unwrap_or(identity.provider.as_str()),
        )),
        source: "identity",
    }
}

/// 按用户选定的文案来源解析 name / bio / platform 标签。
pub async fn resolve_profile_text(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<ResolvedProfileText, String> {
    let row = load_user_text_row(db, user_id)
        .await?
        .ok_or_else(|| "User not found".to_string())?;
    let current_ref = row.source_ref.clone().unwrap_or_default();
    let profiles = if row.is_owner {
        owner_platform_profiles(db, user_id).await
    } else {
        Vec::new()
    };
    let identities = load_identities(db, user_id).await?;

    let selected = match row.kind {
        ProfileTextSourceKind::Auto => Some(auto_resolved(&row, &profiles)),
        ProfileTextSourceKind::Account => Some(account_resolved(&row)),
        ProfileTextSourceKind::Platform => {
            let want = current_ref.as_str();
            profiles
                .iter()
                .find(|(k, _)| k == want)
                .map(|(_, p)| platform_resolved(p))
        }
        ProfileTextSourceKind::Identity => {
            let id: Option<i32> = current_ref.parse().ok();
            identities
                .iter()
                .find(|i| Some(i.id) == id)
                .map(identity_resolved)
                .or_else(|| {
                    // 同站合并：库里可能仍是 identity，但列表以 platform 展示；
                    // 若该 identity 映射的平台有抓取，优先用平台 bio（更完整）
                    let identity = identities.iter().find(|i| Some(i.id) == id)?;
                    let key = identity_provider_platform_key(&identity.provider)?;
                    profiles
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, p)| platform_resolved(p))
                        .or_else(|| Some(identity_resolved(identity)))
                })
        }
    };

    // 选中源失效（平台清了、identity 解绑）→ auto 阶梯，而不是空白
    Ok(selected.unwrap_or_else(|| auto_resolved(&row, &profiles)))
}

/// 列出可选文案来源（同站 identity+platform 合并为一行，kind=platform）。
pub async fn list_profile_text_sources(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<ProfileTextSource>, String> {
    let row = load_user_text_row(db, user_id)
        .await?
        .ok_or_else(|| "User not found".to_string())?;
    let current_ref = row.source_ref.clone().unwrap_or_default();
    let profiles = if row.is_owner {
        owner_platform_profiles(db, user_id).await
    } else {
        Vec::new()
    };
    let identities = load_identities(db, user_id).await?;

    let mut sources = Vec::new();

    sources.push(ProfileTextSource {
        kind: ProfileTextSourceKind::Account,
        source_ref: String::new(),
        label: "account".to_string(),
        sublabel: account_name(&row),
        preview_name: account_name(&row),
        preview_bio: Some(account_bio(&row)),
        is_current: row.kind == ProfileTextSourceKind::Account,
    });

    let mut consumed_platforms = std::collections::HashMap::<&str, bool>::new();

    for identity in &identities {
        let id_str = identity.id.to_string();
        let identity_is_current =
            row.kind == ProfileTextSourceKind::Identity && current_ref == id_str;

        if let Some(platform_key) = identity_provider_platform_key(&identity.provider) {
            if let Some((_, profile)) = profiles.iter().find(|(k, _)| k == platform_key) {
                consumed_platforms.insert(platform_key, true);
                let platform_is_current =
                    row.kind == ProfileTextSourceKind::Platform && current_ref == platform_key;
                let resolved = platform_resolved(profile);
                sources.push(ProfileTextSource {
                    kind: ProfileTextSourceKind::Platform,
                    source_ref: platform_key.to_string(),
                    label: platform_display_label(platform_key),
                    sublabel: resolved.name.clone().or_else(|| identity.username.clone()),
                    preview_name: resolved.name.clone(),
                    preview_bio: Some(resolved.bio),
                    is_current: platform_is_current || identity_is_current,
                });
                continue;
            }
        }

        let resolved = identity_resolved(identity);
        sources.push(ProfileTextSource {
            kind: ProfileTextSourceKind::Identity,
            source_ref: id_str,
            label: identity.provider.clone(),
            sublabel: identity.username.clone(),
            preview_name: resolved.name,
            preview_bio: Some(resolved.bio),
            is_current: identity_is_current,
        });
    }

    for platform_key in PLATFORM_ORDER {
        if consumed_platforms.contains_key(platform_key) {
            continue;
        }
        let Some((_, profile)) = profiles.iter().find(|(k, _)| k == platform_key) else {
            continue;
        };
        let resolved = platform_resolved(profile);
        sources.push(ProfileTextSource {
            kind: ProfileTextSourceKind::Platform,
            source_ref: platform_key.to_string(),
            label: profile.platform.to_string(),
            sublabel: profile.name.clone(),
            preview_name: resolved.name,
            preview_bio: Some(resolved.bio),
            is_current: row.kind == ProfileTextSourceKind::Platform && current_ref == platform_key,
        });
    }

    Ok(sources)
}

pub async fn current_profile_text_source(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<(ProfileTextSourceKind, Option<String>), String> {
    let row = load_user_text_row(db, user_id)
        .await?
        .ok_or_else(|| "User not found".to_string())?;
    Ok((row.kind, row.source_ref))
}

/// 写入文案来源选择。校验：来源必须属于该用户（与 set_avatar_source 同原则）。
pub async fn set_profile_text_source(
    db: &DatabaseConnection,
    user_id: i32,
    kind: ProfileTextSourceKind,
    source_ref: Option<&str>,
) -> Result<ResolvedProfileText, String> {
    let source_ref = source_ref.map(str::trim).filter(|s| !s.is_empty());

    let stored_ref: Option<String> = match kind {
        ProfileTextSourceKind::Auto | ProfileTextSourceKind::Account => None,
        ProfileTextSourceKind::Identity => {
            let raw = source_ref.ok_or_else(|| "identity source requires a ref".to_string())?;
            let identity_id: i32 = raw
                .parse()
                .map_err(|_| "identity ref must be an identity id".to_string())?;
            let identities = load_identities(db, user_id).await?;
            if !identities.iter().any(|i| i.id == identity_id) {
                return Err("Identity not found for this user".to_string());
            }
            Some(identity_id.to_string())
        }
        ProfileTextSourceKind::Platform => {
            let platform =
                source_ref.ok_or_else(|| "platform source requires a ref".to_string())?;
            // Platform text source is site-owner only. Gate on is_owner before profiles
            // so disk-cache fallback cannot validate platform refs for non-owners.
            let row = load_user_text_row(db, user_id)
                .await?
                .ok_or_else(|| "User not found".to_string())?;
            if !row.is_owner {
                return Err(
                    "Platform profile text source is only available for the site owner".to_string(),
                );
            }
            let available = owner_platform_profiles(db, user_id).await;
            if !available.iter().any(|(name, _)| name == platform) {
                return Err("Platform profile is not available for this user".to_string());
            }
            Some(platform.to_string())
        }
    };

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE users SET profile_text_source_kind = $1, profile_text_source_ref = $2, \
         updated_at = NOW() WHERE id = $3",
        vec![
            SeaValue::String(Some(kind.as_str().to_string())),
            match stored_ref {
                Some(r) => SeaValue::String(Some(r)),
                None => SeaValue::String(None),
            },
            SeaValue::Int(Some(user_id)),
        ],
    ))
    .await
    .map_err(|e| format!("Failed to save profile text source: {e}"))?;

    resolve_profile_text(db, user_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_profiles() -> Vec<(String, PlatformProfile)> {
        vec![
            (
                "bilibili".into(),
                PlatformProfile {
                    platform: "Bilibili",
                    name: Some("阿绫".into()),
                    avatar: None,
                    bio: "B站签名".into(),
                },
            ),
            (
                "github".into(),
                PlatformProfile {
                    platform: "GitHub",
                    name: Some("octocat".into()),
                    avatar: None,
                    bio: "GitHub bio".into(),
                },
            ),
        ]
    }

    #[test]
    fn source_kind_round_trips() {
        for kind in [
            ProfileTextSourceKind::Auto,
            ProfileTextSourceKind::Account,
            ProfileTextSourceKind::Identity,
            ProfileTextSourceKind::Platform,
        ] {
            assert_eq!(ProfileTextSourceKind::parse(Some(kind.as_str())), kind);
        }
        assert_eq!(
            ProfileTextSourceKind::parse(None),
            ProfileTextSourceKind::Auto
        );
        assert_eq!(
            ProfileTextSourceKind::parse(Some("nope")),
            ProfileTextSourceKind::Auto
        );
    }

    #[test]
    fn auto_picks_first_platform_in_order_for_owner() {
        let row = UserTextRow {
            kind: ProfileTextSourceKind::Auto,
            source_ref: None,
            display_name: Some("Account".into()),
            username: Some("acct".into()),
            bio: Some("account bio".into()),
            is_owner: true,
        };
        let resolved = auto_resolved(&row, &sample_profiles());
        assert_eq!(resolved.name.as_deref(), Some("阿绫"));
        assert_eq!(resolved.bio, "B站签名");
        assert_eq!(resolved.platform.as_deref(), Some("Bilibili"));
        assert_eq!(resolved.source, "platform");
    }

    #[test]
    fn auto_falls_back_to_account_for_non_owner() {
        let row = UserTextRow {
            kind: ProfileTextSourceKind::Auto,
            source_ref: None,
            display_name: Some("Local".into()),
            username: Some("local".into()),
            bio: Some("hi".into()),
            is_owner: false,
        };
        // even if profiles are passed, non-owner auto ignores them
        let resolved = auto_resolved(&row, &sample_profiles());
        assert_eq!(resolved.name.as_deref(), Some("Local"));
        assert_eq!(resolved.bio, "hi");
        assert!(resolved.platform.is_none());
        assert_eq!(resolved.source, "account");
    }

    #[test]
    fn platform_resolved_keeps_chosen_platform_not_first() {
        let profiles = sample_profiles();
        let gh = &profiles[1].1;
        let resolved = platform_resolved(gh);
        assert_eq!(resolved.name.as_deref(), Some("octocat"));
        assert_eq!(resolved.bio, "GitHub bio");
        assert_eq!(resolved.platform.as_deref(), Some("GitHub"));
    }

    #[test]
    fn account_prefers_display_name_over_username() {
        let row = UserTextRow {
            kind: ProfileTextSourceKind::Account,
            source_ref: None,
            display_name: Some("Pretty".into()),
            username: Some("raw".into()),
            bio: None,
            is_owner: false,
        };
        let resolved = account_resolved(&row);
        assert_eq!(resolved.name.as_deref(), Some("Pretty"));
        assert_eq!(resolved.bio, LAZY_BIO);
    }

    #[test]
    fn platform_disk_cache_gate_matches_avatar_module() {
        // Same rule as avatar::allow_platform_disk_cache_for_user — non-owners
        // must never validate platform sources via the site-owner cache.
        use crate::services::avatar::allow_platform_disk_cache_for_user;
        assert!(allow_platform_disk_cache_for_user(true));
        assert!(!allow_platform_disk_cache_for_user(false));
    }
}
