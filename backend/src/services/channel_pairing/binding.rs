//! A binding is a revocable authorization premise, not merely a chat address.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ChannelBinding {
    pub user_id: i32,
    pub provider: String,
    pub identity_id: i32,
    pub scope: String,
}

pub(crate) fn provider_for_platform(platform: &str) -> &str {
    if platform == "discord" {
        "discord_dm"
    } else {
        platform
    }
}

pub(crate) async fn credential_scope(provider: &str) -> String {
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    let (app_id, secret) = match provider {
        "qq" => (
            config.qq_bot_app_id.as_str(),
            config.qq_bot_app_secret.as_deref(),
        ),
        "telegram" => ("", config.telegram_bot_token.as_deref()),
        "discord_dm" => ("", config.discord_bot_token.as_deref()),
        "feishu" => (
            config.feishu_bot_app_id.as_str(),
            config.feishu_bot_app_secret.as_deref(),
        ),
        _ => return String::new(),
    };
    // Only the digest is persisted; credentials never enter a run or its context.
    hex::encode(Sha256::digest(
        serde_json::to_vec(&(provider, app_id.trim(), secret.unwrap_or("").trim())).unwrap(),
    ))
}

pub(crate) async fn channel_enabled(provider: &str) -> bool {
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    match provider {
        "qq" => config.qq_bot_enabled,
        "telegram" => config.telegram_bot_enabled,
        "discord_dm" => config.discord_bot_enabled,
        "feishu" => config.feishu_bot_enabled,
        _ => false,
    }
}

impl ChannelBinding {
    pub async fn resolve(
        db: &DatabaseConnection,
        platform: &str,
        user_id: i32,
        sender_id: &str,
    ) -> Result<Option<Self>, DbErr> {
        let provider = provider_for_platform(platform);
        let scope = credential_scope(provider).await;
        if !channel_enabled(provider).await {
            return Ok(None);
        }
        let row = db.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "SELECT MIN(id) AS id FROM user_identities WHERE user_id = $1 AND provider = $2 \
             AND raw_profile->>'channel_scope' = $4 AND EXISTS (SELECT 1 FROM user_identities \
             WHERE user_id = $1 AND provider = $2 AND provider_user_id = $3 AND raw_profile->>'channel_scope' = $4)",
            [user_id.into(), provider.into(), sender_id.into(), scope.clone().into()])).await?;
        Ok(row
            .and_then(|row| row.try_get::<i32>("", "id").ok())
            .map(|identity_id| Self {
                user_id,
                provider: provider.to_string(),
                identity_id,
                scope,
            }))
    }

    pub fn session_key(&self, chat_key: &str) -> String {
        // The identity sequence changes even when the same two accounts re-pair.
        format!(
            "v2:{}:{}:{}",
            self.provider,
            self.identity_id,
            hex::encode(Sha256::digest(chat_key.as_bytes()))
        )
    }

    pub async fn is_current(&self, db: &DatabaseConnection) -> bool {
        if !channel_enabled(&self.provider).await
            || credential_scope(&self.provider).await != self.scope
        {
            return false;
        }
        let row = db.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "SELECT id FROM user_identities WHERE id = $1 AND user_id = $2 AND provider = $3 AND raw_profile->>'channel_scope' = $4",
            [self.identity_id.into(), self.user_id.into(), self.provider.clone().into(), self.scope.clone().into()])).await;
        if !matches!(row, Ok(Some(_))) {
            return false;
        }
        crate::services::agent::ensure_agent_usage_allowed(db, self.user_id)
            .await
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn re_pairing_never_reuses_previous_chat_state() {
        let first = ChannelBinding {
            user_id: 10,
            provider: "telegram".into(),
            identity_id: 1,
            scope: "bot".into(),
        };
        let next = ChannelBinding {
            identity_id: 2,
            ..first.clone()
        };
        assert_ne!(first.session_key("chat"), next.session_key("chat"));
        assert_ne!(first.session_key("chat"), first.session_key("other"));
    }
}

