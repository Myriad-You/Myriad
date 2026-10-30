//! The first meeting of two of her (H3d, E7): a device that has a her of
//! its own connects to the site, which has her too, and its her is merged
//! into the site's. She lives on the site after it; the device is a child.
//!
//! - The device asks with what it holds (`import::propose`, kind `merge`):
//!   the manifest, its persona and wardrobe for the owner to see, and who
//!   she is when her names differ (chosen on the device). The site's her
//!   lives on; the lease notes the merge so no other move starts.
//! - The owner confirms on the site, logged in (Q4), with a passphrase used
//!   once: the site seals the her it has into `data/agent/retired/` first
//!   (H3d-12), then takes the device's pages and files while she lives on
//!   (H3d-1 B).
//! - All of it here and checked against the manifest, she stops for the
//!   moment it takes (frozen: no end has her), the shared core plans the
//!   merge (`myriad_persona_sync::merge::plan`), it lands in one
//!   transaction with the lease one epoch on (H3d-18), and she lives on.
//!   Anything failing before the commit leaves her as she was.
//!
//! Grants never come, nor settings; the merged her's grants are the site's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::Utc;
use myriad_persona_sync::{
    EntityVersion, Kind, State,
    entity::{
        AffectAnchorBody, AffectEventBody, ConversationBody, DiaryBody, FieldHistoryBody,
        IntentionBody, MemoryBody, MemoryVenue, OutfitBody, OwnerStateBody, PaceBody, Picture,
        PresentBody, ProactiveLineBody, StickerBody, WearingBody,
    },
    hlc,
    merge::{self, Device, MergeInput, MergePlan, Side},
    persona::{FieldRev, PersonaBody, VISUAL_PREFIX},
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait};

use serde_json::{Value, json};

use super::import::{
    ImportError, KEEP_DAYS, KEEP_PRESENT_DAYS, Landed, Session, Whole, at, her_on, keep_files,
    locked, merge_refused, one_at_a_time, phase, statement, utc, vectors_fit,
};
use super::{assets, lease, snapshot};
use crate::services::agent::merope::api::sync as her;

/// Where the site keeps what it sealed of its her before a merge.
pub fn retired_dir() -> PathBuf {
    crate::services::data_paths::paths().agent.join("retired")
}

/// The file the site seals of its her when the owner confirms merge `id`.
fn retired_name(id: &str) -> String {
    format!("her-before-merge-{id}.her-archive")
}

/// A merge called off before she was taken together: the copy the site
/// sealed of its her goes (nothing changed). Harmless for an import.
pub async fn forget_retired(id: &str) {
    if !super::import::is_import(id) || id.contains(['/', '\\', '.']) {
        return;
    }
    let path = retired_dir().join(retired_name(id));
    if let Err(error) = tokio::fs::remove_file(&path).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, merge = %id, "[merge] could not remove the copy sealed before it");
    }
}

/// The sealed copies of her the site keeps from before a merge, newest
/// first: name, bytes, when.
pub async fn retired() -> Vec<Value> {
    let mut out = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(retired_dir()).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".her-archive") || name.starts_with('.') {
                continue;
            }
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            let at = meta
                .modified()
                .ok()
                .map(chrono::DateTime::<Utc>::from)
                .map(|at| at.to_rfc3339());
            out.push(json!({ "name": name, "bytes": meta.len(), "at": at }));
        }
    }
    out.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    out
}

/// One of [`retired`] by name, or None when there is no such file.
pub fn retired_path(name: &str) -> Option<PathBuf> {
    let fine = name.ends_with(".her-archive")
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    fine.then(|| retired_dir().join(name))
        .filter(|path| path.is_file())
}

fn side_of(subject: Option<&str>) -> Option<Side> {
    match subject {
        Some("site") => Some(Side::Host),
        Some("device") => Some(Side::Other),
        _ => None,
    }
}

/// The merge's input: the site's her as `host`, the device's as `other`.
fn input(
    host: Vec<EntityVersion>,
    other: Vec<EntityVersion>,
    session: &Session,
    same_lineage: bool,
    device_name: Option<String>,
) -> Result<MergeInput, ImportError> {
    let last = host.iter().map(|entity| entity.rev.clone()).max();
    let rev = hlc::next(
        last.as_ref(),
        crate::clock::now().timestamp_micros(),
        hlc::SITE,
    )
    .map_err(|error| ImportError::Failed(error.to_string()))?;
    let device = session.device_id.clone().unwrap_or_default();
    Ok(MergeInput {
        host,
        other,
        subject: side_of(session.subject.as_deref()),
        same_lineage,
        rev,
        other_end: "local".into(),
        other_end_on_host: (!device.is_empty()).then(|| format!("app:{device}")),
        host_end: hlc::SITE.into(),
        other_device: (!device.is_empty()).then_some(Device {
            id: device,
            name: device_name,
        }),
    })
}

