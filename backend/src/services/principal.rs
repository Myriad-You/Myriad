//! Who holds site authority: a user's current roles, the site owner, and
//! whether the installation has been claimed.
//!
//! Every answer is read from the database (roles through the auth snapshot
//! cache, the owner through a cache invalidated on the same NOTIFY channel).
//! A failed read is `Err`, never a silent "not an admin" or "not claimed".

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::Instant;

use crate::middleware::auth::AUTH_CACHE_TTL;

/// Current durable roles of one account (`users.is_admin` / `users.is_owner`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentRoles {
    pub is_admin: bool,
    pub is_owner: bool,
}

/// Roles of `user_id` right now. `Ok(None)` for guests, id 0 and deleted accounts.
///
/// Shares the request auth snapshot: bounded by [`AUTH_CACHE_TTL`] and
/// dropped early by `myriad_auth_cache_invalidate` after role writes.
pub async fn current_roles(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Option<CurrentRoles>, DbErr> {
    crate::middleware::auth::current_roles_snapshot(db, user_id).await
}

/// Whether `user_id` is an administrator right now. A missing account is not;
/// a failed read is an error.
pub async fn is_current_admin(db: &DatabaseConnection, user_id: i32) -> Result<bool, DbErr> {
    Ok(current_roles(db, user_id)
        .await?
        .is_some_and(|roles| roles.is_admin))
}

/// Durable owner first (lowest id), otherwise the lowest admin id, in one query.
/// `CASE` keeps a NULL `is_owner` out of the owner tier.
pub const SITE_OWNER_ID_SQL: &str = "SELECT id FROM users \
     WHERE is_owner = true OR is_admin = true \
     ORDER BY CASE WHEN is_owner = true THEN 0 ELSE 1 END, id ASC \
     LIMIT 1";

#[derive(Debug, Default)]
struct SiteOwnerCache {
    value: Option<(i32, Instant)>,
    /// Bumped by every invalidation; a load that started before one does
    /// not write its (possibly stale) answer back.
    generation: u64,
}

static SITE_OWNER_CACHE: OnceLock<StdMutex<SiteOwnerCache>> = OnceLock::new();

fn site_owner_cache() -> std::sync::MutexGuard<'static, SiteOwnerCache> {
    SITE_OWNER_CACHE
        .get_or_init(|| StdMutex::new(SiteOwnerCache::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Drop the cached site-owner id in this process. Called for every auth
/// invalidation (local writes and peer NOTIFY alike): any role change can move
/// the lowest-admin fallback.
pub(crate) fn invalidate_site_owner_cache() {
    let mut cache = site_owner_cache();
    cache.generation = cache.generation.wrapping_add(1);
    cache.value = None;
}

fn cached_site_owner_id() -> (Option<i32>, u64) {
    let cache = site_owner_cache();
    let fresh = cache
        .value
        .filter(|(_, cached_at)| cached_at.elapsed() < AUTH_CACHE_TTL)
        .map(|(id, _)| id);
    (fresh, cache.generation)
}

fn store_site_owner_id(id: i32, generation: u64) {
    let mut cache = site_owner_cache();
    if cache.generation == generation {
        cache.value = Some((id, Instant::now()));
    }
}

/// The site owner's user id. `Ok(None)` only before setup (no owner, no admin).
///
/// Only a found id is cached, so the first claim is visible immediately.
pub async fn site_owner_id(db: &DatabaseConnection) -> Result<Option<i32>, DbErr> {
    let (cached, generation) = cached_site_owner_id();
    if cached.is_some() {
        return Ok(cached);
    }
    crate::middleware::auth::ensure_auth_cache_listener(db);
    let id = query_site_owner_id(db).await?;
    if let Some(id) = id {
        store_site_owner_id(id, generation);
    }
    Ok(id)
}

async fn query_site_owner_id(db: &impl ConnectionTrait) -> Result<Option<i32>, DbErr> {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        SITE_OWNER_ID_SQL.to_string(),
    ))
    .await?
    .map(|row| row.try_get::<i32>("", "id"))
    .transpose()
}

/// Whether someone has claimed this installation (an admin or durable owner exists).
///
/// A database without a `users` table is unclaimed: a fact about the schema,
/// not a failure. Any query or decode failure is `Err`. Generic over the
/// connection so create-admin can ask inside its locked transaction.
pub async fn installation_claimed<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
    let schema = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            INSTALLATION_SCHEMA_SQL.to_string(),
        ))
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("installation schema probe".to_string()))?;
    if !schema.try_get::<bool>("", "has_users")? {
        return Ok(false);
    }
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        INSTALLATION_CLAIMED_SQL.to_string(),
    ))
    .await?
    .ok_or_else(|| DbErr::RecordNotFound("installation claim".to_string()))?
    .try_get::<bool>("", "claimed")
}

