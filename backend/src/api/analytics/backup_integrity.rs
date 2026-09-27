//! Analytics backup content hashing and integrity sealing.
//!
//! Export attaches an instance-bound integrity token (AES-GCM via [`myriad_data_key`])
//! over a field-canonical content hash so a hand-edited JSON cannot be imported
//! without the same data key. Import verifies before any DB write.

use chrono::{Duration, NaiveDate};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::intake_helpers::{
    DAILY_RETENTION_DAYS, ENGAGE_MARKER, SITE_PATH, analytics_today, i64_nonneg,
    normalize_country_code, normalize_event_name, normalize_path, normalize_referrer_host,
    parse_day_str, valid_visitor_hash,
};

/// Algorithm id written into export `integrity.alg`.
pub(crate) const INTEGRITY_ALG: &str = "myriad-enc-sha256-v1";

/// Plaintext prefix sealed inside the integrity token (domain separation).
const INTEGRITY_PLAIN_PREFIX: &str = "myriad-analytics-backup-v1:";

/// Hard ceiling for any single metric cell (views / count / engagement_ms / …).
pub(crate) const MAX_METRIC_VALUE: i64 = 1_000_000_000_000;

/// How far past `analytics_today` a backup day may lie (clock skew / TZ edge).
const MAX_DAY_AHEAD: i64 = 1;

/// How far before today a backup day may lie (retention × 3, min 3y).
fn min_import_day(today: NaiveDate) -> NaiveDate {
    let span = (DAILY_RETENTION_DAYS * 3).max(365 * 3);
    today
        .checked_sub_signed(Duration::days(span))
        .unwrap_or(today)
}

fn max_import_day(today: NaiveDate) -> NaiveDate {
    today
        .checked_add_signed(Duration::days(MAX_DAY_AHEAD))
        .unwrap_or(today)
}

/// Length-prefixed UTF-8 into a hasher (avoids delimiter ambiguity).
fn digest_bytes(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u64).to_le_bytes());
    h.update(bytes);
}

fn digest_str(h: &mut Sha256, s: &str) {
    digest_bytes(h, s.as_bytes());
}

fn digest_section(h: &mut Sha256, name: &str) {
    digest_str(h, name);
}

/// JSON string field → trim; missing/null → empty.
fn json_str(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Non-negative integer as decimal string for hashing.
fn json_metric_str(row: &Value, key: &str) -> Option<String> {
    let n = i64_nonneg(row.get(key))?;
    Some(n.to_string())
}

fn digest_page_daily_row(h: &mut Sha256, row: &Value) -> Result<(), &'static str> {
    digest_str(h, &json_str(row, "day"));
    digest_str(h, &json_str(row, "path"));
    digest_str(
        h,
        &json_metric_str(row, "views").ok_or("invalid_page_daily_views")?,
    );
    digest_str(
        h,
        &json_metric_str(row, "unique_visitors").unwrap_or_else(|| "0".into()),
    );
    digest_str(
        h,
        &json_metric_str(row, "engagement_ms").unwrap_or_else(|| "0".into()),
    );
    digest_str(
        h,
        &json_metric_str(row, "engaged_views").unwrap_or_else(|| "0".into()),
    );
    Ok(())
}

fn digest_visitor_seen_row(h: &mut Sha256, row: &Value) -> Result<(), &'static str> {
    digest_str(h, &json_str(row, "day"));
    digest_str(h, &json_str(row, "path"));
    digest_str(h, &json_str(row, "visitor_hash"));
    digest_str(
        h,
        &json_metric_str(row, "ordinal").unwrap_or_else(|| "0".into()),
    );
    Ok(())
}

