use std::future::Future;
use std::time::Duration;

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection,
    DatabaseTransaction, Statement, TransactionTrait,
};

use super::{Lock, check};
use crate::services::agent::merope::{api::store::lock_persona_on, retire_her_on};
use crate::services::her_log::{deputy, lease as grants};
use crate::services::persona_sync::{devices, import, lease, moves};
use myriad_persona_sync::{
    AssetRef, AssetType, Envelope, archive::Manifest, canonical::sha256_hex,
};

/// A database of the test's own. The locks are the whole database's, so in a
/// schema beside the other tests' these would wait on what theirs hold, and
/// the order broken on purpose would break theirs.
struct Own {
    db: DatabaseConnection,
    url: String,
    name: String,
}

impl Own {
    async fn new(migrated: bool) -> Option<Self> {
        use migration::MigratorTrait;
        let url = crate::db::test_url(&[
            "FEDERATION_TEST_DATABASE_URL",
            "MYRIAD_SCHEMA_DRIFT_DB",
            "MYRIAD_MEDIA_TEST_DATABASE_URL",
        ])?;
        let _turn = crate::db::SCHEMA_WORK
            .acquire()
            .await
            .expect("never closed");
        let name = format!("myriad_order_test_{}", uuid::Uuid::new_v4().simple());
        let admin = Database::connect(&url).await.unwrap();
        admin
            .execute_unprepared(&format!("CREATE DATABASE {name} TEMPLATE template0"))
            .await
            .unwrap();
        admin.close().await.unwrap();
        let mut own = url::Url::parse(&url).unwrap();
        own.set_path(&name);
        let mut options = ConnectOptions::new(own.to_string());
        options.max_connections(6).sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        if migrated {
            migration::Migrator::up(&db, None).await.unwrap();
        }
        Some(Self { db, url, name })
    }

    async fn close(self) {
        self.db.close().await.unwrap();
        let admin = Database::connect(&self.url).await.unwrap();
        admin
            .execute_unprepared(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .unwrap();
        admin.close().await.unwrap();
    }

    /// The owner 41, her, and a member 42's memory that is not hers to take.
    async fn with_her() -> Option<Self> {
        let own = Self::new(true).await?;
        own.db
            .execute_unprepared(
                "INSERT INTO users (id, username, is_admin, is_owner) VALUES \
                 (41, 'owner41', true, true), (42, 'member42', false, false); \
                 INSERT INTO agent_persona (id, name, updated_at) VALUES ('site', 'Merope', NOW()); \
                 INSERT INTO agent_memories (id, user_id, kind, content, speaker, source, venue, \
                 audience, concepts, valid_from, created_at, updated_at) VALUES \
                 ('m_member', 42, 'fact', '他养了一只猫', 'user', 'chat', 'private', '[42]', '[]', \
                  NOW(), NOW(), NOW())",
            )
            .await
            .unwrap();
        Some(own)
    }

    /// An import that is over and still kept, held by what is returned.
    async fn an_import_held(&self, id: &str) -> DatabaseTransaction {
        self.db
            .execute_raw(sql(
                "INSERT INTO agent_host_import (id, source, phase, created_at, updated_at) \
                 VALUES ($1, 'file', 'committed', NOW(), NOW())",
                [id.into()],
            ))
            .await
            .unwrap();
        let holding = self.db.begin().await.unwrap();
        holding
            .execute_raw(sql(
                "SELECT id FROM agent_host_import WHERE id = $1 FOR UPDATE",
                [id.into()],
            ))
            .await
            .unwrap();
        holding
    }

    async fn count(&self, query: &str) -> i64 {
        self.db
            .query_one_raw(Statement::from_string(DatabaseBackend::Postgres, query))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "n")
            .unwrap()
    }

    /// Until something here waits on a lock.
    async fn something_waits(&self) {
        self.waiting(1).await;
    }

    /// Until `many` here wait on a lock.
    async fn waiting(&self, many: i64) {
        for _ in 0..400 {
            let waiting = self
                .count(
                    "SELECT COUNT(*)::BIGINT AS n FROM pg_stat_activity \
                     WHERE datname = current_database() AND wait_event_type = 'Lock'",
                )
                .await;
            if waiting >= many {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("nothing waited on the lock held");
    }

    /// `path` waits for what `holding` holds without her lease in its hands,
    /// and goes through once it is let go.
    async fn waits_without_her_lease<T: Send + 'static>(
        &self,
        holding: DatabaseTransaction,
        path: impl Future<Output = T> + Send + 'static,
    ) -> T {
        let path = tokio::spawn(path);
        self.something_waits().await;
        let probe = self.db.begin().await.unwrap();
        probe
            .execute_unprepared("SET LOCAL lock_timeout = '2s'")
            .await
            .unwrap();
        let free = grants::lock(&probe).await;
        probe.rollback().await.unwrap();
        holding.commit().await.unwrap();
        let done = tokio::time::timeout(Duration::from_secs(60), path)
            .await
            .expect("it goes through")
            .unwrap();
        assert!(
            free.is_ok(),
            "it waits for an earlier lock while it holds her lease"
        );
        done
    }
}

fn sql<const N: usize>(text: &str, values: [sea_orm::Value; N]) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, text, values)
}

fn nothing() -> Manifest {
    Manifest::of_envelopes(&[]).unwrap()
}

