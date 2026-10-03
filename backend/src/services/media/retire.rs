//! Retire the media layer that served files under their pre-asset addresses
//! (`/media/federation/…`, `/api/brew/image-cache/…`, and cache paths
//! registered as aliases). Runs at startup while that layer's tables exist,
//! in one transaction: stored content is rewritten to permanent addresses,
//! then the tables go. A database without them has nothing to retire.
//!
//! `Migrator::up` already refused a database whose upgrade job had not copied
//! every file into the asset store.

use std::collections::{BTreeMap, HashMap};

use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
};

use super::urls::{compatible_url, filename_for_mime, registered_local_path};

const PHANTASI_CACHE: &str = "/api/phantasi/image-cache/";
const BREW_CACHE: &str = "/api/brew/image-cache/";
const FEDERATION: &str = "/media/federation/";

/// An old address in stored text: the origin it was written with, if any,
/// then its path up to the first character no stored path carries.
static OLD_ADDRESS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(https?://[^/\s"'()<>\\]+)?(?:/media/federation/|/api/(?:brew|phantasi)/image-cache/)[A-Za-z0-9._\-/]*"#,
    )
    .expect("old address pattern")
});

/// Rewrite, normalise and drop; returns how many stored values changed.
pub(crate) async fn retire_legacy_media(db: &DatabaseConnection) -> Result<u64, DbErr> {
    if !table_exists(db, "media_url_aliases").await?
        && !table_exists(db, "media_migration_jobs").await?
    {
        return Ok(0);
    }
    let origins = super::configured_origins().await;
    let txn = db.begin().await?;
    let addresses = permanent_addresses(&txn).await?;
    let renamed = Renamed {
        paths: old_addresses(&txn, &addresses).await?,
        origins,
    };
    let rewritten = rewrite_stored_values(&txn, &renamed).await?;
    let normalised = normalise_catalog_urls(&txn, &addresses).await?;
    let missing = txn
        .execute_unprepared(
            "UPDATE media_assets SET state = 'missing', state_since = NOW(), updated_at = NOW()
             WHERE state IS NULL",
        )
        .await?
        .rows_affected();
    // Only the upgrade bound queued deliveries; their URLs are retired here.
    let outbox = txn
        .execute_unprepared(
            "DELETE FROM media_references WHERE consumer_type = 'federation_outbox'",
        )
        .await?
        .rows_affected();
    txn.execute_unprepared(
        "DROP TABLE IF EXISTS media_url_aliases; DROP TABLE IF EXISTS media_migration_jobs",
    )
    .await?;
    txn.commit().await?;
    tracing::info!(
        rewritten,
        normalised,
        missing,
        outbox,
        "Retired the old media addresses"
    );
    Ok(rewritten)
}

async fn table_exists(db: &impl ConnectionTrait, table: &str) -> Result<bool, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT to_regclass($1) IS NOT NULL AS present",
            [table.into()],
        ))
        .await?;
    Ok(row
        .map(|row| row.try_get::<bool>("", "present"))
        .transpose()?
        .unwrap_or(false))
}

/// Asset id → permanent address, for every asset that has one.
async fn permanent_addresses(db: &impl ConnectionTrait) -> Result<BTreeMap<i32, String>, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, public_id, name, mime FROM media_assets
             WHERE public_id IS NOT NULL AND state IN ('ready', 'deleting', 'missing')",
        ))
        .await?;
    let mut out = BTreeMap::new();
    for row in rows {
        let id: i32 = row.try_get("", "id")?;
        let public_id: uuid::Uuid = row.try_get("", "public_id")?;
        let name: String = row.try_get("", "name")?;
        let mime: String = row.try_get("", "mime")?;
        if let Ok(filename) = filename_for_mime(&name, &mime, public_id) {
            out.insert(id, compatible_url(public_id, &filename));
        }
    }
    Ok(out)
}

/// Old path → permanent address, and the origins this site wrote absolute
/// addresses under: one under any other origin is another site's file.
struct Renamed {
    paths: HashMap<String, String>,
    origins: Vec<String>,
}

