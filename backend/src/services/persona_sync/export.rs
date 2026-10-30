//! 「导出她」 on the site: everything the site has of her, every kind of
//! entity with the files they refer to and the vectors of the site's model,
//! sealed with the owner's passphrase into one `myriad.her-archive` file
//! (`myriad_persona_sync::archive`). Always sealed (H3-6).
//!
//! Read in one read-only snapshot transaction, so the file is her at one
//! moment though she keeps living meanwhile (H3b-4). Only while she lives
//! here or is moving (H3b-5): living elsewhere, the site's copy is not her.
//! The passphrase is used once, for the key, and is in no log, error or
//! row; nothing secret of the site (keys, tokens, grants, settings) is in
//! the entities the file is made of.
//!
//! The file is for the partner's computer, so when the site keeps her log
//! (`services::her_log`) it carries only the audiences that computer may
//! hold, the partner's private talk and hers, as log pages (a version 2
//! file): the same batches `nodes.wire_send` gives that computer over the
//! link, so a body goes only when it may travel. A group's log and anyone
//! else's private talk with her stay on the site.

use std::collections::BTreeMap;

use myriad_persona_log::{Holder, Vector};
use myriad_persona_sync::{
    AssetRef, AssetType, EntityVersion, Envelope, FORMAT, FORMAT_VERSION, Kind,
    archive::{
        self, ArchiveHeader, HeaderFields, Manifest, NONCE_PREFIX_BYTES, Part, RIG_ATLAS_FILE,
        RIG_MANIFEST_FILE, SALT_BYTES, Sealer,
    },
    entity::{ConversationBody, Embedding},
    hlc,
};
use rand::Rng as _;
use sea_orm::{AccessMode, ConnectionTrait, DatabaseConnection, IsolationLevel, TransactionTrait};
use serde_json::Value;

use super::{assets, lease, snapshot};
use crate::services::her_log;

/// Most entities, and most canonical bytes, on one page of the file.
const PAGE_ENTITIES: usize = 2000;
const PAGE_BYTES: usize = 4 * 1024 * 1024;

/// Why there is no file.
#[derive(Debug)]
pub enum ExportError {
    /// No passphrase: the file is always sealed.
    Passphrase,
    /// She does not live on the site now: the device she lives on, if any.
    Elsewhere(Option<String>),
    /// The site has no her to export.
    Empty,
    /// Something could not be read; the message names what, never a value.
    Failed(String),
}

impl ExportError {
    fn failed(what: impl std::fmt::Display) -> Self {
        Self::Failed(what.to_string())
    }
}

/// A sealed export and the name it is saved under.
pub struct Exported {
    pub bytes: Vec<u8>,
    pub name: String,
}

/// Her, sealed with `passphrase`, as the owner `owner` has her.
pub async fn export(
    db: &DatabaseConnection,
    owner: i32,
    passphrase: String,
) -> Result<Exported, ExportError> {
    if passphrase.is_empty() {
        return Err(ExportError::Passphrase);
    }
    let her = read(db, owner).await?;
    let bytes = seal(&her, passphrase).await?;
    tracing::info!(
        bytes = bytes.len(),
        pages = her.pages.len(),
        logs = her.logs.len(),
        files = her.files.len(),
        "[sync] she was exported"
    );
    Ok(Exported {
        bytes,
        name: format!("her-{}.her-archive", her.at.format("%Y%m%d-%H%M")),
    })
}

/// Her as the site has her at one moment, ready to be sealed (once or with
/// more than one key, as a move to a device does).
pub struct Her {
    pub(super) pages: Vec<Envelope>,
    /// Her log's pages, `{audience, rows, payloads?}` in canonical JSON.
    pub(super) logs: Vec<Vec<u8>>,
    pub(super) files: Vec<(Part, Vec<u8>)>,
    pub(super) manifest: Manifest,
    pub(super) lineage: Option<String>,
    pub(super) epoch: Option<u64>,
    pub(super) at: chrono::DateTime<chrono::Utc>,
}

