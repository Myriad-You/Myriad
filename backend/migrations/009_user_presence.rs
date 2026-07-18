use sea_orm_migration::prelude::*;

/// 用户在线状态跟踪
///
/// 为 users 表新增：
/// - last_seen_at：最近一次通过认证请求活跃的时间（由 auth 中间件节流更新）
/// - online_seconds：累计在线时长（秒）；活跃间隔 ≤5 分钟视为持续在线并累加
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS last_seen_at TIMESTAMPTZ",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS online_seconds BIGINT NOT NULL DEFAULT 0",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE users DROP COLUMN IF EXISTS online_seconds")
            .await?;
        db.execute_unprepared("ALTER TABLE users DROP COLUMN IF EXISTS last_seen_at")
            .await?;
        Ok(())
    }
}
