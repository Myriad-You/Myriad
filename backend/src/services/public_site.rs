//! What the site shows its guests: its branding, which modules are open to
//! them, and the public apps, writing and notes, with absolute addresses.
//! robots.txt, llms.txt, the sitemap and the SEO pages are rendered from it
//! in `api::seo`, and the agent's SEO work reads it to know what exists.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::{Value, json};

use crate::models::entities::{phantasi_items, phantasi_sources, tapps};
use crate::services::tapp_ownership::{find_admin_user_id, public_install_visible_to_viewer};
use myriad_module_visibility::load_module_visibility_preferences;

/// Fixed DB category label for site-owner original Phantasi content.
/// Must match frontend `PHANTASI_MINE_CATEGORY` (`frontend/src/components/phantasi/constants.ts`).
///
/// `api::phantasi::notes` writes this value only when creating a notes source. Public note addresses key off `source_type = note`, not this category.
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
    let stored = config_service.load_config().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "stored configuration could not be read; using defaults");
        crate::config::DynamicConfig::default()
    });

    // Branding fields: an empty value falls through to the default.
    let branding = |db_val: Option<String>, default: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default.to_string())
    };

    let noindex_flag = stored.site_noindex;

    let policy_raw = stored.site_visibility_policy.trim().to_string();
    let policy =
        crate::services::seo_policy::normalize_visibility_policy(&policy_raw, noindex_flag)
            .to_string();
    let noindex = noindex_flag || !crate::services::seo_policy::policy_is_indexable(&policy);

    SiteBranding {
        title: branding(
            stored.site_title.clone(),
            "Myriad - A myriad of lights, in one place.",
        ),
        description: branding(
            stored.site_description.clone(),
            "A myriad of lights, in one place.",
        ),
        favicon: branding(stored.site_favicon.clone(), "/favicon.webp"),
        og_image: stored.site_og_image.clone().unwrap_or_default(),
        noindex,
        policy,
        ai_intro: stored.site_ai_intro.clone().unwrap_or_default(),
        keywords: stored.site_keywords.clone().unwrap_or_default(),
        google_site_verification: stored.google_site_verification.clone().unwrap_or_default(),
    }
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

pub(crate) fn phantasi_item_path(slug: &str) -> String {
    myriad_phantasi::item_path(slug)
}

