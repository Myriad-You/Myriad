//! What the site shows its guests: its branding, which modules are open to
//! them, and the public apps, writing and notes, with absolute addresses.
//! robots.txt, llms.txt, the sitemap and the SEO pages are rendered from it
//! in `api::seo`, and the agent's SEO work reads it to know what exists.

use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Statement,
};
use serde_json::{Value, json};

use crate::models::entities::{phantasi_items, phantasi_sources, tapps};
use crate::services::tapp_ownership::{find_admin_user_id, public_install_visible_to_viewer};
use myriad_module_visibility::load_module_visibility_preferences;

/// Fixed DB category label for site-owner original Phantasi content.
/// Must match frontend `PHANTASI_MINE_CATEGORY` (`frontend/src/components/phantasi/constants.ts`).
///
/// `api::phantasi::notes` writes this value only when creating a notes source. Sitemap still requires `phantasi_source_is_own`; the notes board keeps `source_type = note` even if category is no longer `我`.
pub(crate) const PHANTASI_MINE_CATEGORY: &str = "我";

/// Links on the Phantasi list crawler shell.
pub(crate) const PHANTASI_LIST_SHELL_LIMIT: u64 = 30;
/// Fallback bio from `profile_text` — guests see it, crawlers should not.

// ── types ───────────────────────────────────────────────────────────────────

pub(crate) struct SiteBranding {
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) favicon: String,
    pub(crate) og_image: String,
    pub(crate) noindex: bool,
    pub(crate) policy: String,
    pub(crate) ai_intro: String,
    pub(crate) keywords: String,
    pub(crate) google_site_verification: String,
}

/// Absolute URL when a durable origin is configured; otherwise the path only
/// (relative — no host poisoning surface).
pub(crate) fn public_absolute_url(base: Option<&str>, path: &str) -> String {
    match base {
        Some(b) if !b.is_empty() => format!("{b}{path}"),
        _ => path.to_string(),
    }
}

pub(crate) fn module_is_public_all(level: &str) -> bool {
    level == "all"
}

pub(crate) fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

pub(crate) async fn site_branding_copy(db: &DatabaseConnection) -> (String, String, String) {
    let branding = load_site_branding(db).await;
    (branding.title, branding.description, branding.ai_intro)
}

pub(crate) async fn load_site_branding(db: &DatabaseConnection) -> SiteBranding {
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    let branding = |db_val: Option<String>, env_key: &str, default: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var(env_key).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| default.to_string())
    };
    let clearable = |db_val: Option<String>, env_key: &str| -> String {
        if let Some(v) = db_val {
            return v;
        }
        std::env::var(env_key).unwrap_or_default()
    };

    let noindex_flag = db_config
        .as_ref()
        .map(|c| c.site_noindex)
        .unwrap_or_else(|| {
            std::env::var("SITE_NOINDEX")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
        });

    let policy_raw = db_config
        .as_ref()
        .map(|c| c.site_visibility_policy.clone())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("SITE_VISIBILITY_POLICY").ok())
        .unwrap_or_default();
    let policy =
        crate::api::seo_policy::normalize_visibility_policy(&policy_raw, noindex_flag).to_string();
    let noindex = noindex_flag || !crate::api::seo_policy::policy_is_indexable(&policy);

    SiteBranding {
        title: branding(
            db_config.as_ref().and_then(|c| c.site_title.clone()),
            "SITE_TITLE",
            "Myriad - A myriad of lights, in one place.",
        ),
        description: branding(
            db_config.as_ref().and_then(|c| c.site_description.clone()),
            "SITE_DESCRIPTION",
            "A myriad of lights, in one place.",
        ),
        favicon: branding(
            db_config.as_ref().and_then(|c| c.site_favicon.clone()),
            "SITE_FAVICON",
            "/favicon.webp",
        ),
        og_image: clearable(
            db_config.as_ref().and_then(|c| c.site_og_image.clone()),
            "SITE_OG_IMAGE",
        ),
        noindex,
        policy,
        ai_intro: clearable(
            db_config.as_ref().and_then(|c| c.site_ai_intro.clone()),
            "SITE_AI_INTRO",
        ),
        keywords: clearable(
            db_config.as_ref().and_then(|c| c.site_keywords.clone()),
            "SITE_KEYWORDS",
        ),
        google_site_verification: clearable(
            db_config
                .as_ref()
                .and_then(|c| c.google_site_verification.clone()),
            "GOOGLE_SITE_VERIFICATION",
        ),
    }
}

/// Whether a Phantasi source is site-owner original content.
/// Friend links and third-party feeds must never be treated as own content.
pub(crate) fn phantasi_source_is_own(
    source_type: &phantasi_sources::SourceType,
    category: &Option<String>,
    admin_only: bool,
) -> bool {
    if admin_only {
        return false;
    }
    if *source_type == phantasi_sources::SourceType::Note {
        return true;
    }
    category
        .as_ref()
        .map(|c| {
            c.split(',')
                .map(str::trim)
                .any(|part| part == PHANTASI_MINE_CATEGORY)
        })
        .unwrap_or(false)
}