/// What the owner is shown before confirming: both names, who she is, which
/// of her persona's fields come from where and which go to her history, the
/// sets of her wardrobe kept under another id, and how much comes.
pub async fn preview(
    db: &DatabaseConnection,
    owner: i32,
    session: &Session,
    device_name: Option<String>,
) -> Result<Value, sea_orm::DbErr> {
    let pictures = assets::Pictures::new(db, owner);
    let host = snapshot::looks(db, &pictures).await?;
    let site_lease = lease::read(db).await?;
    let same_lineage = site_lease
        .as_ref()
        .is_some_and(|lease| Some(&lease.lineage) == session.lineage.as_ref());
    let subject = side_of(session.subject.as_deref());
    let Ok(mut merging) = input(
        host,
        session.preview.clone(),
        session,
        same_lineage,
        device_name,
    ) else {
        return Ok(Value::Null);
    };
    // Shown for either answer while the names differ and no side is chosen.
    if merging.subject.is_none() {
        merging.subject = subject.or(Some(Side::Host));
    }
    let plan = match merge::plan(merging) {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!(%error, "[merge] the preview could not be made");
            return Ok(json!({ "unreadable": true }));
        }
    };
    let held = super::holdings(db, owner).await?;
    Ok(json!({
        "sameName": plan.same_name,
        "sameLineage": same_lineage,
        "names": {
            "site": plan.names.get(&Side::Host).cloned().flatten(),
            "device": plan.names.get(&Side::Other).cloned().flatten(),
        },
        "subject": session.subject,
        "fields": plan.fields,
        "renamed": plan.renamed,
        "site": {
            "memories": held.memories,
            "diary": held.diary,
            "affectEvents": held.affect_events,
        },
    }))
}

/// The owner, logged in, confirms a device's merge: the site seals its own
/// her with `passphrase` (used for the key once, kept nowhere) into
/// `data/agent/retired/`, then takes the device's her while she lives on.
pub async fn confirm(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
    passphrase: String,
) -> Result<lease::Lease, ImportError> {
    if passphrase.is_empty() {
        return Err(ImportError::Invalid("passphrase"));
    }
    let sealed =
        super::export::export(db, owner, passphrase)
            .await
            .map_err(|error| match error {
                super::export::ExportError::Elsewhere(_) => ImportError::NotHere,
                super::export::ExportError::Empty => ImportError::NoHer,
                super::export::ExportError::Passphrase => ImportError::Invalid("passphrase"),
                super::export::ExportError::Failed(what) => ImportError::Failed(what),
            })?;
    let dir = retired_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|error| ImportError::Failed(error.to_string()))?;
    let name = retired_name(id);
    let temp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    write_private(&temp, &sealed.bytes).await?;
    tokio::fs::rename(&temp, dir.join(&name))
        .await
        .map_err(|error| ImportError::Failed(error.to_string()))?;
    let confirmed = async {
        let txn = db.begin().await?;
        one_at_a_time(&txn).await?;
        let session = locked(&txn, id).await?.ok_or(ImportError::Unknown)?;
        if session.phase != phase::PROPOSED || !session.is_merge() {
            return Err(ImportError::Phase(session.phase));
        }
        let device = session.device_id.clone().ok_or(ImportError::Unknown)?;
        let written = lease::merge_step(&txn, id, &device, lease::Phase::Transferring)
            .await
            .map_err(merge_refused)?;
        txn.execute_raw(statement(
            "UPDATE agent_host_import SET phase = $2, confirmed_at = $4::timestamptz, retired = $3, \
             updated_at = $4::timestamptz WHERE id = $1",
            vec![
                id.into(),
                phase::CONFIRMED.into(),
                name.clone().into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
        txn.commit().await?;
        Ok(written)
    }
    .await;
    match confirmed {
        Ok(written) => {
            lease::reload(db).await;
            tracing::info!(merge = %id, "[merge] the owner confirmed: the site sealed its her and takes the device's while she lives on");
            Ok(written)
        }
        Err(error) => {
            forget_retired(id).await;
            Err(error)
        }
    }
}

async fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), ImportError> {
    use tokio::io::AsyncWriteExt;
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .await
        .map_err(|error| ImportError::Failed(error.to_string()))?;
    file.write_all(bytes)
        .await
        .map_err(|error| ImportError::Failed(error.to_string()))?;
    file.sync_all()
        .await
        .map_err(|error| ImportError::Failed(error.to_string()))
}

/// All of the device's her is here and checked: she stops, the two of her
/// are taken together and land in one transaction, and she lives on. On a
/// failure she goes on as she was.
pub(super) async fn commit(
    db: &DatabaseConnection,
    owner: i32,
    session: &Session,
    other: Whole,
) -> Result<Landed, ImportError> {
    lease::merge_hold(db, &session.id, false)
        .await
        .map_err(merge_refused)?;
    lease::reload(db).await;
    match merge_in(db, owner, session, other).await {
        Ok(landed) => Ok(landed),
        Err(error) => {
            if let Err(error) = lease::merge_hold(db, &session.id, true).await {
                tracing::error!(?error, merge = %session.id, "[merge] could not let her go on after a failed merge");
            }
            lease::reload(db).await;
            Err(error)
        }
    }
}

async fn merge_in(
    db: &DatabaseConnection,
    owner: i32,
    session: &Session,
    other: Whole,
) -> Result<Landed, ImportError> {
    let urls = keep_files(db, owner, &other.files).await?;
    let device_name = match &session.device_id {
        Some(device) => lease::device_name(db, device).await?,
        None => None,
    };
    let pictures = assets::Pictures::new(db, owner);
    let txn = db.begin().await?;
    one_at_a_time(&txn).await?;
    let locked_session = locked(&txn, &session.id)
        .await?
        .ok_or(ImportError::Unknown)?;
    if locked_session.phase != phase::CONFIRMED && locked_session.phase != phase::TRANSFERRING {
        return Err(ImportError::Phase(locked_session.phase));
    }
    crate::services::agent::merope::api::store::lock_persona_on(&txn)
        .await
        .map_err(|error| ImportError::Failed(error.to_string()))?;
    let site_lease = lease::read(&txn).await?.ok_or(ImportError::NoHer)?;
    let same_lineage = Some(&site_lease.lineage) == session.lineage.as_ref();
    let (host, _) = snapshot::whole(&txn, &pictures, owner, None).await?;
    let merging = input(
        host,
        other.entities.clone(),
        session,
        same_lineage,
        device_name,
    )?;
    let plan = merge::plan(merging).map_err(|error| ImportError::Mismatch(error.to_string()))?;
    // Who she is was chosen for names that differ; names that are now the
    // same (or the other way round) are asked again.
    if plan.same_name == session.subject.is_some() {
        return Err(ImportError::Changed);
    }
    let (vectors, rig) = land(&txn, owner, &plan, &other, &urls).await?;
    let written = lease::merged(&txn, &session.id).await?;
    txn.execute_raw(statement(
        "UPDATE agent_host_import SET phase = $2, updated_at = $3::timestamptz WHERE id = $1",
        vec![
            session.id.clone().into(),
            phase::COMMITTED.into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    txn.execute_raw(statement(
        "DELETE FROM agent_host_import_staging WHERE import_id = $1",
        vec![session.id.clone().into()],
    ))
    .await?;
    txn.commit().await?;
    if let Some(Some(rig)) = rig {
        crate::services::merope_rig::mirror_active_asset(Some(rig)).await;
    }
    let zone = field(&plan, "zone")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    crate::services::agent::merope::clock::note_zone(Some(&zone));
    lease::reload(db).await;
    tracing::info!(
        merge = %session.id,
        epoch = written.epoch,
        same_name = plan.same_name,
        subject = ?plan.subject,
        writes = plan.writes.len(),
        late = plan.late.len(),
        vectors,
        "[merge] the two of her are one, on the site"
    );
    let counts = plan
        .counts
        .iter()
        .map(|(kind, count)| (kind.as_str().to_string(), count.merged))
        .collect();
    Ok(Landed {
        counts,
        vectors,
        her_on: her_on().await,
        log: None,
    })
}

fn find<'a>(plan: &'a MergePlan, kind: Kind, id: &str) -> Option<&'a EntityVersion> {
    plan.entities
        .iter()
        .find(|entity| entity.kind == kind && entity.id == id)
}

fn field<'a>(plan: &'a MergePlan, name: &str) -> Option<&'a Value> {
    find(plan, Kind::Persona, myriad_persona_sync::entity::PERSONA_ID)
        .and_then(|entity| entity.body.get("fields"))
        .and_then(|fields| fields.get(name))
        .and_then(|field| field.get("value"))
        .filter(|value| !value.is_null())
}