fn published_note_path(item: &phantasi_items::Model) -> Option<String> {
    let slug = item
        .slug
        .as_deref()
        .filter(|slug| myriad_phantasi_notes::validate_slug(slug).is_ok())?;
    Some(phantasi_item_path(slug))
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
        .filter(|s| s.source_type == phantasi_sources::SourceType::Note)
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
        let Some(path) = published_note_path(&item) else {
            continue;
        };
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
        .filter(|s| s.source_type == phantasi_sources::SourceType::Note)
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
        let Some(path) = published_note_path(&item) else {
            continue;
        };
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
        ("phantasi", "Blog"),
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
        ("phantasi", "Blog"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segment_encodes_unsafe_chars() {
        assert_eq!(encode_path_segment("com.example.app"), "com.example.app");
        assert!(encode_path_segment("a/b").contains("%2F"));
    }

    #[test]
    fn strip_html_snippet_truncates() {
        let s = strip_html_snippet("<p>Hello <b>world</b> &amp; friends</p>", 200);
        assert_eq!(s, "Hello world & friends");
        let long = "a".repeat(200);
        let out = strip_html_snippet(&long, 20);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 20);
    }

    #[test]
    fn public_absolute_url_omits_origin_when_base_missing() {
        assert_eq!(public_absolute_url(None, "/tapp/run/x"), "/tapp/run/x");
        assert_eq!(
            public_absolute_url(Some("https://ex.com"), "/tapp/run/x"),
            "https://ex.com/tapp/run/x"
        );
    }

    #[test]
    fn geo_label_collapses_whitespace_and_truncates() {
        assert_eq!(sanitize_geo_label("  Hello\nworld  "), "Hello world");
        let long = "あ".repeat(80);
        let out = sanitize_geo_label(&long);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), GEO_PROMPT_LABEL_CHARS);
    }

    #[test]
    fn geo_inspect_link_json_keeps_title_and_blurb() {
        let items = vec![(
            "/blog/posts/hello/".into(),
            "Hello".into(),
            Some("short".into()),
        )];
        let v = geo_link_json(&items);
        assert_eq!(v[0]["title"], "Hello");
        assert_eq!(v[0]["url"], "/blog/posts/hello/");
        assert_eq!(v[0]["blurb"], "short");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SiteLocale {
    pub(crate) html_lang: &'static str,
    pub(crate) og_locale: &'static str,
}

/// 一篇文字的语言。只有英文字母、又太短的认不准，不算。
pub(crate) fn text_locale(title: &str, body: &str) -> Option<SiteLocale> {
    let locale = infer_site_locale(&[title, body]);
    if locale.html_lang == "en" {
        let letters = title
            .chars()
            .chain(body.chars())
            .filter(char::is_ascii_alphabetic)
            .count();
        return (letters >= 20).then_some(locale);
    }
    Some(locale)
}

/// 认得出的里面最多的那种。平票时和 `prefer`（`html_lang`，不分大小写）相同的优先，其次先出现的。
pub(crate) fn majority_locale(found: &[Option<SiteLocale>], prefer: &str) -> Option<SiteLocale> {
    let mut counts: Vec<(SiteLocale, usize)> = Vec::new();
    for locale in found.iter().flatten() {
        match counts.iter_mut().find(|(seen, _)| seen == locale) {
            Some((_, count)) => *count += 1,
            None => counts.push((*locale, 1)),
        }
    }
    // `max_by_key` 平票取最后一个，倒着找才是先出现的。
    counts
        .iter()
        .rev()
        .copied()
        .max_by_key(|(locale, count)| (*count, locale.html_lang.eq_ignore_ascii_case(prefer)))
        .map(|(locale, _)| locale)
}

/// 最多看最近这么多篇已发布笔记。
const CONTENT_LOCALE_SAMPLE: u64 = 50;
const CONTENT_LOCALE_TTL: std::time::Duration = std::time::Duration::from_secs(600);

static CONTENT_LOCALE: std::sync::Mutex<Option<(std::time::Instant, Option<SiteLocale>)>> =
    std::sync::Mutex::new(None);

/// 站点内容的语言：最近已发布笔记里最多的那种。站点标题是英文、文章是中文时，页面该标中文。
/// 没有笔记或都认不出时为 `None`。结果在进程里留十分钟，爬虫壳页每次都要用。
pub(crate) async fn content_locale(db: &DatabaseConnection) -> Option<SiteLocale> {
    if let Ok(cached) = CONTENT_LOCALE.lock()
        && let Some((at, locale)) = *cached
        && at.elapsed() < CONTENT_LOCALE_TTL
    {
        return locale;
    }
    let locale = load_content_locale(db).await;
    if let Ok(mut cached) = CONTENT_LOCALE.lock() {
        *cached = Some((std::time::Instant::now(), locale));
    }
    locale
}

async fn load_content_locale(db: &DatabaseConnection) -> Option<SiteLocale> {
    let sources: Vec<i32> = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .select_only()
        .column(phantasi_sources::Column::Id)
        .into_tuple()
        .all(db)
        .await
        .ok()?;
    if sources.is_empty() {
        return None;
    }
    let notes: Vec<(String, Option<String>)> = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.is_in(sources))
        .filter(phantasi_items::Column::Slug.is_not_null())
        .order_by_desc(phantasi_items::Column::PublishedAt)
        .limit(CONTENT_LOCALE_SAMPLE)
        .select_only()
        .columns([
            phantasi_items::Column::Title,
            phantasi_items::Column::Summary,
        ])
        .into_tuple()
        .all(db)
        .await
        .ok()?;
    let found: Vec<Option<SiteLocale>> = notes
        .iter()
        .map(|(title, summary)| text_locale(title, summary.as_deref().unwrap_or("")))
        .collect();
    majority_locale(&found, "")
}

const TRADITIONAL_MARKERS: &str = "說這個為與萬億軟體檔訊預設網連線憶臺裡麼迴";

pub(crate) fn host_site_locale(tag: &str) -> SiteLocale {
    match tag {
        "zh-TW" => SiteLocale {
            html_lang: "zh-TW",
            og_locale: "zh_TW",
        },
        "zh-CN" => SiteLocale {
            html_lang: "zh-CN",
            og_locale: "zh_CN",
        },
        "ja-JP" => SiteLocale {
            html_lang: "ja",
            og_locale: "ja_JP",
        },
        "ko-KR" => SiteLocale {
            html_lang: "ko",
            og_locale: "ko_KR",
        },
        "fr-FR" => SiteLocale {
            html_lang: "fr",
            og_locale: "fr_FR",
        },
        "de-DE" => SiteLocale {
            html_lang: "de",
            og_locale: "de_DE",
        },
        _ => SiteLocale {
            html_lang: "en",
            og_locale: "en_US",
        },
    }
}

