//! DB introspection and DDL helpers.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};
use std::collections::HashSet;

use super::types::{ColumnDef, IndexDef};

pub(crate) async fn get_table_columns(
    db: &DatabaseConnection,
    table_name: &str,
) -> Result<HashSet<String>, DbErr> {
    // 安全检查：表名只允许字母、数字、下划线
    if !table_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(DbErr::Custom(format!("Invalid table name: {}", table_name)));
    }

    let sql = format!(
        r#"
        SELECT column_name
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = '{}'
        "#,
        table_name
    );

    let rows = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql,
        ))
        .await?;

    let mut columns = HashSet::new();
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "column_name") {
            columns.insert(name);
        }
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
pub(crate) async fn get_existing_indexes(db: &DatabaseConnection) -> Result<HashSet<String>, DbErr> {
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
pub(crate) async fn is_schema_version_applied(db: &DatabaseConnection, version: &str) -> Result<bool, DbErr> {
    // 安全检查：版本号只允许字母、数字、点、下划线、连字符
    if !version
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(DbErr::Custom(format!(
            "Invalid version format: {}",
            version
        )));
    }

    // 先确保版本表存在
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
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                "SELECT 1 FROM _schema_versions WHERE version = '{}'",
                version
            ),
        ))
        .await?;

    Ok(result.is_some())
}

/// 记录 schema 版本已应用
pub(crate) async fn mark_schema_version_applied(db: &DatabaseConnection, version: &str) -> Result<(), DbErr> {
    // 安全检查：版本号只允许字母、数字、点、下划线、连字符
    if !version
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(DbErr::Custom(format!(
            "Invalid version format: {}",
            version
        )));
    }

    db.execute_unprepared(&format!(
        "INSERT INTO _schema_versions (version) VALUES ('{}') ON CONFLICT (version) DO NOTHING",
        version
    ))
    .await?;

    Ok(())
}