fn digest_event_daily_row(h: &mut Sha256, row: &Value) -> Result<(), &'static str> {
    digest_str(h, &json_str(row, "day"));
    digest_str(h, &json_str(row, "event_name"));
    digest_str(h, &json_str(row, "path"));
    digest_str(h, &json_str(row, "target"));
    digest_str(
        h,
        &json_metric_str(row, "count").ok_or("invalid_event_daily_count")?,
    );
    digest_str(
        h,
        &json_metric_str(row, "unique_visitors").unwrap_or_else(|| "0".into()),
    );
    Ok(())
}

fn digest_event_visitor_row(h: &mut Sha256, row: &Value) -> Result<(), &'static str> {
    digest_str(h, &json_str(row, "day"));
    digest_str(h, &json_str(row, "event_name"));
    digest_str(h, &json_str(row, "path"));
    digest_str(h, &json_str(row, "target"));
    digest_str(h, &json_str(row, "visitor_hash"));
    Ok(())
}

fn digest_referrer_daily_row(h: &mut Sha256, row: &Value) -> Result<(), &'static str> {
    digest_str(h, &json_str(row, "day"));
    digest_str(h, &json_str(row, "host"));
    digest_str(
        h,
        &json_metric_str(row, "count").ok_or("invalid_referrer_count")?,
    );
    Ok(())
}

fn digest_country_daily_row(h: &mut Sha256, row: &Value) -> Result<(), &'static str> {
    digest_str(h, &json_str(row, "day"));
    digest_str(h, &json_str(row, "country_code"));
    digest_str(h, &json_str(row, "country_name"));
    digest_str(
        h,
        &json_metric_str(row, "views").ok_or("invalid_country_views")?,
    );
    digest_str(
        h,
        &json_metric_str(row, "unique_visitors").unwrap_or_else(|| "0".into()),
    );
    Ok(())
}

fn digest_country_visitor_row(h: &mut Sha256, row: &Value) -> Result<(), &'static str> {
    digest_str(h, &json_str(row, "day"));
    digest_str(h, &json_str(row, "country_code"));
    digest_str(h, &json_str(row, "visitor_hash"));
    Ok(())
}

fn digest_table(
    h: &mut Sha256,
    section: &str,
    rows: &[Value],
    digest_row: fn(&mut Sha256, &Value) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    digest_section(h, section);
    h.update((rows.len() as u64).to_le_bytes());
    for row in rows {
        digest_row(h, row)?;
    }
    Ok(())
}

/// Field-canonical SHA-256 of the backup payload (hex).
///
/// Key order and pretty-print do not affect the hash — only declared field
/// values. `exported_at` / `timezone` / `integrity` are excluded from the hash.
pub(crate) fn content_hash(
    format: &str,
    version: u32,
    page_daily: &[Value],
    visitor_seen: &[Value],
    event_daily: &[Value],
    event_visitor: &[Value],
    referrer_daily: &[Value],
    country_daily: &[Value],
    country_visitor: &[Value],
) -> Result<String, &'static str> {
    let mut h = Sha256::new();
    digest_str(&mut h, "myriad-analytics-content-v1");
    digest_str(&mut h, format);
    h.update(version.to_le_bytes());
    digest_table(&mut h, "page_daily", page_daily, digest_page_daily_row)?;
    digest_table(
        &mut h,
        "visitor_seen",
        visitor_seen,
        digest_visitor_seen_row,
    )?;
    digest_table(&mut h, "event_daily", event_daily, digest_event_daily_row)?;
    digest_table(
        &mut h,
        "event_visitor",
        event_visitor,
        digest_event_visitor_row,
    )?;
    digest_table(
        &mut h,
        "referrer_daily",
        referrer_daily,
        digest_referrer_daily_row,
    )?;
    digest_table(
        &mut h,
        "country_daily",
        country_daily,
        digest_country_daily_row,
    )?;
    digest_table(
        &mut h,
        "country_visitor",
        country_visitor,
        digest_country_visitor_row,
    )?;
    Ok(hex::encode(h.finalize()))
}

