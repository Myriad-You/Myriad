//! PostgreSQL contracts; CTE and session-local fixtures perform no persistent writes.
use super::*;

#[tokio::test]
#[ignore = "requires a dedicated AVATAR_PROFILE_TEST_DATABASE_URL PostgreSQL database"]
async fn profile_projection_preserves_shapes_precedence_and_database_presence() {
    let url =
        std::env::var("AVATAR_PROFILE_TEST_DATABASE_URL").expect("use a dedicated test database");
    let db = sea_orm::Database::connect(url).await.unwrap();
    async fn project_typed(
        db: &DatabaseConnection,
        fixtures: Value,
        raw_type: &str,
    ) -> HashMap<String, Value> {
        // CTE shadows the real table; test SQL semantics without persistent writes.
        let mut statement = profile_metadata_statement(42);
        statement.sql = format!(
            "WITH platform_metadata AS (\
             SELECT 42 AS user_id, element->>'platform' AS platform_name, \
                    (element->'raw')::{raw_type} AS raw_data, (element->>'rank')::int AS fetched_at \
             FROM jsonb_array_elements($3::jsonb) AS element) {}",
            statement.sql
        );
        statement.values.as_mut().unwrap().0.push(fixtures.into());
        profile_metadata_rows(db.query_all_raw(statement).await.unwrap()).unwrap()
    }
    async fn project(db: &DatabaseConnection, fixtures: Value) -> HashMap<String, Value> {
        let json = project_typed(db, fixtures.clone(), "json").await;
        let jsonb = project_typed(db, fixtures, "jsonb").await;
        assert_eq!(
            json, jsonb,
            "both production JSON and JSONB fixtures must work"
        );
        json
    }
    let cases = [
        (
            "bilibili",
            json!({"user": {"name": "B", "face": "https://example.com/b", "sign": "bio"}}),
        ),
        ("bilibili", json!({"user_info": {"name": "Fallback"}})),
        (
            "bilibili",
            json!({"user": null, "user_info": {"name": "Not selected"}}),
        ),
        (
            "github",
            json!({"user": {"login": "G", "avatar_url": "https://example.com/g"}}),
        ),
        (
            "youtube",
            json!({"channel": {"snippet": {"title": "Y", "description": "bio",
            "thumbnails": {"medium": {"url": "https://example.com/y"}}}}}),
        ),
        (
            "youtube",
            json!({"user_info": {"name": "Legacy", "face": "https://example.com/legacy"}}),
        ),
        (
            "steam",
            json!({"user": {"personaname": "S", "avatarfull": "https://example.com/s"}}),
        ),
        ("github", json!(["user"])),
    ];
    let profile_fields =
        |profile: Option<PlatformProfile>| profile.map(|p| (p.platform, p.name, p.avatar, p.bio));
    for (platform, raw) in cases {
        let expected = profile_fields(platform_profile(platform, &raw));
        let projected = project(&db, json!([{"platform": platform, "raw": raw, "rank": 1}])).await;
        assert_eq!(
            profile_fields(platform_profile(platform, &projected[platform])),
            expected
        );
    }
    let projected = project(
        &db,
        json!([
            {"platform": "bilibili", "rank": 1, "raw": {"user": {"name": "Old"}}},
            {"platform": "bilibili", "rank": 2, "raw": {"user": {"name": "New"}, "videos": [1, 2]}},
            {"platform": "bilibili_chunk_1", "rank": 3, "raw": {"user": {"name": "Chunk"}}},
            {"platform": "netease", "rank": 1, "raw": {"liked_songs": ["x".repeat(65536)]}}
        ]),
    )
    .await;
    assert_eq!(projected["bilibili"], json!({"user": {"name": "New"}}));
    assert_eq!(projected["netease"], Value::Null);
    assert_eq!(projected.len(), 2);
    let non_profile = project(
        &db,
        json!([
            {"platform": "netease", "rank": 1, "raw": {"liked_songs": []}}
        ]),
    )
    .await;
    assert!(
        !non_profile.is_empty(),
        "non-profile DB data must still suppress disk fallback"
    );
    let chunks_only = project(
        &db,
        json!([
            {"platform": "netease_chunk_1", "rank": 1, "raw": {"songs": []}}
        ]),
    )
    .await;
    assert!(
        chunks_only.is_empty(),
        "orphan chunks were not main metadata before projection"
    );
}