fn wearing(plan: &MergePlan) -> WearingBody {
    find(plan, Kind::Wearing, myriad_persona_sync::entity::PERSONA_ID)
        .and_then(|entity| entity.body_as().ok())
        .unwrap_or_default()
}

fn bad(error: impl std::fmt::Display) -> ImportError {
    ImportError::Mismatch(error.to_string())
}

/// Writes what the plan says the site writes, in its transaction. Returns
/// the vectors that came, and the rig the site wears now when its looks
/// were written (to mirror once the transaction is in).
async fn land(
    txn: &DatabaseTransaction,
    owner: i32,
    plan: &MergePlan,
    other: &Whole,
    urls: &BTreeMap<String, String>,
) -> Result<(u64, Option<Option<String>>), ImportError> {
    let picture_url = |picture: Option<&Picture>| -> Option<String> {
        picture
            .and_then(Picture::asset)
            .and_then(|asset| urls.get(&asset.id).cloned())
    };
    let writes: BTreeMap<(Kind, &str), &merge::Write> = plan
        .writes
        .iter()
        .map(|write| ((write.kind, write.id.as_str()), write))
        .collect();
    let written = |kind: Kind| plan.writes.iter().filter(move |write| write.kind == kind);
    let entity_of = |write: &merge::Write| -> Result<&EntityVersion, ImportError> {
        find(plan, write.kind, &write.id).ok_or_else(|| bad("a write the plan does not hold"))
    };

    // Her persona, her wardrobe and which set each end wears.
    let mut rig = None;
    if written(Kind::Persona).next().is_some()
        || written(Kind::Outfit).next().is_some()
        || written(Kind::Wearing).next().is_some()
        || !plan.removes.is_empty()
    {
        rig = Some(land_looks(txn, owner, plan, &writes, other, &picture_url).await?);
    }
    for write in written(Kind::FieldHistory) {
        let past: FieldHistoryBody = entity_of(write)?.body_as().map_err(bad)?;
        txn.execute_raw(statement(
            "INSERT INTO agent_persona_field_history \
             (field, value, rev, origin, replaced_at, replaced_by_rev, reason) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            vec![
                past.field.into(),
                past.value.into(),
                past.rev.to_string().into(),
                past.origin.into(),
                at(past.replaced_at).into(),
                past.replaced_by_rev.to_string().into(),
                past.reason.into(),
            ],
        ))
        .await?;
    }

    // Memories, and the vectors of those that came, when the site's model
    // made them.
    let mut came: BTreeMap<String, String> = BTreeMap::new();
    for write in written(Kind::Memory) {
        let entity = entity_of(write)?;
        let memory: MemoryBody = entity.body_as().map_err(bad)?;
        let (venue, audience) = match memory.audience.venue {
            MemoryVenue::Own => ("own", json!([])),
            MemoryVenue::Private => ("private", json!([owner])),
        };
        let updated = at((entity.rev.wall_ms as i64).saturating_mul(1000));
        txn.execute_raw(statement(
            "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, \
             venue, audience, concepts, importance, valid_from, invalid_at, invalid_reason, \
             created_at, updated_at, superseded_by, origin) VALUES \
             ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18) \
             ON CONFLICT (id) DO UPDATE SET kind = EXCLUDED.kind, content = EXCLUDED.content, \
             evidence = EXCLUDED.evidence, speaker = EXCLUDED.speaker, source = EXCLUDED.source, \
             venue = EXCLUDED.venue, audience = EXCLUDED.audience, concepts = EXCLUDED.concepts, \
             importance = EXCLUDED.importance, valid_from = EXCLUDED.valid_from, \
             invalid_at = EXCLUDED.invalid_at, invalid_reason = EXCLUDED.invalid_reason, \
             updated_at = EXCLUDED.updated_at, superseded_by = EXCLUDED.superseded_by, \
             origin = EXCLUDED.origin",
            vec![
                entity.id.clone().into(),
                owner.into(),
                memory.kind.into(),
                memory.content.clone().into(),
                memory.evidence.into(),
                memory.speaker.into(),
                memory.source.into(),
                venue.into(),
                audience.into(),
                serde_json::to_value(&memory.concepts).map_err(bad)?.into(),
                memory.importance.into(),
                at(memory.valid_from).into(),
                memory.invalid_at.map(at).into(),
                memory.invalid_reason.into(),
                at(memory.created_at).into(),
                updated.into(),
                memory.superseded_by.into(),
                entity.origin.clone().into(),
            ],
        ))
        .await?;
        if entity.state == State::Live && write.from == Some(Side::Other) {
            came.insert(entity.id.clone(), memory.content);
        }
    }
    let mut vectors = 0u64;
    if let Some(model) = vectors_fit(txn, other.model.as_ref()).await? {
        for embedding in &other.embeddings {
            let Some(content) = came.get(&embedding.memory_id) else {
                continue;
            };
            use base64::Engine as _;
            let Ok(vector) = base64::engine::general_purpose::STANDARD.decode(&embedding.vector)
            else {
                continue;
            };
            if embedding.digest != crate::services::agent::memory::meaning::digest(content)
                || vector.len() != model.dims as usize * 4
            {
                continue;
            }
            txn.execute_raw(statement(
                "INSERT INTO agent_memory_embeddings (memory_id, model, digest, vector, created_at) \
                 VALUES ($1, $2, $3, $4, $5::timestamptz) ON CONFLICT (memory_id) DO UPDATE SET \
                 model = EXCLUDED.model, digest = EXCLUDED.digest, vector = EXCLUDED.vector",
                vec![
                    embedding.memory_id.clone().into(),
                    model.model.clone().into(),
                    embedding.digest.clone().into(),
                    vector.into(),
                    crate::clock::now().into(),
                ],
            ))
            .await?;
            vectors += 1;
        }
    }

    for write in written(Kind::Diary) {
        let entity = entity_of(write)?;
        let note: DiaryBody = entity.body_as().map_err(bad)?;
        txn.execute_raw(statement(
            "INSERT INTO agent_diary (id, user_id, content, source, created_at) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO UPDATE SET \
             content = EXCLUDED.content, source = EXCLUDED.source",
            vec![
                entity.id.clone().into(),
                owner.into(),
                note.content.into(),
                note.source.into(),
                at(note.created_at).into(),
            ],
        ))
        .await?;
    }

    for write in written(Kind::Sticker) {
        let entity = entity_of(write)?;
        let sticker: StickerBody = entity.body_as().map_err(bad)?;
        let url = match picture_url(Some(&sticker.image)) {
            Some(url) => url,
            None if write.replaces => {
                // The site's own picture of it stays.
                let kept = txn
                    .query_one_raw(statement(
                        "SELECT asset_id FROM merope_stickers WHERE id = $1",
                        vec![entity.id.clone().into()],
                    ))
                    .await?
                    .and_then(|row| row.try_get::<String>("", "asset_id").ok());
                match kept {
                    Some(url) => url,
                    None => continue,
                }
            }
            None => {
                tracing::warn!(sticker = %entity.id, "[merge] a sticker came without its picture; left out");
                continue;
            }
        };
        txn.execute_raw(statement(
            "INSERT INTO merope_stickers (id, picture, meaning, venue, asset_id, identity, sent, \
             last_sent_at, created_at) VALUES ($1, $2, $3, NULL, $4, $5, $6, $7, $8) \
             ON CONFLICT (id) DO UPDATE SET picture = EXCLUDED.picture, meaning = EXCLUDED.meaning, \
             asset_id = EXCLUDED.asset_id, identity = EXCLUDED.identity, sent = EXCLUDED.sent, \
             last_sent_at = EXCLUDED.last_sent_at",
            vec![
                entity.id.clone().into(),
                sticker.picture.into(),
                sticker.meaning.into(),
                url.into(),
                sticker.identity.into(),
                (sticker.sent.unwrap_or(0).min(i32::MAX as u32) as i32).into(),
                sticker.last_sent_at.map(at).into(),
                at(sticker.created_at).into(),
            ],
        ))
        .await?;
    }

    // Her affect: the anchor replay starts from, every change, and where she
    // stands now (right after the last change, as the site keeps it).
    for write in written(Kind::AffectAnchor) {
        let entity = entity_of(write)?;
        let anchor: AffectAnchorBody = entity.body_as().map_err(bad)?;
        txn.execute_raw(statement(
            "INSERT INTO agent_affect_anchor (user_id, mood, arousal, emotion, emotion_arousal, \
             at, through_event, rev) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (user_id) DO UPDATE SET mood = EXCLUDED.mood, arousal = EXCLUDED.arousal, \
             emotion = EXCLUDED.emotion, emotion_arousal = EXCLUDED.emotion_arousal, at = EXCLUDED.at, \
             through_event = EXCLUDED.through_event, rev = EXCLUDED.rev",
            vec![
                owner.into(),
                anchor.affect.mood.into(),
                anchor.affect.arousal.into(),
                anchor.affect.emotion.into(),
                anchor.affect.emotion_arousal.into(),
                at(anchor.at).into(),
                anchor.through_event.into(),
                entity.rev.to_string().into(),
            ],
        ))
        .await?;
    }
    for write in written(Kind::AffectEvent) {
        let entity = entity_of(write)?;
        let change: AffectEventBody = entity.body_as().map_err(bad)?;
        txn.execute_raw(statement(
            "INSERT INTO agent_affect_events (id, user_id, at, cause, impulse, baseline_mood, \
             baseline_arousal, before, after, late, origin, rev, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13::timestamptz) \
             ON CONFLICT (id) DO UPDATE SET at = EXCLUDED.at, late = EXCLUDED.late",
            vec![
                entity.id.clone().into(),
                owner.into(),
                at(change.at).into(),
                change.cause.clone().into(),
                serde_json::to_value(&change.impulse).map_err(bad)?.into(),
                change.baseline.mood.into(),
                change.baseline.arousal.into(),
                serde_json::to_value(change.before).map_err(bad)?.into(),
                serde_json::to_value(change.after).map_err(bad)?.into(),
                change.late.into(),
                entity.origin.clone().into(),
                entity.rev.to_string().into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    }
    let state: Option<OwnerStateBody> = find(plan, Kind::OwnerState, myriad_persona_sync::OWNER)
        .and_then(|entity| entity.body_as().ok());
    if plan.standing.is_some() || state.is_some() {
        let state = state.unwrap_or_default();
        let (affect, settled) = match &plan.standing {
            Some(standing) => (Some(standing.affect), at(standing.at)),
            None => (None, crate::clock::now().fixed_offset()),
        };
        let activity: String = state
            .activity
            .clone()
            .map(|activity| activity.chars().take(16).collect())
            .unwrap_or_else(|| "idle".to_string());
        // Where she stands is kept when the merge does not move it.
        txn.execute_raw(statement(
            "INSERT INTO agent_addressee_state (user_id, mood, arousal, emotion, emotion_arousal, \
             activity, activity_updated_at, do_not_disturb, dnd_start_minute, dnd_end_minute, \
             last_user_message_at, last_proactive_at, music_mood_credited_at, mood_settled_at, \
             emotion_settled_at, updated_at) VALUES \
             ($1, COALESCE($2, 70), COALESCE($3, 50), COALESCE($4, 50), COALESCE($5, 50), $6, \
              $7, $8, $9, $10, $11, $12, $13, $14, $14, $15::timestamptz) \
             ON CONFLICT (user_id) DO UPDATE SET \
             mood = COALESCE($2, agent_addressee_state.mood), \
             arousal = COALESCE($3, agent_addressee_state.arousal), \
             emotion = COALESCE($4, agent_addressee_state.emotion), \
             emotion_arousal = COALESCE($5, agent_addressee_state.emotion_arousal), \
             activity = EXCLUDED.activity, activity_updated_at = EXCLUDED.activity_updated_at, \
             do_not_disturb = EXCLUDED.do_not_disturb, dnd_start_minute = EXCLUDED.dnd_start_minute, \
             dnd_end_minute = EXCLUDED.dnd_end_minute, \
             last_user_message_at = EXCLUDED.last_user_message_at, \
             last_proactive_at = EXCLUDED.last_proactive_at, \
             music_mood_credited_at = EXCLUDED.music_mood_credited_at, \
             mood_settled_at = CASE WHEN $2 IS NULL THEN agent_addressee_state.mood_settled_at \
                 ELSE EXCLUDED.mood_settled_at END, \
             emotion_settled_at = CASE WHEN $2 IS NULL THEN agent_addressee_state.emotion_settled_at \
                 ELSE EXCLUDED.emotion_settled_at END, \
             updated_at = $15::timestamptz",
            vec![
                owner.into(),
                affect.map(|affect| affect.mood).into(),
                affect.map(|affect| affect.arousal).into(),
                affect.map(|affect| affect.emotion).into(),
                affect.map(|affect| affect.emotion_arousal).into(),
                activity.into(),
                state
                    .activity_updated_at
                    .map(at)
                    .unwrap_or_else(|| crate::clock::now().fixed_offset())
                    .into(),
                state.do_not_disturb.into(),
                state.dnd_start_minute.into(),
                state.dnd_end_minute.into(),
                state.last_user_message_at.map(at).into(),
                state.last_proactive_at.map(at).into(),
                state.music_mood_credited_at.map(at).into(),
                settled.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    }

    // Her own records the site keeps in its registry.
    let keep = |days: i64| (crate::clock::now() + chrono::Duration::days(days)).timestamp();
    let mut registry: Vec<(&str, String, Value, i64)> = Vec::new();
    for write in written(Kind::VitalsDay) {
        let entity = entity_of(write)?;
        let day = entity.id.split('@').next().unwrap_or_default().to_string();
        registry.push((
            her::VITALS_NAMESPACE,
            format!("day:{day}"),
            entity.body.clone(),
            keep(KEEP_DAYS),
        ));
    }
    if written(Kind::SerialFollowing).next().is_some() {
        let following: Vec<Value> = plan
            .entities
            .iter()
            .filter(|entity| entity.kind == Kind::SerialFollowing)
            .map(|entity| entity.body.clone())
            .collect();
        registry.push((
            her::SERIAL_NAMESPACE,
            "reading".into(),
            Value::Array(following),
            keep(KEEP_DAYS),
        ));
    }
    for (kind, record) in [(Kind::SerialPast, "past"), (Kind::SerialShelf, "shelf")] {
        for write in written(kind) {
            registry.push((
                her::SERIAL_NAMESPACE,
                record.into(),
                entity_of(write)?.body.clone(),
                keep(KEEP_DAYS),
            ));
        }
    }
    for write in written(Kind::SerialWork) {
        let entity = entity_of(write)?;
        registry.push((
            her::SERIAL_NAMESPACE,
            format!("work:{}", entity.id),
            entity.body.clone(),
            keep(KEEP_DAYS),
        ));
    }
    for write in written(Kind::SoupGame) {
        registry.push((
            her::SOUP_NAMESPACE,
            format!("p:{owner}"),
            entity_of(write)?.body.clone(),
            keep(KEEP_DAYS),
        ));
    }
    for write in written(Kind::Pace) {
        let pace: PaceBody = entity_of(write)?.body_as().map_err(bad)?;
        if let Some(usual) = &pace.pace {
            registry.push((
                her::PACE_NAMESPACE,
                "pace".into(),
                serde_json::to_value(usual).map_err(bad)?,
                keep(KEEP_DAYS),
            ));
        }
        for (day, minutes) in &pace.lazed {
            registry.push((
                her::PACE_NAMESPACE,
                format!("lazed:{day}"),
                json!(minutes),
                keep(KEEP_DAYS),
            ));
        }
    }
    for write in written(Kind::Present) {
        let present: PresentBody = entity_of(write)?.body_as().map_err(bad)?;
        let kept = json!({
            "doing": present.doing.map(|doing| json!({
                "thing": doing.thing,
                "started": utc(doing.started),
                "ends": utc(doing.ends),
                "why": doing.why,
                "had_before": doing.had_before,
            })),
            "lazing": present.lazing.map(|lazing| json!({
                "kind": lazing.kind,
                "started": utc(lazing.started),
                "ends": utc(lazing.ends),
            })),
        });
        registry.push((
            her::PRESENT_NAMESPACE,
            "now".into(),
            kept,
            keep(KEEP_PRESENT_DAYS),
        ));
    }
    for (namespace, record, payload, expires) in registry {
        txn.execute_raw(statement(
            "INSERT INTO runtime_registry (namespace, record_id, payload, expires_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5::timestamptz) ON CONFLICT (namespace, record_id) \
             DO UPDATE SET payload = EXCLUDED.payload, expires_at = EXCLUDED.expires_at, \
             updated_at = $5::timestamptz",
            vec![
                namespace.into(),
                record.into(),
                payload.into(),
                expires.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    }

    // Her conversations with the owner: a new one with every line, one the
    // site has with the lines it lacks.
    for write in written(Kind::Conversation) {
        let entity = entity_of(write)?;
        let conversation: ConversationBody = entity.body_as().map_err(bad)?;
        let had: BTreeSet<(String, i64, String)> = if write.replaces {
            txn.query_all_raw(statement(
                "SELECT role, content, created_at FROM agent_messages WHERE session_id = $1",
                vec![entity.id.clone().into()],
            ))
            .await?
            .iter()
            .filter_map(|row| {
                let at: chrono::DateTime<chrono::FixedOffset> =
                    row.try_get("", "created_at").ok()?;
                Some((
                    row.try_get("", "role").ok()?,
                    at.timestamp_micros(),
                    row.try_get("", "content").ok()?,
                ))
            })
            .collect()
        } else {
            txn.execute_raw(statement(
                "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
                 created_at, last_active_at) VALUES ($1, $2, $3, $4, 0, false, $5, $6)",
                vec![
                    entity.id.clone().into(),
                    owner.into(),
                    conversation
                        .title
                        .clone()
                        .map(|title| title.chars().take(255).collect::<String>())
                        .into(),
                    json!({ "mode": "chat" }).into(),
                    at(conversation.created_at).into(),
                    at(conversation.last_active_at).into(),
                ],
            ))
            .await?;
            BTreeSet::new()
        };
        let mut added = 0i32;
        for line in &conversation.lines {
            let mut content = line.content.clone();
            for picture in &line.pictures {
                if let Some(url) = picture_url(Some(&picture.image)) {
                    content = content.replace(&picture.url, &url);
                }
            }
            if had.contains(&(line.role.clone(), line.at, content.clone()))
                || had.contains(&(line.role.clone(), line.at, line.content.clone()))
            {
                continue;
            }
            let mut metadata = serde_json::Map::new();
            if let Some(cut) = &line.cut_off {
                metadata.insert("cutOff".into(), json!(cut));
            }
            if let Some(heard) = line.heard_chars {
                metadata.insert("heardChars".into(), json!(heard));
            }
            txn.execute_raw(statement(
                "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) \
                 VALUES ($1, $2, $3, $4, $5)",
                vec![
                    entity.id.clone().into(),
                    line.role.clone().into(),
                    content.into(),
                    (!metadata.is_empty())
                        .then_some(Value::Object(metadata))
                        .into(),
                    at(line.at).into(),
                ],
            ))
            .await?;
            added += 1;
        }
        txn.execute_raw(statement(
            "UPDATE agent_sessions SET message_count = message_count + $2, \
             last_active_at = GREATEST(last_active_at, $3) WHERE id = $1",
            vec![
                entity.id.clone().into(),
                added.into(),
                at(conversation.last_active_at).into(),
            ],
        ))
        .await?;
    }
    for write in written(Kind::ProactiveLine).filter(|write| !write.replaces) {
        let line: ProactiveLineBody = entity_of(write)?.body_as().map_err(bad)?;
        let mut metadata = serde_json::Map::new();
        if let Some(cut) = &line.cut_off {
            metadata.insert("cutOff".into(), json!(cut));
        }
        if let Some(heard) = line.heard_chars {
            metadata.insert("heardChars".into(), json!(heard));
        }
        txn.execute_raw(statement(
            "INSERT INTO agent_proactive_messages (user_id, role, content, event_key, notified, \
             created_at, say_id, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            vec![
                owner.into(),
                line.role.into(),
                line.content.into(),
                line.event_key.into(),
                line.notified.into(),
                at(line.at).into(),
                line.say_id.into(),
                (!metadata.is_empty())
                    .then_some(Value::Object(metadata))
                    .into(),
            ],
        ))
        .await?;
    }
    for write in written(Kind::Intention) {
        let entity = entity_of(write)?;
        let intention: IntentionBody = entity.body_as().map_err(bad)?;
        txn.execute_raw(statement(
            "INSERT INTO agent_intentions (id, user_id, source_event_id, summary, reason_code, \
             status, proposal, result_summary, expires_at, accept_source, created_at, \
             updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             ON CONFLICT (id) DO UPDATE SET summary = EXCLUDED.summary, \
             reason_code = EXCLUDED.reason_code, status = EXCLUDED.status, \
             proposal = EXCLUDED.proposal, result_summary = EXCLUDED.result_summary, \
             expires_at = EXCLUDED.expires_at, accept_source = EXCLUDED.accept_source, \
             updated_at = EXCLUDED.updated_at",
            vec![
                entity.id.clone().into(),
                owner.into(),
                intention
                    .source_event_id
                    .unwrap_or_else(|| format!("import:{}", entity.id))
                    .into(),
                intention.summary.into(),
                intention.reason_code.into(),
                intention.status.into(),
                intention.proposal.unwrap_or_else(|| json!({})).into(),
                intention.result_summary.into(),
                intention.expires_at.map(at).into(),
                intention
                    .accept_source
                    .filter(|source| source.len() <= 16)
                    .unwrap_or_else(|| "user".to_string())
                    .into(),
                at(intention.created_at).into(),
                at(intention.updated_at).into(),
            ],
        ))
        .await?;
    }
    Ok((vectors, rig))
}

/// Her persona row: her fields as merged, her wardrobe with what came and
/// what was kept under another id, which set each end wears, her sticker
/// avatar and main portrait, and her rig (the one of the set the site
/// wears, when it is on the site; returned).
async fn land_looks(
    txn: &DatabaseTransaction,
    owner: i32,
    plan: &MergePlan,
    writes: &BTreeMap<(Kind, &str), &merge::Write>,
    other: &Whole,
    picture_url: &(dyn Fn(Option<&Picture>) -> Option<String> + Sync),
) -> Result<Option<String>, ImportError> {
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
    let row = crate::models::entities::agent_persona::Entity::find_by_id(
        crate::services::agent::merope::api::store::PERSONA_ROW_ID,
    )
    .one(txn)
    .await?
    .ok_or(ImportError::NoHer)?;
    let persona: PersonaBody = find(plan, Kind::Persona, myriad_persona_sync::entity::PERSONA_ID)
        .ok_or_else(|| bad("no persona in the plan"))?
        .body_as()
        .map_err(bad)?;
    let mut visual: serde_json::Map<String, Value> = row
        .visual_profile
        .clone()
        .and_then(|profile| profile.as_object().cloned())
        .unwrap_or_default();
    visual.retain(|key, _| super::fields::VISUAL_NOT_FIELDS.contains(&key.as_str()));
    let mut name = String::new();
    let mut personality = String::new();
    let mut zone = String::new();
    let mut persona_json = serde_json::Map::new();
    let mut revs = serde_json::Map::new();
    for (field, value) in &persona.fields {
        if field == "stickerAvatar" {
            continue;
        }
        revs.insert(
            field.clone(),
            serde_json::to_value(FieldRev {
                rev: value.rev.clone(),
                origin: value.origin.clone(),
            })
            .map_err(bad)?,
        );
        match field.as_str() {
            "name" => name = value.value.as_str().unwrap_or_default().to_string(),
            "personality" => personality = value.value.as_str().unwrap_or_default().to_string(),
            "zone" => zone = value.value.as_str().unwrap_or_default().to_string(),
            _ if value.value.is_null() => {}
            _ => match field.strip_prefix(VISUAL_PREFIX) {
                Some(key) => {
                    visual.insert(key.to_string(), value.value.clone());
                }
                None => {
                    persona_json.insert(field.clone(), value.value.clone());
                }
            },
        }
    }
    let avatar = match plan
        .fields
        .iter()
        .find(|outcome| outcome.field == "stickerAvatar")
        .and_then(|outcome| outcome.from)
    {
        Some(Side::Host) => row.avatar_asset_id.clone(),
        Some(Side::Other) => field(plan, "stickerAvatar")
            .and_then(|value| serde_json::from_value::<Picture>(value.clone()).ok())
            .and_then(|picture| picture_url(Some(&picture))),
        None => None,
    };

    // Her wardrobe, the default set first.
    let existing: BTreeMap<String, Value> = visual
        .get("wardrobe")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| Some((item.get("id")?.as_str()?.to_string(), item)))
        .collect();
    let renamed_from: BTreeMap<&str, &str> = plan
        .renamed
        .iter()
        .filter(|renamed| renamed.side == Side::Host)
        .map(|renamed| (renamed.to.as_str(), renamed.from.as_str()))
        .collect();
    let mut outfits: Vec<&EntityVersion> = plan
        .entities
        .iter()
        .filter(|entity| entity.kind == Kind::Outfit && entity.state == State::Live)
        .collect();
    outfits.sort_by_key(|entity| {
        (
            entity.id != myriad_merope::DEFAULT_WARDROBE_ID,
            entity.id.clone(),
        )
    });
    let mut wardrobe = Vec::new();
    let mut default_portrait: Option<String> = None;
    let mut has_default = false;
    for entity in outfits {
        let body: OutfitBody = entity.body_as().map_err(bad)?;
        let write = writes.get(&(Kind::Outfit, entity.id.as_str()));
        let kept_id = renamed_from.get(entity.id.as_str()).copied();
        let item = match (write, kept_id) {
            // The site's, as it was.
            (None, _) => existing.get(&entity.id).cloned(),
            // The site's, under its new id.
            (Some(_), Some(from)) => existing.get(from).cloned().map(|mut item| {
                item["id"] = json!(entity.id);
                item["name"] = json!(body.name);
                if let Some(reference) = &body.reference_outfit_id {
                    item["referenceOutfitId"] = json!(reference);
                }
                item
            }),
            // From the device (or the device's edit of one the site has).
            (Some(_), None) => {
                let had = existing.get(&entity.id);
                let mut item = serde_json::Map::new();
                item.insert("id".into(), json!(entity.id));
                if let Some(name) = &body.name {
                    item.insert("name".into(), json!(name));
                }
                item.insert("clothingStyle".into(), json!(body.clothing_style));
                item.insert("outfit".into(), body.outfit.clone());
                for (key, value) in [
                    ("visualIdentity", body.visual_identity.clone()),
                    ("profile", body.profile.clone().map(Value::String)),
                    (
                        "generationFingerprint",
                        body.generation_fingerprint.clone().map(Value::String),
                    ),
                    (
                        "referenceOutfitId",
                        body.reference_outfit_id.clone().map(Value::String),
                    ),
                ] {
                    if let Some(value) = value {
                        item.insert(key.into(), value);
                    }
                }
                let portrait = picture_url(body.portrait.as_ref()).or_else(|| {
                    had.and_then(|had| had.get("portraitAssetId"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
                if let Some(url) = portrait {
                    item.insert("portraitAssetId".into(), json!(url));
                }
                if let Some(rig) = &body.rig {
                    let site_has = had
                        .and_then(|had| had.get("rigAssetId"))
                        .and_then(Value::as_str)
                        == Some(rig.id.as_str());
                    if other.files.contains_key(&rig.id) || site_has {
                        item.insert("rigAssetId".into(), json!(rig.id));
                    }
                }
                Some(Value::Object(item))
            }
        };
        let Some(item) = item else {
            continue;
        };
        if entity.id == myriad_merope::DEFAULT_WARDROBE_ID {
            has_default = true;
            default_portrait = item
                .get("portraitAssetId")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        wardrobe.push(item);
    }
    let worn = wearing(plan);
    let rig = worn
        .ends
        .get(hlc::SITE)
        .and_then(|site| site.outfit.as_deref())
        .and_then(|id| {
            wardrobe
                .iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
        })
        .and_then(|item| item.get("rigAssetId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    visual.insert("wardrobe".into(), Value::Array(wardrobe));
    visual.remove("activeOutfitId");
    visual.remove("activeFullBodyOutfitId");
    visual.remove("activeOutfitByEnd");
    if let Some(site) = worn.ends.get(hlc::SITE) {
        if let Some(outfit) = &site.outfit {
            visual.insert("activeOutfitId".into(), json!(outfit));
        }
        if let Some(full) = &site.full_body {
            visual.insert("activeFullBodyOutfitId".into(), json!(full));
        }
    }
    let by_end: serde_json::Map<String, Value> = worn
        .ends
        .iter()
        .filter(|(end, _)| end.starts_with("app:"))
        .filter_map(|(end, worn)| Some((end.clone(), json!(worn.outfit.clone()?))))
        .collect();
    if !by_end.is_empty() {
        visual.insert("activeOutfitByEnd".into(), Value::Object(by_end));
    }
    let visual_profile = Value::Object(visual);
    // The main portrait is the default set's; one the site kept without a set stays.
    let portrait = if has_default {
        default_portrait
    } else {
        row.portrait_asset_id.clone()
    };
    let mut active: crate::models::entities::agent_persona::ActiveModel = row.into();
    active.name = Set(name);
    active.personality = Set(personality);
    active.persona_json = Set((!persona_json.is_empty()).then_some(Value::Object(persona_json)));
    active.visual_profile = Set(Some(visual_profile.clone()));
    active.portrait_asset_id = Set(portrait.clone());
    active.avatar_asset_id = Set(avatar.clone());
    active.zone = Set(zone);
    active.field_revs = Set(Value::Object(revs));
    active.updated_by = Set(Some(owner));
    active.updated_at = Set(crate::clock::now().fixed_offset());
    active.update(txn).await?;
    if rig.is_some() {
        crate::services::merope_rig::persist_active_asset(txn, rig.as_deref())
            .await
            .map_err(|error| ImportError::Failed(error.to_string()))?;
    }
    crate::services::media::bind_persona(
        txn,
        portrait.as_deref(),
        avatar.as_deref(),
        Some(&visual_profile),
        &crate::services::media::configured_origins().await,
    )
    .await
    .map_err(|error| ImportError::Failed(error.to_string()))?;
    Ok(rig)
}
