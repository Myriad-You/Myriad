//! A note published on the site where everyone can read it.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::{Intake, Thing};

/// Notes offered at a time.
pub const OFFERED: usize = 3;
const NOTE_CHARS: usize = 2_500;

/// Notes published on the site where everyone can read them.
pub async fn options(db: &DatabaseConnection) -> Vec<Thing> {
    use crate::models::entities::{phantasi_items, phantasi_sources};
    let Ok(sources) = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .all(db)
        .await
    else {
        return Vec::new();
    };
    if sources.is_empty() {
        return Vec::new();
    }
    phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.is_in(sources.iter().map(|source| source.id)))
        .order_by_desc(phantasi_items::Column::PublishedAt)
        .limit(30)
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|item| !item.title.trim().is_empty())
        .map(|item| Thing::Note {
            item_id: item.id,
            title: item.title.trim().chars().take(80).collect(),
        })
        .collect()
}

async fn text(db: &DatabaseConnection, item_id: i32) -> Option<String> {
    use crate::models::entities::phantasi_items;
    let item = phantasi_items::Entity::find_by_id(item_id)
        .one(db)
        .await
        .ok()??;
    let text = item
        .content_md
        .filter(|text| !text.trim().is_empty())
        .or_else(|| item.content.map(|html| strip_tags(&html)))
        .or(item.summary)?;
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// About three hundred characters a minute, between two and twelve minutes.
pub async fn minutes(db: &DatabaseConnection, item_id: i32) -> i64 {
    let chars = text(db, item_id)
        .await
        .map(|text| text.chars().count())
        .unwrap_or(0) as i64;
    (chars / 300).clamp(2, 12)
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// The note as she read it.
pub async fn intake(db: &DatabaseConnection, item_id: i32) -> Intake {
    Intake::plain(text(db, item_id).await, NOTE_CHARS, "")
}

#[cfg(test)]
pub(crate) fn probe_intake(_material: Option<&str>) -> Intake {
    Intake::plain(None, NOTE_CHARS, "")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_note_reads_without_its_markup() {
        assert_eq!(
            super::strip_tags("<p>秋天的<b>第一杯</b></p>")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(""),
            "秋天的第一杯"
        );
    }
}
