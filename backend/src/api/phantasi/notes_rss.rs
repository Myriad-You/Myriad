//! 已发布笔记的公开 RSS。别人用任何阅读器订阅 `/journal/notes.xml`。
//!
//! 只读 `phantasi_items.content`，不读草稿表。开关关掉或 Phantasi 不对访客开放时 404。

use std::collections::HashMap;

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use myriad_phantasi::SPA_PREFIX;
use myriad_phantasi_notes::{NOTES_RSS_PATH, NOTES_RSS_PREFERENCES_KEY, note_link};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Deserialize;
use serde_json::{Value, json};

use super::helpers::phantasi_http_err;
use crate::api::seo::{
    notes_rss_enabled, notes_rss_is_public, public_absolute_url, public_site_identity,
    resolve_public_base_url, strip_html_snippet, xml_escape,
};
use crate::error::HttpError;
use crate::extract::AdminClaims;
use crate::models::entities::{phantasi_items, phantasi_sources};

const NOTES_RSS_ITEM_LIMIT: u64 = 50;
const NOTES_RSS_SNIPPET: usize = 200;

#[derive(Debug, Clone)]
pub(crate) struct NotesRssItem {
    pub guid: String,
    pub title: String,
    pub link: String,
    pub published_rfc822: String,
    pub author: Option<String>,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub(crate) struct NotesRssChannel {
    pub title: String,
    pub link: String,
    pub description: String,
    pub language: String,
    pub feed_url: String,
    pub last_build: String,
    pub items: Vec<NotesRssItem>,
}

fn cdata(text: &str) -> String {
    text.replace("]]>", "]]]]><![CDATA[>")
}

fn rfc822(value: sea_orm::prelude::DateTimeWithTimeZone) -> String {
    value
        .with_timezone(&Utc)
        .format("%a, %d %b %Y %H:%M:%S +0000")
        .to_string()
}

fn item_description(summary: Option<&str>, content: Option<&str>) -> String {
    if let Some(text) = summary.map(str::trim).filter(|text| !text.is_empty()) {
        return text.to_string();
    }
    content
        .map(|html| strip_html_snippet(html, NOTES_RSS_SNIPPET))
        .unwrap_or_default()
}

pub(crate) fn render_notes_rss(channel: &NotesRssChannel) -> String {
    let mut body = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
"#,
    );
    body.push_str(&format!(
        "    <title>{}</title>\n    <link>{}</link>\n    <description>{}</description>\n    <language>{}</language>\n    <lastBuildDate>{}</lastBuildDate>\n    <atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\"/>\n",
        xml_escape(&channel.title),
        xml_escape(&channel.link),
        xml_escape(&channel.description),
        xml_escape(&channel.language),
        xml_escape(&channel.last_build),
        xml_escape(&channel.feed_url),
    ));
    for item in &channel.items {
        body.push_str("    <item>\n");
        body.push_str(&format!(
            "      <title>{}</title>\n      <link>{}</link>\n      <guid isPermaLink=\"false\">{}</guid>\n      <pubDate>{}</pubDate>\n",
            xml_escape(&item.title),
            xml_escape(&item.link),
            xml_escape(&item.guid),
            xml_escape(&item.published_rfc822),
        ));
        if let Some(author) = item
            .author
            .as_deref()
            .map(str::trim)
            .filter(|author| !author.is_empty())
        {
            body.push_str(&format!(
                "      <dc:creator>{}</dc:creator>\n",
                xml_escape(author)
            ));
        }
        body.push_str(&format!(
            "      <description><![CDATA[{}]]></description>\n",
            cdata(&item.description)
        ));
        if !item.content.trim().is_empty() {
            body.push_str(&format!(
                "      <content:encoded><![CDATA[{}]]></content:encoded>\n",
                cdata(&item.content)
            ));
        }
        body.push_str("    </item>\n");
    }
    body.push_str("  </channel>\n</rss>\n");
    body
}