/// Seal content hash with the instance data key (AES-GCM authenticates).
pub(crate) fn seal_integrity(content_hash_hex: &str) -> Result<Value, String> {
    let plain = format!("{INTEGRITY_PLAIN_PREFIX}{content_hash_hex}");
    let token = myriad_data_key::data_key()
        .encrypt(&plain)
        .map_err(|error| {
            tracing::error!(%error, "integrity seal failed");
            "integrity_seal_failed".to_string()
        })?;
    Ok(json!({
        "alg": INTEGRITY_ALG,
        "key_fingerprint": myriad_data_key::data_key().fingerprint(),
        "content_hash": content_hash_hex,
        "token": token,
    }))
}

/// Verify integrity block against the recomputed content hash.
///
/// Returns `Ok(())` when the token decrypts and matches `expected_hash`.
pub(crate) fn verify_integrity(integrity: &Value, expected_hash: &str) -> Result<(), &'static str> {
    let alg = integrity.get("alg").and_then(|v| v.as_str()).unwrap_or("");
    if alg != INTEGRITY_ALG {
        return Err("unsupported_integrity_alg");
    }

    let declared_hash = integrity
        .get("content_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if declared_hash != expected_hash {
        return Err("content_hash_mismatch");
    }

    let token = integrity
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if token.is_empty() || !myriad_data_key::is_ciphertext(token) {
        return Err("missing_integrity_token");
    }

    let plain = match myriad_data_key::data_key().decrypt(token) {
        Ok(p) => p,
        Err(_) => return Err("invalid_integrity_token"),
    };

    let Some(hash_in_token) = plain.strip_prefix(INTEGRITY_PLAIN_PREFIX) else {
        return Err("invalid_integrity_token");
    };
    if hash_in_token != expected_hash {
        return Err("integrity_token_mismatch");
    }

    // Optional key fingerprint: present non-empty mismatch is Err(integrity_key_mismatch).
    if let Some(fp) = integrity.get("key_fingerprint").and_then(|v| v.as_str()) {
        if !fp.is_empty() && fp != myriad_data_key::data_key().fingerprint() {
            return Err("integrity_key_mismatch");
        }
    }

    Ok(())
}

/// Validate declared `counts` against actual array lengths.
pub(crate) fn validate_counts_object(
    counts: Option<&Value>,
    page_daily: usize,
    visitor_seen: usize,
    event_daily: usize,
    event_visitor: usize,
    referrer_daily: usize,
    country_daily: usize,
    country_visitor: usize,
) -> Result<(), &'static str> {
    let Some(counts) = counts else {
        return Ok(()); // `counts` 省略则跳过这项校验
    };
    if !counts.is_object() {
        return Err("invalid_counts");
    }
    let check = |key: &str, actual: usize| -> Result<(), &'static str> {
        match counts.get(key) {
            None => Ok(()),
            Some(v) => {
                let n = v
                    .as_u64()
                    .or_else(|| v.as_i64().and_then(|i| u64::try_from(i).ok()))
                    .ok_or("invalid_counts")?;
                if n as usize != actual {
                    Err("counts_mismatch")
                } else {
                    Ok(())
                }
            }
        }
    };
    check("page_daily", page_daily)?;
    check("visitor_seen", visitor_seen)?;
    check("event_daily", event_daily)?;
    check("event_visitor", event_visitor)?;
    check("referrer_daily", referrer_daily)?;
    check("country_daily", country_daily)?;
    check("country_visitor", country_visitor)?;
    Ok(())
}

fn day_in_import_range(day: NaiveDate) -> bool {
    let today = analytics_today();
    day >= min_import_day(today) && day <= max_import_day(today)
}

fn metric_in_range(n: i64) -> bool {
    (0..=MAX_METRIC_VALUE).contains(&n)
}