/// Her lease and queued memory work, each held by one and asked for by the
/// other: the old way round Postgres fails one of them; in the order both go
/// through.
#[tokio::test]
async fn two_that_take_them_the_wrong_way_round_wait_on_each_other_for_good() {
    let Some(own) = Own::new(false).await else {
        return;
    };
    const MEMORY_WORK: &str = "SELECT pg_advisory_xact_lock(1296388170, 1)";
    let moving = own.db.begin().await.unwrap();
    let landing = own.db.begin().await.unwrap();
    grants::lock(&moving).await.unwrap();
    lock_persona_on(&landing).await.unwrap();
    // Not through `lock_persona_on`: that panics here.
    let wrong = tokio::spawn(async move { moving.execute_unprepared(MEMORY_WORK).await.map(drop) });
    let right = tokio::spawn(async move { grants::lock(&landing).await });
    let (wrong, right) = (wrong.await.unwrap(), right.await.unwrap());
    let failed = [
        wrong.err().map(|error| error.to_string()),
        right.err().map(|error| format!("{error:?}")),
    ];
    let failed: Vec<&String> = failed.iter().flatten().collect();
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(failed[0].contains("deadlock detected"), "{failed:?}");

    let mut both = Vec::new();
    for _ in 0..2 {
        let db = own.db.clone();
        both.push(tokio::spawn(async move {
            let txn = db.begin().await.unwrap();
            lock_persona_on(&txn).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            grants::lock(&txn).await.unwrap();
            txn.commit().await.unwrap();
        }));
    }
    for one in both {
        one.await.unwrap();
    }
    own.close().await;
}

/// Asked for too late, a lock panics in a test build; what another
/// connection holds, and one held already, do not count.
#[tokio::test]
async fn a_lock_asked_for_too_late_is_caught() {
    let Some(own) = Own::new(false).await else {
        return;
    };
    let caught = |db: DatabaseConnection, asked: Lock, memory: bool| async move {
        let held = db.begin().await.unwrap();
        if memory {
            lock_persona_on(&held).await.unwrap();
        } else {
            grants::lock(&held).await.unwrap();
        }
        tokio::spawn(async move { check(&held, asked).await })
            .await
            .is_err_and(|error| error.is_panic())
    };
    assert!(caught(own.db.clone(), Lock::MemoryWork, false).await);
    assert!(caught(own.db.clone(), Lock::Imports, false).await);
    assert!(caught(own.db.clone(), Lock::Imports, true).await);

    let theirs = own.db.begin().await.unwrap();
    grants::lock(&theirs).await.unwrap();
    let ours = own.db.begin().await.unwrap();
    import::one_at_a_time(&ours).await.unwrap();
    lock_persona_on(&ours).await.unwrap();
    theirs.rollback().await.unwrap();
    grants::lock(&ours).await.unwrap();
    // Held already.
    lock_persona_on(&ours).await.unwrap();
    import::one_at_a_time(&ours).await.unwrap();
    ours.rollback().await.unwrap();
    // Gone with its transaction: nothing of it is left on the connection.
    check(&own.db, Lock::Imports).await.unwrap();
    own.close().await;
}

/// Each is named in one place in the source, and taken for a transaction
/// there: what [`check`] cannot tell from `pg_locks`.
#[test]
fn each_is_taken_in_one_place_and_for_a_transaction() {
    fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    files.retain(|path| {
        !path.ends_with("persona_sync/lease/order.rs")
            && !path.ends_with("persona_sync/lease/order_tests.rs")
    });
    for (name, place) in [
        ("'myriad-her-lease'", "services/her_log/lease.rs"),
        ("1296388170", "services/agent/merope/store/memory_jobs.rs"),
        (
            "hashtext('agent_host_import')",
            "services/persona_sync/import.rs",
        ),
    ] {
        let mut found = Vec::new();
        for path in &files {
            for line in std::fs::read_to_string(path).unwrap().lines() {
                if line.contains(name) {
                    let at = path.strip_prefix(&root).unwrap().to_string_lossy();
                    found.push((at.replace('\\', "/"), line.trim().to_string()));
                }
            }
        }
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert_eq!(found[0].0, place);
        assert!(found[0].1.contains("pg_advisory_xact_lock("), "{found:?}");
    }
}

/// 「已经搬好，删掉这里的她」: as `devices::let_her_go` does it.
#[tokio::test]
async fn letting_her_go_waits_for_memory_work_before_it_takes_her_lease() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    lease::read(&own.db).await.unwrap().unwrap();
    lease::move_out(&own.db).await.unwrap();
    let holding = own.db.begin().await.unwrap();
    lock_persona_on(&holding).await.unwrap();
    let db = own.db.clone();
    let gone = own
        .waits_without_her_lease(holding, async move {
            let txn = db.begin().await.unwrap();
            let gone = lease::let_go_on(&txn).await.unwrap();
            retire_her_on(&txn, 41).await.unwrap();
            txn.commit().await.unwrap();
            gone
        })
        .await;
    assert_eq!(gone.place(), lease::Where::Vacant);
    assert_eq!(
        own.count("SELECT COUNT(*)::BIGINT AS n FROM agent_persona")
            .await,
        0
    );
    assert_eq!(
        own.count("SELECT COUNT(*)::BIGINT AS n FROM agent_memories WHERE id = 'm_member'")
            .await,
        1
    );
    own.close().await;
}

