//! Finish the missing-only tail of the 0.6.1 media job before the SQL gate.
//! Read catalogued paths only; never crawl or remove the old media directory.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use once_cell::sync::Lazy;
use regex::Regex;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
};
use serde_json::Value;

use crate::models::entities::media_assets;
use crate::services::data_paths::DataPaths;

use super::{MediaStore, store::hash_path, urls};

static CITATION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(https?://[^/\s"'()<>\\]+)?(/media/(?:federation|assets)/|/api/media/|/api/(?:brew|phantasi)/image-cache/)[A-Za-z0-9._\-/]*"#)
        .expect("media upgrade citation pattern")
});

fn refused(reason: &str) -> DbErr {
    DbErr::Custom(format!(
        "media upgrade cannot safely continue: {reason}; check the media mounts and finish the 0.6.1 upgrade before retrying"
    ))
}

/// `true` authorizes only the missing-only branch of the migrator's SQL gate.
/// An intact ready catalogue is evidence that DATA_DIR is the existing volume.
/// With no such evidence, the existing operator opt-in is still required.
pub(crate) async fn prepare_legacy_media_upgrade(
    db: &DatabaseConnection,
    paths: &DataPaths,
    origins: &[String],
    accept_missing: bool,
) -> Result<bool, DbErr> {
    let txn = db.begin().await?;
    let present = txn
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT to_regclass('media_migration_jobs') IS NOT NULL AS present",
        ))
        .await?
        .ok_or_else(|| refused("cannot read job table"))?;
    if !present.try_get::<bool>("", "present")? {
        return Ok(false);
    }
    // Same lock as 0.6.1; do not race a last background retry.
    txn.execute_unprepared("SELECT pg_advisory_xact_lock(hashtextextended('media:upgrade:v2', 0)); LOCK TABLE media_migration_jobs IN SHARE ROW EXCLUSIVE MODE").await?;
    let job = txn.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT cursor FROM media_migration_jobs WHERE source_kind = 'upgrade' AND source_key = 'platform_media_v2'"))
        .await?;
    let cursor = job
        .and_then(|row| row.try_get::<Option<String>>("", "cursor").ok().flatten())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
    let Some(cursor) = cursor else {
        return Ok(false);
    };
    if cursor["revision"] != 4 || cursor["complete"] == true || cursor["retrying"] != true {
        return Ok(false); // Let the existing SQL gate explain malformed/unfinished jobs.
    }
    // Do not repair an unsupported database before its support-floor gate runs.
    let floor = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (SELECT 1 FROM _schema_versions WHERE version >= $1) AS supported",
            [migration::SUPPORT_FLOOR_SCHEMA_MARK.into()],
        ))
        .await?;
    if !floor.is_some_and(|row| row.try_get::<bool>("", "supported").unwrap_or(false)) {
        return Ok(false);
    }
    let failures = txn.query_all_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT source_key, error_code, cursor FROM media_migration_jobs WHERE source_kind = 'upgrade_failure' ORDER BY source_key"))
        .await?;
    if failures.is_empty() {
        return Ok(false);
    }
    let failures = failures
        .into_iter()
        .map(|row| {
            Ok(Failure {
                key: row.try_get("", "source_key")?,
                error: row.try_get("", "error_code")?,
                cursor: row.try_get("", "cursor")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;
    let assets = txn
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT * FROM media_assets ORDER BY id FOR UPDATE",
        ))
        .await?
        .into_iter()
        .map(|row| <media_assets::Model as sea_orm::FromQueryResult>::from_query_result(&row, ""))
        .collect::<Result<Vec<_>, _>>()?;
    let by_id: HashMap<_, _> = assets.iter().map(|asset| (asset.id, asset)).collect();
    let store = MediaStore::new(paths.media.clone());
    let mut missing = HashSet::new();
    for failure in &failures {
        let Some(raw_id) = failure.key.strip_prefix("1:0:") else {
            continue;
        };
        let id = raw_id
            .parse::<i32>()
            .map_err(|_| refused("invalid copy failure key"))?;
        let asset = by_id
            .get(&id)
            .ok_or_else(|| refused("copy failure has no catalogue row"))?;
        if failure.error.as_deref() != Some("MEDIA_MISSING")
            || failure.cursor.as_deref() != Some(format!("media_assets:{id}").as_str())
            || !matches!(asset.state.as_deref(), None | Some("missing"))
        {
            return Err(refused("copy failure is not a missing file"));
        }
        let public_id = asset
            .public_id
            .ok_or_else(|| refused("missing asset has no identity"))?;
        // Validate the permanent address now so retirement cannot strand its citations.
        urls::filename_for_mime(&asset.name, &asset.mime, public_id)
            .map_err(|_| refused("unsupported missing media"))?;
        let old = local_path(&asset.url, origins)
            .ok_or_else(|| refused("invalid legacy catalogue address"))?;
        let source =
            legacy_file(paths, &old).ok_or_else(|| refused("unsupported legacy catalogue path"))?;
        require_absent(&source).await?;
        let ext = super::validate::extension_for_mime(&asset.mime)
            .ok_or_else(|| refused("unsupported missing MIME"))?;
        let key = asset.storage_key.clone().unwrap_or(
            urls::storage_key(public_id, ext).map_err(|_| refused("invalid asset identity"))?,
        );
        let dest = store
            .final_path(&key)
            .map_err(|_| refused("invalid storage key"))?;
        require_absent(&dest).await?;
        missing.insert(id);
    }
    if missing.is_empty() {
        return Err(refused("no verified missing copy failures"));
    }
    if assets
        .iter()
        .any(|asset| asset.state.is_none() && !missing.contains(&asset.id))
    {
        return Err(refused(
            "uncopied assets exist outside the missing failures",
        ));
    }
    let mut ready = 0;
    for asset in &assets {
        if asset.state.as_deref() != Some("ready") {
            continue;
        }
        let key = asset
            .storage_key
            .as_deref()
            .ok_or_else(|| refused("ready asset has no storage key"))?;
        let file = store
            .final_path(key)
            .map_err(|_| refused("invalid ready storage key"))?;
        let (size, checksum) = hash_path(&file)
            .await
            .map_err(|_| refused("ready media cannot be read; the volume may be missing"))?;
        if asset.size < 0
            || size != asset.size as u64
            || asset.checksum_sha256.as_deref() != Some(checksum.as_str())
        {
            return Err(refused(
                "ready media does not match its catalogue; check the mounted volume",
            ));
        }
        ready += 1;
    }
    if ready == 0 && !accept_missing {
        return Err(DbErr::Custom("media upgrade has only missing files and no intact ready media to verify the volume: check the mounts; if the files are gone, start once with MYRIAD_ACCEPT_MISSING_MEDIA=1".into()));
    }
    let mut addresses = HashMap::new();
    for asset in &assets {
        if let Some(path) = local_path(&asset.url, origins) {
            addresses.insert(path, asset.id);
        }
        addresses.insert(urls::content_path(asset.id), asset.id);
        if let Some(public_id) = asset.public_id {
            if let Ok(filename) = urls::filename_for_mime(&asset.name, &asset.mime, public_id) {
                addresses.insert(urls::compatible_url(public_id, &filename), asset.id);
            }
        }
    }
    let aliases = txn
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT local_path, asset_id FROM media_url_aliases",
        ))
        .await?;
    for alias in aliases {
        let path: String = alias.try_get("", "local_path")?;
        if let Some(path) = local_path(&path, origins) {
            addresses.insert(path, alias.try_get::<i32>("", "asset_id")?);
        }
    }
    for failure in &failures {
        if failure.key.starts_with("1:0:") {
            continue;
        }
        let (pass, phase, id) = failure
            .parts()
            .ok_or_else(|| refused("invalid failure key"))?;
        if !matches!(pass, 0 | 2)
            || phase == 0
            || !matches!(
                failure.error.as_deref(),
                Some("MEDIA_MISSING" | "MEDIA_NOT_READY") | None
            )
        {
            return Err(refused("an unrelated migration failure remains"));
        }
        // Discovery entries with no error were queued by a failed binding, not
        // proof that discovery itself succeeded. Require that binding record.
        if pass == 0
            && !failures
                .iter()
                .any(|other| other.key == format!("2:{phase}:{id}") && other.error.is_some())
        {
            return Err(refused("discovery failure has no matching binding failure"));
        }
        if pass == 2 && failure.error.is_none() {
            return Err(refused("binding failure has no error"));
        }
        let (table, predicate) =
            consumer(phase, id).ok_or_else(|| refused("unknown failure phase"))?;
        if failure.cursor.as_deref() != Some(format!("{table}:{id}").as_str()) {
            return Err(refused("failure cursor does not match its key"));
        }
        let row = txn
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                format!("SELECT to_jsonb(s) AS payload FROM {table} s WHERE {predicate} FOR SHARE"),
                [id.into()],
            ))
            .await?
            .ok_or_else(|| refused("failed consumer no longer exists"))?;
        let payload: Value = row.try_get("", "payload")?;
        let cited = cited_paths(&payload, origins);
        let mut cites_missing = false;
        for path in cited {
            let id = addresses
                .get(&path)
                .ok_or_else(|| refused("failed consumer cites uncatalogued media"))?;
            let asset = by_id
                .get(id)
                .ok_or_else(|| refused("alias has no catalogue row"))?;
            if missing.contains(id) {
                cites_missing = true;
            } else if asset.state.as_deref() != Some("ready") {
                return Err(refused("consumer has another unready asset"));
            }
        }
        if !cites_missing {
            return Err(refused(
                "consumer failure is unrelated to the missing files",
            ));
        }
    }
    // Commit only after every file and failure was verified. Do not forge a
    // complete cursor: the SQL gate still checks revision/retrying/missing-only.
    for id in &missing {
        txn.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "UPDATE media_assets SET state = 'missing', state_since = NOW(), updated_at = NOW() WHERE id = $1", [(*id).into()])).await?;
    }
    txn.execute_unprepared("UPDATE media_migration_jobs SET error_code = 'MEDIA_MISSING' WHERE source_kind = 'upgrade_failure'").await?;
    txn.commit().await?;
    tracing::warn!(
        missing_assets = missing.len(),
        failures = failures.len(),
        verified_ready = ready,
        "Accepted verified missing legacy media for upgrade"
    );
    Ok(true)
}