/// Structural pre-check used before DB writes.
///
/// Returns a skip count for invalid day/path/hash/range. Import rejects when `skipped > 0`;
/// insert loops coerce some optional metrics to 0 instead of skipping.
pub(crate) fn prevalidate_rows(
    page_daily: &[Value],
    visitor_seen: &[Value],
    event_daily: &[Value],
    event_visitor: &[Value],
    referrer_daily: &[Value],
    country_daily: &[Value],
    country_visitor: &[Value],
) -> Result<u64, &'static str> {
    let mut skipped: u64 = 0;

    for row in page_daily {
        let day_ok = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_in_import_range(*d));
        let path_raw = row.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let path_ok = if path_raw == SITE_PATH {
            true
        } else {
            normalize_path(path_raw).is_some()
        };
        let views = i64_nonneg(row.get("views")).filter(|&n| metric_in_range(n));
        let eng = i64_nonneg(row.get("engagement_ms")).unwrap_or(0);
        let uv = i64_nonneg(row.get("unique_visitors")).unwrap_or(0);
        let eng_v = i64_nonneg(row.get("engaged_views")).unwrap_or(0);
        if day_ok.is_none()
            || !path_ok
            || views.is_none()
            || !metric_in_range(eng)
            || !metric_in_range(uv)
            || !metric_in_range(eng_v)
        {
            skipped += 1;
        }
    }

    for row in visitor_seen {
        let day_ok = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_in_import_range(*d));
        let path_raw = row.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let path_ok = if path_raw == SITE_PATH {
            true
        } else {
            normalize_path(path_raw).is_some()
        };
        let hash = row
            .get("visitor_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ordinal = i64_nonneg(row.get("ordinal")).unwrap_or(0);
        if day_ok.is_none() || !path_ok || !valid_visitor_hash(hash) || !metric_in_range(ordinal) {
            skipped += 1;
        }
    }

    for row in event_daily {
        let day_ok = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_in_import_range(*d));
        let name_raw = row.get("event_name").and_then(|v| v.as_str()).unwrap_or("");
        let name_ok = name_raw == ENGAGE_MARKER || normalize_event_name(name_raw).is_some();
        let path_raw = row.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let path_ok = path_raw.is_empty() || normalize_path(path_raw).is_some();
        let count = i64_nonneg(row.get("count")).filter(|&n| metric_in_range(n));
        let uv = i64_nonneg(row.get("unique_visitors")).unwrap_or(0);
        if day_ok.is_none() || !name_ok || !path_ok || count.is_none() || !metric_in_range(uv) {
            skipped += 1;
        }
    }

    for row in event_visitor {
        let day_ok = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_in_import_range(*d));
        let name_raw = row.get("event_name").and_then(|v| v.as_str()).unwrap_or("");
        let name_ok = name_raw == ENGAGE_MARKER || normalize_event_name(name_raw).is_some();
        let path_raw = row.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let path_ok = path_raw.is_empty() || normalize_path(path_raw).is_some();
        let hash = row
            .get("visitor_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if day_ok.is_none() || !name_ok || !path_ok || !valid_visitor_hash(hash) {
            skipped += 1;
        }
    }

    for row in referrer_daily {
        let day_ok = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_in_import_range(*d));
        let host_raw = row.get("host").and_then(|v| v.as_str()).unwrap_or("");
        let host_ok = normalize_referrer_host(host_raw).is_some();
        let count = i64_nonneg(row.get("count")).filter(|&n| metric_in_range(n));
        if day_ok.is_none() || !host_ok || count.is_none() {
            skipped += 1;
        }
    }

    for row in country_daily {
        let day_ok = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_in_import_range(*d));
        let code_raw = row
            .get("country_code")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let code_ok = normalize_country_code(code_raw).is_some();
        let views = i64_nonneg(row.get("views")).filter(|&n| metric_in_range(n));
        let uv = i64_nonneg(row.get("unique_visitors")).unwrap_or(0);
        if day_ok.is_none() || !code_ok || views.is_none() || !metric_in_range(uv) {
            skipped += 1;
        }
    }

    for row in country_visitor {
        let day_ok = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_in_import_range(*d));
        let code_raw = row
            .get("country_code")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let code_ok = normalize_country_code(code_raw).is_some();
        let hash = row
            .get("visitor_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if day_ok.is_none() || !code_ok || !valid_visitor_hash(hash) {
            skipped += 1;
        }
    }

    Ok(skipped)
}