/// A device, a move to it that only its commit is left of, and that move's
/// digest.
async fn an_applied_move(own: &Own) -> (String, String, String) {
    let (mac, _) = devices::add(&own.db, 41, "Mac").await.unwrap();
    let born = lease::read(&own.db).await.unwrap().unwrap();
    let started = lease::start(&own.db, lease::Kind::Move, &mac.id, lease::Starter::Owner)
        .await
        .unwrap();
    let id = started.moving.unwrap().id;
    let digest = "a".repeat(64);
    lease::agree(&own.db, &id, &mac.id).await.unwrap();
    lease::confirm(&own.db, &id).await.unwrap();
    lease::sealed(&own.db, &id, &digest).await.unwrap();
    for phase in [lease::Phase::Transferring, lease::Phase::Applied] {
        lease::device_step(&own.db, &id, &mac.id, phase)
            .await
            .unwrap();
    }
    own.db
        .execute_raw(sql(
            "INSERT INTO agent_host_backups (id, device_id, day, lineage, epoch, digest, bytes, \
             received, complete, created_at, updated_at) \
             VALUES ('bk_1', $1, '2026-11-23', $2, 1, $3, 1, 1, true, NOW(), NOW())",
            [
                mac.id.clone().into(),
                born.lineage.into(),
                digest.clone().into(),
            ],
        ))
        .await
        .unwrap();
    (mac.id, id, digest)
}

#[tokio::test]
async fn the_commit_of_a_move_waits_for_memory_work_before_it_takes_her_lease() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, id, digest) = an_applied_move(&own).await;
    let holding = own.db.begin().await.unwrap();
    lock_persona_on(&holding).await.unwrap();
    let (db, device) = (own.db.clone(), mac.clone());
    let moved = own
        .waits_without_her_lease(holding, async move {
            moves::commit_to_device(&db, 41, &id, &device, &digest)
                .await
                .unwrap()
        })
        .await;
    assert_eq!(moved.place(), lease::Where::Elsewhere(mac));
    assert_eq!(
        own.count("SELECT COUNT(*)::BIGINT AS n FROM agent_persona")
            .await,
        0
    );
    own.close().await;
}

/// Its device was removed, what that leaves of her not settled yet: the move
/// to it is not committed.
#[tokio::test]
async fn a_move_to_a_device_just_removed_is_not_committed() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, id, digest) = an_applied_move(&own).await;
    let holding = own.imports_held().await;
    assert!(own.remove(&mac).await.unwrap().is_some());
    let refused = moves::commit_to_device(&own.db, 41, &id, &mac, &digest).await;
    assert!(
        matches!(refused, Err(lease::MoveError::Unknown)),
        "{refused:?}"
    );
    assert_eq!(
        own.count("SELECT COUNT(*)::BIGINT AS n FROM agent_persona")
            .await,
        1
    );
    holding.commit().await.unwrap();
    lease::removed::settle_all(&own.db).await.unwrap();
    own.is_settled("removed").await;
    own.close().await;
}

/// Its commit went through, the answer was lost, and then it was removed:
/// asked again, it is told she is on it, whether what it left is settled or
/// not.
#[tokio::test]
async fn a_move_committed_before_its_device_was_removed_is_still_told_so() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, id, digest) = an_applied_move(&own).await;
    moves::commit_to_device(&own.db, 41, &id, &mac, &digest)
        .await
        .unwrap();
    let holding = own.imports_held().await;
    assert!(own.remove(&mac).await.unwrap().is_some());
    let told = moves::commit_to_device(&own.db, 41, &id, &mac, &digest)
        .await
        .unwrap();
    assert_eq!(told.place(), lease::Where::Elsewhere(mac.clone()));
    holding.commit().await.unwrap();
    lease::removed::settle_all(&own.db).await.unwrap();
    let told = moves::commit_to_device(&own.db, 41, &id, &mac, &digest)
        .await
        .unwrap();
    assert_eq!(told.place(), lease::Where::Vacant);
    own.close().await;
}

/// She lives on `a`; what is asked of her from another device is `imp_1`.
async fn reclaimed_from_a(own: &Own) -> lease::Lease {
    lease::seed(&own.db, "dev_a", "l-1", 3, lease::State::Away)
        .await
        .unwrap();
    let txn = own.db.begin().await.unwrap();
    let asked = lease::reclaiming(&txn, "imp_1", "dev_b").await.unwrap();
    txn.commit().await.unwrap();
    asked
}

#[tokio::test]
async fn calling_a_move_off_waits_for_its_import_before_it_takes_her_lease() {
    let Some(own) = Own::new(true).await else {
        return;
    };
    let id = reclaimed_from_a(&own).await.moving.unwrap().id;
    let holding = own.an_import_held("imp_1").await;
    let db = own.db.clone();
    let back = own
        .waits_without_her_lease(holding, async move {
            lease::cancel(&db, &id, None).await.unwrap().unwrap()
        })
        .await;
    assert_eq!(
        (back.place(), back.moving),
        (lease::Where::Elsewhere("dev_a".into()), None)
    );
    own.close().await;
}