impl Her {
    /// The digest of everything in it, as the archive's manifest has it.
    pub fn digest(&self) -> &str {
        &self.manifest.digest
    }
}

/// Reads her in one read-only snapshot transaction, while she lives here,
/// is moving, or moved out and is not let go yet (H3b-5).
pub async fn read(db: &DatabaseConnection, owner: i32) -> Result<Her, ExportError> {
    let lease = lease::read(db).await.map_err(ExportError::failed)?;
    if let Some(lease) = &lease {
        match lease.place() {
            lease::Where::Here | lease::Where::Moving | lease::Where::MovedOut => {}
            lease::Where::Elsewhere(device) => return Err(ExportError::Elsewhere(Some(device))),
            lease::Where::Vacant => return Err(ExportError::Elsewhere(None)),
        }
    }
    let holdings = super::holdings(db, owner)
        .await
        .map_err(ExportError::failed)?;
    if myriad_persona_sync::is_empty(&holdings) {
        return Err(ExportError::Empty);
    }

    let pictures = assets::Pictures::new(db, owner);
    let model = super::embedding_model(db)
        .await
        .map_err(ExportError::failed)?;
    let snapshot = db
        .begin_with_config(
            Some(IsolationLevel::RepeatableRead),
            Some(AccessMode::ReadOnly),
        )
        .await
        .map_err(ExportError::failed)?;
    let (entities, embeddings) = snapshot::whole(&snapshot, &pictures, owner, model.as_ref())
        .await
        .map_err(ExportError::failed)?;
    let reader = her_log::pc(owner);
    let logs = log_pages(&snapshot, owner, &reader).await?;
    snapshot.commit().await.map_err(ExportError::failed)?;

    let lineage = lease.as_ref().map(|lease| lease.lineage.clone());
    let pages = pages(lineage.clone(), model.clone(), entities, embeddings);
    let files = files(&pictures, &pages).await?;
    let manifest = Manifest::of_envelopes(&pages)
        .and_then(|manifest| manifest.with_logs(&logs, |audience| reader.holds(audience)))
        .map_err(ExportError::failed)?;
    let logs = logs
        .iter()
        .map(myriad_persona_sync::canonical::canonical_bytes)
        .collect();
    Ok(Her {
        pages,
        logs,
        files,
        manifest,
        lineage,
        epoch: lease.as_ref().map(|lease| lease.epoch.max(0) as u64),
        at: crate::clock::now(),
    })
}

/// [`Her`] sealed with `passphrase` (used once, for the key, kept nowhere).
pub async fn seal(her: &Her, passphrase: String) -> Result<Vec<u8>, ExportError> {
    if passphrase.is_empty() {
        return Err(ExportError::Passphrase);
    }
    let (salt, nonce_prefix) = {
        let mut rng = rand::rng();
        let mut salt = [0u8; SALT_BYTES];
        let mut prefix = [0u8; NONCE_PREFIX_BYTES];
        rng.fill_bytes(&mut salt);
        rng.fill_bytes(&mut prefix);
        (salt, prefix)
    };
    let header = ArchiveHeader::new(
        &salt,
        &nonce_prefix,
        HeaderFields {
            lineage: her.lineage.clone(),
            epoch: her.epoch,
            origin: hlc::SITE.to_string(),
            created_at: her.at.timestamp_micros(),
            digest: her.manifest.digest.clone(),
            logs: !her.logs.is_empty(),
        },
    )
    .map_err(ExportError::failed)?;
    let kdf = header.kdf.clone();
    // argon2id takes its memory and a moment: off the async threads.
    let key = tokio::task::spawn_blocking(move || archive::derive_key(&passphrase, &kdf))
        .await
        .map_err(ExportError::failed)?
        .map_err(ExportError::failed)?;

    let mut sealer = Sealer::new(key, header).map_err(ExportError::failed)?;
    let mut out = Vec::new();
    for page in &her.pages {
        let bytes =
            myriad_persona_sync::canonical::canonical_of(page).map_err(ExportError::failed)?;
        let part = Part::Envelope {
            bytes: bytes.len() as u64,
        };
        out.extend(sealer.part(&part, &bytes).map_err(ExportError::failed)?);
    }
    for page in &her.logs {
        let part = Part::Log {
            bytes: page.len() as u64,
        };
        out.extend(sealer.part(&part, page).map_err(ExportError::failed)?);
    }
    for (part, payload) in &her.files {
        out.extend(sealer.part(part, payload).map_err(ExportError::failed)?);
    }
    out.extend(
        sealer
            .part(
                &Part::End {
                    manifest: her.manifest.clone(),
                },
                &[],
            )
            .map_err(ExportError::failed)?,
    );
    out.extend(sealer.finish().map_err(ExportError::failed)?);
    Ok(out)
}

