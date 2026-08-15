//! Re-export of workspace crate [`myriad_data_key`], plus SeaORM migration helper.
//!
//! Pure crypto lives in `myriad-data-key` so `federation` can depend on keying without
//! pulling HTTP/API layers. Configuration table migration stays here (DB access).

pub use myriad_data_key::*;

/// 把 `configurations` 里遗留的明文敏感值就地升级为密文。
///
/// 启动时跑一次。幂等且可重入：已经是密文的行会被跳过，所以中途崩溃、
/// 反复重启都没问题，不需要维护窗口。
///
/// 返回本次升级的行数。
pub async fn migrate_plaintext_config_values(
    db: &sea_orm::DatabaseConnection,
) -> anyhow::Result<usize> {
    use anyhow::Context;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    if data_key().source().is_fallback() {
        // 兜底密钥仍然绑在 JWT_SECRET 上。此时加密只会把"明文风险"换成
        // "轮换即失数据"的风险，得不偿失 —— 等运维修好密钥文件再迁。
        tracing::warn!("Skipping configuration encryption migration: data key is in fallback mode");
        return Ok(0);
    }

    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT key, value FROM configurations".to_string(),
        ))
        .await
        .context("Failed to read configurations for encryption migration")?;

    let mut migrated = 0usize;
    for row in rows {
        let (Ok(key), Ok(value)) = (
            row.try_get::<String>("", "key"),
            row.try_get::<serde_json::Value>("", "value"),
        ) else {
            continue;
        };

        if !is_sensitive_config_key(&key) {
            continue;
        }
        let serde_json::Value::String(ref plain) = value else {
            continue;
        };
        if plain.is_empty() || is_ciphertext(plain) {
            continue; // 已迁移或无值
        }

        let sealed = seal_config_value(&key, value.clone());
        if sealed == value {
            continue; // 加密失败，seal 已经记过日志
        }

        match db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE configurations SET value = $2, is_encrypted = true WHERE key = $1",
                vec![key.clone().into(), sealed.into()],
            ))
            .await
        {
            Ok(_) => migrated += 1,
            // 逐行推进：一行失败不影响其余行，下次启动会再试。
            Err(e) => tracing::error!(key, "Failed to encrypt stored configuration: {e}"),
        }
    }

    if migrated > 0 {
        tracing::info!("🔐 Encrypted {migrated} previously plaintext configuration value(s)");
    }
    Ok(migrated)
}

