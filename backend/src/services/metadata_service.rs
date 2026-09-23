use crate::models::entities::{activity_events, metadata_history, platform_metadata};
use crate::services::activity_event_service::{
    ActivityPayload, build_activity_payload, platform_label,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr,
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait, sea_query::Expr,
};
use serde_json::{Value, json};
use std::collections::HashMap;

/// 元数据服务，用于管理平台原始元数据的存储和变化历史
pub struct MetadataService {
    db: DatabaseConnection,
}

impl MetadataService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// 保存或更新平台元数据，并记录内部差异和用户可读活动。
    ///
    /// 所有平台共用同一条 upsert 路径。PostgreSQL JSONB 保存完整快照。
    pub async fn save_platform_metadata(
        &self,
        user_id: i32,
        platform_name: &str,
        raw_data: Value,
    ) -> Result<i32, Box<dyn std::error::Error>> {
        tracing::info!(
            "💾 Saving platform metadata to database: {} for user {}",
            platform_name,
            user_id
        );

        let estimated_size = Self::estimate_json_size(&raw_data);
        tracing::info!(
            "✓ Metadata snapshot ({} bytes), saving through unified history path",
            estimated_size
        );
        let data_to_save = raw_data.clone();
        let txn = self.db.begin().await?;
        let existing = platform_metadata::Entity::find()
            .filter(platform_metadata::Column::UserId.eq(user_id))
            .filter(platform_metadata::Column::PlatformName.eq(platform_name))
            .order_by_desc(platform_metadata::Column::FetchedAt)
            .one(&txn)
            .await?;

        let now = Utc::now().naive_utc();
        let (metadata_id, ingest) = match existing {
            Some(old_metadata) => {
                self.update_existing_metadata(
                    &txn,
                    old_metadata,
                    user_id,
                    platform_name,
                    raw_data,
                    data_to_save,
                    now,
                )
                .await?
            }
            None => {
                txn.execute_unprepared("SAVEPOINT metadata_insert").await?;
                let new_metadata = platform_metadata::ActiveModel {
                    user_id: Set(user_id),
                    platform_name: Set(platform_name.to_string()),
                    raw_data: Set(data_to_save.clone()),
                    fetched_at: Set(now),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                };
                match new_metadata.insert(&txn).await {
                    Ok(inserted) => {
                        txn.execute_unprepared("RELEASE SAVEPOINT metadata_insert")
                            .await?;
                        let all_fields: Vec<String> = raw_data
                            .as_object()
                            .map(|object| object.keys().cloned().collect())
                            .unwrap_or_default();
                        let ingest = self
                            .record_metadata_change(
                                &txn,
                                inserted.id,
                                user_id,
                                platform_name,
                                all_fields,
                                None,
                                data_to_save,
                            )
                            .await?;
                        tracing::info!(
                            "✅ New platform metadata created (id: {})",
                            inserted.id
                        );
                        (inserted.id, Some(ingest))
                    }
                    Err(err) if is_unique_metadata(&err) => {
                        txn.execute_unprepared("ROLLBACK TO SAVEPOINT metadata_insert")
                            .await?;
                        let old_metadata = platform_metadata::Entity::find()
                            .filter(platform_metadata::Column::UserId.eq(user_id))
                            .filter(platform_metadata::Column::PlatformName.eq(platform_name))
                            .one(&txn)
                            .await?
                            .ok_or("unique metadata conflict without a row")?;
                        self.update_existing_metadata(
                            &txn,
                            old_metadata,
                            user_id,
                            platform_name,
                            raw_data,
                            data_to_save,
                            now,
                        )
                        .await?
                    }
                    Err(err) => return Err(err.into()),
                }
            }
        };
        txn.commit().await?;
        if let Some(ingest) = ingest {
            if ingest.imported {
                crate::services::agent::merope::spawn_diary(user_id, ingest.summary);
            } else if ingest.high_value {
                crate::services::agent::merope::spawn_ingest(
                    user_id,
                    "agent.merope.platform_activity",
                    ingest.summary,
                );
            }
        }
        Ok(metadata_id)
    }

    async fn update_existing_metadata<C: ConnectionTrait>(
        &self,
        db: &C,
        old_metadata: platform_metadata::Model,
        user_id: i32,
        platform_name: &str,
        raw_data: Value,
        data_to_save: Value,
        now: chrono::NaiveDateTime,
    ) -> Result<(i32, Option<RecordedIngest>), Box<dyn std::error::Error>> {
        let changed_fields = self.detect_changes(&old_metadata.raw_data, &raw_data);
        if changed_fields.is_empty() {
            tracing::info!("   No changes detected, skipping save");
            return Ok((old_metadata.id, None));
        }
        tracing::info!("   Detected {} field changes", changed_fields.len());
        let mut active_model: platform_metadata::ActiveModel = old_metadata.clone().into();
        active_model.raw_data = Set(data_to_save);
        active_model.fetched_at = Set(now);
        active_model.updated_at = Set(now);
        let updated = active_model.update(db).await?;
        let ingest = self
            .record_metadata_change(
                db,
                updated.id,
                user_id,
                platform_name,
                changed_fields,
                Some(old_metadata.raw_data),
                raw_data,
            )
            .await?;
        tracing::info!("✅ Platform metadata updated (id: {})", updated.id);
        Ok((updated.id, Some(ingest)))
    }

    /// 检测两个JSON对象之间的变化
    /// 对于大型数据结构，使用迭代而非递归以避免栈溢出
    /// 超大数组（len>200）不展开字段路径；等长时仍做整数组相等比较。
    fn detect_changes(&self, old_data: &Value, new_data: &Value) -> Vec<String> {
        let mut changed_fields = Vec::new();

        // 内存保护：限制变化检测的总迭代次数
        const MAX_ITERATIONS: usize = 10000;

        // 使用栈模拟递归，避免栈溢出
        let mut stack: Vec<(String, &Value, &Value)> = vec![("".to_string(), old_data, new_data)];
        let max_depth = 50; // 限制最大深度
        let mut iteration_count = 0;

        while let Some((prefix, old, new)) = stack.pop() {
            iteration_count += 1;
            if iteration_count > MAX_ITERATIONS {
                // 防止无限循环或过度内存使用
                tracing::warn!(
                    "⚠️ detect_changes reached max iterations ({}), truncating comparison",
                    MAX_ITERATIONS
                );
                changed_fields.push(format!("{} (comparison truncated)", prefix));
                break;
            }

            match (old, new) {
                (Value::Object(old_map), Value::Object(new_map)) => {
                    // 检查新增和修改的字段
                    for (key, new_val) in new_map {
                        let field_path = if prefix.is_empty() {
                            key.clone()
                        } else {
                            format!("{}.{}", prefix, key)
                        };

                        match old_map.get(key) {
                            Some(old_val) => {
                                if old_val != new_val {
                                    // 限制递归深度
                                    let current_depth = field_path.matches('.').count();
                                    if current_depth < max_depth {
                                        stack.push((field_path, old_val, new_val));
                                    } else {
                                        changed_fields
                                            .push(format!("{} (deep change)", field_path));
                                    }
                                }
                            }
                            None => {
                                // 新增字段
                                changed_fields.push(field_path);
                            }
                        }
                    }

                    // 检查删除的字段
                    for key in old_map.keys() {
                        if !new_map.contains_key(key) {
                            let field_path = if prefix.is_empty() {
                                key.clone()
                            } else {
                                format!("{}.{}", prefix, key)
                            };
                            changed_fields.push(format!("{} (deleted)", field_path));
                        }
                    }
                }
                (Value::Array(old_arr), Value::Array(new_arr)) => {
                    const MAX_ARRAY_COMPARE: usize = 50; // 数组最多比较前50个元素

                    // 大数组不展开字段路径，但内容变化仍必须触发语义事件。
                    // 超大数组不展开字段路径，只记长度/内容变化标记。
                    if old_arr.len() > 200 || new_arr.len() > 200 {
                        if old_arr.len() != new_arr.len() {
                            changed_fields.push(format!(
                                "{} (large array length: {} -> {})",
                                prefix,
                                old_arr.len(),
                                new_arr.len()
                            ));
                        } else if old_arr != new_arr {
                            changed_fields
                                .push(format!("{} (large array content changed)", prefix));
                        }
                    } else {
                        // 中小型数组：检查长度和内容变化
                        if old_arr.len() != new_arr.len() {
                            changed_fields.push(format!(
                                "{} (array length: {} -> {})",
                                prefix,
                                old_arr.len(),
                                new_arr.len()
                            ));
                        } else {
                            // 只检查前N个元素的变化，避免处理超大数组
                            let check_count = old_arr.len().min(MAX_ARRAY_COMPARE);
                            for (i, (old_item, new_item)) in old_arr
                                .iter()
                                .zip(new_arr.iter())
                                .take(check_count)
                                .enumerate()
                            {
                                if old_item != new_item {
                                    let item_path = format!("{}[{}]", prefix, i);
                                    let current_depth = item_path.matches('.').count();
                                    if current_depth < max_depth {
                                        stack.push((item_path, old_item, new_item));
                                    } else {
                                        changed_fields.push(format!("{} (deep change)", item_path));
                                    }
                                }
                            }
                            if old_arr.len() > MAX_ARRAY_COMPARE {
                                tracing::debug!(
                                    "⚡ Array {} has {} more elements not checked",
                                    prefix,
                                    old_arr.len() - MAX_ARRAY_COMPARE
                                );
                            }
                        }
                    }
                }
                _ => {
                    // 其余形状（标量或混型）；根级空 prefix 不记。
                    if old != new && !prefix.is_empty() {
                        changed_fields.push(prefix);
                    }
                }
            }
        }

        changed_fields
    }

    /// 记录元数据变化历史。`< 50KB` 存完整 JSON；`>= 50KB` 只存变化摘要。完整库仍在 `platform_metadata`。
    async fn record_metadata_change<C: ConnectionTrait>(
        &self,
        db: &C,
        metadata_id: i32,
        user_id: i32,
        platform_name: &str,
        changed_fields: Vec<String>,
        old_data: Option<Value>,
        new_data: Value,
    ) -> Result<RecordedIngest, Box<dyn std::error::Error>> {
        // 输入体积开关：>= 此值只存摘要
        const MAX_SUMMARY_SIZE: usize = 50_000;

        let now = Utc::now().naive_utc();
        let activity = build_activity_payload(
            platform_name,
            old_data.as_ref(),
            &new_data,
            changed_fields.len(),
        );

        // 智能摘要策略：
        // 1. 对于小数据(<50KB)：保存完整数据
        // 2. 对于大数据(>=50KB)：只保存变化摘要，不保存完整JSON
        let new_data_size = Self::estimate_json_size(&new_data);

        let (new_data_to_save, old_data_to_save) = if new_data_size >= MAX_SUMMARY_SIZE {
            tracing::info!(
                "📊 Large metadata detected ({} bytes), saving change summary only",
                new_data_size
            );

            // 创建轻量摘要（changed_fields.len() + 当前快照计数，不是完整 raw）
            let new_summary =
                Self::create_change_summary(&new_data, platform_name, &changed_fields);
            let old_summary = old_data
                .as_ref()
                .map(|d| Self::create_change_summary(d, platform_name, &changed_fields));

            (Some(new_summary), old_summary)
        } else {
            // 小数据集：保存完整数据用于详细对比
            (Some(new_data), old_data)
        };

        let history = metadata_history::ActiveModel {
            metadata_id: Set(Some(metadata_id)),
            user_id: Set(user_id),
            platform_name: Set(platform_name.to_string()),
            changed_fields: Set(json!(changed_fields)),
            old_data: Set(old_data_to_save),
            new_data: Set(new_data_to_save),
            change_date: Set(now),
            ..Default::default()
        };

        let inserted_history = history.insert(db).await?;
        let activity_summary = platform_activity_summary(platform_name, &activity);
        let ingest = RecordedIngest {
            imported: activity.event_type == "imported",
            high_value: activity.importance >= 80 && activity.event_type != "suppressed",
            summary: activity_summary,
        };

        let activity_event = activity_events::ActiveModel {
            metadata_history_id: Set(inserted_history.id),
            metadata_id: Set(Some(metadata_id)),
            user_id: Set(user_id),
            platform_name: Set(platform_name.to_string()),
            event_type: Set(activity.event_type),
            title: Set(activity.title),
            changes: Set(serde_json::to_value(activity.changes)?),
            change_count: Set(i32::try_from(activity.change_count).unwrap_or(i32::MAX)),
            importance: Set(activity.importance),
            occurred_at: Set(now),
            created_at: Set(now),
            ..Default::default()
        };
        activity_event.insert(db).await?;
        tracing::info!(
            "✅ Metadata change history recorded (summary mode: {})",
            new_data_size >= MAX_SUMMARY_SIZE
        );
        Ok(ingest)
    }

    /// 估算 JSON 数据的大小（不进行实际序列化，避免OOM）
    /// 采用递归深度优先遍历，计算结构大小
    fn estimate_json_size(value: &Value) -> usize {
        const MAX_DEPTH: usize = 50;
        Self::estimate_json_size_recursive(value, 0, MAX_DEPTH)
    }

    /// 递归估算 JSON 大小
    fn estimate_json_size_recursive(value: &Value, depth: usize, max_depth: usize) -> usize {
        if depth > max_depth {
            return 100; // 深度过大，返回固定估值
        }

        match value {
            Value::Null => 4,    // "null"
            Value::Bool(_) => 5, // "true" or "false"
            Value::Number(n) => n.to_string().len(),
            Value::String(s) => s.len() + 2, // 包含引号
            Value::Array(arr) => {
                let mut size = 2; // []
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 {
                        size += 1; // 逗号
                    }
                    // 优化：对超大数组(>100元素)进行采样估算，避免遍历全部
                    if i < 100 {
                        size += Self::estimate_json_size_recursive(item, depth + 1, max_depth);
                    } else {
                        // 采样前100个元素的平均大小，推断剩余元素
                        let sample_avg = size / 100;
                        size += sample_avg * (arr.len() - 100);
                        break;
                    }
                }
                size
            }
            Value::Object(map) => {
                let mut size = 2; // {}
                for (i, (key, val)) in map.iter().enumerate() {
                    if i > 0 {
                        size += 1; // 逗号
                    }
                    size += key.len() + 3; // "key":
                    size += Self::estimate_json_size_recursive(val, depth + 1, max_depth);
                }
                size
            }
        }
    }

    /// 轻量摘要：变化字段计数 + 当前快照里的计数/id，不含完整 raw。
    fn create_change_summary(
        data: &Value,
        platform_name: &str,
        changed_fields: &[String],
    ) -> Value {
        match platform_name {
            "netease" => {
                json!({
                    "_type": "change_summary",
                    "_note": "Lightweight summary - full data in platform_metadata table",
                    "changed_fields_count": changed_fields.len(),
                    "profile_exists": data.get("profile").is_some(),
                    "liked_songs_count": data.get("liked_songs")
                        .and_then(|s| s.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
            }
            "steam" => {
                json!({
                    "_type": "change_summary",
                    "_note": "Lightweight summary - full data in platform_metadata table",
                    "changed_fields_count": changed_fields.len(),
                    "games_count": data.get("games")
                        .and_then(|g| g.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
            }
            "bilibili" => {
                json!({
                    "_type": "change_summary",
                    "_note": "Lightweight summary - full data in platform_metadata table",
                    "changed_fields_count": changed_fields.len(),
                    "videos_count": data.get("videos")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "bangumi_count": data.get("bangumi")
                        .and_then(|b| b.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
            }
            "github" => {
                json!({
                    "_type": "change_summary",
                    "_note": "Lightweight summary - full data in platform_metadata table",
                    "changed_fields_count": changed_fields.len(),
                    "repos_count": data.get("repos")
                        .and_then(|r| r.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
            }
            "youtube" => {
                json!({
                    "_type": "change_summary",
                    "_note": "Lightweight summary - full data in platform_metadata table",
                    "changed_fields_count": changed_fields.len(),
                    "videos_sample_count": data.get("videos")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "channel_id": data.pointer("/channel/id").cloned().unwrap_or(Value::Null),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
            }
            "x" => {
                json!({
                    "_type": "change_summary",
                    "_note": "Lightweight summary - full data in platform_metadata table",
                    "changed_fields_count": changed_fields.len(),
                    "tweets_count": data.get("tweets")
                        .and_then(|t| t.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
            }
            "mal" => {
                json!({
                    "_type": "change_summary",
                    "_note": "Lightweight summary - full data in platform_metadata table",
                    "changed_fields_count": changed_fields.len(),
                    "anime_count": data.get("anime_list")
                        .and_then(|t| t.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "manga_count": data.get("manga_list")
                        .and_then(|t| t.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
            }
            _ => {
                json!({
                    "_type": "change_summary",
                    "_note": "Lightweight summary - full data in platform_metadata table",
                    "changed_fields_count": changed_fields.len(),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
            }
        }
    }

    /// 获取所有平台的最新元数据
    /// 合并 `platform_name` 为 `*_chunk_*` 的分片行（如网易云 liked_songs）。
    pub async fn get_all_latest_metadata(
        &self,
        user_id: i32,
    ) -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
        let all_metadata = platform_metadata::Entity::find()
            .filter(platform_metadata::Column::UserId.eq(user_id))
            .order_by_desc(platform_metadata::Column::FetchedAt)
            .all(&self.db)
            .await?;

        // 按平台分组，取最新的
        let mut result = HashMap::new();
        let mut seen_platforms = std::collections::HashSet::new();
        // 收集分片数据以便后续合并
        // Key: (base_platform, chunk_index) -> Value: raw_data (只保留最新的)
        let mut chunk_data: HashMap<String, HashMap<i32, Value>> = HashMap::new();

        for metadata in &all_metadata {
            // 检查是否是分片记录 (如 netease_chunk_1)
            if metadata.platform_name.contains("_chunk_") {
                // 提取原始平台名称 (如 netease_chunk_1 -> netease)
                if let Some(base_platform) = metadata.platform_name.split("_chunk_").next() {
                    // 提取分片索引
                    if let Some(idx_str) = metadata.platform_name.split("_chunk_").nth(1) {
                        if let Ok(idx) = idx_str.parse::<i32>() {
                            // 由于数据按 fetched_at DESC 排序，只保留每个 chunk_index 的第一条（最新）
                            chunk_data
                                .entry(base_platform.to_string())
                                .or_default()
                                .entry(idx)
                                .or_insert_with(|| metadata.raw_data.clone());
                        }
                    }
                }
                continue;
            }

            if !seen_platforms.contains(&metadata.platform_name) {
                result.insert(metadata.platform_name.clone(), metadata.raw_data.clone());
                seen_platforms.insert(metadata.platform_name.clone());
            }
        }

        // 合并分片数据到主记录
        for (platform, chunks) in chunk_data {
            if let Some(main_data) = result.get_mut(&platform) {
                merge_chunk_rows(&platform, main_data, chunks);
            }
        }

        Ok(result)
    }

    /// 只读取一个平台的最新元数据（含其 `*_chunk_*` 分片行并合并），不读其它平台。
    ///
    /// 结果与 `get_all_latest_metadata(user_id).remove(platform)` 相同。
    pub async fn get_latest_platform_metadata(
        &self,
        user_id: i32,
        platform: &str,
    ) -> Result<Option<Value>, DbErr> {
        // 含 `_chunk_` 的名字本身就是分片行，不会作为主记录返回。
        if platform.contains("_chunk_") {
            return Ok(None);
        }
        let rows = platform_metadata::Entity::find()
            .select_only()
            .column(platform_metadata::Column::PlatformName)
            .column(platform_metadata::Column::RawData)
            .filter(platform_metadata::Column::UserId.eq(user_id))
            .filter(
                Condition::any()
                    .add(platform_metadata::Column::PlatformName.eq(platform))
                    // starts_with 按字面前缀匹配，平台名中的 `_` 不会被当作通配符。
                    .add(Expr::cust_with_values(
                        "starts_with(platform_name, $1)",
                        [format!("{platform}_chunk_")],
                    )),
            )
            .order_by_desc(platform_metadata::Column::FetchedAt)
            .into_tuple::<(String, Value)>()
            .all(&self.db)
            .await?;

        let mut main = None;
        let mut chunks: HashMap<i32, Value> = HashMap::new();
        for (name, raw_data) in rows {
            if name == platform {
                // 按 fetched_at DESC，只取第一条（最新）
                if main.is_none() {
                    main = Some(raw_data);
                }
                continue;
            }
            let mut parts = name.split("_chunk_");
            if parts.next() != Some(platform) {
                continue;
            }
            if let Some(idx) = parts.next().and_then(|idx| idx.parse::<i32>().ok()) {
                chunks.entry(idx).or_insert(raw_data);
            }
        }
        let Some(mut main) = main else {
            return Ok(None);
        };
        merge_chunk_rows(platform, &mut main, chunks);
        Ok(Some(main))
    }
}

/// 主记录标记 `_chunked` 时，把分片的 `songs` 按分片序号追加到 `liked_songs`。
fn merge_chunk_rows(platform: &str, main_data: &mut Value, chunks: HashMap<i32, Value>) {
    let is_chunked = main_data
        .get("_chunked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_chunked {
        return;
    }
    // 按分片索引排序 (HashMap -> Vec，按 key 排序)
    let mut sorted_chunks: Vec<_> = chunks.into_iter().collect();
    sorted_chunks.sort_by_key(|(idx, _)| *idx);
    let chunks_count = sorted_chunks.len();

    // 合并所有分片的 songs 到 liked_songs
    if let Some(main_songs) = main_data
        .get_mut("liked_songs")
        .and_then(|s| s.as_array_mut())
    {
        let original_count = main_songs.len();
        for (idx, mut chunk) in sorted_chunks {
            if let Some(Value::Array(chunk_songs)) = chunk.get_mut("songs").map(Value::take) {
                tracing::debug!("🎵 Merged chunk {} with {} songs", idx, chunk_songs.len());
                main_songs.extend(chunk_songs);
            }
        }
        tracing::info!(
            "✅ Merged {} chunks for {}: {} -> {} songs",
            chunks_count,
            platform,
            original_count,
            main_songs.len()
        );
    }
}

struct RecordedIngest {
    imported: bool,
    high_value: bool,
    summary: String,
}

fn is_unique_metadata(err: &DbErr) -> bool {
    let s = err.to_string().to_lowercase();
    s.contains("duplicate key") || s.contains("unique constraint") || s.contains("23505")
}

fn platform_activity_summary(platform: &str, activity: &ActivityPayload) -> String {
    let label = platform_label(platform);
    if activity.event_type == "imported" {
        return format!("{label} finished the first import");
    }
    let heads: Vec<&str> = activity
        .changes
        .iter()
        .filter_map(|change| change.subject_title.as_deref())
        .take(3)
        .collect();
    if heads.is_empty() {
        format!("{label}: {}", activity.title)
    } else {
        format!("{label}: {}", heads.join(", "))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn save_platform_metadata_commits_snapshot_and_history_together() {
        let src = include_str!("metadata_service.rs");
        let save = src
            .split("pub async fn save_platform_metadata")
            .nth(1)
            .and_then(|rest| rest.split("async fn update_existing_metadata").next())
            .expect("save_platform_metadata");
        assert!(save.contains("self.db.begin()"));
        assert!(save.contains("SAVEPOINT metadata_insert"));
        assert!(save.contains("ROLLBACK TO SAVEPOINT metadata_insert"));
        assert!(save.contains("txn.commit()"));
        assert!(
            save.find("txn.commit()").unwrap() < save.find("spawn_diary").unwrap(),
            "diary side effects must wait until snapshot+history commit"
        );
    }
}

#[cfg(test)]
mod platform_read_tests {
    use super::*;
    use sea_orm::{Database, DatabaseBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn single_platform_read_matches_full_read_when_db_provided() {
        let Ok(url) = std::env::var("METADATA_TEST_DATABASE_URL") else {
            return;
        };
        let db = Database::connect(&url).await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        let user_id: i32 = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ($1) RETURNING id",
                [format!("metadata-{}", uuid::Uuid::new_v4().simple()).into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();
        let now = Utc::now().naive_utc();
        for (name, raw) in [
            ("netease", json!({ "_chunked": true, "liked_songs": [1] })),
            ("netease_chunk_2", json!({ "songs": [3] })),
            ("netease_chunk_1", json!({ "songs": [2] })),
            ("neteasex_chunk_1", json!({ "songs": [99] })),
            ("github", json!({ "repos": 1 })),
        ] {
            platform_metadata::ActiveModel {
                user_id: Set(user_id),
                platform_name: Set(name.into()),
                raw_data: Set(raw),
                fetched_at: Set(now),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let service = MetadataService::new(db.clone());
        let mut all = service.get_all_latest_metadata(user_id).await.unwrap();
        let netease = service
            .get_latest_platform_metadata(user_id, "netease")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(netease["liked_songs"], json!([1, 2, 3]));
        assert_eq!(Some(netease), all.remove("netease"));
        assert_eq!(
            service
                .get_latest_platform_metadata(user_id, "github")
                .await
                .unwrap(),
            all.remove("github")
        );
        for missing in ["steam", "netease_chunk_1", "neteasex"] {
            assert_eq!(
                service
                    .get_latest_platform_metadata(user_id, missing)
                    .await
                    .unwrap(),
                None,
                "{missing}"
            );
        }
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM platform_metadata WHERE user_id = $1",
            [user_id.into()],
        ))
        .await
        .unwrap();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .unwrap();
    }
}
