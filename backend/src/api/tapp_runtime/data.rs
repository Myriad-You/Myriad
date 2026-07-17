//! 数据转换处理 API

use axum::{extract::State, http::StatusCode, Extension, Json};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;

use super::common::{
    acquire_platform_lock, authorize_tapp_permissions, get_cached_platform_data, parse_user_id,
    update_cached_platform_data, validate_platform_name, verify_tapp_ownership,
};
use super::runtime_grant::RuntimeGrantContext;
use crate::api::tapp_store::{
    storage_write_forbidden_error, validate_storage_key, validate_storage_value_size,
    TappStorageAccess,
};

#[derive(Debug, Deserialize)]
pub struct DataTransformRequest {
    pub tapp_id: String,
    pub input: DataInput,
    pub pipeline: Vec<ProcessStep>,
    pub output: Option<DataOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "source")]
pub enum DataInput {
    #[serde(rename = "platform")]
    Platform { platform: String },
    #[serde(rename = "storage")]
    Storage { key: String },
    #[serde(rename = "inline")]
    Inline { data: Value },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "target")]
pub enum DataOutput {
    #[serde(rename = "platform")]
    Platform { platform: String },
    #[serde(rename = "storage")]
    Storage { key: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ProcessStep {
    #[serde(rename = "filter")]
    Filter {
        field: String,
        operator: String,
        value: Value,
    },
    #[serde(rename = "sort")]
    Sort {
        field: String,
        order: Option<String>,
    },
    #[serde(rename = "limit")]
    Limit { count: usize },
    #[serde(rename = "offset")]
    Offset { count: usize },
    #[serde(rename = "select")]
    Select { fields: Vec<String> },
    #[serde(rename = "group")]
    Group { by: String },
    #[serde(rename = "aggregate")]
    Aggregate {
        operation: String,
        field: Option<String>,
    },
    #[serde(rename = "dedupe")]
    Dedupe { key: String },
    #[serde(rename = "map")]
    Map { operations: Vec<MapOp> },
}

/// 声明式字段映射操作（纯数据驱动，无表达式求值，杜绝注入风险）
#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
pub enum MapOp {
    /// 重命名字段：`{ "op": "rename", "from": "old", "to": "new" }`
    #[serde(rename = "rename")]
    Rename { from: String, to: String },
    /// 删除字段：`{ "op": "remove", "field": "name" }`
    #[serde(rename = "remove")]
    Remove { field: String },
    /// 设置静态值：`{ "op": "set", "field": "status", "value": "active" }`
    #[serde(rename = "set")]
    Set { field: String, value: Value },
    /// 复制字段：`{ "op": "copy", "from": "title", "to": "name" }`
    #[serde(rename = "copy")]
    Copy { from: String, to: String },
    /// 模板插值：`{ "op": "template", "field": "label", "template": "{title} - {artist}" }`
    /// 仅支持 `{fieldName}` 占位符，不执行任何表达式
    #[serde(rename = "template")]
    Template { field: String, template: String },
    /// 转小写：`{ "op": "lower", "field": "name" }`
    #[serde(rename = "lower")]
    Lower { field: String },
    /// 转大写：`{ "op": "upper", "field": "name" }`
    #[serde(rename = "upper")]
    Upper { field: String },
    /// 转字符串：`{ "op": "to_string", "field": "count" }`
    #[serde(rename = "to_string")]
    ToString { field: String },
    /// 转数字：`{ "op": "to_number", "field": "score" }`
    #[serde(rename = "to_number")]
    ToNumber { field: String },
    /// 取默认值：`{ "op": "default", "field": "cover", "value": "/placeholder.png" }`
    #[serde(rename = "default")]
    Default { field: String, value: Value },
    /// 字段拼接：`{ "op": "concat", "fields": ["first", "last"], "separator": " ", "to": "name" }`
    #[serde(rename = "concat")]
    Concat {
        fields: Vec<String>,
        separator: Option<String>,
        to: String,
    },
    /// 多字段取优先非空值：`{ "op": "coalesce", "fields": ["name_cn", "name_en", "id"], "to": "display_name" }`
    #[serde(rename = "coalesce")]
    Coalesce { fields: Vec<String>, to: String },
}

/// POST /api/tapp/data/transform
pub async fn data_transform(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<DataTransformRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    let mut required_permissions = Vec::with_capacity(2);
    match &req.input {
        DataInput::Platform { .. } => required_permissions.push(TappPermission::PlatformRead),
        DataInput::Storage { key } => {
            validate_storage_key(key)
                .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
            required_permissions.push(TappPermission::Storage);
        }
        DataInput::Inline { .. } => {}
    }
    match &req.output {
        Some(DataOutput::Platform { .. }) => {
            required_permissions.push(TappPermission::PlatformWrite)
        }
        Some(DataOutput::Storage { key }) => {
            validate_storage_key(key)
                .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
            if !required_permissions.contains(&TappPermission::Storage) {
                required_permissions.push(TappPermission::Storage);
            }
        }
        None => {}
    }
    for permission in &required_permissions {
        runtime_grant.require(*permission)?;
    }

    if required_permissions.is_empty() {
        let user_id = parse_user_id(&claims)?;
        verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;
    } else {
        authorize_tapp_permissions(&db, &claims, &req.tapp_id, &required_permissions).await?;
    }
    let storage_access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)
        .map_err(|status| (status, Json(json!({ "error": "Invalid runtime grant subject" }))))?;
    // Storage I/O is namespaced by installation owner even when the pipeline mixes
    // other data sources. Non-owners may read but never write owner storage.
    let storage_owner_id = storage_access.storage_namespace();

    tracing::debug!(
        "[TAPP] data_transform - User: {}, Tapp: {}, Steps: {}",
        claims.username,
        req.tapp_id,
        req.pipeline.len()
    );

    if req.pipeline.len() > 20 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Too many pipeline steps (max 20)" })),
        ));
    }

    // 1. 获取输入数据
    let mut items: Vec<Value> = match req.input {
        DataInput::Platform { platform } => {
            let data = get_cached_platform_data(&platform).await.map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error })),
                )
            })?;
            data.get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        }
        DataInput::Storage { key } => {
            use crate::models::entities::tapp_storage;
            let item = tapp_storage::Entity::find()
                .filter(tapp_storage::Column::UserId.eq(storage_owner_id))
                .filter(tapp_storage::Column::TappId.eq(&req.tapp_id))
                .filter(tapp_storage::Column::Key.eq(&key))
                .one(&db)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": "Failed to read storage" })),
                    )
                })?;
            match item {
                Some(i) => i.value.as_array().cloned().unwrap_or_default(),
                None => Vec::new(),
            }
        }
        DataInput::Inline { data } => data.as_array().cloned().unwrap_or_else(|| vec![data]),
    };

    // 2. 执行处理管道
    for step in req.pipeline {
        items = apply_process_step(items, step)?;
    }

    // 3. 输出结果
    if let Some(output) = req.output {
        match output {
            DataOutput::Platform { platform } => {
                validate_platform_name(&platform)
                    .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
                let _platform_guard = acquire_platform_lock(&platform)
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
                let cache_dir = std::path::Path::new("cache/platforms");
                let cache_file =
                    cache_dir.join(format!("{}_filtered.json", platform.to_lowercase()));
                let data = json!({ "items": items });
                tokio::fs::create_dir_all(cache_dir).await.map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": "Failed to create platform cache directory" })),
                    )
                })?;
                let tmp_file =
                    cache_file.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
                tokio::fs::write(&tmp_file, serde_json::to_vec_pretty(&data).unwrap())
                    .await
                    .map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": "Failed to write platform data" })),
                        )
                    })?;
                if tokio::fs::rename(&tmp_file, &cache_file).await.is_err() {
                    let _ = tokio::fs::remove_file(&tmp_file).await;
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": "Failed to commit platform data" })),
                    ));
                }
                update_cached_platform_data(&platform, data)
                    .await
                    .map_err(|error| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": error })),
                        )
                    })?;
            }
            DataOutput::Storage { key } => {
                use crate::models::entities::tapp_storage;
                use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

                storage_access
                    .require_write()
                    .map_err(|_| storage_write_forbidden_error())?;
                let storage_value = json!(items);
                validate_storage_value_size(&storage_value).map_err(|status| {
                    (status, Json(json!({ "error": "Storage value too large" })))
                })?;
                let now = chrono::Utc::now().fixed_offset();
                let existing = tapp_storage::Entity::find()
                    .filter(tapp_storage::Column::UserId.eq(storage_owner_id))
                    .filter(tapp_storage::Column::TappId.eq(&req.tapp_id))
                    .filter(tapp_storage::Column::Key.eq(&key))
                    .one(&db)
                    .await
                    .map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": "Failed to check storage" })),
                        )
                    })?;

                if let Some(item) = existing {
                    let mut active: tapp_storage::ActiveModel = item.into();
                    active.value = Set(storage_value.clone());
                    active.updated_at = Set(now);
                    active.update(&db).await.map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": "Failed to update storage" })),
                        )
                    })?;
                } else {
                    let new_item = tapp_storage::ActiveModel {
                        id: NotSet,
                        tapp_id: Set(req.tapp_id.clone()),
                        user_id: Set(storage_owner_id),
                        key: Set(key),
                        value: Set(storage_value),
                        created_at: Set(now),
                        updated_at: Set(now),
                    };
                    new_item.insert(&db).await.map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": "Failed to save storage" })),
                        )
                    })?;
                }
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "count": items.len(),
        "data": items
    })))
}

