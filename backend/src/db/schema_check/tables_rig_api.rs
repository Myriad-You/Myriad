//! Expected schema for the rig API's tenants, keys and quota counters
//! (migration 004, `rig_api.sql`; healed by `ensure_rig_api_tables`).
use super::types::{ColumnDef, TableDef};

pub(crate) fn tables() -> Vec<TableDef> {
    vec![
        TableDef {
            name: "rig_api_tenants".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("owner_user_id", "integer").not_null(),
                ColumnDef::new("name", "character varying").not_null(),
                ColumnDef::new("max_concurrent_jobs", "integer"),
                ColumnDef::new("max_jobs_per_period", "integer"),
                ColumnDef::new("job_period", "character varying"),
                ColumnDef::new("requests_per_minute", "integer"),
                ColumnDef::new("disabled_at", "timestamp with time zone"),
                ColumnDef::new("created_at", "timestamp with time zone")
                    .not_null()
                    .default_value("now()"),
                ColumnDef::new("updated_at", "timestamp with time zone")
                    .not_null()
                    .default_value("now()"),
            ],
        },
        TableDef {
            name: "rig_api_keys".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("tenant_id", "character varying").not_null(),
                ColumnDef::new("name", "character varying")
                    .not_null()
                    .default_value("''"),
                ColumnDef::new("prefix", "character varying").not_null(),
                ColumnDef::new("secret_hash", "character varying").not_null(),
                ColumnDef::new("created_by_user_id", "integer"),
                ColumnDef::new("created_at", "timestamp with time zone")
                    .not_null()
                    .default_value("now()"),
                ColumnDef::new("expires_at", "timestamp with time zone"),
                ColumnDef::new("revoked_at", "timestamp with time zone"),
                ColumnDef::new("last_used_at", "timestamp with time zone"),
            ],
        },
        TableDef {
            name: "rig_api_job_usage".to_string(),
            columns: vec![
                ColumnDef::new("tenant_id", "character varying").not_null(),
                ColumnDef::new("period_start", "timestamp with time zone").not_null(),
                ColumnDef::new("jobs", "integer")
                    .not_null()
                    .default_value("0"),
            ],
        },
        TableDef {
            name: "rig_api_job_slots".to_string(),
            columns: vec![
                ColumnDef::new("tenant_id", "character varying").not_null(),
                ColumnDef::new("job_id", "character varying").not_null(),
                ColumnDef::new("period_start", "timestamp with time zone").not_null(),
                ColumnDef::new("reserved_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "rig_api_request_windows".to_string(),
            columns: vec![
                ColumnDef::new("tenant_id", "character varying").not_null(),
                ColumnDef::new("window_start", "timestamp with time zone").not_null(),
                ColumnDef::new("requests", "integer").not_null(),
            ],
        },
    ]
}
