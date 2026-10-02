//! Retire the media layer that served files under their pre-asset addresses
//! (`/media/federation/…`, `/api/brew/image-cache/…`, and cache paths
//! registered as aliases). Runs at startup while that layer's tables exist,
//! in one transaction: stored content is rewritten to permanent addresses,
//! then the tables go. A database without them has nothing to retire.
//!
//! `Migrator::up` already refused a database whose upgrade job had not copied
//! every file into the asset store.

use std::collections::BTreeMap;

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
};

use super::urls::{compatible_url, filename_for_mime, registered_local_path};

const PHANTASI_CACHE: &str = "/api/phantasi/image-cache/";
const BREW_CACHE: &str = "/api/brew/image-cache/";
const FEDERATION: &str = "/media/federation/";

/// Rewrite, normalise and drop; returns how many stored values changed.
pub(crate) async fn retire_legacy_media(db: &DatabaseConnection) -> Result<u64, DbErr> {
    if !table_exists(db, "media_url_aliases").await?
        && !table_exists(db, "media_migration_jobs").await?
    {
        return Ok(0);
    }
    let txn = db.begin().await?;
    let addresses = permanent_addresses(&txn).await?;
    let renamed = old_addresses(&txn, &addresses).await?;
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

/// Old address → permanent address, from the aliases and from catalogue rows
/// still named by an old address. A cached file answers to both spellings.
async fn old_addresses(
    db: &impl ConnectionTrait,
    addresses: &BTreeMap<i32, String>,
) -> Result<Vec<(String, String)>, DbErr> {
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
    let mut map = BTreeMap::new();
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
    // Longest first, so no address is rewritten through a shorter one.
    let mut pairs: Vec<_> = map.into_iter().collect();
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    Ok(pairs)
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

fn rewrite(text: &str, renamed: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (old, new) in renamed {
        if out.contains(old.as_str()) {
            out = out.replace(old.as_str(), new);
        }
    }
    // A cached file nothing imported keeps its cache address.
    out.replace(BREW_CACHE, PHANTASI_CACHE)
}

/// Every text and JSON column outside the media catalogue that cites an old
/// address. Each row is rewritten with its triggers off: this is the same
/// content under its permanent address, not an edit (no history revision,
/// no timestamp).
async fn rewrite_stored_values(
    db: &impl ConnectionTrait,
    renamed: &[(String, String)],
) -> Result<u64, DbErr> {
    let mut alternatives = vec![regex_escape(FEDERATION), regex_escape(BREW_CACHE)];
    alternatives.extend(
        renamed
            .iter()
            .filter(|(old, _)| old.starts_with(PHANTASI_CACHE))
            .map(|(old, _)| regex_escape(old)),
    );
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
                format!(
                    "SELECT ctid::text AS tid, {col}::text AS value FROM {tbl} WHERE {col}::text ~ $1",
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
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                format!(
                    "UPDATE {tbl} SET {col} = $1{cast} WHERE ctid = $2::tid",
                    col = quote_ident(&name),
                    tbl = quote_ident(&table),
                ),
                [updated.into(), tid.into()],
            ))
            .await?;
            changed += 1;
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

    #[test]
    fn a_cached_file_is_rewritten_under_both_spellings() {
        let phantasi = format!("{PHANTASI_CACHE}44/{HASH}.png");
        let renamed: Vec<_> = spellings(&phantasi)
            .into_iter()
            .map(|old| (old, "/media/assets/u/a.png".to_string()))
            .collect();
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
    fn regex_escape_quotes_metacharacters() {
        assert_eq!(regex_escape("/a.b(c)"), r"/a\.b\(c\)");
    }
}
