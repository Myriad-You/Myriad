//! G0: the site's group behaviour, recorded. A test sets up a world (who is
//! paired, what the model answers, what she sees in a picture) on its own
//! thread; every call `io` and the model make then goes here instead, on a
//! clock that moves only with tokio's paused time, and is written down with
//! when it happened. The recordings are the goldens the group mind is held
//! to as it moves into the shared core (D2).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::{Value, json};

use crate::services::runtime_registry::RegistryRow;
use myriad_agent_rules::channel::{GroupImage, PairingLookup};

/// Her speaking soul in every recording.
pub(crate) const SOUL: &str = "你是测试里的她，说话简短。";

/// What the world does, as a test sets it up.
pub(crate) struct World {
    pub base: chrono::DateTime<chrono::Utc>,
    pub start: tokio::time::Instant,
    pub roll: f64,
    pub away: bool,
    pub asleep_for: Option<f64>,
    pub up_since: Option<chrono::DateTime<chrono::Utc>>,
    pub owner: Option<i32>,
    /// Platform ids of paired members, and their user ids.
    pub members: HashMap<String, i32>,
    /// Members whose binding stops holding while she thinks.
    pub unbound_after: HashSet<i32>,
    /// Inbound ids already claimed.
    pub claimed: HashSet<String>,
    /// Answers of the model by operation, in order; none is a failure.
    pub models: HashMap<String, VecDeque<Option<String>>>,
    /// How long the model takes to answer, by operation, in order (none
    /// given: at once).
    pub delays: HashMap<String, VecDeque<f64>>,
    /// What she sees in a picture, by its key.
    pub seen: HashMap<String, myriad_merope::seeing::Seen>,
    /// Text sends (counted from 0) the platform refuses.
    pub refused: HashSet<usize>,
    /// Who cannot be whispered to (`<platform>:<id>`): the platform refuses.
    pub unreachable: HashSet<String>,
    pub sends: usize,
    pub registry: HashMap<(String, String), Value>,
    pub spoke_up: HashMap<String, Vec<(chrono::DateTime<chrono::Utc>, bool)>>,
    /// Chats the platform lets her send into (as the configuration would).
    pub sendable: HashSet<String>,
    /// Reads and writes of the runtime registry that fail, by namespace:
    /// how many more of each (the database is down for them).
    pub failing: HashMap<(String, String), u32>,
    /// The scene's own database (a schema in a throwaway one), when it has
    /// one; else nothing reaches a database.
    pub db: Option<sea_orm::DatabaseConnection>,
    /// The vector of each text, as an embedding model would give it.
    pub vectors: HashMap<String, Vec<f32>>,
    /// What the slang and meme dictionaries have for a term: (title, text,
    /// address).
    pub defined: HashMap<String, (String, String, String)>,
    /// What a search for a query turns up: (text, address).
    pub found: HashMap<String, (String, String)>,
    /// Turtle-soup surfaces she used lately, in this world.
    pub recent_surfaces: Vec<String>,
    /// What each group calls itself, as its platform would say.
    pub titles: HashMap<String, String>,
    /// What she says to a whole speaking prompt, by operation, in order:
    /// how long it takes, and the words or how it failed.
    pub says: HashMap<String, VecDeque<(f64, Result<String, &'static str>)>>,
    pub log: Vec<Value>,
}

impl World {
    pub(crate) fn new(roll: f64) -> Self {
        Self {
            // 14:00 at +08:00, a weekday afternoon.
            base: chrono::DateTime::parse_from_rfc3339("2026-10-14T06:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            start: tokio::time::Instant::now(),
            roll,
            away: false,
            asleep_for: None,
            up_since: None,
            owner: Some(1),
            members: HashMap::new(),
            unbound_after: HashSet::new(),
            claimed: HashSet::new(),
            models: HashMap::new(),
            delays: HashMap::new(),
            seen: HashMap::new(),
            refused: HashSet::new(),
            unreachable: HashSet::new(),
            sends: 0,
            registry: HashMap::new(),
            spoke_up: HashMap::new(),
            sendable: HashSet::new(),
            failing: HashMap::new(),
            db: None,
            vectors: HashMap::new(),
            defined: HashMap::new(),
            found: HashMap::new(),
            recent_surfaces: Vec::new(),
            titles: HashMap::new(),
            says: HashMap::new(),
            log: Vec::new(),
        }
    }
}

thread_local! {
    static WORLD: RefCell<Option<World>> = const { RefCell::new(None) };
}

pub(crate) fn enter(world: World) {
    // A panic in a task of the scene is part of what happened: written
    // down, so a golden never hides one.
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            note("panic", json!({ "message": info.to_string() }));
            before(info);
        }));
    });
    WORLD.with(|cell| *cell.borrow_mut() = Some(world));
}

