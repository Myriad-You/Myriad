//! 预检时提前找会自己动的部件：预检编译通过后就在后台看一次立绘，结果按这次预检的
//! 内容暂存在内存里，没找成的也记着，同一份内容再预检不再问。激活时内容对得上，
//! 找到的直接带进包；还在找的由激活接着等同一次查找；没找成的由激活自己找一次。
//! 暂存只认它那张立绘：立绘换了、那一套没了就清掉，在找的停下（发出去的那次请求
//! 让它自己结束并记账，不再发后面的），等它的激活也到此为止；页面关掉、切到别处都不清。

use std::{future::Future, sync::Mutex, time::Instant};

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use super::{
    ApiResult,
    master::{MasterProvenance, MasterSlot, master_for, master_from_persona},
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    models::entities::agent_persona,
    services::{agent::merope, image_generation, rig_part_analysis},
};

/// Bump when what tells one staged search from another changes.
const IDENTITY_VERSION: &str = "rig-staged-parts-v1";
/// Searches kept at once: the oldest one no longer looking makes room.
const KEEP: usize = 8;

/// Where a staged search stands.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Search {
    Looking,
    /// The profile the vision model gave.
    Found(Value),
    /// The vision model gave nothing usable. Kept like a find, so another
    /// preview of the same content does not ask again.
    Failed,
    /// Its picture was replaced or its set removed while it looked: there is
    /// nothing left to find parts for.
    Stopped,
}

struct Staged {
    id: String,
    /// The picture it was made of: kept only while that is still its set's.
    master: MasterProvenance,
    began: Instant,
    /// Where it stands; the search itself holds another handle on it.
    search: watch::Sender<Search>,
}

impl Staged {
    fn looking(&self) -> bool {
        *self.search.borrow() == Search::Looking
    }
}

/// Ends a search still looking; one already ended keeps how it ended.
fn end(search: &watch::Sender<Search>, ended: Search) {
    search.send_modify(|standing| {
        if *standing == Search::Looking {
            *standing = ended;
        }
    });
}

/// Asked by a search between its requests to the vision model: no longer
/// true once it was stopped. The request under way then ends by itself and is
/// accounted for, and no further one is made.
struct Wanted(watch::Sender<Search>);

impl Wanted {
    fn still(&self) -> bool {
        *self.0.borrow() == Search::Looking
    }
}

static STAGED: Mutex<Vec<Staged>> = Mutex::new(Vec::new());

fn store() -> std::sync::MutexGuard<'static, Vec<Staged>> {
    STAGED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A search's end, told to whoever waits on it: one that ends without a find
/// (it gave none, or its task died) has failed, so nobody waits on it forever.
struct Report(watch::Sender<Search>);

impl Drop for Report {
    fn drop(&mut self) {
        end(&self.0, Search::Failed);
    }
}