/// Her log's pages for `reader`, the partner's computer: every row of each
/// audience it may hold, in the batches `nodes.wire_send` gives it, each
/// as `{audience, rows, payloads?}`. None while the site does not keep her
/// log for `owner`.
async fn log_pages<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    reader: &Holder,
) -> Result<Vec<Value>, ExportError> {
    if !her_log::enabled()
        || !her_log::store::account(db, owner)
            .await
            .map_err(ExportError::failed)?
            .is_some_and(|account| account.on())
    {
        return Ok(Vec::new());
    }
    pages_for(db, her_log::keys::root(), reader).await
}

/// Her log's pages for `reader`, from what the site keeps of her log.
pub(super) async fn pages_for<C: ConnectionTrait>(
    db: &C,
    root: &dyn her_log::keys::Root,
    reader: &Holder,
) -> Result<Vec<Value>, ExportError> {
    let mut pages = Vec::new();
    for audience in [format!("personal:{}", reader.owner), "own:".to_string()] {
        let sent = her_log::store::send(db, root, &audience, &Vector::new(), Vec::new(), reader)
            .await
            .map_err(ExportError::failed)?;
        for mut batch in sent.batches {
            // A page says no more than its rows and payloads: a body that
            // did not come along is simply not there.
            batch.withheld.clear();
            pages.push(serde_json::to_value(batch).map_err(ExportError::failed)?);
        }
    }
    Ok(pages)
}

/// The entities in pages of at most [`PAGE_ENTITIES`] and about
/// [`PAGE_BYTES`], each with the vectors and the files of its own.
fn pages(
    lineage: Option<String>,
    model: Option<myriad_persona_sync::entity::EmbeddingModel>,
    entities: Vec<EntityVersion>,
    embeddings: Vec<Embedding>,
) -> Vec<Envelope> {
    let mut vectors: BTreeMap<String, Embedding> = embeddings
        .into_iter()
        .map(|embedding| (embedding.memory_id.clone(), embedding))
        .collect();
    let mut out: Vec<Envelope> = Vec::new();
    let mut page: Vec<EntityVersion> = Vec::new();
    let mut bytes = 0usize;
    let close = |page: Vec<EntityVersion>, vectors: &mut BTreeMap<String, Embedding>| Envelope {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        lineage: lineage.clone(),
        origin: hlc::SITE.into(),
        embedding_model: model.clone(),
        embeddings: page
            .iter()
            .filter(|entity| entity.kind == Kind::Memory)
            .filter_map(|entity| vectors.remove(&entity.id))
            .collect(),
        assets: assets::referenced(&page),
        entities: page,
    };
    for entity in entities {
        let size = entity.canonical_bytes().len();
        if !page.is_empty() && (page.len() >= PAGE_ENTITIES || bytes + size > PAGE_BYTES) {
            out.push(close(std::mem::take(&mut page), &mut vectors));
            bytes = 0;
        }
        bytes += size;
        page.push(entity);
    }
    if !page.is_empty() || out.is_empty() {
        out.push(close(page, &mut vectors));
    }
    out
}