pub(crate) fn leave() -> Option<World> {
    WORLD.with(|cell| cell.borrow_mut().take())
}

pub(crate) fn with<T>(act: impl FnOnce(&mut World) -> T) -> Option<T> {
    WORLD.with(|cell| cell.borrow_mut().as_mut().map(act))
}

pub(crate) fn on() -> bool {
    with(|_| ()).is_some()
}

fn elapsed_ms(world: &World) -> i64 {
    (tokio::time::Instant::now() - world.start).as_millis() as i64
}

pub(crate) fn now() -> Option<chrono::DateTime<chrono::Utc>> {
    with(|world| world.base + chrono::Duration::milliseconds(elapsed_ms(world)))
}

pub(crate) fn roll() -> Option<f64> {
    with(|world| world.roll)
}

pub(crate) fn away() -> Option<bool> {
    with(|world| world.away)
}

pub(crate) fn asleep_for() -> Option<Option<f64>> {
    with(|world| world.asleep_for)
}

pub(crate) fn up_since() -> Option<chrono::DateTime<chrono::Utc>> {
    with(|world| world.up_since).flatten()
}

pub(crate) fn note(kind: &str, value: Value) {
    with(|world| {
        let t = elapsed_ms(world);
        world
            .log
            .push(json!({ "t": t, "kind": kind, "what": value }));
    });
}

pub(crate) fn owner() -> Option<i32> {
    with(|world| world.owner).flatten()
}

pub(crate) fn claim(id: &str) -> bool {
    note("claim", json!({ "id": id }));
    with(|world| world.claimed.insert(id.to_string())).unwrap_or(false)
}

pub(crate) fn lookup(from: &str) -> Option<PairingLookup> {
    with(|world| match world.members.get(from) {
        Some(user_id) => PairingLookup::Paired { user_id: *user_id },
        None => PairingLookup::Unpaired,
    })
}

pub(crate) fn binding(user_id: i32) -> Option<crate::services::channel_pairing::ChannelBinding> {
    Some(crate::services::channel_pairing::ChannelBinding {
        user_id,
        provider: "g0".into(),
        identity_id: 0,
        scope: String::new(),
    })
}

pub(crate) fn still_bound(user_id: i32) -> bool {
    with(|world| !world.unbound_after.contains(&user_id)).unwrap_or(true)
}

pub(crate) fn kept(namespace: &str, key: &str) -> Option<Value> {
    with(|world| {
        world
            .registry
            .get(&(namespace.to_string(), key.to_string()))
            .cloned()
    })
    .flatten()
}

pub(crate) fn keep(namespace: &str, key: &str, value: Value, until: i64) {
    let left =
        with(|world| until - (world.base.timestamp() + elapsed_ms(world) / 1000)).unwrap_or(0);
    note(
        "keep",
        json!({ "namespace": namespace, "key": key, "value": value, "keptFor": left }),
    );
    with(|world| {
        world
            .registry
            .insert((namespace.to_string(), key.to_string()), value)
    });
}

pub(crate) fn forget_kept(namespace: &str, key: &str) {
    note("forget", json!({ "namespace": namespace, "key": key }));
    with(|world| {
        world
            .registry
            .remove(&(namespace.to_string(), key.to_string()))
    });
}