#[tokio::test]
async fn asking_her_back_waits_for_imports_before_it_takes_her_lease() {
    let Some(own) = Own::new(true).await else {
        return;
    };
    lease::seed(&own.db, "dev_a", "l-1", 3, lease::State::Away)
        .await
        .unwrap();
    let holding = own.an_import_held("imp_old").await;
    let db = own.db.clone();
    let asked = own
        .waits_without_her_lease(holding, async move {
            let proposal = import::Proposal {
                lineage: "l-1".into(),
                epoch: 2,
                name: None,
                manifest: nothing(),
                kind: Some("recover".into()),
                subject: None,
                preview: Vec::new(),
            };
            import::propose(&db, 41, "dev_b", proposal).await.unwrap()
        })
        .await;
    let held = lease::read(&own.db).await.unwrap().unwrap();
    assert_eq!(held.moving.unwrap().import, Some(asked.id));
    own.close().await;
}

#[tokio::test]
async fn a_device_that_froze_her_waits_for_imports_before_it_takes_her_lease() {
    let Some(own) = Own::new(true).await else {
        return;
    };
    lease::seed(&own.db, "dev_a", "l-1", 3, lease::State::Away)
        .await
        .unwrap();
    let back = lease::start_back(&own.db, "dev_a", lease::Starter::Owner)
        .await
        .unwrap();
    let id = back.moving.unwrap().id;
    let holding = own.an_import_held("imp_old").await;
    let db = own.db.clone();
    let carrying = own
        .waits_without_her_lease(holding, async move {
            let frozen = moves::Frozen {
                lineage: "l-1".into(),
                epoch: 3,
                name: None,
                manifest: nothing(),
            };
            moves::report_frozen(&db, &id, "dev_a", frozen)
                .await
                .unwrap()
        })
        .await;
    let held = lease::read(&own.db).await.unwrap().unwrap();
    assert_eq!(held.moving.unwrap().import, Some(carrying.id));
    own.close().await;
}

/// An import the owner turns down after confirming it: no her, no lease, and
/// another can come.
#[tokio::test]
async fn an_import_called_off_leaves_the_site_as_it_was() {
    let Some(own) = Own::new(true).await else {
        return;
    };
    own.db
        .execute_unprepared(
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES (41, 'owner41', true, true)",
        )
        .await
        .unwrap();
    let (mac, _) = devices::add(&own.db, 41, "Mac").await.unwrap();
    for _ in 0..2 {
        let proposal = import::Proposal {
            lineage: "l-1".into(),
            epoch: 3,
            name: None,
            manifest: nothing(),
            kind: None,
            subject: None,
            preview: Vec::new(),
        };
        let asked = import::propose(&own.db, 41, &mac.id, proposal)
            .await
            .unwrap();
        let coming = import::confirm(&own.db, 41, &asked.id, None, false)
            .await
            .unwrap();
        assert!(coming.frozen);
        assert!(matches!(
            import::cancel(&own.db, &asked.id, Some("dev_other")).await,
            Err(import::ImportError::Unknown)
        ));
        import::cancel(&own.db, &asked.id, None).await.unwrap();
        assert!(lease::read(&own.db).await.unwrap().is_none());
        assert!(import::current(&own.db).await.unwrap().is_none());
    }
    own.close().await;
}

/// When the deputy of [`Own::a_device_moving`] was issued.
const ISSUED: &str = "001764000000000-000000-site";

/// Removes the device, waiting a second for what another holds, as the site
/// does: what is held in a database of the test's own is held on purpose.
async fn remove(
    db: &DatabaseConnection,
    id: &str,
) -> Result<Option<devices::Removed>, sea_orm::DbErr> {
    devices::remove(
        db,
        41,
        id,
        devices::RevokeReason::ByOwner,
        Duration::from_secs(1),
    )
    .await
}

impl Own {
    /// The imports, held as by one that is landing.
    async fn imports_held(&self) -> DatabaseTransaction {
        let holding = self.db.begin().await.unwrap();
        import::hold_all(&holding).await.unwrap();
        holding
    }

    /// A device, its token, and a merge from it under way with its import.
    async fn a_device_moving(&self, name: &str) -> (String, String) {
        let (mac, token) = devices::add(&self.db, 41, name).await.unwrap();
        let moving = lease::start(&self.db, lease::Kind::Merge, &mac.id, lease::Starter::Owner)
            .await
            .unwrap();
        assert!(moving.frozen);
        self.db
            .execute_raw(sql(
                "INSERT INTO agent_host_import (id, source, device_id, phase, created_at, updated_at) \
                 VALUES ($1, 'device', $2, 'transferring', NOW(), NOW())",
                [format!("imp_{name}").into(), mac.id.clone().into()],
            ))
            .await
            .unwrap();
        self.a_deputy_of(&mac.id).await;
        assert_eq!(
            deputy::for_holders(&self.db, &[mac.id.as_str()])
                .await
                .unwrap()
                .len(),
            1
        );
        (mac.id, token)
    }

    async fn a_deputy_of(&self, device: &str) {
        self.db
            .execute_raw(sql(
                "INSERT INTO agent_lease_deputies (holder, issued, under, until_micros, ord, issued_at) \
                 VALUES ($1, $2, 1, 1, 1, NOW())",
                [device.into(), ISSUED.into()],
            ))
            .await
            .unwrap();
    }

    async fn remove(&self, id: &str) -> Result<Option<devices::Device>, sea_orm::DbErr> {
        match remove(&self.db, id).await? {
            Some(devices::Removed {
                again: true,
                settled: Err(error),
                ..
            }) => Err(error),
            removed => Ok(removed.map(|removed| removed.device)),
        }
    }

