//! Brew 阅读 - 用户阅读状态实体
//!
//! 存储用户的已读、收藏等状态，支持多用户同步

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "brew_user_states")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 用户 ID
    pub user_id: i32,
    /// 文章 ID
    pub item_id: i32,
    /// 是否已读
    pub is_read: bool,
    /// 是否收藏
    pub is_starred: bool,
    /// 阅读时间
    pub read_at: Option<DateTimeWithTimeZone>,
    /// 阅读进度百分比 (0.0 – 100.0). API 边界统一 0–100；见 `normalize_read_progress`.
    pub read_progress: Option<f32>,
    /// 收藏时间
    pub starred_at: Option<DateTimeWithTimeZone>,
    /// 用户笔记
    #[sea_orm(column_type = "Text", nullable)]
    pub notes: Option<String>,
    pub updated_at: DateTimeWithTimeZone,
    /// Server-owned version; database trigger increments on every update.
    pub revision: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::brew_items::Entity",
        from = "Column::ItemId",
        to = "super::brew_items::Column::Id"
    )]
    BrewItem,
}

impl Related<super::brew_items::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::BrewItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// 批量标记已读请求
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarkAllReadRequest {
    /// 订阅源 ID（可选，不传则标记所有）
    pub source_id: Option<i32>,
    /// 分类（可选）
    pub category: Option<String>,
    /// 截止时间（可选，标记此时间之前的文章）
    pub before: Option<i64>,
}

/// 离线同步请求（批量状态更新）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncStatesRequest {
    pub states: Vec<SyncStateItem>,
}

impl SyncStatesRequest {
    /// Each target has one unambiguous intent per batch. Validate before I/O.
    pub fn validate_targets(&self) -> Result<(), &'static str> {
        let mut ids = std::collections::HashSet::new();
        for state in &self.states {
            if state.expected_revision.is_some_and(|revision| revision < 0) {
                return Err("Invalid state revision");
            }
            if state.item_id <= 0 {
                return Err("Invalid article ID");
            }
            if !ids.insert(state.item_id) {
                return Err("Duplicate article ID in sync batch");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncStateItem {
    pub expected_revision: Option<i64>,
    pub item_id: i32,
    pub is_read: Option<bool>,
    pub is_starred: Option<bool>,
    /// Scroll progress. Canonical API scale is **0–100** (percent).
    /// Do not send 0–1 fractions — `1` means 1%, not 100%.
    pub read_progress: Option<f32>,
    pub updated_at: i64,
}

impl SyncStateItem {
    pub fn conflicts_with(&self, revision: i64, updated_at: i64) -> bool {
        match self.expected_revision {
            Some(expected) => expected != revision,
            None => updated_at > self.updated_at,
        }
    }
}

/// Clamp client read progress to the canonical **0–100** percent scale.
///
/// FE `updateReadProgress` already rounds to 0–100. We only reject non-finite
/// values and clamp — never re-scale `(0,1]` as a fraction (that would turn
/// legitimate 1% into 100%).
pub fn normalize_read_progress(raw: Option<f32>) -> Option<f32> {
    let v = raw?;
    if !v.is_finite() {
        return None;
    }
    Some(v.clamp(0.0, 100.0))
}

#[cfg(test)]
mod read_progress_tests {
    use super::normalize_read_progress;

    #[test]
    fn clamps_percent_scale_without_fraction_rescaling() {
        assert_eq!(normalize_read_progress(None), None);
        assert_eq!(normalize_read_progress(Some(0.0)), Some(0.0));
        // 1.0 is 1% on the 0–100 contract — must NOT become 100.
        assert_eq!(normalize_read_progress(Some(1.0)), Some(1.0));
        assert_eq!(normalize_read_progress(Some(0.5)), Some(0.5));
        assert_eq!(normalize_read_progress(Some(50.0)), Some(50.0));
        assert_eq!(normalize_read_progress(Some(150.0)), Some(100.0));
        assert_eq!(normalize_read_progress(Some(-3.0)), Some(0.0));
        assert_eq!(normalize_read_progress(Some(f32::NAN)), None);
    }
}

/// 同步响应
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncStatesResponse {
    pub revisions: std::collections::HashMap<i32, i64>,
    pub confirmed: Vec<i32>,
    pub failed: Vec<i32>,
    pub synced: i32,
    pub conflicts: Vec<SyncConflict>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncConflict {
    pub server_revision: i64,
    pub item_id: i32,
    pub server_updated_at: i64,
    pub client_updated_at: i64,
}

#[cfg(test)]
mod sync_target_tests {
    use super::*;

    fn request(ids: &[i32]) -> SyncStatesRequest {
        SyncStatesRequest {
            states: ids
                .iter()
                .map(|&item_id| SyncStateItem {
                    item_id,
                    expected_revision: None,
                    is_read: Some(true),
                    is_starred: None,
                    read_progress: None,
                    updated_at: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn rejects_duplicates_and_invalid_targets_before_any_write() {
        assert!(request(&[]).validate_targets().is_ok());
        assert!(request(&[1, 2]).validate_targets().is_ok());
        assert_eq!(
            request(&[1, 2, 1]).validate_targets(),
            Err("Duplicate article ID in sync batch")
        );
        assert_eq!(request(&[0]).validate_targets(), Err("Invalid article ID"));
        assert!(request(&[-1]).validate_targets().is_err());
    }
    #[test]
    fn expected_revision_ignores_client_clock_and_detects_stale_versions() {
        let mut state = request(&[1]).states.remove(0);
        state.expected_revision = Some(3);
        assert!(!state.conflicts_with(3, i64::MAX));
        state.updated_at = i64::MAX;
        assert!(state.conflicts_with(4, 0));
        state.expected_revision = Some(0);
        assert!(!state.conflicts_with(0, 0));
        assert!(state.conflicts_with(1, 0));
    }
}