#[cfg(test)]
mod postgres_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires MYRIAD_CHANNEL_TEST_DATABASE_URL pointing to a disposable *_channel_test database"]
    async fn pairing_revocation_and_rotation_use_real_postgres() {
        let url = std::env::var("MYRIAD_CHANNEL_TEST_DATABASE_URL").expect("test database URL");
        assert!(url::Url::parse(&url)
            .unwrap()
            .path()
            .ends_with("_channel_test"));
        let db = sea_orm::Database::connect(&url).await.unwrap();
        db.execute_unprepared("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, username TEXT, is_admin BOOLEAN, is_owner BOOLEAN DEFAULT false); \
            CREATE TABLE IF NOT EXISTS user_identities (id SERIAL PRIMARY KEY, user_id INTEGER REFERENCES users(id), provider TEXT, provider_user_id TEXT, is_primary BOOLEAN DEFAULT false, linked_at TIMESTAMPTZ DEFAULT NOW(), raw_profile JSONB, UNIQUE(provider, provider_user_id)); \
            CREATE TABLE IF NOT EXISTS tapp_runtime_registry (namespace TEXT, record_id TEXT, subject_id INTEGER, owner_id INTEGER, tapp_id TEXT, runtime_id TEXT, payload JSONB NOT NULL, expires_at BIGINT, updated_at TIMESTAMPTZ DEFAULT NOW(), PRIMARY KEY(namespace, record_id)); \
            TRUNCATE user_identities, tapp_runtime_registry, users RESTART IDENTITY; \
            INSERT INTO users VALUES (101, 'first', true, false), (102, 'second', true, false);").await.unwrap();
        {
            let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
            config.feishu_bot_enabled = true;
            config.feishu_bot_app_id = "test-app".into();
            config.feishu_bot_app_secret = Some("test-secret".into());
        }
        let keys = vec!["open-user".into(), "user-alias".into()];
        let code = mint_code(&db, FEISHU, 101).await.unwrap();
        assert_eq!(
            consume_code_keys(&db, FEISHU, &keys, &code.code)
                .await
                .unwrap(),
            PairingBindResult::Bound { user_id: 101 }
        );
        let first = ChannelBinding::resolve(&db, "feishu", 101, "open-user")
            .await
            .unwrap()
            .unwrap();
        let alias = ChannelBinding::resolve(&db, "feishu", 101, "user-alias")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.identity_id, alias.identity_id);
        assert!(first.is_current(&db).await);
        assert!(ChannelBinding::resolve(&db, "feishu", 102, "open-user")
            .await
            .unwrap()
            .is_none());
        shared_registry::put(
            &db,
            "feishu_p2p_pending",
            &first.session_key("chat"),
            RegistryIdentity {
                subject_id: Some(101),
                owner_id: Some(101),
                tapp_id: None,
                runtime_id: None,
            },
            &"private old prompt",
            Utc::now().timestamp() + 600,
        )
        .await
        .unwrap();
        shared_registry::put(
            &db,
            "feishu_p2p_session",
            &first.session_key("chat"),
            RegistryIdentity {
                subject_id: Some(101),
                owner_id: Some(101),
                tapp_id: None,
                runtime_id: None,
            },
            &serde_json::json!({"session_id": "", "binding": first}),
            Utc::now().timestamp() + 600,
        )
        .await
        .unwrap();
        let revoked_code = mint_code(&db, FEISHU, 101).await.unwrap();
        assert!(unpair(&db, FEISHU, 101).await.unwrap());
        assert_eq!(
            bind_openids(&db, FEISHU, 101, &keys, &first.scope, &revoked_code.code)
                .await
                .unwrap(),
            PairingBindResult::InvalidOrExpired
        );
        assert!(!first.is_current(&db).await);
        ensure_aliases(&db, FEISHU, 101, &keys).await.unwrap();
        assert_eq!(
            lookup_any(&db, FEISHU, &keys).await.unwrap(),
            PairingLookup::Unpaired
        );
        assert!(
            shared_registry::list(&db, "feishu_p2p_pending", Some(101), None)
                .await
                .unwrap()
                .is_empty()
        );
        let code = mint_code(&db, FEISHU, 102).await.unwrap();
        assert_eq!(
            consume_code_keys(&db, FEISHU, &keys, &code.code)
                .await
                .unwrap(),
            PairingBindResult::Bound { user_id: 102 }
        );
        let second = ChannelBinding::resolve(&db, "feishu", 102, "open-user")
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first.session_key("chat"), second.session_key("chat"));
        let unused_code = mint_code(&db, FEISHU, 101).await.unwrap();
        crate::GLOBAL_DYNAMIC_CONFIG
            .write()
            .await
            .feishu_bot_app_secret = Some("rotated-secret".into());
        assert!(!second.is_current(&db).await);
        assert_eq!(
            lookup_any(&db, FEISHU, &keys).await.unwrap(),
            PairingLookup::Unpaired
        );
        // An old code must not authorize a different bot configuration.
        assert_eq!(
            consume_code_keys(&db, FEISHU, &keys, &unused_code.code)
                .await
                .unwrap(),
            PairingBindResult::InvalidOrExpired
        );
    }
}