    /// The device is one no more, and what it left of her stands as it did.
    async fn is_removed_and_not_settled(&self, token: &str, about: &str) {
        assert!(
            matches!(
                devices::authenticate(&self.db, token, Some(41)).await,
                Err(devices::Refused::Revoked)
            ),
            "{about}"
        );
        let here = lease::read(&self.db).await.unwrap().unwrap();
        assert!(here.frozen && here.moving.is_some(), "{about}: {here:?}");
        assert_eq!(self.left_of_her().await, (1, 0), "{about}");
    }

    async fn is_settled(&self, about: &str) {
        let here = lease::read(&self.db).await.unwrap().unwrap();
        assert_eq!(
            (here.place(), here.frozen, &here.moving),
            (lease::Where::Here, false, &None),
            "{about}"
        );
        assert_eq!(self.left_of_her().await, (0, 0), "{about}");
    }

    /// Imports and deputies kept.
    async fn left_of_her(&self) -> (i64, i64) {
        (
            self.count("SELECT COUNT(*)::BIGINT AS n FROM agent_host_import")
                .await,
            self.count("SELECT COUNT(*)::BIGINT AS n FROM agent_lease_deputies")
                .await,
        )
    }

    /// Every step of settling a removed device, failing in turn.
    async fn failing_at_each_step(&self) -> Vec<(&'static str, &'static str)> {
        self.db
            .execute_unprepared(
                "CREATE FUNCTION fails() RETURNS trigger LANGUAGE plpgsql AS \
                 $$ BEGIN RAISE EXCEPTION 'failed here'; END $$",
            )
            .await
            .unwrap();
        vec![
            ("agent_host_import", "DELETE"),
            ("agent_her_moves", "DELETE"),
            ("agent_her_copy", "UPDATE"),
            ("agent_lease_grants", "INSERT"),
        ]
    }

    async fn failing(&self, (table, what): (&str, &str)) {
        self.db
            .execute_unprepared(&format!(
                "CREATE TRIGGER fails BEFORE {what} ON {table} \
                 FOR EACH STATEMENT EXECUTE FUNCTION fails()"
            ))
            .await
            .unwrap();
    }

    async fn failing_no_more(&self, (table, _): (&str, &str)) {
        self.db
            .execute_unprepared(&format!("DROP TRIGGER fails ON {table}"))
            .await
            .unwrap();
    }
}

/// Removing a device takes its token and its deputies back and closes its
/// link at once, with an import landing meanwhile that holds her lease too:
/// only what it leaves of her waits.
#[tokio::test]
async fn a_device_is_removed_at_once_while_an_import_lands() {
    use crate::services::agent::link::{Close, opened};
    let Some(own) = Own::with_her().await else {
        return;
    };
    lease::read(&own.db).await.unwrap().unwrap();
    let (mac, token) = own.a_device_moving("mac").await;
    let link = opened(&mac, 41);
    let holding = own.imports_held().await;
    grants::lock(&holding).await.unwrap();
    let removing = tokio::spawn({
        let (db, id) = (own.db.clone(), mac.clone());
        async move { remove(&db, &id).await }
    });
    let closed = tokio::time::timeout(Duration::from_secs(30), link)
        .await
        .expect("its link closes while the import lands")
        .unwrap();
    assert_eq!(closed, Close::Revoked);
    own.is_removed_and_not_settled(&token, "the import lands")
        .await;
    // And it does not wait for the import for long.
    let removed = tokio::time::timeout(Duration::from_secs(30), removing)
        .await
        .expect("removed while the import lands")
        .unwrap()
        .unwrap();
    let removed = removed.unwrap();
    assert_eq!((removed.device.id, removed.again), (mac.clone(), false));
    assert!(removed.settled.is_err());
    own.is_removed_and_not_settled(&token, "the import still lands")
        .await;
    // Its deputy is no one's: nothing is placed against it, it cannot be
    // taken, and one given just as it was removed counts for nothing.
    assert!(
        deputy::for_holders(&own.db, &[mac.as_str()])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !deputy::received(&own.db, &mac, &ISSUED.parse().unwrap())
            .await
            .unwrap()
    );
    own.a_deputy_of(&mac).await;
    assert!(
        deputy::for_holders(&own.db, &[mac.as_str()])
            .await
            .unwrap()
            .is_empty()
    );
    holding.commit().await.unwrap();
    lease::removed::settle_all(&own.db).await.unwrap();
    own.is_settled("the import landed").await;
    own.close().await;
}

#[tokio::test]
async fn a_removed_device_is_settled_waiting_for_imports_before_it_takes_her_lease() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, _) = devices::add(&own.db, 41, "Mac").await.unwrap();
    lease::read(&own.db).await.unwrap().unwrap();
    own.remove(&mac.id).await.unwrap().unwrap();
    let holding = own.an_import_held("imp_old").await;
    let db = own.db.clone();
    own.waits_without_her_lease(holding, async move {
        lease::removed::settle_within(&db, &mac.id, Duration::from_secs(60))
            .await
            .unwrap()
    })
    .await;
    own.close().await;
}

