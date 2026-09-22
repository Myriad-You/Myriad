//! Catalog reference labels. Resumable consumer backfill lives in `upgrade`.
#[cfg(test)]
use super::cite::extract_registered_paths;
use super::error::MediaError;
use crate::models::entities::media_references;
use sea_orm::{ColumnTrait, Condition, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect};
use std::collections::HashMap;

/// Map live consumer types onto the catalog UI's notes/articles/site labels.
pub fn catalog_reference_labels(consumer_types: &[String]) -> Vec<String> {
    let mut labels = Vec::new();
    let push = |labels: &mut Vec<String>, label: &str| {
        if !labels.iter().any(|existing| existing == label) {
            labels.push(label.to_string());
        }
    };
    for kind in consumer_types {
        match kind.as_str() {
            "note_draft" | "note_published" | "note_history" => push(&mut labels, "notes"),
            "rss_item" | "federation_activity" | "federation_outbox" => {
                push(&mut labels, "articles")
            }
            _ => push(&mut labels, "site"),
        }
    }
    labels
}

pub async fn catalog_labels_for_assets(
    db: &impl ConnectionTrait,
    asset_ids: &[i32],
) -> Result<HashMap<i32, Vec<String>>, MediaError> {
    if asset_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let now = chrono::Utc::now().fixed_offset();
    let rows = media_references::Entity::find()
        .select_only()
        .columns([
            media_references::Column::AssetId,
            media_references::Column::ConsumerType,
        ])
        .distinct()
        .filter(media_references::Column::AssetId.is_in(asset_ids.iter().copied()))
        .filter(
            Condition::any()
                .add(media_references::Column::ExpiresAt.is_null())
                .add(media_references::Column::ExpiresAt.gt(now)),
        )
        .into_tuple::<(i32, String)>()
        .all(db)
        .await?;
    let mut grouped: HashMap<i32, Vec<String>> = HashMap::new();
    for (asset_id, consumer_type) in rows {
        grouped.entry(asset_id).or_default().push(consumer_type);
    }
    Ok(grouped
        .into_iter()
        .map(|(id, kinds)| (id, catalog_reference_labels(&kinds)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_labels_group_consumer_types() {
        assert_eq!(
            catalog_reference_labels(&[
                "note_draft".into(),
                "note_history".into(),
                "sticker".into()
            ]),
            vec!["notes".to_string(), "site".to_string()]
        );
    }

    #[test]
    fn extract_helper_is_available_to_scan() {
        assert!(extract_registered_paths("no media", &[]).is_empty());
    }

    #[test]
    fn bounded_scan_does_not_stamp_every_ready_asset_complete() {
        let src = include_str!("scan.rs");
        let needle = format!("UPDATE {} SET references_complete", "media_assets");
        assert!(!src.contains(&needle));
    }
}