async fn load_notes_rss_channel(db: &DatabaseConnection) -> Result<NotesRssChannel, String> {
    let (site_title, site_description, language) = public_site_identity(db).await;
    let base = resolve_public_base_url();
    let feed_url = public_absolute_url(base.as_deref(), NOTES_RSS_PATH);
    let channel_link = public_absolute_url(base.as_deref(), SPA_PREFIX);

    let sources = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .all(db)
        .await
        .map_err(|error| error.to_string())?;
    let names: HashMap<i32, String> = sources
        .iter()
        .map(|source| (source.id, source.name.clone()))
        .collect();
    let source_ids: Vec<i32> = sources.iter().map(|source| source.id).collect();

    let items = if source_ids.is_empty() {
        Vec::new()
    } else {
        phantasi_items::Entity::find()
            .filter(phantasi_items::Column::SourceId.is_in(source_ids))
            .order_by_desc(phantasi_items::Column::PublishedAt)
            .limit(NOTES_RSS_ITEM_LIMIT)
            .all(db)
            .await
            .map_err(|error| error.to_string())?
    };

    let last_build = items
        .first()
        .map(|item| rfc822(item.published_at))
        .unwrap_or_else(|| Utc::now().format("%a, %d %b %Y %H:%M:%S +0000").to_string());

    let rss_items = items
        .into_iter()
        .map(|item| {
            let author = item
                .author
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    names
                        .get(&item.source_id)
                        .map(|name| name.trim().to_string())
                        .filter(|name| !name.is_empty())
                })
                .or_else(|| {
                    let title = site_title.trim();
                    if title.is_empty() {
                        None
                    } else {
                        Some(title.to_string())
                    }
                });
            let path = if item.link.trim().starts_with('/') {
                item.link.trim().to_string()
            } else {
                note_link(item.id)
            };
            NotesRssItem {
                guid: if item.guid.trim().is_empty() {
                    path.clone()
                } else {
                    item.guid
                },
                title: item.title,
                link: public_absolute_url(base.as_deref(), &path),
                published_rfc822: rfc822(item.published_at),
                author,
                description: item_description(item.summary.as_deref(), item.content.as_deref()),
                content: item.content.unwrap_or_default(),
            }
        })
        .collect();

    Ok(NotesRssChannel {
        title: site_title,
        link: channel_link,
        description: site_description,
        language: language.to_string(),
        feed_url,
        last_build,
        items: rss_items,
    })
}

fn notes_rss_response(body: String) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/rss+xml; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        body,
    )
        .into_response()
}

fn notes_rss_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "Not found",
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub(crate) struct NotesRssSettingsRequest {
    pub enabled: bool,
}

fn notes_rss_settings_json(enabled: bool) -> Value {
    json!({
        "success": true,
        "enabled": enabled,
        "path": NOTES_RSS_PATH,
    })
}

/// GET `/api/phantasi/notes/rss` — 站长看开关。
pub async fn get_notes_rss_settings(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
) -> Result<Json<Value>, HttpError> {
    Ok(Json(notes_rss_settings_json(notes_rss_enabled(&db).await)))
}

/// PUT `/api/phantasi/notes/rss` — 站长改开关。默认关。
pub async fn put_notes_rss_settings(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Json(req): Json<NotesRssSettingsRequest>,
) -> Result<Json<Value>, HttpError> {
    let config_service = crate::services::config_service::ConfigService::new(db);
    if let Err(error) = config_service
        .update_config(NOTES_RSS_PREFERENCES_KEY, json!(req.enabled))
        .await
    {
        tracing::error!(%error, "failed to save notes RSS preference");
        return Err(phantasi_http_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save notes RSS setting",
        ));
    }
    Ok(Json(notes_rss_settings_json(req.enabled)))
}