/// Every file the pages refer to, as parts: a picture by the address it is
/// read from (checked against its id), a rig as its manifest and atlas.
async fn files(
    pictures: &assets::Pictures,
    pages: &[Envelope],
) -> Result<Vec<(Part, Vec<u8>)>, ExportError> {
    // A picture in a conversation is read from the address in the line.
    let mut in_lines: BTreeMap<String, String> = BTreeMap::new();
    for entity in pages.iter().flat_map(|page| &page.entities) {
        if entity.kind != Kind::Conversation {
            continue;
        }
        let Ok(conversation) = entity.body_as::<ConversationBody>() else {
            continue;
        };
        for picture in conversation.lines.iter().flat_map(|line| &line.pictures) {
            if let Some(asset) = picture.image.asset() {
                in_lines
                    .entry(asset.id.clone())
                    .or_insert_with(|| picture.url.clone());
            }
        }
    }
    let mut wanted: BTreeMap<String, AssetRef> = BTreeMap::new();
    for asset in pages.iter().flat_map(|page| &page.assets) {
        wanted
            .entry(asset.id.clone())
            .or_insert_with(|| asset.clone());
    }
    let mut out = Vec::new();
    for (id, asset) in wanted {
        let gone = || ExportError::Failed(format!("the file {id} can no longer be read"));
        match asset.kind {
            AssetType::Rig => {
                let manifest = crate::services::merope_rig::read_manifest_bytes(&id)
                    .await
                    .map_err(|_| gone())?;
                let atlas = crate::services::merope_rig::read_atlas_bytes(&id)
                    .await
                    .map_err(|_| gone())?;
                for (file, media_type, bytes) in [
                    (RIG_MANIFEST_FILE, "application/json", manifest),
                    (RIG_ATLAS_FILE, "image/png", atlas),
                ] {
                    out.push((
                        Part::Asset {
                            id: id.clone(),
                            kind: AssetType::Rig,
                            media_type: media_type.into(),
                            file: Some(file.into()),
                            bytes: bytes.len() as u64,
                        },
                        bytes,
                    ));
                }
            }
            AssetType::Image => {
                let url = match in_lines.get(&id) {
                    Some(url) => Some(url.clone()),
                    None => match assets::find(pictures, &id)
                        .await
                        .map_err(ExportError::failed)?
                    {
                        Some(assets::Source::Image { url, .. }) => Some(url),
                        _ => None,
                    },
                };
                let bytes = match url {
                    Some(url) => assets::image_bytes(pictures, &url, &id).await,
                    None => None,
                }
                .ok_or_else(gone)?;
                out.push((
                    Part::Asset {
                        id: id.clone(),
                        kind: AssetType::Image,
                        media_type: asset.media_type.clone(),
                        file: None,
                        bytes: bytes.len() as u64,
                    },
                    bytes,
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An export on a real database (an isolated schema; skipped without a
    /// test database) holds every kind she moves with, opens with the
    /// passphrase and only with it, and holds none of the site's secrets.
    #[tokio::test]
    async fn an_export_holds_all_of_her_and_none_of_the_sites_secrets() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let planted = "sk-planted-0123456789abcdef";
        for statement in [
            "INSERT INTO users (id, username) VALUES (41, 'owner41'), (42, 'guest42')".to_string(),
            "INSERT INTO agent_persona (id, name, personality, zone, visual_profile, updated_at) \
             VALUES ('site', 'Merope', '安静', 'Asia/Tokyo', \
             '{\"activeOutfitId\":\"default\",\"activeOutfitByEnd\":{\"app:dev_1\":\"default\"},\
             \"wardrobe\":[{\"id\":\"default\",\"clothingStyle\":\"everyday\",\"outfit\":{}}]}'::jsonb, \
             '2026-10-09T00:00:00Z')"
                .to_string(),
            "INSERT INTO agent_memories (id, user_id, kind, content, speaker, source, venue, \
             audience, concepts, valid_from, created_at, updated_at) VALUES \
             ('m_owner', 41, 'fact', '她喜欢雨天', 'user', 'chat', 'private', '[41]', '[]', \
              '2026-10-02Z', '2026-10-02Z', '2026-10-02Z'), \
             ('m_guest', 42, 'fact', '别人的事', 'user', 'chat', 'private', '[42]', '[]', \
              '2026-10-02Z', '2026-10-02Z', '2026-10-02Z')"
                .to_string(),
            "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
             created_at, last_active_at) VALUES \
             ('s_owner', 41, '雨', '{\"mode\":\"chat\"}', 2, false, '2026-10-09Z', '2026-10-09Z'), \
             ('s_group', 41, NULL, '{\"mode\":\"chat\",\"venue\":\"telegram:-1\"}', 1, false, \
              '2026-10-09Z', '2026-10-09Z'), \
             ('s_gone', 41, NULL, '{\"mode\":\"chat\"}', 1, true, '2026-10-09Z', '2026-10-09Z'), \
             ('s_guest', 42, NULL, '{\"mode\":\"chat\"}', 1, false, '2026-10-09Z', '2026-10-09Z')"
                .to_string(),
            "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) VALUES \
             ('s_owner', 'user', '下雨了', NULL, '2026-10-09T00:00:00Z'), \
             ('s_owner', 'assistant', '带伞了吗', '{\"cutOff\":\"unheard_end\",\"heardChars\":2}', \
              '2026-10-09T00:00:01Z'), \
             ('s_group', 'user', '群里的话', NULL, '2026-10-09T00:00:00Z'), \
             ('s_gone', 'user', '删掉的', NULL, '2026-10-09T00:00:00Z'), \
             ('s_guest', 'user', '别人的私聊', NULL, '2026-10-09T00:00:00Z')"
                .to_string(),
            "INSERT INTO agent_proactive_messages (user_id, role, content, event_key, notified, \
             created_at, say_id) VALUES (41, 'assistant', '雨停了', 'reach', true, \
             '2026-10-09T01:00:00Z', 'say1'), (42, 'assistant', '给别人的', 'reach', true, \
             '2026-10-09T01:00:00Z', NULL)"
                .to_string(),
            "INSERT INTO agent_addressee_state (user_id, mood, arousal, emotion, emotion_arousal, \
             activity, mood_settled_at, emotion_settled_at, updated_at) VALUES \
             (41, 40, 50, 60, 50, '写代码', '2026-10-09Z', '2026-10-09Z', '2026-10-09Z')"
                .to_string(),
            format!(
                "INSERT INTO configurations (key, value) VALUES ('aux_api_key', '\"{planted}\"') \
                 ON CONFLICT DO NOTHING"
            ),
            "INSERT INTO runtime_registry (namespace, record_id, payload, expires_at, updated_at) \
             VALUES ('merope_pace', 'pace', \
             '{\"day\":\"2026-10-09\",\"usualMinutes\":240.0,\"daysPastUsual\":0,\"tone\":\"even\"}', \
             9999999999, '2026-10-09Z'), \
             ('merope_pace', 'lazed:2026-10-09', '12.5', 9999999999, '2026-10-09Z')"
                .to_string(),
        ] {
            // The configurations table may be shaped differently; the secret
            // only has to be somewhere the export could have read it.
            if let Err(error) = db.execute_unprepared(&statement).await {
                assert!(statement.contains("configurations"), "{error}: {statement}");
            }
        }

        assert!(matches!(
            export(db, 41, String::new()).await,
            Err(ExportError::Passphrase)
        ));
        let file = export(db, 41, "雨天的伞".into()).await.unwrap();
        assert!(file.name.ends_with(".her-archive"));
        let text = String::from_utf8_lossy(&file.bytes);
        assert!(!text.contains("她喜欢雨天"), "the file is sealed");
        assert!(archive::open("晴天的伞", &file.bytes).is_err());
        let opened = archive::open("雨天的伞", &file.bytes).unwrap();
        let count = |kind: &str| {
            opened
                .manifest
                .kinds
                .get(kind)
                .map_or(0, |tally| tally.count)
        };
        assert_eq!(count("memory"), 1);
        assert_eq!(count("conversation"), 1);
        assert_eq!(count("proactive_line"), 1);
        assert_eq!(count("owner_state"), 1);
        assert_eq!(count("pace"), 1);
        assert_eq!(count("wearing"), 1);
        assert_eq!(count("persona"), 1);
        let inside = opened
            .envelopes
            .iter()
            .map(|page| serde_json::to_string(page).unwrap())
            .collect::<String>();
        for absent in [planted, "别人", "群里的话", "删掉的", "aux_api_key"] {
            assert!(!inside.contains(absent), "{absent} is in the export");
        }
        let conversation = opened
            .envelopes
            .iter()
            .flat_map(|page| &page.entities)
            .find(|entity| entity.kind == Kind::Conversation)
            .unwrap();
        assert_eq!(conversation.body["lines"][1]["cutOff"], "unheard_end");
        assert_eq!(conversation.body["lines"][1]["heardChars"], 2);
        assert_eq!(opened.header.origin, "site");
        assert!(opened.header.lineage.is_some());

        // Living elsewhere, the site's copy is not her: no export.
        db.execute_unprepared("UPDATE agent_host_lease SET holder = 'dev_1', epoch = epoch + 1")
            .await
            .unwrap();
        assert!(matches!(
            export(db, 41, "雨天的伞".into()).await,
            Err(ExportError::Elsewhere(Some(_)))
        ));
        fixture.close().await;
    }

    /// Her log goes into the file only as far as the partner's computer may
    /// hold it, in the batches the link would send it (a version 2 file),
    /// and lands on another site as it left: the partner's private talk and
    /// hers, never anyone else's.
    #[tokio::test]
    async fn the_files_log_pages_are_only_what_the_partners_computer_may_hold() {
        use crate::services::her_log::{self, keys::test_root::TestRoot, store};
        use myriad_persona_log::Row;
        use sea_orm::ConnectionTrait;
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let from = crate::db::IsolatedSchema::migrated(&url, "export_logs_from").await;
        let to = crate::db::IsolatedSchema::migrated(&url, "export_logs_to").await;
        for schema in [&from, &to] {
            schema
                .db
                .execute_unprepared("INSERT INTO users (id, username) VALUES (41, 'owner')")
                .await
                .unwrap();
            store::start(&schema.db, 41).await.unwrap();
        }
        let root = TestRoot([9; 32]);
        let base64 = base64::engine::general_purpose::STANDARD;
        let now = 1_761_200_000_000_000;
        for (n, audience) in ["personal:41", "own:", "personal:99"]
            .into_iter()
            .enumerate()
        {
            let body = serde_json::json!({ "at": 1_761_200_000_000i64, "kind": "fact", "text": format!("第 {n} 件事") });
            let row: Row = serde_json::from_value(serde_json::json!({
                "version": 1, "id": format!("r{n}"), "origin": "dev_mac", "author": "owner",
                "dseq": 1, "hlc": format!("00176120000000{n}-000000-dev_mac"),
                "audience": audience, "object": format!("memory:m{n}"), "kind": "memory.write",
                "payload": { "ref": format!("doc:r{n}") }, "replicate": "shared",
            }))
            .unwrap();
            let batch: myriad_nodes::wire::Batch = serde_json::from_value(serde_json::json!({
                "audience": audience,
                "rows": [row],
                "payloads": [{
                    "ref": format!("doc:r{n}"),
                    "data": base64::Engine::encode(
                        &base64,
                        myriad_persona_sync::canonical::canonical_bytes(&body),
                    ),
                }],
            }))
            .unwrap();
            let taken = store::take(
                &from.db,
                &root,
                "dev_mac",
                &her_log::server(41),
                batch,
                &Default::default(),
                now,
            )
            .await
            .unwrap();
            assert_eq!(taken.added.len(), 1, "{audience}: {:?}", taken.not_taken);
        }
        // One body that no longer opens: its row still goes, without it.
        from.db
            .execute_unprepared(
                "UPDATE agent_log_payloads SET sealed = '\\x00' WHERE row_id = 'r1'",
            )
            .await
            .unwrap();

        let reader = her_log::pc(41);
        let logs = pages_for(&from.db, &root, &reader).await.unwrap();
        let audiences: Vec<&str> = logs
            .iter()
            .map(|page| page["audience"].as_str().unwrap())
            .collect();
        assert_eq!(audiences, ["personal:41", "own:"]);
        assert_eq!(logs[1]["rows"].as_array().unwrap().len(), 1);
        assert!(logs[1].get("payloads").is_none());
        let pages = pages(None, None, Vec::new(), Vec::new());
        let manifest = Manifest::of_envelopes(&pages).unwrap();
        let elsewhere = serde_json::json!({ "audience": "personal:99", "rows": [] });
        assert!(
            manifest
                .clone()
                .with_logs(&[elsewhere], |audience| reader.holds(audience))
                .is_err()
        );
        let her = Her {
            manifest: manifest
                .with_logs(&logs, |audience| reader.holds(audience))
                .unwrap(),
            pages,
            logs: logs
                .iter()
                .map(myriad_persona_sync::canonical::canonical_bytes)
                .collect(),
            files: Vec::new(),
            lineage: None,
            epoch: None,
            at: crate::clock::now(),
        };
        let sealed = seal(&her, "雨天的伞".into()).await.unwrap();
        let opened = archive::open("雨天的伞", &sealed).unwrap();
        assert_eq!(opened.header.version, archive::ARCHIVE_LOG_VERSION);
        assert_eq!(opened.logs, logs);

        // A file edited to carry what the partner's computer may not hold:
        // those pages are not taken, a group's forget least of all.
        let mut pages = opened.logs;
        pages.push(serde_json::json!({ "audience": "personal:99", "rows": [] }));
        pages.push(serde_json::json!({ "audience": "group:g1", "rows": [{
            "version": 1, "id": "g1", "origin": "dev_mac", "author": "owner", "dseq": 1,
            "hlc": "001761200000009-000000-dev_mac", "audience": "group:g1",
            "object": "memory:m9", "kind": "forget", "replicate": "shared",
        }] }));
        let landed = super::super::import::take_pages(
            &to.db,
            &root,
            41,
            pages,
            super::super::import::LogLanded::default(),
        )
        .await;
        assert_eq!(
            (
                landed.added,
                landed.not_taken,
                landed.withheld,
                landed.pages_refused
            ),
            (2, 0, 1, 2)
        );
        assert!(store::rows(&to.db, "group:g1").await.unwrap().is_empty());
        assert_eq!(
            to.db
                .query_one_raw(sea_orm::Statement::from_string(
                    sea_orm::DatabaseBackend::Postgres,
                    "SELECT COUNT(*) AS n FROM agent_log_rows WHERE received_from = 'file'",
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get::<i64>("", "n")
                .unwrap(),
            2
        );
        for audience in ["personal:41", "own:"] {
            assert_eq!(
                store::rows(&to.db, audience).await.unwrap(),
                store::rows(&from.db, audience).await.unwrap()
            );
        }
        assert!(store::rows(&to.db, "personal:99").await.unwrap().is_empty());
        async fn sent(db: &sea_orm::DatabaseConnection) -> myriad_nodes::wire::Sent {
            let pc = crate::services::her_log::pc(41);
            let root = crate::services::her_log::keys::test_root::TestRoot([9; 32]);
            crate::services::her_log::store::send(
                db,
                &root,
                "personal:41",
                &Default::default(),
                Vec::new(),
                &pc,
            )
            .await
            .unwrap()
        }
        assert_eq!(sent(&to.db).await, sent(&from.db).await);
        from.drop().await;
        to.drop().await;
    }
}