struct Failure {
    key: String,
    error: Option<String>,
    cursor: Option<String>,
}
impl Failure {
    fn parts(&self) -> Option<(u8, usize, &str)> {
        let mut parts = self.key.splitn(3, ':');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?,
        ))
    }
}

// 0.6.1 revision 4 phase identities. Never interpolate persisted table names.
fn consumer(phase: usize, id: &str) -> Option<(&'static str, &'static str)> {
    Some(match phase {
        1 => ("phantasi_note_docs", "id = $1::bigint"),
        2 => ("phantasi_items", "id = $1::bigint"),
        3 => (
            "phantasi_note_history",
            "doc_id = split_part($1, ':', 1)::bigint AND revision = split_part($1, ':', 2)::bigint",
        ),
        4 => ("agent_persona", "id = $1"),
        5 if id == "dashboard_layout" => ("configurations", "key = $1"),
        6 => (
            "federation_activities",
            "id = $1::bigint AND is_local = TRUE",
        ),
        7 => (
            "federation_delivery_queue",
            "id = $1::bigint AND status IN ('pending', 'delivering', 'failed')",
        ),
        8 => (
            "runtime_registry",
            "record_id = $1 AND namespace = 'ai_task'",
        ),
        9 => (
            "federation_channel_messages",
            "id = $1::bigint AND is_encrypted = FALSE",
        ),
        10 => ("agent_messages", "id = $1::bigint"),
        11 if id == "ui_wallpaper_url" => ("configurations", "key = $1"),
        12 => ("tapp_storage", "id = $1::bigint"),
        _ => return None,
    })
}