/// Whichever step fails, the device is one no more and all of what it left
/// stands; removing it again tells while it still fails, and goes through
/// once it does not.
#[tokio::test]
async fn what_a_removed_device_leaves_is_settled_by_removing_it_again() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    lease::read(&own.db).await.unwrap().unwrap();
    for step in own.failing_at_each_step().await {
        let (table, _) = step;
        let (mac, token) = own.a_device_moving(table).await;
        own.failing(step).await;
        assert!(own.remove(&mac).await.unwrap().is_some(), "{table}");
        own.is_removed_and_not_settled(&token, table).await;
        assert!(own.remove(&mac).await.is_err(), "{table}: it fails again");
        own.is_removed_and_not_settled(&token, table).await;
        own.failing_no_more(step).await;
        let again = own.remove(&mac).await.unwrap().expect("not unknown");
        assert_eq!(again.revoked_reason.as_deref(), Some("owner_revoked"));
        own.is_settled(table).await;
        // Once more, it changes nothing.
        let settled = lease::read(&own.db).await.unwrap().unwrap();
        own.remove(&mac).await.unwrap().unwrap();
        assert_eq!(lease::read(&own.db).await.unwrap().unwrap(), settled);
        assert!(devices::forget(&own.db, 41, &mac).await.unwrap());
    }
    own.close().await;
}

/// Its token and its deputies are taken back together, or it is still a
/// device.
#[tokio::test]
async fn a_device_whose_deputies_cannot_be_withdrawn_is_not_removed() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    lease::read(&own.db).await.unwrap().unwrap();
    let (mac, token) = own.a_device_moving("mac").await;
    own.failing_at_each_step().await;
    let step = ("agent_lease_deputies", "DELETE");
    own.failing(step).await;
    assert!(own.remove(&mac).await.is_err());
    assert!(
        devices::authenticate(&own.db, &token, Some(41))
            .await
            .is_ok()
    );
    assert_eq!(own.left_of_her().await, (1, 1));
    own.failing_no_more(step).await;
    own.remove(&mac).await.unwrap().unwrap();
    own.is_settled("removed").await;
    own.close().await;
}

/// Removed again, it has no link left, one that slipped in either.
#[tokio::test]
async fn a_device_removed_again_has_its_link_closed_again() {
    use crate::services::agent::link::{Close, opened};
    let Some(own) = Own::with_her().await else {
        return;
    };
    lease::read(&own.db).await.unwrap().unwrap();
    let (mac, _) = own.a_device_moving("mac").await;
    own.remove(&mac).await.unwrap().unwrap();
    let step = own.failing_at_each_step().await[0];
    own.failing(step).await;
    for (reason, close) in [
        ("owner_revoked", Close::Revoked),
        ("device_left", Close::Revoked),
        ("owner_changed", Close::OwnerChanged),
    ] {
        own.db
            .execute_raw(sql(
                "UPDATE agent_sync_devices SET revoked_reason = $2 WHERE id = $1",
                [mac.clone().into(), reason.into()],
            ))
            .await
            .unwrap();
        let link = opened(&mac, 41);
        // Closed first: settling what it left fails here.
        assert!(own.remove(&mac).await.is_err());
        let closed = tokio::time::timeout(Duration::from_secs(10), link)
            .await
            .expect("its link closes")
            .unwrap();
        assert_eq!(closed, close, "{reason}");
    }
    own.close().await;
}

/// Two requests of a device of a former owner at once: one removes it, the
/// other finds it removed; both are refused for what it is, settled or not.
#[tokio::test]
async fn a_former_owners_device_is_refused_as_one_though_it_is_not_settled() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, token) = devices::add(&own.db, 42, "Old Mac").await.unwrap();
    let step = own.failing_at_each_step().await[0];
    own.failing(step).await;
    let holding = own.db.begin().await.unwrap();
    holding
        .execute_raw(sql(
            "SELECT id FROM agent_sync_devices WHERE id = $1 FOR UPDATE",
            [mac.id.clone().into()],
        ))
        .await
        .unwrap();
    let mut asking = Vec::new();
    for _ in 0..2 {
        let (db, token, id) = (own.db.clone(), token.clone(), mac.id.clone());
        asking.push(tokio::spawn(async move {
            devices::authenticate(&db, &token, Some(41))
                .await
                .map_err(|refused| format!("{refused:?}"))
                .err()
                .map(|refused| (refused, id))
        }));
    }
    let (db, id) = (own.db.clone(), mac.id.clone());
    asking.push(tokio::spawn(async move {
        devices::recheck(&db, &id, Some(41))
            .await
            .map_err(|refused| format!("{refused:?}"))
            .err()
            .map(|refused| (refused, id))
    }));
    own.waiting(3).await;
    holding.commit().await.unwrap();
    for asked in asking {
        let refused = asked.await.unwrap();
        assert_eq!(refused, Some(("OwnerChanged".to_string(), mac.id.clone())));
    }
    own.close().await;
}

/// She lives on a device and is on her way back from its backup, sent by
/// another: the device removed, that import goes with the move it carried,
/// and what both kept on disk.
#[tokio::test]
async fn an_import_another_device_sends_goes_with_the_move_it_carries() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, _) = devices::add(&own.db, 41, "Mac").await.unwrap();
    let (other, _) = devices::add(&own.db, 41, "Other").await.unwrap();
    lease::seed(&own.db, &mac.id, "l-1", 9, lease::State::Away)
        .await
        .unwrap();
    let import = format!("imp_{}", uuid::Uuid::new_v4().simple());
    own.db
        .execute_raw(sql(
            "INSERT INTO agent_host_import (id, source, device_id, phase, created_at, updated_at) \
             VALUES ($1, 'device', $2, 'transferring', NOW(), NOW())",
            [import.clone().into(), other.id.clone().into()],
        ))
        .await
        .unwrap();
    let txn = own.db.begin().await.unwrap();
    let asked = lease::reclaiming(&txn, &import, &other.id).await.unwrap();
    txn.commit().await.unwrap();
    let files = Files::of(&[&import, &asked.moving.unwrap().id]);
    own.remove(&mac.id).await.unwrap().unwrap();
    let here = lease::read(&own.db).await.unwrap().unwrap();
    assert_eq!((here.place(), here.moving), (lease::Where::Vacant, None));
    assert_eq!(own.left_of_her().await, (0, 0));
    files.are_gone();
    own.close().await;
}