#[tokio::test]
#[ignore = "requires a dedicated AVATAR_PROFILE_TEST_DATABASE_URL PostgreSQL database"]
async fn profile_request_shares_profiles_without_cross_request_or_user_staleness() {
    use crate::services::profile_text::{resolve_profile_text, resolve_profile_text_with_context};
    let url =
        std::env::var("AVATAR_PROFILE_TEST_DATABASE_URL").expect("use a dedicated test database");
    let mut options = sea_orm::ConnectOptions::new(url);
    options.max_connections(1).min_connections(1);
    let db = sea_orm::Database::connect(options).await.unwrap();
    // Max one connection keeps these temp tables session-local and shadows real tables.
    db.execute_unprepared(r#"
        CREATE TEMP TABLE users (
            id INT PRIMARY KEY, is_owner BOOLEAN,
            avatar_source_kind TEXT, avatar_source_ref TEXT, avatar_url TEXT,
            profile_text_source_kind TEXT, profile_text_source_ref TEXT,
            display_name TEXT, username TEXT, bio TEXT
        );
        CREATE TEMP TABLE user_identities (
            id INT, user_id INT, provider TEXT, provider_username TEXT,
            avatar_url TEXT, is_primary BOOLEAN, last_login_at TIMESTAMP, linked_at TIMESTAMP
        );
        CREATE TEMP TABLE platform_metadata (
            user_id INT, platform_name TEXT, raw_data JSON, fetched_at TIMESTAMP
        );
        INSERT INTO users VALUES
            (42, true, 'auto', NULL, 'https://example.com/account',
             'platform', 'github', 'Account', 'account', 'Account bio'),
            (43, false, 'auto', NULL, 'https://example.com/other',
             'auto', NULL, 'Other', 'other', NULL);
        INSERT INTO platform_metadata VALUES
            (42, 'bilibili', '{"user":{"name":"B","face":"https://example.com/old-b"}}', NOW()),
            (42, 'github', '{"user":{"name":"Old G","avatar_url":"https://example.com/old-g"}}', NOW());
    "#).await.unwrap();

    let context = ProfileReadContext::new(&db, 42);
    assert_eq!(
        resolve_avatar_with_context(&context).await,
        proxied_avatar(Some("https://example.com/old-b".into())),
    );
    db.execute_unprepared(
        r#"
        UPDATE platform_metadata SET raw_data = '{"user":{"name":"New G"}}'
        WHERE platform_name = 'github';
    "#,
    )
    .await
    .unwrap();
    let shared = resolve_profile_text_with_context(&context).await.unwrap();
    assert_eq!(shared.name.as_deref(), Some("Old G"));
    assert_eq!(shared.platform.as_deref(), Some("GitHub"));
    assert_eq!(shared.source, "platform");
    assert_eq!(
        resolve_profile_text(&db, 42).await.unwrap().name.as_deref(),
        Some("New G"),
        "a subsequent request must read the new profile",
    );

    let other = ProfileReadContext::new(&db, 43);
    assert_eq!(
        resolve_avatar_with_context(&other).await,
        proxied_avatar(Some("https://example.com/other".into())),
    );
    let text = resolve_profile_text_with_context(&other).await.unwrap();
    assert_eq!(text.name.as_deref(), Some("Other"));
    assert_eq!(text.source, "account");
    assert!(
        other.profiles.get().is_none(),
        "non-owner skips owner profile loading"
    );

    // Recheck current user ownership even when this request already has profiles.
    db.execute_unprepared("UPDATE users SET is_owner=false WHERE id=42")
        .await
        .unwrap();
    let revoked = resolve_profile_text_with_context(&context).await.unwrap();
    assert_eq!(revoked.name.as_deref(), Some("Account"));
    assert_eq!(revoked.source, "account");
    assert_eq!(
        resolve_avatar_with_context(&context).await,
        proxied_avatar(Some("https://example.com/account".into())),
    );
    db.close().await.unwrap();
}