pub(crate) fn strip_html_snippet(raw: &str, max_len: usize) -> String {
    let mut plain = String::with_capacity(raw.len().min(max_len * 2));
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(ch),
            _ => {}
        }
    }
    let plain = plain
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if plain.chars().count() <= max_len {
        return plain;
    }
    let truncated: String = plain.chars().take(max_len.saturating_sub(1)).collect();
    format!("{}…", truncated.trim_end())
}

pub(crate) fn phantasi_item_path(item_id: i32) -> String {
    myriad_phantasi::item_path(item_id)
}

pub(crate) fn name_from_manifest(manifest: &Value, fallback: &str) -> String {
    manifest
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

pub(crate) fn description_from_manifest(
    manifest: &Value,
    fallback: Option<&str>,
) -> Option<String> {
    manifest
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            fallback
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
        })
}

pub(crate) async fn public_tapp_links(
    db: &DatabaseConnection,
    base: Option<&str>,
) -> Vec<(String, String, Option<String>)> {
    let mut links = Vec::new();
    let Ok(Some(admin_id)) = find_admin_user_id(db).await else {
        return links;
    };
    let Ok(admin_tapps) = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(admin_id))
        .all(db)
        .await
    else {
        return links;
    };
    for tapp in admin_tapps {
        if !public_install_visible_to_viewer(&tapp.visibility, false) {
            continue;
        }
        let name = name_from_manifest(&tapp.manifest, &tapp.name);
        let desc = description_from_manifest(&tapp.manifest, tapp.description.as_deref())
            .map(|s| strip_html_snippet(&s, 80));
        let path = format!("/tapp/run/{}", encode_path_segment(&tapp.tapp_id));
        links.push((public_absolute_url(base, &path), name, desc));
        if links.len() >= 50 {
            break;
        }
    }
    links
}

pub(crate) async fn own_phantasi_item_links(
    db: &DatabaseConnection,
    base: Option<&str>,
) -> Vec<(String, String, Option<String>)> {
    let mut links = Vec::new();
    let Ok(sources) = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .all(db)
        .await
    else {
        return links;
    };
    let own_source_ids: Vec<i32> = sources
        .into_iter()
        .filter(|s| phantasi_source_is_own(&s.source_type, &s.category, s.admin_only))
        .map(|s| s.id)
        .collect();
    if own_source_ids.is_empty() {
        return links;
    }
    let Ok(items) = phantasi_items::preview_query(phantasi_items::Entity::find())
        .filter(phantasi_items::Column::SourceId.is_in(own_source_ids))
        .order_by_desc(phantasi_items::Column::PublishedAt)
        .limit(PHANTASI_LIST_SHELL_LIMIT)
        .all(db)
        .await
    else {
        return links;
    };
    for item in items {
        let path = phantasi_item_path(item.id);
        let blurb = item
            .summary
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| strip_html_snippet(s, 80));
        links.push((public_absolute_url(base, &path), item.title, blurb));
    }
    links
}

pub(crate) const GEO_PROMPT_ITEM_LIMIT: usize = 8;

pub(crate) const GEO_PROMPT_LABEL_CHARS: usize = 60;

pub(crate) fn sanitize_geo_label(s: &str) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = collapsed.chars().count();
    if count <= GEO_PROMPT_LABEL_CHARS {
        collapsed
    } else {
        collapsed
            .chars()
            .take(GEO_PROMPT_LABEL_CHARS.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

pub(crate) async fn own_phantasi_note_links(
    db: &DatabaseConnection,
    base: Option<&str>,
) -> Vec<(String, String, Option<String>)> {
    let mut links = Vec::new();
    let Ok(sources) = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .all(db)
        .await
    else {
        return links;
    };
    let note_source_ids: Vec<i32> = sources
        .into_iter()
        .filter(|s| {
            s.source_type == phantasi_sources::SourceType::Note
                || phantasi_source_is_own(&s.source_type, &s.category, s.admin_only)
        })
        .map(|s| s.id)
        .collect();
    if note_source_ids.is_empty() {
        return links;
    }
    let Ok(items) = phantasi_items::preview_query(phantasi_items::Entity::find())
        .filter(phantasi_items::Column::SourceId.is_in(note_source_ids))
        .order_by_desc(phantasi_items::Column::PublishedAt)
        .limit(PHANTASI_LIST_SHELL_LIMIT)
        .all(db)
        .await
    else {
        return links;
    };
    for item in items {
        let path = phantasi_item_path(item.id);
        let blurb = item
            .summary
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| strip_html_snippet(s, 80));
        links.push((public_absolute_url(base, &path), item.title, blurb));
    }
    links
}

pub(crate) fn geo_link_json(items: &[(String, String, Option<String>)]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|(url, title, blurb)| {
                json!({
                    "url": url,
                    "title": title,
                    "blurb": blurb,
                })
            })
            .collect(),
    )
}

