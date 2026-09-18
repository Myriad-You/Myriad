//! DB introspection and DDL helpers.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};
use std::collections::{HashMap, HashSet};

use super::types::{ColumnDef, IndexDef};

pub(crate) async fn get_all_table_columns(
    db: &DatabaseConnection,
) -> Result<HashMap<String, HashSet<String>>, DbErr> {
    let sql = r#"
        SELECT table_name, column_name
        FROM information_schema.columns
        WHERE table_schema = 'public'
    "#;

    let rows = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await?;

    let mut columns: HashMap<String, HashSet<String>> = HashMap::new();
    for row in rows {
        let Ok(table) = row.try_get::<String>("", "table_name") else {
            continue;
        };
        let Ok(column) = row.try_get::<String>("", "column_name") else {
            continue;
        };
        columns.entry(table).or_default().insert(column);
    }

    Ok(columns)
}

/// 从数据库获取所有表名
pub(crate) async fn get_existing_tables(db: &DatabaseConnection) -> Result<HashSet<String>, DbErr> {
    let sql = r#"
        SELECT table_name
        FROM information_schema.tables
        WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
    "#;

    let rows = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await?;

    let mut tables = HashSet::new();
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "table_name") {
            tables.insert(name);
        }
    }

    Ok(tables)
}

/// 从数据库获取现有索引
pub(crate) async fn get_existing_indexes(
    db: &DatabaseConnection,
) -> Result<HashSet<String>, DbErr> {
    let sql = r#"
        SELECT indexname
        FROM pg_indexes
        WHERE schemaname = 'public'
    "#;

    let rows = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await?;

    let mut indexes = HashSet::new();
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "indexname") {
            indexes.insert(name);
        }
    }

    Ok(indexes)
}

/// 生成 ADD COLUMN DDL
pub(crate) fn generate_add_column_ddl(table: &str, col: &ColumnDef) -> String {
    let mut ddl = format!(
        "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {}",
        table, col.name, col.data_type
    );

    if col.not_null {
        ddl.push_str(" NOT NULL");
    }

    if let Some(ref default) = col.default_value {
        ddl.push_str(&format!(" DEFAULT {}", default));
    }

    ddl
}

/// 生成 CREATE INDEX DDL
pub(crate) fn generate_create_index_ddl(idx: &IndexDef) -> String {
    let unique = if idx.is_unique { "UNIQUE " } else { "" };
    let columns = idx.columns.join(", ");
    format!(
        "CREATE {}INDEX IF NOT EXISTS {} ON {}({})",
        unique, idx.name, idx.table, columns
    )
}

/// 检查 schema 版本是否已应用
pub(crate) async fn is_schema_version_applied(
    db: &DatabaseConnection,
    version: &str,
) -> Result<bool, DbErr> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS _schema_versions (
            version VARCHAR(50) PRIMARY KEY,
            applied_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .await?;

    let result = db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT 1 FROM _schema_versions WHERE version = $1",
            [version.into()],
        ))
        .await?;

    Ok(result.is_some())
}

/// 记录 schema 版本已应用
pub(crate) async fn mark_schema_version_applied(
    db: &DatabaseConnection,
    version: &str,
) -> Result<(), DbErr> {
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO _schema_versions (version) VALUES ($1) ON CONFLICT (version) DO NOTHING",
        [version.into()],
    ))
    .await?;

    Ok(())
}
