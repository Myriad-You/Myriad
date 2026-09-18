//! Expected table definitions (tables_analytics).
use super::types::{ColumnDef, TableDef};

pub(crate) fn tables() -> Vec<TableDef> {
    vec![
        TableDef {
            name: "analytics_page_daily".to_string(),
            columns: vec![
                ColumnDef {
                    name: "day".into(),
                    data_type: "date".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "path".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "views".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "unique_visitors".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "engagement_ms".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "engaged_views".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "analytics_visitor_seen".to_string(),
            columns: vec![
                ColumnDef {
                    name: "day".into(),
                    data_type: "date".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "path".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "visitor_hash".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "ordinal".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "analytics_event_daily".to_string(),
            columns: vec![
                ColumnDef {
                    name: "day".into(),
                    data_type: "date".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "event_name".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "path".into(),
                    data_type: "text".into(),
                    default_value: Some("''".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "target".into(),
                    data_type: "text".into(),
                    default_value: Some("''".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "count".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "unique_visitors".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "analytics_event_visitor".to_string(),
            columns: vec![
                ColumnDef {
                    name: "day".into(),
                    data_type: "date".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "event_name".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "path".into(),
                    data_type: "text".into(),
                    default_value: Some("''".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "target".into(),
                    data_type: "text".into(),
                    default_value: Some("''".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "visitor_hash".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "analytics_referrer_daily".to_string(),
            columns: vec![
                ColumnDef {
                    name: "day".into(),
                    data_type: "date".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "host".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "count".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "analytics_country_daily".to_string(),
            columns: vec![
                ColumnDef {
                    name: "day".into(),
                    data_type: "date".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "country_code".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "country_name".into(),
                    data_type: "text".into(),
                    default_value: Some("''".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "views".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "unique_visitors".into(),
                    data_type: "bigint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "analytics_country_visitor".to_string(),
            columns: vec![
                ColumnDef {
                    name: "day".into(),
                    data_type: "date".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "country_code".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "visitor_hash".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
    ]
}