/// The sealed copy of her, its key and what an import sent so far do not
/// stay on disk with nothing that names them; kept while it is not settled.
#[tokio::test]
async fn what_a_move_and_an_import_kept_goes_once_their_device_is_settled() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, id, _) = an_applied_move(&own).await;
    let import = format!("imp_{}", uuid::Uuid::new_v4().simple());
    own.db
        .execute_raw(sql(
            "INSERT INTO agent_host_import (id, source, device_id, phase, created_at, updated_at) \
             VALUES ($1, 'device', $2, 'proposed', NOW(), NOW())",
            [import.clone().into(), mac.clone().into()],
        ))
        .await
        .unwrap();
    let files = Files::of(&[&import, &id]);
    let holding = own.imports_held().await;
    own.remove(&mac).await.unwrap().unwrap();
    files.are_kept();
    holding.commit().await.unwrap();
    lease::removed::settle_all(&own.db).await.unwrap();
    own.is_settled("settled").await;
    files.are_gone();
    own.close().await;
}

/// The device she lives on is removed while she is on her way back from it:
/// what had come goes, and it is said which import that was and whether all
/// of her had come.
#[tokio::test]
async fn her_way_back_from_a_removed_device_is_said_to_be_let_go() {
    let picture = b"a picture of her".to_vec();
    let asset = AssetRef {
        id: sha256_hex(&picture),
        kind: AssetType::Image,
        bytes: picture.len() as u64,
        media_type: "image/png".into(),
    };
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../core/crates/myriad-persona-sync/fixtures/merge/other.json");
    let page = myriad_persona_sync::canonicalize_envelope(Envelope {
        format: myriad_persona_sync::FORMAT.into(),
        format_version: myriad_persona_sync::FORMAT_VERSION,
        lineage: Some("l-1".into()),
        origin: "dev_mac1".into(),
        embedding_model: None,
        entities: serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap(),
        embeddings: Vec::new(),
        assets: vec![asset.clone()],
    })
    .unwrap();
    let manifest = Manifest::of(&page.entities, &[], &page.assets).unwrap();
    for (come, whole) in [("nothing", false), ("pages", false), ("all", true)] {
        let Some(own) = Own::with_her().await else {
            return;
        };
        let (mac, _) = devices::add(&own.db, 41, "Mac").await.unwrap();
        lease::seed(&own.db, &mac.id, "l-1", 3, lease::State::Away)
            .await
            .unwrap();
        let back = lease::start_back(&own.db, &mac.id, lease::Starter::Owner)
            .await
            .unwrap()
            .moving
            .unwrap();
        let frozen = moves::Frozen {
            lineage: "l-1".into(),
            epoch: 3,
            name: None,
            manifest: manifest.clone(),
        };
        let import = moves::report_frozen(&own.db, &back.id, &mac.id, frozen)
            .await
            .unwrap()
            .id;
        if come != "nothing" {
            let staged = import::take_page(&own.db, &import, &mac.id, page.clone())
                .await
                .unwrap();
            assert_eq!(staged, page.entities.len() as u64);
        }
        if come == "all" {
            let picture = picture.clone();
            import::take_asset(&own.db, &import, &mac.id, &asset.id, None, picture)
                .await
                .unwrap();
        }
        let files = Files::of(&[&import, &back.id]);
        let logged = Logged::start();
        own.remove(&mac.id).await.unwrap().unwrap();
        let said = logged.warned("on her way back");
        drop(logged);
        assert_eq!(said.len(), 1, "{come}: {said:?}");
        assert!(said[0].contains(&format!("import={import}")), "{said:?}");
        assert!(
            said[0].contains(&format!("whole={whole}")),
            "{come}: {said:?}"
        );
        let here = lease::read(&own.db).await.unwrap().unwrap();
        assert_eq!((here.place(), here.moving), (lease::Where::Vacant, None));
        assert_eq!(own.left_of_her().await, (0, 0));
        files.are_gone();
        own.close().await;
    }
}

/// What this thread logs while it lives (a test's runtime is its own thread).
struct Logged {
    buffer: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    _guard: tracing::subscriber::DefaultGuard,
}

impl Logged {
    fn start() -> Self {
        #[derive(Clone)]
        struct Buffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buffer = Buffer(std::sync::Arc::default());
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .finish();
        Self {
            buffer: buffer.0,
            _guard: tracing::subscriber::set_default(subscriber),
        }
    }

    fn warned(&self, message: &str) -> Vec<String> {
        String::from_utf8(self.buffer.lock().unwrap().clone())
            .unwrap()
            .lines()
            .filter(|line| line.contains("WARN") && line.contains(message))
            .map(str::to_string)
            .collect()
    }
}

/// As an import, a merge or a move keeps them on disk.
struct Files(Vec<std::path::PathBuf>);