pub(crate) fn all_kept(namespace: &str) -> Vec<RegistryRow> {
    with(|world| {
        let mut rows: Vec<RegistryRow> = world
            .registry
            .iter()
            .filter(|((ns, _), _)| ns == namespace)
            .map(|((_, key), value)| RegistryRow {
                record_id: key.clone(),
                runtime_id: None,
                payload: value.clone(),
            })
            .collect();
        rows.sort_by(|a, b| a.record_id.cmp(&b.record_id));
        rows
    })
    .unwrap_or_default()
}

pub(crate) fn spoke_up_lately(venue: &str) -> Vec<(chrono::DateTime<chrono::Utc>, bool)> {
    with(|world| world.spoke_up.get(venue).cloned().unwrap_or_default()).unwrap_or_default()
}

pub(crate) fn look(image: &GroupImage, sticker_soul: bool) -> Option<myriad_merope::seeing::Seen> {
    note(
        "look",
        json!({ "key": image.key, "hint": image.hint, "sticker": image.sticker, "keeps": image.sticker && sticker_soul }),
    );
    with(|world| world.seen.get(&image.key).cloned()).flatten()
}

/// A model call: what was asked, and the answer the world gives.
pub(crate) fn model(operation: &str, system: &str, input: &str) -> Option<Option<String>> {
    if !on() {
        return None;
    }
    note(
        "model",
        json!({ "operation": operation, "system": system, "input": input }),
    );
    Some(
        with(|world| {
            world
                .models
                .get_mut(operation)
                .and_then(VecDeque::pop_front)
                .flatten()
        })
        .flatten(),
    )
}

/// How long the model takes over this answer of `operation`, when the
/// scene says.
pub(crate) fn delay(operation: &str) -> Option<f64> {
    with(|world| {
        world
            .delays
            .get_mut(operation)
            .and_then(VecDeque::pop_front)
    })
    .flatten()
}

/// A send to the platform; whether it took it.
pub(crate) fn send(kind: &str, value: Value) -> bool {
    note(kind, value);
    if kind != "send" {
        return true;
    }
    with(|world| {
        let index = world.sends;
        world.sends += 1;
        !world.refused.contains(&index)
    })
    .unwrap_or(true)
}

/// A private send to `who`, as the platform's request: whether it took it.
pub(crate) fn outbound(kind: &str, value: Value, who: &str) -> bool {
    note(kind, value);
    with(|world| !world.unreachable.contains(who)).unwrap_or(true)
}

/// Every value with its object keys in order, so maps compare by content.
pub(crate) fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for key in keys {
                out.insert(key.clone(), canonical(&map[key]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// The log as a golden: canonical, and what happened at the same moment in
/// a fixed order (two things at one instant have no order of their own).
pub(crate) fn golden(log: &[Value]) -> Value {
    let mut log: Vec<Value> = log.iter().map(canonical).collect();
    log.sort_by(|a, b| {
        a["t"]
            .as_i64()
            .cmp(&b["t"].as_i64())
            .then_with(|| a.to_string().cmp(&b.to_string()))
    });
    Value::Array(log)
}

/// Compare `log` to the golden `name`, or write it with `MYRIAD_RECORD_G0=1`.
pub(crate) fn check(name: &str, log: &[Value]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/services/channel_group/g0")
        .join(format!("{name}.json"));
    let got = golden(log);
    assert!(
        log.iter().all(|event| event["kind"] != "panic"),
        "{name}: a task panicked: {log:?}"
    );
    if std::env::var("MYRIAD_RECORD_G0").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        return;
    }
    if let Ok(dir) = std::env::var("MYRIAD_G0_GOT") {
        let _ = std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.json")),
            serde_json::to_string_pretty(&got).unwrap() + "\n",
        );
    }
    let want: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("no golden {name}")),
    )
    .unwrap();
    if got != want {
        let got = serde_json::to_string_pretty(&got).unwrap();
        let want = serde_json::to_string_pretty(&want).unwrap();
        let first = got
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "{name}: differs from its golden at line {first}\n--- got\n{}\n--- want\n{}",
            got.lines()
                .skip(first.saturating_sub(5))
                .take(30)
                .collect::<Vec<_>>()
                .join("\n"),
            want.lines()
                .skip(first.saturating_sub(5))
                .take(30)
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}

