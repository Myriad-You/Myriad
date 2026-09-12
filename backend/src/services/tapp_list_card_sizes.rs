//! Per-user Tapp list page card sizes (`1x1` | `2x1`), stored on `users.tapp_list_card_sizes`.

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_ENTRIES: usize = 256;
const MAX_TAPP_ID_LEN: usize = 128;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TappListCardSizes {
    /// Map of installation / manifest id → widget-style span.
    #[serde(default)]
    pub sizes: BTreeMap<String, String>,
    /// Preferred display order (tapp ids). Missing ids append in catalog order.
    #[serde(default)]
    pub order: Vec<String>,
}

impl TappListCardSizes {
    pub fn normalized(self) -> Self {
        let mut sizes = BTreeMap::new();
        for (id, size) in self.sizes {
            let id = id.trim();
            if id.is_empty() || id.len() > MAX_TAPP_ID_LEN {
                continue;
            }
            let size = size.trim();
            if size != "1x1" && size != "2x1" {
                continue;
            }
            sizes.insert(id.to_string(), size.to_string());
            if sizes.len() >= MAX_ENTRIES {
                break;
            }
        }

        let mut seen = std::collections::HashSet::new();
        let mut order = Vec::new();
        for id in self.order {
            let id = id.trim();
            if id.is_empty() || id.len() > MAX_TAPP_ID_LEN {
                continue;
            }
            if !seen.insert(id.to_string()) {
                continue;
            }
            order.push(id.to_string());
            if order.len() >= MAX_ENTRIES {
                break;
            }
        }

        Self { sizes, order }
    }
}

pub async fn load(db: &DatabaseConnection, user_id: i32) -> TappListCardSizes {
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT tapp_list_card_sizes FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => row
            .try_get::<Value>("", "tapp_list_card_sizes")
            .ok()
            .and_then(|value| {
                // Wrapped `TappListCardSizes` first (`sizes`/`order` default empty; extra keys ignored).
                if let Ok(wrapped) = serde_json::from_value::<TappListCardSizes>(value.clone()) {
                    return Some(wrapped.normalized());
                }
                if let Ok(map) = serde_json::from_value::<BTreeMap<String, String>>(value) {
                    return Some(
                        TappListCardSizes {
                            sizes: map,
                            order: Vec::new(),
                        }
                        .normalized(),
                    );
                }
                None
            })
            .unwrap_or_default(),
        Ok(None) => TappListCardSizes::default(),
        Err(error) => {
            tracing::warn!(user_id, "Failed to load tapp list card sizes: {}", error);
            TappListCardSizes::default()
        }
    }
}

/// Viewer layout split so personal prefs are never contaminated by site-owner fills.
///
/// - `personal`: durable prefs for the viewer (empty for guests)
/// - `site`: site-owner public layout (empty if no site owner)
///
/// Display merge is the client's job: **mine** uses personal only; **site** uses
/// site only. Saving always writes pure personal — never a merged map.
#[derive(Debug, Clone, Default)]
pub struct ViewerListCardLayout {
    pub personal: TappListCardSizes,
    pub site: TappListCardSizes,
    /// True when viewer has a durable user row and may PUT layout.
    pub writable: bool,
    /// `"site_owner"` for guests / when serving owner layout as primary;
    /// `"viewer"` when personal prefs are the primary `sizes`/`order` payload.
    pub source: &'static str,
}

/// Layout for list viewers (no silent merge into personal storage).
///
/// - guests → personal empty, site = owner layout, source `site_owner`
/// - site owner → personal = site = own layout, source `viewer`
/// - other logged-in users → personal = own row only, site = owner layout
pub async fn load_for_viewer(
    db: &DatabaseConnection,
    viewer_user_id: Option<i32>,
) -> ViewerListCardLayout {
    let site_owner = match crate::services::site_owner::site_owner_user_id(db).await {
        Ok(id) => Some(id),
        Err(error) => {
            tracing::warn!("list card sizes: no site owner ({error})");
            None
        }
    };

    let site = match site_owner {
        Some(id) => load(db, id).await,
        None => TappListCardSizes::default(),
    };

    match viewer_user_id {
        None => ViewerListCardLayout {
            personal: TappListCardSizes::default(),
            site,
            writable: false,
            source: "site_owner",
        },
        Some(uid) if site_owner == Some(uid) => ViewerListCardLayout {
            personal: site.clone(),
            site,
            writable: true,
            source: "viewer",
        },
        Some(uid) => ViewerListCardLayout {
            personal: load(db, uid).await,
            site,
            writable: true,
            source: "viewer",
        },
    }
}

pub async fn save(
    db: &DatabaseConnection,
    user_id: i32,
    preferences: TappListCardSizes,
) -> Result<TappListCardSizes, String> {
    let preferences = preferences.normalized();
    let value = serde_json::to_value(&preferences).map_err(|e| e.to_string())?;
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET tapp_list_card_sizes = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            [value.into(), user_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;
    if result.rows_affected() == 0 {
        return Err("User not found".to_string());
    }
    Ok(preferences)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_keeps_only_valid_sizes() {
        let prefs = TappListCardSizes {
            sizes: BTreeMap::from([
                ("com.a".into(), "2x1".into()),
                ("com.b".into(), "3x3".into()),
                ("".into(), "1x1".into()),
                ("com.c".into(), "1x1".into()),
            ]),
            order: vec!["com.a".into(), "com.a".into(), "com.c".into(), "".into()],
        }
        .normalized();
        assert_eq!(prefs.sizes.get("com.a").map(String::as_str), Some("2x1"));
        assert_eq!(prefs.sizes.get("com.c").map(String::as_str), Some("1x1"));
        assert!(!prefs.sizes.contains_key("com.b"));
        assert_eq!(prefs.sizes.len(), 2);
        assert_eq!(prefs.order, vec!["com.a".to_string(), "com.c".to_string()]);
    }

    #[test]
    fn roundtrip_json_shape() {
        let prefs = TappListCardSizes {
            sizes: BTreeMap::from([("x".into(), "2x1".into())]),
            order: vec!["x".into()],
        };
        let v = serde_json::to_value(&prefs).unwrap();
        assert_eq!(v, json!({"sizes": {"x": "2x1"}, "order": ["x"]}));
    }
}
