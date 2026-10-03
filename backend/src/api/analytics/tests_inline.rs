//! Unit tests for analytics helpers.

use chrono::NaiveDate;
use serde_json::json;

use super::admin_api::{
    i64_nonneg, normalize_import_event_name, normalize_import_event_path, normalize_import_path,
    parse_day_str, valid_visitor_hash, vid_from_query,
};
use super::intake_helpers::*;

#[test]
fn normalizes_root_and_known() {
    assert_eq!(normalize_path("/").as_deref(), Some("/"));
    assert_eq!(normalize_path("/library").as_deref(), Some("/library"));
    assert_eq!(
        normalize_path("/tapp/abc123?x=1").as_deref(),
        Some("/tapp/:id")
    );
}

#[test]
fn rejects_empty() {
    assert!(normalize_path("").is_none());
}

#[test]
fn vid_and_event() {
    assert!(is_valid_vid("0123456789abcdef"));
    assert!(!is_valid_vid("x"));
    assert_eq!(
        normalize_event_name("Login_Success").as_deref(),
        Some("login_success")
    );
    assert!(normalize_event_name("!!!").is_none());
    assert!(normalize_event_name("a").is_none());
    assert!(
        normalize_event_name("__engage__").is_none(),
        "reserved internal names rejected"
    );
    assert!(normalize_event_name("__custom").is_none());
    assert_eq!(normalize_target(""), "");
    assert_eq!(normalize_target("  My-Tapp_01  "), "my-tapp_01");
    assert_eq!(normalize_target("weather@github.com"), "weather@github.com");
    assert!(normalize_target("!!!").is_empty());
}

#[test]
fn parses_vid_from_query() {
    let uri = |s: &str| s.parse::<axum::http::Uri>().unwrap();
    assert_eq!(
        vid_from_query(&uri("/api/analytics/visitor?vid=0123456789abcdef")).as_deref(),
        Some("0123456789abcdef")
    );
    // 位置无关，且不会被前缀相同的键骗到
    assert_eq!(
        vid_from_query(&uri("/x?days=7&vid=abcdefghijklmnop&z=1")).as_deref(),
        Some("abcdefghijklmnop")
    );
    assert_eq!(vid_from_query(&uri("/x?myvid=nope")), None);
    assert_eq!(vid_from_query(&uri("/x")), None);
    assert_eq!(vid_from_query(&uri("/x?vid")), None);
    // 取到的值仍要过 is_valid_vid（过短不通过；指纹回退不在本函数）
    assert!(
        !is_valid_vid(&vid_from_query(&uri("/x?vid=short")).unwrap()),
        "too-short vid must not pass validation"
    );
}

#[test]
fn visitor_hash_prefers_vid() {
    let a = resolve_visitor_hash(Some("0123456789abcdef01"), None, "Mozilla");
    let b = resolve_visitor_hash(
        Some("0123456789abcdef01"),
        Some("1.2.3.4".parse().unwrap()),
        "Other",
    );
    assert_eq!(a, b);
    assert!(
        a.is_some(),
        "dev/default salt path must still hash visitors"
    );
}

/// Pure salt resolver tests — no process env mutation (safe under parallel tests).
#[test]
fn analytics_salt_production_fails_closed_without_env_or_jwt() {
    assert_eq!(
        resolve_analytics_salt(None, true, None),
        Err(AnalyticsSaltUnavailable)
    );
    assert_eq!(
        resolve_analytics_salt(Some("   "), true, None),
        Err(AnalyticsSaltUnavailable)
    );
}

#[test]
fn analytics_salt_production_derives_from_jwt_when_salt_empty() {
    // Compose often ships ENVIRONMENT=production + JWT_SECRET + empty ANALYTICS_SALT.
    assert_eq!(
        resolve_analytics_salt(None, true, Some("jwt-secret-value")).as_deref(),
        Ok("myriad-analytics-prod|jwt-secret-value")
    );
    assert_eq!(
        resolve_analytics_salt(Some(""), true, Some("jwt-secret-value")).as_deref(),
        Ok("myriad-analytics-prod|jwt-secret-value")
    );
}

#[test]
fn analytics_salt_production_accepts_configured() {
    assert_eq!(
        resolve_analytics_salt(Some("prod-unique-salt"), true, None).as_deref(),
        Ok("prod-unique-salt")
    );
    assert_eq!(
        resolve_analytics_salt(Some("  trimmed  "), true, None).as_deref(),
        Ok("trimmed")
    );
}