pub(crate) fn infer_site_locale(texts: &[&str]) -> SiteLocale {
    let mut cjk = 0usize;
    let mut kana = 0usize;
    let mut hangul = 0usize;
    let mut traditional = 0usize;
    for text in texts {
        for ch in text.chars() {
            match ch {
                '\u{3040}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9D}' => kana += 1,
                '\u{AC00}'..='\u{D7A3}' => hangul += 1,
                '\u{4E00}'..='\u{9FFF}' => {
                    cjk += 1;
                    if TRADITIONAL_MARKERS.contains(ch) {
                        traditional += 1;
                    }
                }
                _ => {}
            }
        }
    }
    if kana >= 4 || (kana > 0 && kana * 3 >= cjk.max(1)) {
        return host_site_locale("ja-JP");
    }
    if hangul >= 4 || (hangul > 0 && hangul * 3 >= cjk.max(1)) {
        return host_site_locale("ko-KR");
    }
    if cjk >= 4 {
        if traditional * 2 >= cjk.max(1) || traditional >= 2 {
            return host_site_locale("zh-TW");
        }
        return host_site_locale("zh-CN");
    }
    host_site_locale("en-US")
}

/// Durable public origin only: `FRONTEND_URL`, then `BASE_URL`.
///
/// **Never** derive absolute SEO URLs from client `Host` / `X-Forwarded-Host`
/// (or `X-Forwarded-Proto`) — a poisoned Host can poison cached sitemap /
/// robots / canonical absolute links. When unset, callers must omit absolute
/// URLs (fail closed) rather than invent an origin.
pub(crate) fn resolve_public_base_url() -> Option<String> {
    for key in ["FRONTEND_URL", "BASE_URL"] {
        if let Ok(raw) = std::env::var(key) {
            let trimmed = raw.trim().trim_end_matches('/');
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

pub(crate) async fn public_site_identity(
    db: &DatabaseConnection,
) -> (String, String, &'static str) {
    let branding = load_site_branding(db).await;
    let locale = infer_site_locale(&[&branding.title, &branding.description]);
    (branding.title, branding.description, locale.html_lang)
}

pub(crate) fn normalize_listed_category_token(token: &str) -> String {
    let trimmed = token.trim();
    match trimmed.to_lowercase().as_str() {
        "friends" | "friend" | "friendlink" | "friend-link" | "friend_link" | "friend-links"
        | "friend_links" | "friend links" | "友链" | "友情連結" | "友情链接" => {
            "友情链接".to_string()
        }
        "mine" | "me" | "我的" | "我" => "我".to_string(),
        _ => trimmed.to_string(),
    }
}

pub(crate) fn source_matches_listed_category(
    category: Option<&str>,
    source_type: &phantasi_sources::SourceType,
    wanted: &str,
) -> bool {
    let want = normalize_listed_category_token(wanted);
    if want.is_empty() {
        return false;
    }
    let hit = category
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| normalize_listed_category_token(part) == want);
    if hit {
        return true;
    }
    want == "友情链接"
        && *source_type == phantasi_sources::SourceType::Link
        && category.map(str::trim).unwrap_or("").is_empty()
}

pub(crate) fn listed_board_keeps(
    source_type: &phantasi_sources::SourceType,
    category: Option<&str>,
    admin_only: bool,
    board: &str,
) -> bool {
    match board {
        "feeds" => {
            *source_type != phantasi_sources::SourceType::Link
                && *source_type != phantasi_sources::SourceType::Note
        }
        "notes" => {
            *source_type == phantasi_sources::SourceType::Note
                || (!admin_only && source_matches_listed_category(category, source_type, "我"))
        }
        "sites" => {
            *source_type == phantasi_sources::SourceType::Link
                || source_matches_listed_category(category, source_type, "友情链接")
        }
        _ => true,
    }
}

/// 站长笔记 RSS 开关。缺行 = 关。
pub(crate) async fn notes_rss_enabled(db: &impl sea_orm::ConnectionTrait) -> bool {
    let result = db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT value FROM configurations WHERE key = $1",
            vec![myriad_phantasi_notes::NOTES_RSS_PREFERENCES_KEY.into()],
        ))
        .await;
    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => myriad_phantasi_notes::notes_rss_enabled_from_value(&value),
            Err(error) => {
                tracing::warn!(%error, "failed to read notes RSS preference");
                false
            }
        },
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(%error, "failed to load notes RSS preference");
            false
        }
    }
}