/// Cap check applied during insert loops (defense in depth after prevalidate).
pub(crate) fn metric_ok(n: i64) -> bool {
    metric_in_range(n)
}

pub(crate) fn day_ok_for_import(day: NaiveDate) -> bool {
    day_in_import_range(day)
}

#[cfg(test)]
mod tests {
    use super::super::intake_helpers::ANALYTICS_BACKUP_FORMAT;
    use super::*;
    use serde_json::json;

    fn sample_page() -> Value {
        json!({
            "day": "2026-07-01",
            "path": "/",
            "views": 10,
            "unique_visitors": 3,
            "engagement_ms": 1000,
            "engaged_views": 2,
        })
    }

    #[test]
    fn content_hash_stable_and_order_insensitive_keys() {
        let a = vec![sample_page()];
        let empty: Vec<Value> = vec![];
        let h1 = content_hash(
            ANALYTICS_BACKUP_FORMAT,
            1,
            &a,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
        )
        .unwrap();

        // Same values, different key order in the object
        let b = vec![json!({
            "views": 10,
            "path": "/",
            "engaged_views": 2,
            "day": "2026-07-01",
            "engagement_ms": 1000,
            "unique_visitors": 3,
        })];
        let h2 = content_hash(
            ANALYTICS_BACKUP_FORMAT,
            1,
            &b,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
        )
        .unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn content_hash_changes_when_metric_tampered() {
        let empty: Vec<Value> = vec![];
        let good = vec![sample_page()];
        let mut bad_row = sample_page();
        bad_row
            .as_object_mut()
            .unwrap()
            .insert("views".into(), json!(999_999));
        let bad = vec![bad_row];
        let h1 = content_hash(
            ANALYTICS_BACKUP_FORMAT,
            1,
            &good,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
        )
        .unwrap();
        let h2 = content_hash(
            ANALYTICS_BACKUP_FORMAT,
            1,
            &bad,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
        )
        .unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn seal_and_verify_roundtrip() {
        let empty: Vec<Value> = vec![];
        let rows = vec![sample_page()];
        let hash = content_hash(
            ANALYTICS_BACKUP_FORMAT,
            1,
            &rows,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
            &empty,
        )
        .unwrap();
        let integrity = seal_integrity(&hash).expect("seal");
        assert_eq!(
            integrity.get("alg").and_then(|v| v.as_str()),
            Some(INTEGRITY_ALG)
        );
        verify_integrity(&integrity, &hash).expect("verify ok");

        // Tamper content_hash field while keeping token → mismatch
        let mut tampered = integrity.clone();
        tampered
            .as_object_mut()
            .unwrap()
            .insert("content_hash".into(), json!("0".repeat(64)));
        assert_eq!(
            verify_integrity(&tampered, &hash).unwrap_err(),
            "content_hash_mismatch"
        );

        // Wrong expected hash (body tampered)
        assert!(verify_integrity(&integrity, &"1".repeat(64)).is_err());
    }

    #[test]
    fn counts_must_match_lengths() {
        let counts = json!({
            "page_daily": 1,
            "visitor_seen": 0,
        });
        assert!(validate_counts_object(Some(&counts), 1, 0, 0, 0, 0, 0, 0).is_ok());
        assert_eq!(
            validate_counts_object(Some(&counts), 2, 0, 0, 0, 0, 0, 0).unwrap_err(),
            "counts_mismatch"
        );
    }

    #[test]
    fn metric_ceiling() {
        assert!(metric_ok(0));
        assert!(metric_ok(MAX_METRIC_VALUE));
        assert!(!metric_ok(MAX_METRIC_VALUE + 1));
        assert!(!metric_ok(-1));
    }
}
