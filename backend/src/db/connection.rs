use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::time::Duration;

pub async fn establish_connection(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(database_url.to_owned());

    // 配置连接池以提升并发性能
    opt.max_connections(20) // 最大连接数（默认为10）
        .min_connections(5) // 最小连接数（默认为1）
        .connect_timeout(Duration::from_secs(10)) // 连接超时
        .acquire_timeout(Duration::from_secs(10)) // 获取连接超时
        .idle_timeout(Duration::from_secs(300)) // 空闲连接超时（5分钟）
        .max_lifetime(Duration::from_secs(3600)) // 连接最大存活时间（1小时）
        .sqlx_logging(false); // 关闭SQL日志以提升性能（开发时可设为true）

    let db = Database::connect(opt).await?;
    Ok(db)
}