/// What tells a staged search from every other: whose picture it is of (the
/// set, its portrait and that portrait's generation) and everything the
/// search depends on (rig_part_analysis::inventory_identity).
fn identity(master: &MasterProvenance, playback: &Value, reference: &[u8]) -> Option<String> {
    let asked = rig_part_analysis::inventory_identity(playback, reference)?;
    let mut hasher = Sha256::new();
    for part in [
        IDENTITY_VERSION,
        if master.profile.is_bust() {
            "bust"
        } else {
            "fullBody"
        },
        master.outfit_id.as_deref().unwrap_or_default(),
        master.asset_id.as_str(),
        master.generation_fingerprint.as_deref().unwrap_or_default(),
        asked.as_str(),
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    Some(hex::encode(hasher.finalize()))
}

/// Stages a search, unless one of the same content is kept already, whether
/// looking, found or failed: a preview never asks about the same content
/// twice. False when none is kept and none can be staged now.
fn stage<F>(
    store: &mut Vec<Staged>,
    id: &str,
    master: &MasterProvenance,
    find: impl FnOnce(Wanted) -> F,
) -> bool
where
    F: Future<Output = Option<Value>> + Send + 'static,
{
    if store.iter().any(|staged| staged.id == id) {
        return true;
    }
    if store.len() >= KEEP {
        let Some(oldest) = store
            .iter()
            .enumerate()
            .filter(|(_, staged)| !staged.looking())
            .min_by_key(|(_, staged)| staged.began)
            .map(|(index, _)| index)
        else {
            return false;
        };
        store.remove(oldest);
    }
    let (search, _) = watch::channel(Search::Looking);
    let finding = find(Wanted(search.clone()));
    let report = Report(search.clone());
    tokio::spawn(async move {
        if let Some(profile) = finding.await {
            end(&report.0, Search::Found(profile));
        }
    });
    store.push(Staged {
        id: id.to_owned(),
        master: master.clone(),
        began: Instant::now(),
        search,
    });
    true
}

/// Forgets the searches whose picture is gone and stops the ones still
/// looking: whoever waits on one ends there too.
fn forget(store: &mut Vec<Staged>, lost: impl Fn(&MasterProvenance) -> bool) {
    store.retain(|staged| {
        let gone = lost(&staged.master);
        if gone {
            end(&staged.search, Search::Stopped);
        }
        !gone
    });
}

/// Whether the picture a search was made of is still its set's: the worn
/// bust's portrait, a bust set's that is not worn now, or a full-body set's.
/// A bust set not worn is told by its portrait, and by its fingerprint only
/// when the set keeps one: worn, a set without one is matched by the
/// portrait's own generation record, which says nothing of it once another
/// set is worn.
fn still_stands(master: &MasterProvenance, persona: Option<&agent_persona::Model>) -> bool {
    let Some(persona) = persona else {
        return false;
    };
    let same = |asset_id: &str, fingerprint: Option<&str>| {
        asset_id == master.asset_id && fingerprint == master.generation_fingerprint.as_deref()
    };
    if !master.profile.is_bust() {
        return master
            .outfit_id
            .as_deref()
            .and_then(|id| master_for(persona, MasterSlot::FullBody(id)))
            .is_some_and(|now| same(&now.asset_id, now.generation_fingerprint.as_deref()));
    }
    if let Some(worn) =
        master_from_persona(persona).filter(|worn| worn.outfit_id == master.outfit_id)
    {
        return same(&worn.asset_id, worn.generation_fingerprint.as_deref());
    }
    master
        .outfit_id
        .as_deref()
        .and_then(|id| myriad_merope::wardrobe_bust_portrait(persona.visual_profile.as_ref(), id))
        .is_some_and(|(portrait, fingerprint)| {
            portrait == master.asset_id
                && fingerprint.is_none_or(|kept| {
                    Some(kept.as_str()) == master.generation_fingerprint.as_deref()
                })
        })
}

/// Begins looking for a compiled preview's moving parts, in the background,
/// unless that very content is being looked at or was, found or not. The
/// search's id, for the page to ask after; none when nothing can be looked
/// for now.
pub(super) fn begin(
    user_id: i32,
    master: &MasterProvenance,
    playback: &Value,
    reference: Vec<u8>,
) -> Option<String> {
    let id = identity(master, playback, &reference)?;
    // The same call a search after the import makes, on the same inputs.
    let find = |wanted: Wanted| {
        let playback = playback.clone();
        async move {
            let reference = image_generation::ImageReference::new(reference, "image/png").ok()?;
            crate::services::ai_cost_ledger::with_site_ai_ledger(
                user_id,
                "merope",
                "rig-part-analysis",
                rig_part_analysis::inventory(&playback, &reference, || wanted.still()),
            )
            .await
        }
    };
    stage(&mut store(), &id, master, find).then_some(id)
}

/// The search made of this very content, for an import to take its find from
/// or to wait on; none when there is none, or it failed (the import then
/// looks for itself, once).
pub(super) fn standing(
    master: &MasterProvenance,
    playback: &Value,
    reference: &[u8],
) -> Option<watch::Receiver<Search>> {
    let id = identity(master, playback, reference)?;
    store()
        .iter()
        .find(|staged| staged.id == id)
        .filter(|staged| *staged.search.borrow() != Search::Failed)
        .map(|staged| staged.search.subscribe())
}

/// Where a search stands at this moment.
fn read(search: &watch::Receiver<Search>) -> Search {
    search.borrow().clone()
}

/// What a search has found, once it has.
pub(super) fn found(search: &watch::Receiver<Search>) -> Option<Value> {
    match read(search) {
        Search::Found(profile) => Some(profile),
        Search::Looking | Search::Failed | Search::Stopped => None,
    }
}

/// Waits a search out: how it ended.
pub(super) async fn settled(mut search: watch::Receiver<Search>) -> Search {
    if search
        .wait_for(|standing| *standing != Search::Looking)
        .await
        .is_err()
    {
        return Search::Failed;
    }
    read(&search)
}

/// Forgets the searches whose picture is no longer their set's: a portrait
/// replaced or a set removed takes its staged search with it, found or not.
/// Run before a preview stages another, and with the process's periodic cleanup.
pub(crate) async fn sweep(db: &DatabaseConnection) {
    if store().is_empty() {
        return;
    }
    let Ok(persona) = merope::get_persona(db).await else {
        return;
    };
    forget(&mut store(), |master| {
        !still_stands(master, persona.as_ref())
    });
}

/// Where the search a preview began stands, for the page showing that
/// preview: `looking`, `found` with its profile, or `gone` (it found nothing
/// usable, its picture changed, or the server restarted).
pub async fn staged_part_search(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let standing = store()
        .iter()
        .find(|staged| staged.id == id)
        .map(|staged| staged.search.borrow().clone());
    Ok(Json(match standing {
        Some(Search::Looking) => json!({ "state": "looking" }),
        Some(Search::Found(profile)) => json!({ "state": "found", "partProfile": profile }),
        Some(Search::Failed | Search::Stopped) | None => json!({ "state": "gone" }),
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use myriad_merope::CharacterAssetProfile;

    use super::*;

    fn master(asset_id: &str) -> MasterProvenance {
        MasterProvenance {
            profile: CharacterAssetProfile::Bust,
            asset_id: asset_id.to_string(),
            generation_fingerprint: None,
            gender: "female".to_string(),
            outfit_id: Some("a".to_string()),
        }
    }

    fn playback() -> Value {
        json!({
            "pixelCanvas": { "width": 1000, "height": 2000 },
            "layers": [
                { "name": "front-hair", "role": "front-hair", "x": 300, "y": 50, "w": 400, "h": 400 },
            ],
        })
    }

    fn profile() -> Value {
        json!({ "version": 1, "source": "ai-vision", "parts": [] })
    }

    #[test]
    fn a_search_is_of_one_picture_and_one_content() {
        let id = identity(&master("/p.png"), &playback(), b"at rest").unwrap();
        assert_eq!(
            identity(&master("/p.png"), &playback(), b"at rest").unwrap(),
            id
        );
        assert_ne!(
            identity(&master("/q.png"), &playback(), b"at rest").unwrap(),
            id,
            "another portrait"
        );
        assert_ne!(
            identity(&master("/p.png"), &playback(), b"redrawn").unwrap(),
            id,
            "another picture of the rig at rest"
        );
        let mut moved = playback();
        moved["layers"][0]["x"] = json!(301);
        assert_ne!(
            identity(&master("/p.png"), &moved, b"at rest").unwrap(),
            id,
            "another layer table"
        );
        let mut other_set = master("/p.png");
        other_set.outfit_id = Some("b".to_string());
        let mut full_body = master("/p.png");
        full_body.profile = CharacterAssetProfile::FullBody;
        let mut regenerated = master("/p.png");
        regenerated.generation_fingerprint = Some("c".repeat(64));
        for (other, what) in [
            (other_set, "another set"),
            (full_body, "a full-body set"),
            (regenerated, "another generation of the portrait"),
        ] {
            assert_ne!(
                identity(&other, &playback(), b"at rest").unwrap(),
                id,
                "{what}"
            );
        }
        assert!(identity(&master("/p.png"), &json!({}), b"at rest").is_none());
    }

    /// A search that never answers by itself: it ends only when stopped.
    async fn unanswered(wanted: Wanted) -> Option<Value> {
        let mut standing = wanted.0.subscribe();
        drop(standing.wait_for(|search| *search != Search::Looking).await);
        None
    }

    #[tokio::test]
    async fn the_same_content_is_looked_for_once_and_its_find_handed_on() {
        let mut store = Vec::new();
        let asked = Arc::new(AtomicUsize::new(0));
        let (answer, answered) = tokio::sync::oneshot::channel::<Option<Value>>();
        let count = asked.clone();
        assert!(stage(
            &mut store,
            "one",
            &master("/p.png"),
            |_| async move {
                count.fetch_add(1, Ordering::SeqCst);
                answered.await.ok().flatten()
            }
        ));
        // Previewed again while it looks: the search under way is the one.
        let count = asked.clone();
        assert!(stage(
            &mut store,
            "one",
            &master("/p.png"),
            |_| async move {
                count.fetch_add(1, Ordering::SeqCst);
                None::<Value>
            }
        ));
        assert_eq!(store.len(), 1);
        let search = store[0].search.subscribe();
        assert_eq!(found(&search), None);
        // An import made meanwhile waits on it instead of asking again.
        let waiting = tokio::spawn(settled(search.clone()));
        answer.send(Some(profile())).unwrap();
        assert_eq!(waiting.await.unwrap(), Search::Found(profile()));
        assert_eq!(found(&search), Some(profile()));
        // Previewed again once found: still the one.
        let count = asked.clone();
        assert!(stage(
            &mut store,
            "one",
            &master("/p.png"),
            |_| async move {
                count.fetch_add(1, Ordering::SeqCst);
                None::<Value>
            }
        ));
        assert_eq!(store.len(), 1);
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_search_that_found_nothing_is_kept_and_not_asked_again() {
        let mut store = Vec::new();
        let asked = Arc::new(AtomicUsize::new(0));
        let count = asked.clone();
        assert!(stage(
            &mut store,
            "one",
            &master("/p.png"),
            |_| async move {
                count.fetch_add(1, Ordering::SeqCst);
                None::<Value>
            }
        ));
        assert_eq!(settled(store[0].search.subscribe()).await, Search::Failed);
        // Previewed again, as after drawing expressions: the failure stands for this content.
        let count = asked.clone();
        assert!(stage(
            &mut store,
            "one",
            &master("/p.png"),
            |_| async move {
                count.fetch_add(1, Ordering::SeqCst);
                Some(profile())
            }
        ));
        assert_eq!(store.len(), 1);
        assert_eq!(*store[0].search.borrow(), Search::Failed);
        assert!(!store[0].looking());
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        // It goes like any other when its picture does.
        forget(&mut store, |_| true);
        assert!(store.is_empty());
    }

    #[tokio::test]
    async fn a_search_goes_with_its_picture_and_whoever_waited_ends_there() {
        let mut store = Vec::new();
        assert!(stage(&mut store, "kept", &master("/p.png"), unanswered));
        // This one's request is under way when its picture goes, and answers afterwards.
        let (answer, answered) = tokio::sync::oneshot::channel::<Option<Value>>();
        let asked_on = Arc::new(AtomicUsize::new(0));
        let count = asked_on.clone();
        assert!(stage(
            &mut store,
            "gone",
            &master("/q.png"),
            |wanted| async move {
                let first = answered.await.ok().flatten();
                // Between its requests it asks whether it is still wanted.
                if wanted.still() {
                    count.fetch_add(1, Ordering::SeqCst);
                }
                first
            }
        ));
        let gone = store[1].search.subscribe();
        let waiting = tokio::spawn(settled(gone.clone()));
        forget(&mut store, |master| master.asset_id == "/q.png");
        assert_eq!(store.len(), 1);
        assert_eq!(store[0].id, "kept");
        assert!(store[0].looking());
        assert_eq!(
            waiting.await.unwrap(),
            Search::Stopped,
            "told apart from a search that found nothing"
        );
        // Its request ends by itself; what it answers changes nothing, and it asks no more.
        answer.send(Some(profile())).unwrap();
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert_eq!(read(&gone), Search::Stopped);
        assert_eq!(found(&gone), None);
        assert_eq!(asked_on.load(Ordering::SeqCst), 0);
        forget(&mut store, |_| true);
    }

    #[tokio::test]
    async fn the_oldest_ended_search_makes_room_and_none_is_cut_short() {
        let mut store = Vec::new();
        for index in 0..KEEP {
            assert!(stage(
                &mut store,
                &format!("looking-{index}"),
                &master("/p.png"),
                unanswered
            ));
        }
        assert!(
            !stage(&mut store, "one-more", &master("/p.png"), unanswered),
            "every one of them is still looking"
        );
        assert_eq!(store.len(), KEEP);
        // One that found nothing is kept, until room is needed.
        end(&store[3].search, Search::Failed);
        assert!(stage(&mut store, "one-more", &master("/p.png"), unanswered));
        assert_eq!(store.len(), KEEP);
        assert!(store.iter().all(|staged| staged.id != "looking-3"));
        forget(&mut store, |_| true);
    }

    #[test]
    fn a_search_stands_while_its_picture_is_still_its_sets() {
        let persona = agent_persona::Model {
            id: "site".into(),
            name: "Merope".into(),
            personality: String::new(),
            persona_json: None,
            visual_profile: Some(json!({
                "activeOutfitId": "a",
                "gender": "female",
                "wardrobe": [
                    { "id": "a", "portraitAssetId": "master-a" },
                    { "id": "b", "portraitAssetId": "master-b" },
                    { "id": "w-full", "profile": "fullBody", "portraitAssetId": "full-a" },
                ],
            })),
            portrait_asset_id: Some("master-a".into()),
            portrait_generation: None,
            avatar_asset_id: None,
            avatar_generation: None,
            updated_by: None,
            updated_at: crate::clock::now().fixed_offset(),
            zone: String::new(),
            field_revs: serde_json::json!({}),
        };
        let of = |profile, outfit: &str, asset_id: &str| MasterProvenance {
            profile,
            outfit_id: Some(outfit.to_string()),
            ..master(asset_id)
        };
        let bust = CharacterAssetProfile::Bust;
        let full = CharacterAssetProfile::FullBody;
        assert!(still_stands(&of(bust, "a", "master-a"), Some(&persona)));
        assert!(
            !still_stands(&of(bust, "a", "master-old"), Some(&persona)),
            "the worn portrait was replaced"
        );
        assert!(
            still_stands(&of(bust, "b", "master-b"), Some(&persona)),
            "a set not worn now keeps its search"
        );
        assert!(!still_stands(&of(bust, "b", "master-old"), Some(&persona)));
        assert!(
            !still_stands(&of(bust, "c", "master-a"), Some(&persona)),
            "its set was removed"
        );
        assert!(still_stands(&of(full, "w-full", "full-a"), Some(&persona)));
        assert!(!still_stands(
            &of(full, "w-full", "full-old"),
            Some(&persona)
        ));
        assert!(!still_stands(&of(full, "w-gone", "full-a"), Some(&persona)));
        let mut regenerated = of(bust, "a", "master-a");
        regenerated.generation_fingerprint = Some("c".repeat(64));
        assert!(!still_stands(&regenerated, Some(&persona)));
        assert!(!still_stands(&of(bust, "a", "master-a"), None));

        // Worn, a set that keeps no fingerprint is matched by the portrait's own
        // generation record; once another set is worn only its portrait tells.
        let mut matched_by_record = of(bust, "b", "master-b");
        matched_by_record.generation_fingerprint = Some("c".repeat(64));
        assert!(still_stands(&matched_by_record, Some(&persona)));
        let mut fingerprinted = persona.clone();
        fingerprinted.visual_profile.as_mut().unwrap()["wardrobe"][1]["generationFingerprint"] =
            json!("d".repeat(64));
        assert!(
            !still_stands(&matched_by_record, Some(&fingerprinted)),
            "a set that keeps a fingerprint is told by it"
        );
        let mut same_generation = of(bust, "b", "master-b");
        same_generation.generation_fingerprint = Some("d".repeat(64));
        assert!(still_stands(&same_generation, Some(&fingerprinted)));
    }
}