fn apply_process_step(
    mut items: Vec<Value>,
    step: ProcessStep,
) -> Result<Vec<Value>, (StatusCode, Json<Value>)> {
    match step {
        ProcessStep::Filter {
            field,
            operator,
            value,
        } => {
            items.retain(|item| {
                let item_value = item.get(&field);
                match operator.as_str() {
                    "eq" => item_value == Some(&value),
                    "ne" => item_value != Some(&value),
                    "gt" => matches!((item_value.and_then(|v| v.as_f64()), value.as_f64()), (Some(a), Some(b)) if a > b),
                    "gte" => matches!((item_value.and_then(|v| v.as_f64()), value.as_f64()), (Some(a), Some(b)) if a >= b),
                    "lt" => matches!((item_value.and_then(|v| v.as_f64()), value.as_f64()), (Some(a), Some(b)) if a < b),
                    "lte" => matches!((item_value.and_then(|v| v.as_f64()), value.as_f64()), (Some(a), Some(b)) if a <= b),
                    "contains" => matches!((item_value.and_then(|v| v.as_str()), value.as_str()), (Some(a), Some(b)) if a.contains(b)),
                    "in" => value.as_array().map(|arr| item_value.map(|v| arr.contains(v)).unwrap_or(false)).unwrap_or(false),
                    "exists" => item_value.is_some() && !item_value.unwrap().is_null(),
                    _ => true,
                }
            });
        }
        ProcessStep::Sort { field, order } => {
            let desc = order.as_deref() == Some("desc");
            items.sort_by(|a, b| {
                let va = a.get(&field);
                let vb = b.get(&field);
                let cmp = match (va, vb) {
                    (Some(Value::Number(a)), Some(Value::Number(b))) => a
                        .as_f64()
                        .partial_cmp(&b.as_f64())
                        .unwrap_or(std::cmp::Ordering::Equal),
                    (Some(Value::String(a)), Some(Value::String(b))) => a.cmp(b),
                    _ => std::cmp::Ordering::Equal,
                };
                if desc {
                    cmp.reverse()
                } else {
                    cmp
                }
            });
        }
        ProcessStep::Limit { count } => {
            items.truncate(count);
        }
        ProcessStep::Offset { count } => {
            items = items.into_iter().skip(count).collect();
        }
        ProcessStep::Select { fields } => {
            items = items
                .into_iter()
                .map(|item| {
                    let mut new_item = json!({});
                    if let Some(obj) = item.as_object() {
                        for field in &fields {
                            if let Some(value) = obj.get(field) {
                                new_item[field] = value.clone();
                            }
                        }
                    }
                    new_item
                })
                .collect();
        }
        ProcessStep::Group { by } => {
            let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
            for item in items {
                let key = item
                    .get(&by)
                    .and_then(|v| v.as_str())
                    .unwrap_or("_unknown")
                    .to_string();
                groups.entry(key).or_default().push(item);
            }
            items = groups
                .into_iter()
                .map(|(key, values)| json!({ "key": key, "items": values, "count": values.len() }))
                .collect();
        }
        ProcessStep::Aggregate { operation, field } => {
            let result = match operation.as_str() {
                "count" => json!({ "count": items.len() }),
                "sum" => {
                    let sum: f64 = items
                        .iter()
                        .filter_map(|i| {
                            field
                                .as_ref()
                                .and_then(|f| i.get(f))
                                .and_then(|v| v.as_f64())
                        })
                        .sum();
                    json!({ "sum": sum })
                }
                "avg" => {
                    let values: Vec<f64> = items
                        .iter()
                        .filter_map(|i| {
                            field
                                .as_ref()
                                .and_then(|f| i.get(f))
                                .and_then(|v| v.as_f64())
                        })
                        .collect();
                    let avg = if values.is_empty() {
                        0.0
                    } else {
                        values.iter().sum::<f64>() / values.len() as f64
                    };
                    json!({ "avg": avg })
                }
                "min" => {
                    let min = items
                        .iter()
                        .filter_map(|i| {
                            field
                                .as_ref()
                                .and_then(|f| i.get(f))
                                .and_then(|v| v.as_f64())
                        })
                        .fold(f64::INFINITY, f64::min);
                    json!({ "min": if min.is_infinite() { Value::Null } else { json!(min) } })
                }
                "max" => {
                    let max = items
                        .iter()
                        .filter_map(|i| {
                            field
                                .as_ref()
                                .and_then(|f| i.get(f))
                                .and_then(|v| v.as_f64())
                        })
                        .fold(f64::NEG_INFINITY, f64::max);
                    json!({ "max": if max.is_infinite() { Value::Null } else { json!(max) } })
                }
                _ => json!({ "error": "Unknown aggregation" }),
            };
            items = vec![result];
        }
        ProcessStep::Dedupe { key } => {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            items.retain(|item| {
                let k = item.get(&key).map(|v| v.to_string()).unwrap_or_default();
                seen.insert(k)
            });
        }
        ProcessStep::Map { operations } => {
            if operations.len() > 50 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Too many map operations (max 50)" })),
                ));
            }
            items = items
                .into_iter()
                .map(|mut item| {
                    for op in &operations {
                        apply_map_op(&mut item, op);
                    }
                    item
                })
                .collect();
        }
    }
    Ok(items)
}

