//! 从库里取出要导出的笔记和友链。公开笔记不含 `admin_only` 源。

use std::collections::HashSet;

use chrono::Utc;
use myriad_blog::{BlogPost, BlogSettings, BlogSite, FriendLink, blog_settings_from_value};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    QuerySelect, Statement,
};

use crate::models::entities::{
    phantasi_annotations, phantasi_items, phantasi_podcasts, phantasi_sources,
};
use crate::services::public_site::{self, listed_board_keeps};

pub async fn load_settings(db: &impl ConnectionTrait) -> BlogSettings {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT value FROM configurations WHERE key = $1",
            vec![myriad_blog::BLOG_SETTINGS_KEY.into()],
        ))
        .await;
    let Ok(Some(row)) = row else {
        return BlogSettings::default();
    };
    let Ok(value) = row.try_get::<serde_json::Value>("", "value") else {
        return BlogSettings::default();
    };
    let value = crate::services::data_key::open_config_value(myriad_blog::BLOG_SETTINGS_KEY, value)
        .unwrap_or(serde_json::Value::Null);
    blog_settings_from_value(&value)
}

pub async fn load_site(db: &DatabaseConnection, settings: &BlogSettings) -> BlogSite {
    let branding = public_site::load_site_branding(db).await;
    let locale = public_site::infer_site_locale(&[&branding.title, &branding.description]);
    // 没配站点地址就只写站内路径，不编一个域名进 canonical 和分享卡。
    let origin = public_site::resolve_public_base_url().unwrap_or_default();
    let feed = if public_site::notes_rss_enabled(db).await {
        myriad_phantasi_notes::NOTES_RSS_PATH.to_string()
    } else {
        String::new()
    };
    BlogSite {
        origin,
        title: if settings.site_title.trim().is_empty() {
            branding.title
        } else {
            settings.site_title.clone()
        },
        description: if settings.description.trim().is_empty() {
            branding.description
        } else {
            settings.description.clone()
        },
        locale: locale.html_lang.to_ascii_lowercase(),
        params: serde_json::json!({}),
        icon: branding.favicon,
        image: branding.og_image,
        feed,
        noindex: branding.noindex,
    }
}

/// 博客的语言跟着文章走：站点标题是英文、文章是中文时，页面该标中文。
/// 认得出语言的文章里最多的那种定为博客语言；没有文章或都认不出时用站点标题推断的。
/// 和博客语言不同的文章单独标上自己的语言。
pub fn settle_locales(site: &mut BlogSite, posts: &mut [BlogPost]) {
    let found: Vec<Option<public_site::SiteLocale>> = posts
        .iter()
        .map(|post| {
            public_site::text_locale(&post.title, &myriad_blog::summarize(&post.content_html))
        })
        .collect();
    if let Some(winner) = public_site::majority_locale(&found, &site.locale) {
        site.locale = winner.html_lang.to_ascii_lowercase();
    }
    for (post, locale) in posts.iter_mut().zip(found) {
        post.locale = locale
            .map(|locale| locale.html_lang)
            .filter(|locale| !locale.eq_ignore_ascii_case(&site.locale))
            .map(str::to_string)
            .unwrap_or_default();
    }
}

pub async fn load_posts(db: &DatabaseConnection) -> Result<Vec<BlogPost>, &'static str> {
    let sources = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .all(db)
        .await
        .map_err(|_| "Could not read published notes")?;
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i32> = sources.iter().map(|source| source.id).collect();
    let mut items = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.is_in(ids))
        .all(db)
        .await
        .map_err(|_| "Could not read published notes")?;
    items.sort_by_key(|item| item.published_at);
    let item_ids: Vec<i32> = items.iter().map(|item| item.id).collect();
    let companions = companion_item_ids(db, &item_ids)
        .await
        .map_err(|_| "Could not read AI annotations and podcasts")?;
    Ok(items
        .into_iter()
        .map(|item| {
            let companion = companions.contains(&item.id);
            item_to_post(item, companion)
        })
        .collect())
}