/// What she sends into `chat` with: only a chat the scene lets her send to.
pub(crate) fn token_for(
    _platform: crate::services::channel_platform::ChannelPlatform,
    chat: &str,
) -> Option<String> {
    with(|world| {
        world
            .sendable
            .contains(chat)
            .then(|| "g0-token".to_string())
    })
    .flatten()
}

/// Whether this read or write (`what`) of `namespace` fails, as the scene
/// set it up; one that fails is written down.
pub(crate) fn fails(what: &str, namespace: &str, key: &str) -> bool {
    let failed = with(|world| {
        match world
            .failing
            .get_mut(&(what.to_string(), namespace.to_string()))
        {
            Some(left) if *left > 0 => {
                *left -= 1;
                true
            }
            _ => false,
        }
    })
    .unwrap_or(false);
    if failed {
        note(
            "failed",
            json!({ "what": what, "namespace": namespace, "key": key }),
        );
    }
    failed
}

/// The scene's database, if it has one.
pub(crate) fn database() -> Option<sea_orm::DatabaseConnection> {
    with(|world| world.db.clone()).flatten()
}

/// What the dictionaries have for `term`, in the scene.
pub(crate) fn define(term: &str) -> Option<Option<(String, String, String)>> {
    if !on() {
        return None;
    }
    note("define", json!({ "term": term }));
    with(|world| world.defined.get(term).cloned())
}

/// What a search for `query` turns up, in the scene.
pub(crate) fn look_up(query: &str, slang: bool) -> Option<Option<(String, String)>> {
    if !on() {
        return None;
    }
    note("lookUp", json!({ "query": query, "slang": slang }));
    with(|world| world.found.get(query).cloned())
}

/// What the group `venue` calls itself, in the scene.
pub(crate) fn title(venue: &str) -> Option<Option<String>> {
    with(|world| world.titles.get(venue).cloned())
}

/// What she says to `prompt`, in the scene: written down, and answered
/// after the time it takes.
pub(crate) async fn say(operation: &str, prompt: &str) -> Option<Result<String, &'static str>> {
    if !on() {
        return None;
    }
    note("say", json!({ "operation": operation, "prompt": prompt }));
    let (seconds, said) = with(|world| world.says.get_mut(operation).and_then(VecDeque::pop_front))
        .flatten()
        .unwrap_or((0.0, Err("unavailable")));
    tokio::time::sleep(std::time::Duration::from_secs_f64(seconds)).await;
    Some(said)
}

/// What the embedding model gives for `text`, in the scene.
pub(crate) fn vector(text: &str) -> Option<Vec<f32>> {
    with(|world| world.vectors.get(text).cloned()).flatten()
}

/// A throwaway database for a scene: a fresh schema in the database `url`
/// names, migrated, worked by a thread of its own on a runtime of its own.
/// The scene's connection asks it and waits, holding its thread: a paused
/// clock never moves while the scene waits on the database (it would jump
/// to the next timer, and the database's own timeouts with it).
pub(crate) struct Throwaway {
    jobs: std::sync::mpsc::Sender<Job>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub db: sea_orm::DatabaseConnection,
}

enum Job {
    Query(
        sea_orm::Statement,
        std::sync::mpsc::Sender<Result<Vec<sea_orm::ProxyRow>, sea_orm::DbErr>>,
    ),
    Execute(
        sea_orm::Statement,
        std::sync::mpsc::Sender<Result<sea_orm::ProxyExecResult, sea_orm::DbErr>>,
    ),
    Plain(String),
    Drop(std::sync::mpsc::Sender<()>),
}

#[derive(Debug)]
struct Asking(std::sync::Mutex<std::sync::mpsc::Sender<Job>>);

impl Asking {
    fn send(&self, job: Job) {
        if let Ok(jobs) = self.0.lock() {
            let _ = jobs.send(job);
        }
    }
}

#[async_trait::async_trait]
impl sea_orm::ProxyDatabaseTrait for Asking {
    async fn query(
        &self,
        statement: sea_orm::Statement,
    ) -> Result<Vec<sea_orm::ProxyRow>, sea_orm::DbErr> {
        let (answer, answered) = std::sync::mpsc::channel();
        self.send(Job::Query(statement, answer));
        answered.recv().unwrap_or_else(|_| {
            Err(sea_orm::DbErr::Custom(
                "the scene's database went away".into(),
            ))
        })
    }