fn local_path(raw: &str, origins: &[String]) -> Option<String> {
    let raw = raw.trim();
    let path = if raw.contains("://") {
        let url = url::Url::parse(raw).ok()?;
        if !matches!(url.scheme(), "http" | "https") || !urls::is_allowed_origin(raw, origins) {
            return None;
        }
        url.path().to_string()
    } else {
        raw.split('?').next()?.to_string()
    };
    if path.contains("..") || path.contains(['\\', '\0', '%']) {
        return None;
    }
    if path.starts_with("/media/federation/") || path.starts_with("/api/brew/image-cache/") {
        Some(path.replacen("/api/brew/", "/api/phantasi/", 1))
    } else {
        urls::registered_local_path(&path)
    }
}

fn cited_paths(payload: &Value, origins: &[String]) -> HashSet<String> {
    let mut found = HashSet::new();
    fn walk(value: &Value, origins: &[String], found: &mut HashSet<String>) {
        match value {
            Value::String(text) => {
                for citation in CITATION.find_iter(text) {
                    if let Some(path) = local_path(citation.as_str(), origins) {
                        found.insert(path);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    walk(value, origins, found);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    walk(value, origins, found);
                }
            }
            _ => {}
        }
    }
    walk(payload, origins, &mut found);
    found
}

fn legacy_file(paths: &DataPaths, path: &str) -> Option<PathBuf> {
    if let Some(rest) = path.strip_prefix("/media/federation/") {
        let (owner, file) = rest.split_once('/')?;
        if owner.is_empty()
            || !owner.bytes().all(|b| b.is_ascii_digit())
            || file.is_empty()
            || file.starts_with('.')
            || file.contains('/')
            || !file
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return None;
        }
        return Some(paths.root.join("federation_media").join(owner).join(file));
    }
    let rest = path.strip_prefix("/api/phantasi/image-cache/")?;
    let (prefix, file) = rest.split_once('/')?;
    let (hash, ext) = file.rsplit_once('.')?;
    if prefix.len() != 2
        || hash.len() != 64
        || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        || !hash.starts_with(prefix)
        || !matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp")
    {
        return None;
    }
    Some(
        paths
            .cache_images
            .join(prefix.to_ascii_lowercase())
            .join(file.to_ascii_lowercase()),
    )
}

async fn require_absent(path: &std::path::Path) -> Result<(), DbErr> {
    match tokio::fs::metadata(path).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(refused("cannot inspect media file")),
        Ok(_) => Err(refused(
            "a file marked missing is present; finish copying it first",
        )),
    }
}

#[cfg(test)]
#[path = "upgrade_preflight_tests.rs"]
mod tests;