/// 对单个 item 执行一个声明式映射操作
fn apply_map_op(item: &mut Value, op: &MapOp) {
    let obj = match item.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    match op {
        MapOp::Rename { from, to } => {
            if let Some(val) = obj.remove(from.as_str()) {
                obj.insert(to.clone(), val);
            }
        }
        MapOp::Remove { field } => {
            obj.remove(field.as_str());
        }
        MapOp::Set { field, value } => {
            obj.insert(field.clone(), value.clone());
        }
        MapOp::Copy { from, to } => {
            if let Some(val) = obj.get(from.as_str()).cloned() {
                obj.insert(to.clone(), val);
            }
        }
        MapOp::Template { field, template } => {
            // 安全模板：仅支持 {fieldName} 占位符，不做嵌套/递归
            let mut result = template.clone();
            for (k, v) in obj.iter() {
                let placeholder = format!("{{{}}}", k);
                if result.contains(&placeholder) {
                    let replacement = match v {
                        Value::String(s) => s.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    result = result.replace(&placeholder, &replacement);
                }
            }
            obj.insert(field.clone(), json!(result));
        }
        MapOp::Lower { field } => {
            if let Some(Value::String(s)) = obj.get(field.as_str()) {
                let lowered = s.to_lowercase();
                obj.insert(field.clone(), json!(lowered));
            }
        }
        MapOp::Upper { field } => {
            if let Some(Value::String(s)) = obj.get(field.as_str()) {
                let uppered = s.to_uppercase();
                obj.insert(field.clone(), json!(uppered));
            }
        }
        MapOp::ToString { field } => {
            if let Some(val) = obj.get(field.as_str()) {
                let s = match val {
                    Value::String(_) => return, // 已经是字符串
                    Value::Null => "".to_string(),
                    other => other.to_string(),
                };
                obj.insert(field.clone(), json!(s));
            }
        }
        MapOp::ToNumber { field } => {
            if let Some(Value::String(s)) = obj.get(field.as_str()) {
                if let Ok(n) = s.parse::<f64>() {
                    obj.insert(field.clone(), json!(n));
                }
            }
        }
        MapOp::Default { field, value } => {
            let needs_default = match obj.get(field.as_str()) {
                None | Some(Value::Null) => true,
                Some(Value::String(s)) if s.is_empty() => true,
                _ => false,
            };
            if needs_default {
                obj.insert(field.clone(), value.clone());
            }
        }
        MapOp::Concat {
            fields,
            separator,
            to,
        } => {
            let sep = separator.as_deref().unwrap_or("");
            let parts: Vec<String> = fields
                .iter()
                .filter_map(|f| {
                    obj.get(f.as_str()).and_then(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        Value::Null => None,
                        other => Some(other.to_string()),
                    })
                })
                .collect();
            if !parts.is_empty() {
                obj.insert(to.clone(), json!(parts.join(sep)));
            }
        }
        MapOp::Coalesce { fields, to } => {
            for f in fields {
                match obj.get(f.as_str()) {
                    Some(Value::Null) | None => continue,
                    Some(Value::String(s)) if s.is_empty() => continue,
                    Some(val) => {
                        obj.insert(to.clone(), val.clone());
                        break;
                    }
                }
            }
        }
    }
}
