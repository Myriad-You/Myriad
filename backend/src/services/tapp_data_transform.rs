//! Pure declarative data transform pipeline for Tapp data.transform.
//!
//! Filter/sort/map/aggregate steps are free of HTTP and storage I/O so agent
//! and API paths share one evaluator. Handlers load/save items and call
//! [`apply_pipeline`].

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const MAX_PIPELINE_STEPS: usize = 20;
pub const MAX_MAP_OPERATIONS: usize = 50;

/// Domain errors for pure pipeline evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataTransformError {
    TooManySteps,
    TooManyMapOps,
}

impl DataTransformError {
    #[allow(dead_code)] // Stable machine code for future API adapters.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooManySteps => "TRANSFORM_TOO_MANY_STEPS",
            Self::TooManyMapOps => "TRANSFORM_TOO_MANY_MAP_OPS",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::TooManySteps => "Too many pipeline steps (max 20)",
            Self::TooManyMapOps => "Too many map operations (max 50)",
        }
    }

    pub fn status_hint(&self) -> u16 {
        400
    }
}

impl std::fmt::Display for DataTransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for DataTransformError {}

#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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

/// Run a full pipeline with the historical step/map caps.
pub fn apply_pipeline(
    mut items: Vec<Value>,
    pipeline: Vec<ProcessStep>,
) -> Result<Vec<Value>, DataTransformError> {
    if pipeline.len() > MAX_PIPELINE_STEPS {
        return Err(DataTransformError::TooManySteps);
    }
    for step in pipeline {
        items = apply_process_step(items, step)?;
    }
    Ok(items)
}

pub fn apply_process_step(
    mut items: Vec<Value>,
    step: ProcessStep,
) -> Result<Vec<Value>, DataTransformError> {
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
                    "in" => value
                        .as_array()
                        .map(|arr| item_value.map(|v| arr.contains(v)).unwrap_or(false))
                        .unwrap_or(false),
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
            if operations.len() > MAX_MAP_OPERATIONS {
                return Err(DataTransformError::TooManyMapOps);
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
pub fn apply_map_op(item: &mut Value, op: &MapOp) {
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

/// Normalize inline/platform/storage payloads into a list of items.
pub fn items_from_value(data: Value) -> Vec<Value> {
    data.as_array().cloned().unwrap_or_else(|| vec![data])
}

/// Agent `data.transform` input shape: array, or object with `items`, else empty.
///
/// Differs from [`items_from_value`] which wraps a lone object as a single item.
pub fn items_from_agent_input(input: Value) -> Vec<Value> {
    match input {
        Value::Array(arr) => arr,
        Value::Object(obj) => obj
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// Deserialize free-form pipeline JSON steps.
///
/// Unknown step shapes are skipped (historical agent `data.transform` behavior
/// for unrecognized `type` values). Length is still capped at
/// [`MAX_PIPELINE_STEPS`].
pub fn parse_pipeline_steps_lenient(
    pipeline: &[Value],
) -> Result<Vec<ProcessStep>, DataTransformError> {
    if pipeline.len() > MAX_PIPELINE_STEPS {
        return Err(DataTransformError::TooManySteps);
    }
    Ok(pipeline
        .iter()
        .filter_map(|step| serde_json::from_value(step.clone()).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_map_op, apply_pipeline, items_from_agent_input, items_from_value,
        parse_pipeline_steps_lenient, DataTransformError, MapOp, ProcessStep, MAX_PIPELINE_STEPS,
    };
    use serde_json::json;

    #[test]
    fn filter_eq_and_limit() {
        let items = vec![
            json!({ "name": "a", "score": 1 }),
            json!({ "name": "b", "score": 2 }),
            json!({ "name": "c", "score": 2 }),
        ];
        let out = apply_pipeline(
            items,
            vec![
                ProcessStep::Filter {
                    field: "score".into(),
                    operator: "eq".into(),
                    value: json!(2),
                },
                ProcessStep::Limit { count: 1 },
            ],
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["name"], "b");
    }

    #[test]
    fn agent_input_and_lenient_pipeline_json() {
        assert_eq!(
            items_from_agent_input(json!([{ "a": 1 }, { "a": 2 }])).len(),
            2
        );
        assert_eq!(
            items_from_agent_input(json!({ "items": [{ "x": 1 }], "meta": true })).len(),
            1
        );
        assert!(items_from_agent_input(json!({ "not": "items" })).is_empty());
        assert!(items_from_agent_input(json!("scalar")).is_empty());

        let steps = parse_pipeline_steps_lenient(&[
            json!({ "type": "filter", "field": "a", "operator": "eq", "value": 1 }),
            json!({ "type": "unknown_noop" }),
            json!({ "type": "limit", "count": 5 }),
        ])
        .unwrap();
        assert_eq!(steps.len(), 2);

        let too_many = (0..=MAX_PIPELINE_STEPS)
            .map(|_| json!({ "type": "limit", "count": 1 }))
            .collect::<Vec<_>>();
        assert_eq!(
            parse_pipeline_steps_lenient(&too_many).unwrap_err(),
            DataTransformError::TooManySteps
        );
    }

    #[test]
    fn map_rename_template_and_coalesce() {
        let mut item = json!({ "title": "Song", "artist": "A", "name_cn": "", "name_en": "EN" });
        apply_map_op(
            &mut item,
            &MapOp::Rename {
                from: "title".into(),
                to: "name".into(),
            },
        );
        apply_map_op(
            &mut item,
            &MapOp::Template {
                field: "label".into(),
                template: "{name} - {artist}".into(),
            },
        );
        apply_map_op(
            &mut item,
            &MapOp::Coalesce {
                fields: vec!["name_cn".into(), "name_en".into()],
                to: "display".into(),
            },
        );
        assert_eq!(item["name"], "Song");
        assert_eq!(item["label"], "Song - A");
        assert_eq!(item["display"], "EN");
    }

    #[test]
    fn pipeline_step_cap() {
        let steps = (0..=MAX_PIPELINE_STEPS)
            .map(|_| ProcessStep::Limit { count: 10 })
            .collect::<Vec<_>>();
        let err = apply_pipeline(vec![json!({})], steps).unwrap_err();
        assert_eq!(err, DataTransformError::TooManySteps);
        assert_eq!(err.message(), "Too many pipeline steps (max 20)");
    }

    #[test]
    fn map_op_cap() {
        let ops = (0..51)
            .map(|i| MapOp::Set {
                field: format!("f{i}"),
                value: json!(i),
            })
            .collect();
        let err = apply_pipeline(
            vec![json!({})],
            vec![ProcessStep::Map { operations: ops }],
        )
        .unwrap_err();
        assert_eq!(err, DataTransformError::TooManyMapOps);
    }

    #[test]
    fn items_from_value_wraps_objects() {
        assert_eq!(items_from_value(json!([1, 2])).len(), 2);
        assert_eq!(items_from_value(json!({ "a": 1 })).len(), 1);
    }

    #[test]
    fn aggregate_count_and_sum() {
        let items = vec![json!({ "n": 1 }), json!({ "n": 3 })];
        let out = apply_pipeline(
            items,
            vec![ProcessStep::Aggregate {
                operation: "sum".into(),
                field: Some("n".into()),
            }],
        )
        .unwrap();
        assert_eq!(out[0]["sum"], 4.0);
    }
}