/// `to_regclass` resolves through `search_path`, so a session TEMP `users`
/// counts too (tests rely on it).
const INSTALLATION_SCHEMA_SQL: &str = "SELECT to_regclass('users') IS NOT NULL AS has_users";
const INSTALLATION_CLAIMED_SQL: &str = "SELECT EXISTS (SELECT 1 FROM users \
     WHERE is_admin = true OR is_owner = true) AS claimed";

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, Database};

    #[test]
    fn owner_query_prefers_owner_tier_then_lowest_id() {
        let sql = SITE_OWNER_ID_SQL;
        assert!(sql.contains("WHERE is_owner = true OR is_admin = true"));
        assert!(sql.contains("ORDER BY CASE WHEN is_owner = true THEN 0 ELSE 1 END, id ASC"));
        assert!(sql.ends_with("LIMIT 1"));
    }

    /// Owner and admin answers have one source each; new copies of the raw
    /// queries would reintroduce per-site caching and error policies.
    #[test]
    fn role_and_owner_queries_live_only_here() {
        fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, offenders);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                let production = source.split("#[cfg(test)]").next().unwrap_or("");
                for needle in [
                    "SELECT id FROM users WHERE is_owner = true ORDER BY id",
                    "ADMIN_ID_CACHE",
                    "SELECT is_admin FROM users WHERE id = $1",
                    // A live admin probe that swallows read failures as "no".
                    "ensure_current_admin_on(claims, db).await.is_ok()",
                    "ensure_current_admin_on(&claims, &db).await.is_ok()",
                    // Lowest-admin-as-owner subqueries (scheduler audience).
                    "WHERE is_admin = true\n              ORDER BY id\n              LIMIT 1",
                ] {
                    if production.contains(needle) {
                        offenders.push(format!("{}: {needle:?}", path.display()));
                    }
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        visit(&root, &mut offenders);
        // The live admin gate keeps its own uncached read on purpose.
        offenders.retain(|entry| !entry.contains("middleware/auth.rs"));
        assert!(offenders.is_empty(), "{offenders:#?}");
    }

    #[test]
    fn invalidation_fences_a_load_that_started_before_it() {
        invalidate_site_owner_cache();
        let (_, generation) = cached_site_owner_id();
        invalidate_site_owner_cache();
        store_site_owner_id(41, generation);
        assert_eq!(cached_site_owner_id().0, None);

        let (_, generation) = cached_site_owner_id();
        store_site_owner_id(42, generation);
        assert_eq!(cached_site_owner_id().0, Some(42));
        // The auth listener's handler clears the owner cache too.
        crate::middleware::auth::invalidate_auth_cache_local(987_654);
        assert_eq!(cached_site_owner_id().0, None);
    }

    async fn temp_users_db() -> Option<DatabaseConnection> {
        let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
        let mut options = ConnectOptions::new(url);
        // One connection: TEMP tables below are session-local.
        options.max_connections(1).min_connections(1);
        Some(Database::connect(options).await.unwrap())
    }

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn installation_claim_reads_schema_facts_and_fails_closed() {
        let Some(db) = temp_users_db().await else {
            return;
        };
        // Resolved against this session's TEMP schema only when it exists;
        // make sure no permanent `users` is visible either way.
        exec(&db, "SET search_path TO pg_temp").await;
        assert!(!installation_claimed(&db).await.unwrap(), "no users table");

        exec(
            &db,
            "CREATE TEMP TABLE users (id INT PRIMARY KEY, is_admin BOOLEAN, is_owner BOOLEAN)",
        )
        .await;
        assert!(!installation_claimed(&db).await.unwrap());
        exec(&db, "INSERT INTO users VALUES (1, false, false)").await;
        assert!(!installation_claimed(&db).await.unwrap());
        exec(&db, "UPDATE users SET is_admin = true").await;
        assert!(installation_claimed(&db).await.unwrap(), "admin claims");

        exec(&db, "UPDATE users SET is_admin = false, is_owner = true").await;
        assert!(installation_claimed(&db).await.unwrap(), "owner claims");
        exec(&db, "UPDATE users SET is_owner = false").await;
        assert!(!installation_claimed(&db).await.unwrap());

        // A broken read is an error, not "unclaimed".
        exec(&db, "ALTER TABLE users ALTER COLUMN is_admin TYPE TEXT").await;
        assert!(installation_claimed(&db).await.is_err());
        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn roles_and_owner_follow_invalidation_and_surface_errors() {
        let Some(db) = temp_users_db().await else {
            return;
        };
        exec(&db, "SET search_path TO pg_temp").await;
        exec(
            &db,
            "CREATE TEMP TABLE users (id INT PRIMARY KEY, is_admin BOOLEAN, \
             is_owner BOOLEAN, token_version INTEGER)",
        )
        .await;
        let (first, second) = (910_301, 910_302);
        let forget = || {
            crate::middleware::auth::invalidate_auth_cache_local(first);
            crate::middleware::auth::invalidate_auth_cache_local(second);
        };
        forget();

        assert_eq!(site_owner_id(&db).await.unwrap(), None, "before setup");
        assert_eq!(current_roles(&db, first).await.unwrap(), None);
        assert!(!is_current_admin(&db, -5).await.unwrap(), "guest");

        exec(
            &db,
            &format!(
                "INSERT INTO users VALUES ({first}, true, false, 0), ({second}, true, false, 0)"
            ),
        )
        .await;
        // The negative snapshot above predates the fixture insert.
        forget();
        // An old database without a marked owner: the lowest admin stands in.
        assert_eq!(site_owner_id(&db).await.unwrap(), Some(first));
        assert!(is_current_admin(&db, first).await.unwrap());

        // Demote the stand-in the way admin_users does: write, then invalidate.
        exec(
            &db,
            &format!("UPDATE users SET is_admin = false WHERE id = {first}"),
        )
        .await;
        crate::middleware::auth::invalidate_auth_cache_local(first);
        assert_eq!(site_owner_id(&db).await.unwrap(), Some(second));
        assert!(!is_current_admin(&db, first).await.unwrap());
        assert_eq!(
            current_roles(&db, second).await.unwrap(),
            Some(CurrentRoles {
                is_admin: true,
                is_owner: false
            })
        );

        // Broken reads are errors, never "not an admin" / "no owner".
        forget();
        exec(
            &db,
            "ALTER TABLE users RENAME COLUMN is_admin TO unavailable",
        )
        .await;
        assert!(site_owner_id(&db).await.is_err());
        assert!(current_roles(&db, second).await.is_err());
        assert!(is_current_admin(&db, second).await.is_err());
        forget();
        db.close().await.unwrap();
    }
}