pub(crate) fn sample_titles(items: &[(String, String, Option<String>)]) -> Vec<String> {
    items
        .iter()
        .take(GEO_PROMPT_ITEM_LIMIT)
        .map(|(_, title, _)| sanitize_geo_label(title))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Guest-visible modules plus a sample of public apps/writing/notes for SEO/GEO copy.
pub(crate) async fn public_geo_prompt_facts(db: &DatabaseConnection) -> String {
    let prefs = load_module_visibility_preferences(db).await;
    let modules = &prefs.modules;
    let mut lines = Vec::new();
    let mut visible = vec!["Home".to_string()];
    let mut hidden = Vec::new();
    for (key, label) in [
        ("library", "Library"),
        ("phantasi", "Journal"),
        ("reports", "Reports"),
        ("tapp", "Tapp"),
    ] {
        let level = modules.get(key).map(String::as_str).unwrap_or("all");
        if module_is_public_all(level) {
            visible.push(label.to_string());
        } else {
            hidden.push(label);
        }
    }
    lines.push(format!("Guest-visible modules: {}", visible.join(", ")));
    if !hidden.is_empty() {
        lines.push(format!(
            "Not guest-visible (do not mention): {}",
            hidden.join(", ")
        ));
    }

    if module_is_public_all(modules.get("tapp").map(String::as_str).unwrap_or("all")) {
        let apps = public_tapp_links(db, None).await;
        if apps.is_empty() {
            lines.push("Public apps: none listed.".into());
        } else {
            lines.push(format!(
                "Public apps (sample): {}",
                sample_titles(&apps).join("; ")
            ));
        }
    }

    if module_is_public_all(modules.get("phantasi").map(String::as_str).unwrap_or("all")) {
        let writing = own_phantasi_item_links(db, None).await;
        if writing.is_empty() {
            lines.push("Public writing: none listed.".into());
        } else {
            lines.push(format!(
                "Public writing titles (sample): {}",
                sample_titles(&writing).join("; ")
            ));
        }
        let notes = own_phantasi_note_links(db, None).await;
        if notes.is_empty() {
            lines.push("Public notes: none listed.".into());
        } else {
            lines.push(format!(
                "Public note titles (sample): {}",
                sample_titles(&notes).join("; ")
            ));
        }
    }

    lines.join("\n")
}

/// Branding plus guest-visible original content. No friend-links or third-party feeds.
pub(crate) async fn public_geo_inspect(db: &DatabaseConnection) -> Value {
    let branding = load_site_branding(db).await;
    let prefs = load_module_visibility_preferences(db).await;
    let modules = &prefs.modules;
    let mut visible = vec!["Home".to_string()];
    let mut hidden = Vec::new();
    for (key, label) in [
        ("library", "Library"),
        ("phantasi", "Journal"),
        ("reports", "Reports"),
        ("tapp", "Tapp"),
    ] {
        let level = modules.get(key).map(String::as_str).unwrap_or("all");
        if module_is_public_all(level) {
            visible.push(label.to_string());
        } else {
            hidden.push(label.to_string());
        }
    }

    let tapp_open = module_is_public_all(modules.get("tapp").map(String::as_str).unwrap_or("all"));
    let phantasi_open =
        module_is_public_all(modules.get("phantasi").map(String::as_str).unwrap_or("all"));
    let apps = if tapp_open {
        public_tapp_links(db, None).await
    } else {
        Vec::new()
    };
    let writing = if phantasi_open {
        own_phantasi_item_links(db, None).await
    } else {
        Vec::new()
    };
    let notes = if phantasi_open {
        own_phantasi_note_links(db, None).await
    } else {
        Vec::new()
    };

    let mut owner = json!({});
    if let Ok(uid) = crate::services::site_owner::site_owner_user_id(db).await {
        if let Ok(text) = crate::services::profile_text::resolve_profile_text(db, uid).await {
            if let Some(name) = text.name.filter(|s| !s.trim().is_empty()) {
                owner["name"] = json!(name);
            }
            let bio = text.bio.trim();
            if !bio.is_empty() && !crate::services::avatar::is_placeholder_bio(bio) {
                owner["bio"] = json!(bio);
            }
        }
    }

    json!({
        "branding": {
            "title": branding.title,
            "description": branding.description,
            "keywords": branding.keywords,
            "ai_intro": branding.ai_intro,
            "visibility_policy": branding.policy,
            "noindex": branding.noindex,
        },
        "owner": owner,
        "modules": {
            "visible": visible,
            "hidden": hidden,
        },
        "apps": geo_link_json(&apps),
        "writing": geo_link_json(&writing),
        "notes": geo_link_json(&notes),
        "facts": public_geo_prompt_facts(db).await,
    })
}