    async fn execute(
        &self,
        statement: sea_orm::Statement,
    ) -> Result<sea_orm::ProxyExecResult, sea_orm::DbErr> {
        let (answer, answered) = std::sync::mpsc::channel();
        self.send(Job::Execute(statement, answer));
        answered.recv().unwrap_or_else(|_| {
            Err(sea_orm::DbErr::Custom(
                "the scene's database went away".into(),
            ))
        })
    }

    async fn begin(&self) {
        self.send(Job::Plain("BEGIN".into()));
    }

    async fn commit(&self) {
        self.send(Job::Plain("COMMIT".into()));
    }

    async fn rollback(&self) {
        self.send(Job::Plain("ROLLBACK".into()));
    }

    fn start_rollback(&self) {
        self.send(Job::Plain("ROLLBACK".into()));
    }
}

impl Throwaway {
    pub(crate) fn open(url: &str) -> Self {
        use sea_orm::ConnectionTrait;
        let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
        let (ready, readied) = std::sync::mpsc::channel();
        let url = url.to_string();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let schema = format!("g0_{}", uuid::Uuid::new_v4().simple());
                let admin = sea_orm::Database::connect(&url).await.unwrap();
                admin
                    .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
                    .await
                    .unwrap();
                let mut options = sea_orm::ConnectOptions::new(url.clone());
                options
                    .set_schema_search_path(&schema)
                    .max_connections(1)
                    .sqlx_logging(false);
                let db = sea_orm::Database::connect(options).await.unwrap();
                crate::db::Migrator::up(&db, None).await.unwrap();
                let _ = ready.send(());
                while let Ok(job) = inbox.recv() {
                    match job {
                        Job::Query(statement, answer) => {
                            let rows = db.query_all_raw(statement).await.map(|rows| {
                                rows.iter()
                                    .map(sea_orm::from_query_result_to_proxy_row)
                                    .map(with_offsets)
                                    .collect()
                            });
                            let _ = answer.send(rows);
                        }
                        Job::Execute(statement, answer) => {
                            let done = db.execute_raw(statement).await.map(|done| {
                                sea_orm::ProxyExecResult {
                                    last_insert_id: 0,
                                    rows_affected: done.rows_affected(),
                                }
                            });
                            let _ = answer.send(done);
                        }
                        Job::Plain(sql) => {
                            let _ = db.execute_unprepared(&sql).await;
                        }
                        Job::Drop(done) => {
                            db.close().await.ok();
                            admin
                                .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
                                .await
                                .unwrap();
                            admin.close().await.ok();
                            let _ = done.send(());
                            return;
                        }
                    }
                }
            });
        });
        readied.recv().expect("the scene's database is ready");
        let asking = Asking(std::sync::Mutex::new(jobs.clone()));
        let db = futures::executor::block_on(sea_orm::Database::connect_proxy(
            sea_orm::DbBackend::Postgres,
            std::sync::Arc::new(Box::new(asking)),
        ))
        .unwrap();
        Self {
            jobs,
            thread: Some(thread),
            db,
        }
    }

    /// Drop the scene's schema, and the thread with it.
    pub(crate) fn drop_schema(mut self) {
        let (done, dropped) = std::sync::mpsc::channel();
        let _ = self.jobs.send(Job::Drop(done));
        let _ = dropped.recv();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The proxy reads a `timestamptz` as UTC; every entity here reads it with
/// its offset.
fn with_offsets(row: sea_orm::ProxyRow) -> sea_orm::ProxyRow {
    use sea_orm::sea_query::Value;
    sea_orm::ProxyRow {
        values: row
            .values
            .into_iter()
            .map(|(column, value)| match value {
                Value::ChronoDateTimeUtc(at) => (
                    column,
                    Value::ChronoDateTimeWithTimeZone(at.map(|at| at.fixed_offset())),
                ),
                other => (column, other),
            })
            .collect(),
    }
}