/// Old address → permanent address, from the aliases and from catalogue rows
/// still named by an old address. A cached file answers to both spellings.
async fn old_addresses(
    db: &impl ConnectionTrait,
    addresses: &BTreeMap<i32, String>,
) -> Result<HashMap<String, String>, DbErr> {
    let mut sql = String::from(
        "SELECT url AS old, id AS asset_id FROM media_assets
         WHERE url ~ '/media/federation/|/api/(phantasi|brew)/image-cache/'",
    );
    if table_exists(db, "media_url_aliases").await? {
        sql.push_str(" UNION ALL SELECT local_path, asset_id FROM media_url_aliases");
    }
    let rows = db
        .query_all_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await?;
    let mut map = HashMap::new();
    for row in rows {
        let old: String = row.try_get("", "old")?;
        let asset_id: i32 = row.try_get("", "asset_id")?;
        let (Some(path), Some(permanent)) = (registered_old_path(&old), addresses.get(&asset_id))
        else {
            continue;
        };
        for spelling in spellings(&path) {
            map.entry(spelling).or_insert_with(|| permanent.clone());
        }
    }
    Ok(map)
}

/// The path of an old address. These prefixes are no longer citable, so
/// [`registered_local_path`] checks the rest with a current prefix in place.
fn registered_old_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let path = match trimmed.split_once("://") {
        Some((_, rest)) => &rest[rest.find('/')?..],
        None => trimmed,
    };
    let path = path.split('?').next().unwrap_or(path);
    for prefix in [FEDERATION, BREW_CACHE, PHANTASI_CACHE] {
        if let Some(rest) = path.strip_prefix(prefix) {
            registered_local_path(&format!("/media/assets/{rest}"))?;
            return Some(path.to_string());
        }
    }
    None
}

fn spellings(path: &str) -> Vec<String> {
    if let Some(rest) = path.strip_prefix(BREW_CACHE) {
        return vec![path.to_string(), format!("{PHANTASI_CACHE}{rest}")];
    }
    if let Some(rest) = path.strip_prefix(PHANTASI_CACHE) {
        return vec![path.to_string(), format!("{BREW_CACHE}{rest}")];
    }
    vec![path.to_string()]
}

/// Each old address this site wrote, as its permanent address. Matched as a
/// whole path, so a longer path is never cut through a shorter one; a path
/// ending a sentence keeps its full stop.
fn rewrite(text: &str, renamed: &Renamed) -> String {
    OLD_ADDRESS
        .replace_all(text, |found: &Captures| {
            let whole = &found[0];
            let origin = found.get(1).map_or("", |origin| origin.as_str());
            if !origin.is_empty() && !super::urls::is_allowed_origin(origin, &renamed.origins) {
                return whole.to_string();
            }
            let address = &whole[origin.len()..];
            let path = address.trim_end_matches('.');
            let tail = &address[path.len()..];
            if let Some(permanent) = renamed.paths.get(path) {
                return format!("{origin}{permanent}{tail}");
            }
            // A cached file nothing imported keeps its cache address.
            match path.strip_prefix(BREW_CACHE) {
                Some(rest) => format!("{origin}{PHANTASI_CACHE}{rest}{tail}"),
                None => whole.to_string(),
            }
        })
        .into_owned()
}

/// Every text and JSON column outside the media catalogue that cites an old
/// address. Each row is rewritten with its triggers off: this is the same
/// content under its permanent address, not an edit (no history revision,
/// no timestamp).
async fn rewrite_stored_values(db: &impl ConnectionTrait, renamed: &Renamed) -> Result<u64, DbErr> {
    let mut alternatives = vec![regex_escape(FEDERATION), regex_escape(BREW_CACHE)];
    if renamed
        .paths
        .keys()
        .any(|old| old.starts_with(PHANTASI_CACHE))
    {
        alternatives.push(regex_escape(PHANTASI_CACHE));
    }
    let pattern = alternatives.join("|");
    let columns = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT c.table_name, c.column_name, c.data_type FROM information_schema.columns c
               JOIN information_schema.tables t
                 ON t.table_schema = c.table_schema AND t.table_name = c.table_name
              WHERE c.table_schema = current_schema() AND t.table_type = 'BASE TABLE'
                AND c.data_type IN ('text', 'character varying', 'json', 'jsonb')
                AND c.table_name NOT IN ('media_assets', 'media_url_aliases', 'media_migration_jobs')
              ORDER BY 1, 2",
        ))
        .await?;
    let mut changed = 0;
    for column in columns {
        let table: String = column.try_get("", "table_name")?;
        let name: String = column.try_get("", "column_name")?;
        let data_type: String = column.try_get("", "data_type")?;
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                // Locked: a worker updating a row in between would move it to
                // another ctid and the rewrite below would miss it.
                format!(
                    "SELECT ctid::text AS tid, {col}::text AS value FROM {tbl} WHERE {col}::text ~ $1 FOR UPDATE",
                    col = quote_ident(&name),
                    tbl = quote_ident(&table),
                ),
                [pattern.clone().into()],
            ))
            .await?;
        if rows.is_empty() {
            continue;
        }
        let cast = match data_type.as_str() {
            "json" => "::json",
            "jsonb" => "::jsonb",
            _ => "",
        };
        db.execute_unprepared(&format!(
            "ALTER TABLE {} DISABLE TRIGGER USER",
            quote_ident(&table)
        ))
        .await?;
        for row in rows {
            let tid: String = row.try_get("", "tid")?;
            let value: String = row.try_get("", "value")?;
            let updated = rewrite(&value, renamed);
            if updated == value {
                continue;
            }
            changed += db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    format!(
                        "UPDATE {tbl} SET {col} = $1{cast} WHERE ctid = $2::tid",
                        col = quote_ident(&name),
                        tbl = quote_ident(&table),
                    ),
                    [updated.into(), tid.into()],
                ))
                .await?
                .rows_affected();
        }
        db.execute_unprepared(&format!(
            "ALTER TABLE {} ENABLE TRIGGER USER",
            quote_ident(&table)
        ))
        .await?;
    }
    Ok(changed)
}