/// GET `/journal/notes.xml`（以及 `/api/phantasi/notes.xml`）— 已发布笔记的 RSS 2.0。
pub async fn notes_rss(State(db): State<DatabaseConnection>) -> Response {
    if !notes_rss_is_public(&db).await {
        return notes_rss_not_found();
    }
    match load_notes_rss_channel(&db).await {
        Ok(channel) => notes_rss_response(render_notes_rss(&channel)),
        Err(error) => {
            tracing::error!(%error, "failed to build notes RSS");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                "Failed to build feed",
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::phantasi_parser::FeedParser;
    use chrono::DateTime;

    fn sample_channel() -> NotesRssChannel {
        NotesRssChannel {
            title: "Site & Notes".into(),
            link: "https://ex.com/journal".into(),
            description: "Hello <world>".into(),
            language: "zh-CN".into(),
            feed_url: "https://ex.com/journal/notes.xml".into(),
            last_build: "Mon, 14 Sep 2026 00:00:00 +0000".into(),
            items: vec![NotesRssItem {
                guid: "note:abc".into(),
                title: "A & B".into(),
                link: "https://ex.com/journal/articles/12".into(),
                published_rfc822: "Mon, 14 Sep 2026 00:00:00 +0000".into(),
                author: Some("Ada".into()),
                description: "plain".into(),
                content: "<p>Full</p><script>]]></script>".into(),
            }],
        }
    }

    #[test]
    fn feed_path_is_the_public_notes_rss() {
        assert_eq!(NOTES_RSS_PATH, "/journal/notes.xml");
    }

    #[test]
    fn cdata_splits_terminator() {
        assert_eq!(cdata("a]]>b"), "a]]]]><![CDATA[>b");
    }

    #[test]
    fn empty_channel_is_valid_rss() {
        let xml = render_notes_rss(&NotesRssChannel {
            title: "Site".into(),
            link: "/journal".into(),
            description: "Notes".into(),
            language: "en".into(),
            feed_url: "/journal/notes.xml".into(),
            last_build: "Mon, 14 Sep 2026 00:00:00 +0000".into(),
            items: Vec::new(),
        });
        assert!(xml.contains(r#"<rss version="2.0""#));
        assert!(xml.contains("<atom:link href=\"/journal/notes.xml\""));
        assert!(!xml.contains("<item>"));
    }

    #[test]
    fn render_escapes_and_keeps_html_in_content() {
        let xml = render_notes_rss(&sample_channel());
        assert!(xml.contains("<title>Site &amp; Notes</title>"));
        assert!(xml.contains("<description>Hello &lt;world&gt;</description>"));
        assert!(xml.contains("<title>A &amp; B</title>"));
        assert!(xml.contains("<guid isPermaLink=\"false\">note:abc</guid>"));
        assert!(xml.contains("<dc:creator>Ada</dc:creator>"));
        assert!(xml.contains("<![CDATA[<p>Full</p><script>]]]]><![CDATA[></script>]]>"));
        assert!(!xml.contains("phantasi_note_docs"));
    }

    #[test]
    fn phantasi_parser_reads_the_notes_feed() {
        let xml = render_notes_rss(&sample_channel());
        let feed = FeedParser::new()
            .parse_content(
                &xml,
                "application/rss+xml",
                "https://ex.com/journal/notes.xml",
            )
            .expect("parse notes rss");
        assert_eq!(feed.items.len(), 1);
        let item = &feed.items[0];
        assert_eq!(item.link, "https://ex.com/journal/articles/12");
        assert_eq!(item.guid, "note:abc");
        assert_eq!(item.author.as_deref(), Some("Ada"));
        assert!(
            item.content
                .as_deref()
                .unwrap_or("")
                .contains("<p>Full</p>")
        );
    }

    #[test]
    fn query_does_not_read_draft_docs() {
        let src = include_str!("notes_rss.rs");
        let code = src.split("mod tests").next().expect("impl");
        assert!(code.contains("SourceType::Note"));
        assert!(code.contains("notes_rss_is_public"));
        assert!(code.contains("NOTES_RSS_PREFERENCES_KEY"));
        assert!(
            !code.contains("note_docs"),
            "drafts must not leak into the public notes RSS"
        );
    }

    #[test]
    fn description_prefers_plain_summary() {
        assert_eq!(
            item_description(Some("  hello  "), Some("<p>html</p>")),
            "hello"
        );
        assert_eq!(
            item_description(Some("   "), Some("<p>html body</p>")),
            "html body"
        );
    }

    #[test]
    fn rfc822_uses_gmt_offset() {
        let dt: DateTime<Utc> = "2026-09-14T01:02:03Z".parse().unwrap();
        let stamped: sea_orm::prelude::DateTimeWithTimeZone = dt.into();
        assert_eq!(rfc822(stamped), "Mon, 14 Sep 2026 01:02:03 +0000");
    }
}