#[test]
fn analytics_salt_dev_uses_default_or_jwt() {
    assert_eq!(
        resolve_analytics_salt(None, false, None).as_deref(),
        Ok("myriad-analytics-v1")
    );
    assert_eq!(
        resolve_analytics_salt(None, false, Some("dev-jwt-abc")).as_deref(),
        Ok("myriad-analytics-dev|dev-jwt-abc")
    );
    // Explicit salt always wins over JWT derivation.
    assert_eq!(
        resolve_analytics_salt(Some("explicit"), false, Some("dev-jwt-abc")).as_deref(),
        Ok("explicit")
    );
}

#[test]
fn bots_detected() {
    assert!(is_bot_ua("Googlebot/2.1"));
    assert!(!is_bot_ua(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
    ));
}

/// Empty allowlist uses narrow default (loopback + docker0), not full
/// RFC1918. Public peers and arbitrary private nets still cannot forge.
#[test]
fn country_headers_empty_allowlist_private_vs_public_peer() {
    use axum::http::{HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert("cf-ipcountry", HeaderValue::from_static("JP"));
    headers.insert("x-country-code", HeaderValue::from_static("US"));
    let docker0_peer: std::net::IpAddr = "172.17.0.2".parse().unwrap();
    let loopback_peer: std::net::IpAddr = "127.0.0.1".parse().unwrap();
    let rfc1918_peer: std::net::IpAddr = "10.0.0.2".parse().unwrap();
    let public_peer: std::net::IpAddr = "192.0.2.7".parse().unwrap();

    assert!(
        country_from_headers_with_trust(&headers, Some(docker0_peer), true, &[]).is_some(),
        "empty allowlist + docker0 peer should honor CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(loopback_peer), true, &[]).is_some(),
        "empty allowlist + loopback peer should honor CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(rfc1918_peer), true, &[]).is_none(),
        "empty allowlist + arbitrary RFC1918 peer must not honor CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(public_peer), true, &[]).is_none(),
        "empty allowlist + public peer must ignore forged CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(docker0_peer), false, &[]).is_none(),
        "trust disabled must ignore country headers"
    );
}

#[test]
fn country_headers_honored_only_for_trusted_proxy_peer() {
    use axum::http::{HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert("cf-ipcountry", HeaderValue::from_static("JP"));
    let peer: std::net::IpAddr = "10.0.0.2".parse().unwrap();
    let allow: [ipnet::IpNet; 1] = ["10.0.0.0/8".parse().unwrap()];
    let outside: std::net::IpAddr = "192.0.2.7".parse().unwrap();

    assert!(
        country_from_headers_with_trust(&headers, Some(peer), true, &allow).is_some(),
        "trusted peer + allowlist must accept CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(outside), true, &allow).is_none(),
        "peer outside allowlist must ignore country headers"
    );
    assert!(
        country_from_headers_with_trust(&headers, None, true, &allow).is_none(),
        "missing peer must not trust country headers"
    );
}

#[test]
fn parse_day_str_accepts_iso_and_trims() {
    assert_eq!(
        parse_day_str("2026-07-30"),
        Some(NaiveDate::from_ymd_opt(2026, 7, 30).unwrap())
    );
    assert_eq!(
        parse_day_str("  2026-01-01  "),
        Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
    );
    assert!(parse_day_str("").is_none());
    assert!(parse_day_str("2026/07/30").is_none());
    assert!(parse_day_str("not-a-date").is_none());
    assert!(parse_day_str("2026-13-01").is_none());
}

#[test]
fn valid_visitor_hash_is_hex_length_bounded() {
    assert!(valid_visitor_hash("0123456789abcdef")); // 16 hex
    assert!(valid_visitor_hash(&"ab".repeat(16))); // 32 hex (sha16 style)
    assert!(!valid_visitor_hash("short"));
    assert!(!valid_visitor_hash("not-hex-zzzzzzzz"));
    assert!(!valid_visitor_hash(""));
    assert!(!valid_visitor_hash(&"a".repeat(65)));
}

#[test]
fn i64_nonneg_accepts_json_numbers() {
    assert_eq!(i64_nonneg(Some(&json!(0))), Some(0));
    assert_eq!(i64_nonneg(Some(&json!(42))), Some(42));
    assert_eq!(i64_nonneg(Some(&json!(u64::MAX))), None); // doesn't fit i64
    assert!(i64_nonneg(Some(&json!(-1))).is_none());
    assert!(i64_nonneg(Some(&json!("1"))).is_none());
    assert!(i64_nonneg(None).is_none());
}

#[test]
fn import_path_and_event_helpers() {
    assert_eq!(normalize_import_path(SITE_PATH).as_deref(), Some(SITE_PATH));
    assert_eq!(
        normalize_import_path("/library").as_deref(),
        Some("/library")
    );
    assert!(normalize_import_path("relative").is_none());

    assert_eq!(
        normalize_import_event_name(ENGAGE_MARKER).as_deref(),
        Some(ENGAGE_MARKER)
    );
    assert_eq!(
        normalize_import_event_name("Login_OK").as_deref(),
        Some("login_ok")
    );
    assert!(normalize_import_event_name("!!").is_none());

    assert_eq!(normalize_import_event_path("").as_deref(), Some(""));
    assert_eq!(
        normalize_import_event_path("/tapp/xyz").as_deref(),
        Some("/tapp/:id")
    );
}

#[test]
fn backup_format_constants_stable() {
    assert_eq!(ANALYTICS_BACKUP_FORMAT, "myriad-analytics-backup");
    assert_eq!(ANALYTICS_BACKUP_VERSION, 1);
}

#[tokio::test]
async fn sealed_import_replace_then_merge_applies_table_semantics() {
    use super::backup_integrity::{content_hash, seal_integrity};
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let Ok(url) = std::env::var("ANALYTICS_TEST_DATABASE_URL") else {
        return;
    };
    // `replace` truncates every analytics table: only ever run it in a private schema.
    let isolated = crate::db::IsolatedSchema::migrated(&url, "analytics_import_test").await;
    let db = isolated.db.clone();
    let day = (chrono::Utc::now().date_naive() - chrono::Duration::days(1)).to_string();
    let hash = "0123456789abcdef";
    let tables = json!({
        "page_daily": [
            { "day": day, "path": "/", "views": 10, "unique_visitors": 0, "engagement_ms": 100, "engaged_views": 0 },
            { "day": day, "path": "/library", "views": 3, "unique_visitors": 0, "engagement_ms": 0, "engaged_views": 0 }
        ],
        "visitor_seen": [{ "day": day, "path": "/", "visitor_hash": hash, "ordinal": 1 }],
        "event_daily": [{ "day": day, "event_name": "click", "path": "", "target": "", "count": 4, "unique_visitors": 0 }],
        "event_visitor": [{ "day": day, "event_name": "click", "path": "", "target": "", "visitor_hash": hash }],
        "referrer_daily": [{ "day": day, "host": "example.com", "count": 2 }],
        "country_daily": [{ "day": day, "country_code": "JP", "country_name": "Japan", "views": 5 }],
        "country_visitor": [{ "day": day, "country_code": "JP", "visitor_hash": hash }]
    });
    let rows = |name: &str| tables[name].as_array().unwrap().clone();
    let digest = content_hash(
        ANALYTICS_BACKUP_FORMAT,
        ANALYTICS_BACKUP_VERSION,
        &rows("page_daily"),
        &rows("visitor_seen"),
        &rows("event_daily"),
        &rows("event_visitor"),
        &rows("referrer_daily"),
        &rows("country_daily"),
        &rows("country_visitor"),
    )
    .unwrap();
    let mut body = tables.clone();
    body["format"] = json!(ANALYTICS_BACKUP_FORMAT);
    body["version"] = json!(ANALYTICS_BACKUP_VERSION);
    body["integrity"] = seal_integrity(&digest).unwrap();

    let scalar = |sql: &'static str| {
        let db = &db;
        async move {
            db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
                .await
                .unwrap()
                .unwrap()
                .try_get::<i64>("", "n")
                .unwrap()
        }
    };
    for (mode, views, visitors_inserted) in [("replace", 13, 1), ("merge", 26, 0)] {
        body["mode"] = json!(mode);
        let (status, response) = super::import_analytics(
            crate::extract::Db(db.clone()),
            axum::Json(serde_json::from_value(body.clone()).unwrap()),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::OK, "{mode}: {:?}", response.0);
        let inserted = &response.0["inserted"];
        assert_eq!(inserted["page_daily"], json!(2), "{mode}");
        assert_eq!(inserted["visitor_seen"], json!(visitors_inserted), "{mode}");
        assert_eq!(inserted["country_visitor"], json!(visitors_inserted), "{mode}");
        assert_eq!(
            scalar("SELECT SUM(views)::bigint AS n FROM analytics_page_daily").await,
            views
        );
    }
    // merge adds counts; UV comes from the recompute over detail rows.
    assert_eq!(
        scalar("SELECT count::bigint AS n FROM analytics_event_daily").await,
        8
    );
    assert_eq!(
        scalar("SELECT count::bigint AS n FROM analytics_referrer_daily").await,
        4
    );
    assert_eq!(
        scalar("SELECT views::bigint AS n FROM analytics_country_daily").await,
        10
    );
    assert_eq!(
        scalar("SELECT unique_visitors::bigint AS n FROM analytics_page_daily WHERE path = '/'")
            .await,
        1
    );
    drop(db);
    isolated.drop().await;
}

#[tokio::test]
async fn import_batch_chunks_splits_repeated_keys_and_counts_rows() {
    use super::admin_api::ImportBatch;
    use sea_orm::{
        ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement, Value as SeaValue,
    };
    let Ok(url) = std::env::var("ANALYTICS_TEST_DATABASE_URL") else {
        return;
    };
    let mut options = ConnectOptions::new(url);
    options.max_connections(1).min_connections(1);
    let db = Database::connect(options).await.unwrap();
    db.execute_unprepared(
        "CREATE TEMP TABLE import_probe (
            day integer, k text, v bigint, tag text, PRIMARY KEY (day, k)
        )",
    )
    .await
    .unwrap();
    let row = |day: i32, k: &str, v: i64| [SeaValue::from(day), k.into(), SeaValue::from(v)];

    // Additive DO UPDATE: a key repeated inside one pending chunk must be
    // applied sequentially instead of failing the statement.
    let mut merge = ImportBatch::with_max_rows(
        "INSERT INTO import_probe (day, k, v, tag) VALUES ",
        3,
        ", 'merge'",
        " ON CONFLICT (day, k) DO UPDATE SET v = import_probe.v + EXCLUDED.v",
        3,
    );
    for (day, k, v) in [(1, "a", 1), (1, "b", 2), (1, "a", 3), (2, "a", 4), (2, "b", 5)] {
        merge
            .push(&db, Some(format!("{day}\u{0}{k}")), row(day, k, v))
            .await
            .unwrap();
    }
    merge.flush(&db).await.unwrap();
    assert_eq!(merge.affected(), 5);
    let totals = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT day, k, v, tag FROM import_probe ORDER BY day, k",
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|r| {
            (
                r.try_get::<i32>("", "day").unwrap(),
                r.try_get::<String>("", "k").unwrap(),
                r.try_get::<i64>("", "v").unwrap(),
                r.try_get::<String>("", "tag").unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        totals,
        vec![
            (1, "a".into(), 4, "merge".into()),
            (1, "b".into(), 2, "merge".into()),
            (2, "a".into(), 4, "merge".into()),
            (2, "b".into(), 5, "merge".into()),
        ]
    );

    // DO NOTHING counts only new rows; far more rows than one statement can bind.
    db.execute_unprepared("TRUNCATE import_probe").await.unwrap();
    let mut fresh = ImportBatch::new(
        "INSERT INTO import_probe (day, k, v) VALUES ",
        3,
        "",
        " ON CONFLICT (day, k) DO NOTHING",
    );
    for i in 0..25_000 {
        fresh.push(&db, None, row(i, "x", 1)).await.unwrap();
    }
    fresh.push(&db, None, row(0, "x", 9)).await.unwrap();
    fresh.flush(&db).await.unwrap();
    assert_eq!(fresh.affected(), 25_000);
    let count = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n, SUM(v)::bigint AS s FROM import_probe",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count.try_get::<i64>("", "n").unwrap(), 25_000);
    assert_eq!(count.try_get::<i64>("", "s").unwrap(), 25_000);
}

#[tokio::test]
async fn summary_cache_miss_aggregates_in_one_wave_and_fails_whole_on_error() {
    use sea_orm::ConnectionTrait;
    let Ok(url) = std::env::var("ANALYTICS_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = crate::db::IsolatedSchema::migrated(&url, "analytics_summary_test").await;
    let db = isolated.db.clone();
    let today = analytics_today();
    let day = |offset: i64| (today - chrono::Duration::days(offset)).to_string();
    db.execute_unprepared(&format!(
        r#"
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views) VALUES
    ('{d0}', '/a', 5, 0, 100, 2), ('{d0}', '/b', 3, 0, 0, 0), ('{d0}', '__site__', 0, 1, 0, 0),
    ('{d1}', '/a', 4, 0, 0, 0), ('{d1}', '__site__', 0, 5, 0, 0), ('{d9}', '/a', 7, 0, 0, 0),
    ('{d120}', '__site__', 0, 4, 0, 0);
INSERT INTO analytics_visitor_seen (day, path, visitor_hash) VALUES
    ('{d0}', '__site__', 'v1'), ('{d0}', '__site__', 'v2'), ('{d1}', '__site__', 'v1'),
    ('{d9}', '__site__', 'v3'), ('{d0}', '/a', 'v1'), ('{d0}', '/a', 'v2');
INSERT INTO analytics_event_daily (day, event_name, path, target, count) VALUES
    ('{d0}', 'click', '/a', '', 2), ('{d0}', 'click', '/a', 'buy', 3), ('{d0}', '__engage__', '', '', 9);
INSERT INTO analytics_event_visitor (day, event_name, path, target, visitor_hash) VALUES
    ('{d0}', 'click', '/a', 'buy', 'v1'), ('{d0}', 'click', '/a', '', 'v2');
INSERT INTO analytics_country_daily (day, country_code, country_name, views, unique_visitors) VALUES
    ('{d0}', 'JP', 'Japan', 5, 1), ('{d1}', 'JP', 'Japan', 1, 1);
INSERT INTO analytics_country_visitor (day, country_code, visitor_hash) VALUES
    ('{d0}', 'JP', 'v1'), ('{d1}', 'JP', 'v1');
"#,
        d0 = day(0),
        d1 = day(1),
        d9 = day(9),
        d120 = day(120),
    ))
    .await
    .unwrap();
    let query = || SummaryQuery {
        days: Some(7),
        from: None,
        to: None,
    };
    invalidate_summary_cache().await;
    let (status, axum::Json(body)) = super::admin_api::build_analytics_summary(&db, query()).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["range"]["views"], 12);
    assert_eq!(body["range"]["unique_visitors"], 2);
    // Day UV comes from the seen set like range UV, not from the (drifted) counter.
    assert_eq!(body["today"]["unique_visitors"], 2);
    assert_eq!(body["daily"][5]["unique_visitors"], 1);
    assert_eq!(body["compare"]["day"]["unique_visitors"]["current"], 2);
    assert_eq!(body["compare"]["day"]["unique_visitors"]["previous"], 1);
    assert_eq!(body["compare"]["day"]["views"]["previous"], 4);
    assert_eq!(body["compare"]["range"]["views"]["previous"], 7);
    assert_eq!(body["compare"]["range"]["unique_visitors"]["previous"], 1);
    assert_eq!(body["all_time"]["views"], 19);
    assert_eq!(body["all_time"]["unique_visitors"], 3);
    assert_eq!(body["pages"][0]["path"], "/a");
    assert_eq!(body["pages"][0]["unique_visitors"], 2);
    assert_eq!(body["events"].as_array().unwrap().len(), 1);
    assert_eq!(body["events"][0]["count"], 5);
    assert_eq!(body["events"][0]["targets"][0]["target"], "buy");
    assert_eq!(body["events"][0]["targets"][0]["unique_visitors"], 1);
    assert_eq!(body["countries"][0]["unique_visitors"], 1);

    // Past visitor-seen retention only the counter rollup is left.
    let old = day(120);
    let (_, axum::Json(body)) = super::admin_api::build_analytics_summary(
        &db,
        SummaryQuery {
            days: None,
            from: Some(old.clone()),
            to: Some(old),
        },
    )
    .await;
    assert_eq!(body["daily"][0]["unique_visitors"], 4);

    invalidate_summary_cache().await;
    let card = super::admin_api::visitor_card_aggregate(&db).await.unwrap();
    assert_eq!(card["today"]["unique_visitors"], 2);
    assert_eq!(card["daily"][3]["unique_visitors"], 1);

    invalidate_summary_cache().await;
    db.execute_unprepared("ALTER TABLE analytics_country_visitor RENAME TO country_visitor_off")
        .await
        .unwrap();
    let (status, axum::Json(body)) = super::admin_api::build_analytics_summary(&db, query()).await;
    assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        body.get("range").is_none(),
        "no partial statistics on failure"
    );
    invalidate_summary_cache().await;
    isolated.drop().await;
}