/// Posts that already have AI annotations or a podcast script.
async fn companion_item_ids(
    db: &DatabaseConnection,
    item_ids: &[i32],
) -> Result<HashSet<i32>, sea_orm::DbErr> {
    if item_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let mut found: HashSet<i32> = phantasi_annotations::Entity::find()
        .select_only()
        .column(phantasi_annotations::Column::ItemId)
        .distinct()
        .filter(phantasi_annotations::Column::ItemId.is_in(item_ids.iter().copied()))
        .into_tuple::<i32>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    found.extend(
        phantasi_podcasts::Entity::find()
            .select_only()
            .column(phantasi_podcasts::Column::ItemId)
            .distinct()
            .filter(phantasi_podcasts::Column::ItemId.is_in(item_ids.iter().copied()))
            .into_tuple::<i32>()
            .all(db)
            .await?,
    );
    Ok(found)
}

pub async fn load_friends(db: &DatabaseConnection) -> Result<Vec<FriendLink>, &'static str> {
    let sources = phantasi_sources::Entity::find()
        .all(db)
        .await
        .map_err(|_| "Could not read friend links")?;
    let mut friends: Vec<_> = sources
        .into_iter()
        .filter(|source| {
            source.enabled
                && !source.admin_only
                && listed_board_keeps(
                    &source.source_type,
                    source.category.as_deref(),
                    source.admin_only,
                    "sites",
                )
        })
        .map(|source| FriendLink {
            name: source.name,
            url: source
                .site_url
                .filter(|url| !url.trim().is_empty())
                .unwrap_or(source.url),
            description: source.description.unwrap_or_default(),
            icon: source.icon.unwrap_or_default(),
        })
        .collect();
    friends.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(friends)
}

fn item_to_post(item: phantasi_items::Model, companion: bool) -> BlogPost {
    let authors = item
        .author
        .unwrap_or_default()
        .split(" · ")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    BlogPost {
        item_id: item.id,
        slug: item.slug.unwrap_or_default(),
        title: item.title,
        published_at: item.published_at.to_rfc3339(),
        updated_at: item.fetched_at.with_timezone(&Utc).to_rfc3339(),
        content_html: item.content.unwrap_or_default(),
        // `item.summary` 里的公式已经拍平成 TeX 源码，认不出来；只有正文 HTML 还带公式 span。
        summary_source: String::new(),
        category: item.topic.filter(|topic| !topic.trim().is_empty()),
        authors,
        cover: item.image.filter(|image| !image.trim().is_empty()),
        companion,
        locale: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(title: &str, html: &str) -> BlogPost {
        BlogPost {
            item_id: 1,
            slug: "a".to_string(),
            title: title.to_string(),
            published_at: String::new(),
            updated_at: String::new(),
            content_html: html.to_string(),
            summary_source: String::new(),
            category: None,
            authors: Vec::new(),
            cover: None,
            companion: false,
            locale: String::new(),
        }
    }

    #[test]
    fn blog_language_follows_the_posts() {
        let mut site = BlogSite {
            locale: "en".to_string(),
            ..BlogSite::default()
        };
        let mut posts = vec![
            post(
                "自己的桌子",
                "<p>我把写作从时间线里搬出来，不是因为讨厌人。</p>",
            ),
            post("早晨的咖啡", "<p>豆子不必好看，只要是真的烘过。</p>"),
            post("朝のコーヒー", "<p>ゆっくり書くことにした。</p>"),
            post("42", "<p>1 2 3</p>"),
        ];
        settle_locales(&mut site, &mut posts);
        assert_eq!(site.locale, "zh-cn");
        assert_eq!(posts[0].locale, "");
        assert_eq!(posts[2].locale, "ja");
        assert_eq!(posts[3].locale, "");

        // 没有认得出的文章：留着站点标题推断的。
        let mut site = BlogSite {
            locale: "ja".to_string(),
            ..BlogSite::default()
        };
        let mut posts = vec![post("v2", "<p>ok</p>")];
        settle_locales(&mut site, &mut posts);
        assert_eq!(site.locale, "ja");

        // 平票时站点原来的语言优先。
        let mut site = BlogSite {
            locale: "en".to_string(),
            ..BlogSite::default()
        };
        let mut posts = vec![
            post("早晨的咖啡", "<p>豆子不必好看，只要是真的烘过。</p>"),
            post(
                "Morning coffee",
                "<p>The beans do not need to look good, only to be roasted for real.</p>",
            ),
        ];
        settle_locales(&mut site, &mut posts);
        assert_eq!(site.locale, "en");
        assert_eq!(posts[0].locale, "zh-CN");
    }
}