/// Catalogue rows carry their asset's permanent address.
async fn normalise_catalog_urls(
    db: &impl ConnectionTrait,
    addresses: &BTreeMap<i32, String>,
) -> Result<u64, DbErr> {
    let mut changed = 0;
    for (id, url) in addresses {
        changed += db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE media_assets SET url = $1 WHERE id = $2 AND url IS DISTINCT FROM $1",
                [url.clone().into(), (*id).into()],
            ))
            .await?
            .rows_affected();
    }
    Ok(changed)
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn regex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if r"\.^$|?*+()[]{}".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "4461b51fe11b6d1d8928451c0a10a7166882f3cf058c6ef679cd993adcc8e2d4";

    #[test]
    fn old_addresses_keep_their_path_and_reject_traversal() {
        assert_eq!(
            registered_old_path("https://old.example/media/federation/1/a.png?x=1").as_deref(),
            Some("/media/federation/1/a.png")
        );
        assert_eq!(registered_old_path("/media/federation/../secret"), None);
        assert_eq!(registered_old_path("/media/assets/x/a.png"), None);
    }

    fn renamed(pairs: &[(&str, &str)]) -> Renamed {
        let mut paths = HashMap::new();
        for (old, new) in pairs {
            for spelling in spellings(old) {
                paths.insert(spelling, new.to_string());
            }
        }
        Renamed {
            paths,
            origins: vec!["https://own.example".into()],
        }
    }

    #[test]
    fn a_cached_file_is_rewritten_under_both_spellings() {
        let phantasi = format!("{PHANTASI_CACHE}44/{HASH}.png");
        let renamed = renamed(&[(&phantasi, "/media/assets/u/a.png")]);
        let text =
            format!("![]({phantasi}) ![]({BREW_CACHE}44/{HASH}.png) ![]({BREW_CACHE}ab/other.png)");
        assert_eq!(
            rewrite(&text, &renamed),
            format!(
                "![](/media/assets/u/a.png) ![](/media/assets/u/a.png) ![]({PHANTASI_CACHE}ab/other.png)"
            )
        );
    }

    #[test]
    fn only_this_sites_addresses_are_rewritten_and_only_whole() {
        let renamed = renamed(&[("/media/federation/1/a.png", "/media/assets/u/a.png")]);
        let text = "https://own.example/media/federation/1/a.png?x=1 \
                    https://OWN.example/media/federation/1/a.png \
                    https://other.example/media/federation/1/a.png \
                    https://other.example/api/brew/image-cache/ab/c.png \
                    /media/federation/1/a.png.bak, see /media/federation/1/a.png.";
        assert_eq!(
            rewrite(text, &renamed),
            "https://own.example/media/assets/u/a.png?x=1 \
             https://OWN.example/media/assets/u/a.png \
             https://other.example/media/federation/1/a.png \
             https://other.example/api/brew/image-cache/ab/c.png \
             /media/federation/1/a.png.bak, see /media/assets/u/a.png."
        );
    }

    #[test]
    fn regex_escape_quotes_metacharacters() {
        assert_eq!(regex_escape("/a.b(c)"), r"/a\.b\(c\)");
    }
}