async fn analytics_scalar(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap()
}

#[tokio::test]
async fn failed_counter_write_leaves_no_seen_row() {
    use sea_orm::ConnectionTrait;
    let Ok(url) = std::env::var("ANALYTICS_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = crate::db::IsolatedSchema::migrated(&url, "analytics_atomic_test").await;
    let db = isolated.db.clone();
    let day = analytics_today();
    let jp = CountryInfo {
        code: "JP".into(),
        name: "Japan".into(),
    };
    // Every counter table rejects writes: the seen half of each pair must roll back with it.
    db.execute_unprepared(
        r#"
CREATE FUNCTION analytics_fail() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'injected counter failure'; END $$;
CREATE TRIGGER fail_page BEFORE INSERT OR UPDATE ON analytics_page_daily
    FOR EACH ROW EXECUTE FUNCTION analytics_fail();
CREATE TRIGGER fail_event BEFORE INSERT OR UPDATE ON analytics_event_daily
    FOR EACH ROW EXECUTE FUNCTION analytics_fail();
CREATE TRIGGER fail_country BEFORE INSERT OR UPDATE ON analytics_country_daily
    FOR EACH ROW EXECUTE FUNCTION analytics_fail();
"#,
    )
    .await
    .unwrap();
    assert!(bump_pageview(&db, day, "/a", "v1", true).await.is_err());
    assert!(record_site_unique(&db, day, "v1").await.is_err());
    assert!(bump_engagement(&db, day, "/a", "v1", 5_000).await.is_err());
    assert!(bump_event(&db, day, "click", "/a", "", "v1").await.is_err());
    assert!(bump_country(&db, day, &jp, "v1", true).await.is_err());
    for table in [
        "analytics_visitor_seen",
        "analytics_event_visitor",
        "analytics_country_visitor",
    ] {
        let n = analytics_scalar(&db, &format!("SELECT COUNT(*) AS n FROM {table}")).await;
        assert_eq!(n, 0, "{table} kept a seen row whose counter failed");
    }

    // Once the counters accept writes the same visitor is still a first visit.
    db.execute_unprepared(
        "DROP TRIGGER fail_page ON analytics_page_daily;
         DROP TRIGGER fail_event ON analytics_event_daily;
         DROP TRIGGER fail_country ON analytics_country_daily;",
    )
    .await
    .unwrap();
    bump_pageview(&db, day, "/a", "v1", true).await.unwrap();
    assert_eq!(record_site_unique(&db, day, "v1").await.unwrap(), Some(1));
    bump_engagement(&db, day, "/a", "v1", 5_000).await.unwrap();
    bump_event(&db, day, "click", "/a", "", "v1").await.unwrap();
    bump_country(&db, day, &jp, "v1", true).await.unwrap();
    let q = |sql: &'static str| analytics_scalar(&db, sql);
    assert_eq!(
        q("SELECT (views * 10 + unique_visitors)::bigint AS n
           FROM analytics_page_daily WHERE path = '/a'")
        .await,
        11
    );
    assert_eq!(
        q("SELECT engaged_views::bigint AS n FROM analytics_page_daily WHERE path = '/a'").await,
        1
    );
    assert_eq!(
        q("SELECT unique_visitors::bigint AS n FROM analytics_page_daily WHERE path = '__site__'")
            .await,
        1
    );
    assert_eq!(
        q("SELECT unique_visitors::bigint AS n FROM analytics_event_daily").await,
        1
    );
    assert_eq!(
        q("SELECT unique_visitors::bigint AS n FROM analytics_country_daily").await,
        1
    );
    drop(db);
    isolated.drop().await;
}

#[tokio::test]
async fn concurrent_duplicate_visits_count_once() {
    let Ok(url) = std::env::var("ANALYTICS_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = crate::db::IsolatedSchema::migrated(&url, "analytics_race_test").await;
    let db = isolated.db.clone();
    let day = analytics_today();

    let views = (0..16).map(|_| bump_pageview(&db, day, "/c", "same", true));
    for result in futures::future::join_all(views).await {
        result.unwrap();
    }
    let events = (0..8).map(|_| bump_event(&db, day, "click", "/c", "", "same"));
    for result in futures::future::join_all(events).await {
        result.unwrap();
    }
    let q = |sql: &'static str| analytics_scalar(&db, sql);
    assert_eq!(
        q("SELECT views::bigint AS n FROM analytics_page_daily WHERE path = '/c'").await,
        16
    );
    assert_eq!(
        q("SELECT unique_visitors::bigint AS n FROM analytics_page_daily WHERE path = '/c'").await,
        1
    );
    assert_eq!(
        q("SELECT (count * 10 + unique_visitors)::bigint AS n FROM analytics_event_daily").await,
        81
    );

    // Distinct first visits get distinct ordinals 1..=N; duplicates share one.
    let visitors: Vec<String> = (0..12).map(|i| format!("visitor-{i:02}")).collect();
    let firsts = visitors.iter().map(|v| record_site_unique(&db, day, v));
    let mut ordinals: Vec<i64> = futures::future::join_all(firsts)
        .await
        .into_iter()
        .map(|r| r.unwrap().unwrap())
        .collect();
    ordinals.sort_unstable();
    assert_eq!(ordinals, (1..=12).collect::<Vec<i64>>());
    let repeats = (0..8).map(|_| record_site_unique(&db, day, "late"));
    for result in futures::future::join_all(repeats).await {
        assert_eq!(result.unwrap(), Some(13));
    }
    assert_eq!(
        q("SELECT unique_visitors::bigint AS n FROM analytics_page_daily WHERE path = '__site__'")
            .await,
        13
    );
    assert_eq!(
        q("SELECT COUNT(*) AS n FROM analytics_visitor_seen WHERE path = '__site__'").await,
        13
    );
    drop(db);
    isolated.drop().await;
}

#[test]
fn summary_cache_drops_results_computed_across_an_invalidation() {
    let mut caches = SummaryCaches::default();
    let key = || "2026-01-01..2026-01-07".to_string();

    // A compute that started before an intake's invalidation must not land.
    let started = caches.generation();
    caches.invalidate();
    assert!(!caches.store_summary(started, key(), json!({ "stale": true })));
    assert!(!caches.store_card(started, json!({ "stale": true })));
    assert!(caches.summary(&key()).is_none());
    assert!(caches.card().is_none());

    // One that started after it lands, and the next invalidation clears it.
    let started = caches.generation();
    assert!(caches.store_summary(started, key(), json!({ "fresh": true })));
    assert!(caches.store_card(started, json!({ "fresh": true })));
    assert_eq!(caches.summary(&key()), Some(json!({ "fresh": true })));
    assert_eq!(caches.card(), Some(json!({ "fresh": true })));
    caches.invalidate();
    assert!(caches.summary(&key()).is_none());
    assert!(caches.card().is_none());
}

#[test]
fn intake_items_log_every_write_failure() {
    let src = include_str!("intake_helpers.rs");
    let body = src
        .split("async fn process_items")
        .nth(1)
        .and_then(|rest| rest.split("async fn parse_json_body").next())
        .expect("process_items");
    assert!(
        !body.contains("let _ ="),
        "a write error is silently dropped"
    );
    assert!(
        !body.contains(".is_ok()"),
        "a write error is silently dropped"
    );
    assert_eq!(body.matches("intake_write_ok(").count(), 6);
}

/// Both endpoints go through `admit_intake`; neither re-implements a gate.
#[test]
fn intake_endpoints_share_one_gate_chain() {
    let src = include_str!("intake_helpers.rs");
    let handler = |name: &str| {
        src.split(&format!("pub async fn {name}("))
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_else(|| panic!("{name}"))
            .to_string()
    };
    for (name, body) in [("collect", "CollectRequest")] {
        let handler = handler(name);
        assert!(
            handler.contains(&format!("admit_intake::<{body}>(")),
            "{name}"
        );
        assert!(handler.contains("run_intake(&ctx, &items)"), "{name}");
        for gate in [
            "analytics_collection_enabled",
            "is_staff",
            "is_bot_ua",
            "rate_limited",
            "resolve_visitor_hash",
            "resolve_country",
            "invalidate_summary_cache",
            "maybe_prune",
        ] {
            assert!(!handler.contains(gate), "{name} re-implements {gate}");
        }
    }
}

#[test]
fn intake_bodies_validate_into_items() {
    let collect: CollectRequest = serde_json::from_value(json!({ "items": [] })).unwrap();
    let (status, body) = collect.into_items().unwrap_err();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(body.0["error"], "empty");
}