impl Files {
    fn of(ids: &[&str]) -> Self {
        let agent = &crate::services::data_paths::paths().agent;
        let mut files = Vec::new();
        for id in ids {
            files.extend([
                agent.join("import").join(id).join("her.her-archive"),
                agent
                    .join("retired")
                    .join(format!("her-before-merge-{id}.her-archive")),
                agent.join("moves").join(id).join("her.her-archive"),
                agent.join("moves").join(id).join("key"),
                agent.join("retired").join(moves::retired_name(id)),
            ]);
        }
        // Only what each kind keeps: an import's, or a move's.
        files.retain(|file| {
            let import = file.to_string_lossy().contains("imp_");
            let path = file.to_string_lossy();
            import == (path.contains("/import/") || path.contains("her-before-merge-"))
        });
        for file in &files {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b"kept").unwrap();
        }
        assert_eq!(files.len(), 5);
        Self(files)
    }

    fn are_kept(&self) {
        for file in &self.0 {
            assert!(file.exists(), "{}", file.display());
        }
    }

    fn are_gone(&self) {
        for file in &self.0 {
            assert!(!file.exists(), "{}", file.display());
        }
    }
}

/// As the listener does when it starts and from time to time.
#[tokio::test]
async fn what_a_removed_device_leaves_is_settled_without_being_asked() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    lease::read(&own.db).await.unwrap().unwrap();
    for step in own.failing_at_each_step().await {
        let (table, _) = step;
        let (mac, token) = own.a_device_moving(table).await;
        own.failing(step).await;
        own.remove(&mac).await.unwrap().unwrap();
        lease::removed::settle_all(&own.db).await.unwrap();
        own.is_removed_and_not_settled(&token, table).await;
        own.failing_no_more(step).await;
        lease::removed::settle_all(&own.db).await.unwrap();
        own.is_settled(table).await;
    }
    own.close().await;
}

/// Its row is how it is known to have been removed: it stays until what it
/// left is settled.
#[tokio::test]
async fn a_removed_device_is_forgotten_only_once_it_is_settled() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    lease::read(&own.db).await.unwrap().unwrap();
    let (mac, token) = own.a_device_moving("mac").await;
    let step = own.failing_at_each_step().await[0];
    own.failing(step).await;
    own.remove(&mac).await.unwrap().unwrap();
    assert!(devices::forget(&own.db, 41, &mac).await.is_err());
    own.is_removed_and_not_settled(&token, "not forgotten")
        .await;
    own.failing_no_more(step).await;
    assert!(devices::forget(&own.db, 41, &mac).await.unwrap());
    own.is_settled("forgotten").await;
    assert!(devices::list(&own.db, 41).await.unwrap().is_empty());
    own.close().await;
}

/// She lives on a device that is removed, and is taken back from its backup
/// before what it left is settled: that reclaim goes on, and the lease is
/// settled only once it is called off.
#[tokio::test]
async fn a_reclaim_begun_after_the_device_was_removed_goes_on() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, token) = devices::add(&own.db, 41, "Mac").await.unwrap();
    let (other, _) = devices::add(&own.db, 41, "Other").await.unwrap();
    lease::seed(&own.db, &mac.id, "l-1", 9, lease::State::Away)
        .await
        .unwrap();
    let holding = own.imports_held().await;
    own.remove(&mac.id).await.unwrap().unwrap();
    assert!(matches!(
        devices::authenticate(&own.db, &token, Some(41)).await,
        Err(devices::Refused::Revoked)
    ));
    holding.commit().await.unwrap();
    let txn = own.db.begin().await.unwrap();
    let reclaim = lease::reclaiming(&txn, "imp_back", &other.id)
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(reclaim.moving.as_ref().unwrap().from, mac.id);
    lease::removed::settle_all(&own.db).await.unwrap();
    own.remove(&mac.id).await.unwrap().unwrap();
    assert_eq!(lease::read(&own.db).await.unwrap().unwrap(), reclaim);
    let id = reclaim.moving.unwrap().id;
    lease::cancel(&own.db, &id, None).await.unwrap();
    assert!(
        lease::read(&own.db)
            .await
            .unwrap()
            .unwrap()
            .lives_on(&mac.id)
    );
    lease::removed::settle_all(&own.db).await.unwrap();
    let here = lease::read(&own.db).await.unwrap().unwrap();
    assert_eq!((here.place(), here.moving), (lease::Where::Vacant, None));
    own.close().await;
}

#[tokio::test]
async fn her_mind_on_a_removed_device_comes_back_once_it_is_settled() {
    let Some(own) = Own::with_her().await else {
        return;
    };
    let (mac, _) = devices::add(&own.db, 41, "Mac").await.unwrap();
    lease::seed(&own.db, &mac.id, "l-1", 9, lease::State::Live)
        .await
        .unwrap();
    let mind = || async { lease::read(&own.db).await.unwrap().unwrap().place() };
    assert_eq!(mind().await, lease::Where::MindOn(mac.id.clone()));
    let holding = own.imports_held().await;
    own.remove(&mac.id).await.unwrap().unwrap();
    assert_eq!(mind().await, lease::Where::MindOn(mac.id.clone()));
    holding.commit().await.unwrap();
    lease::removed::settle_all(&own.db).await.unwrap();
    assert_eq!(mind().await, lease::Where::Here);
    own.close().await;
}
