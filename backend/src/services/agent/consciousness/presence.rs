//! Process-local live presence. Not a table, not a grant.
//!
//! Pages of the site and native children on her link share one lease table,
//! told apart by [`LiveSurface`]. Whether the person is here with her
//! ([`owner_here`]: arriving, who is present, the panel being open) reads
//! every surface, so someone on the Mac app is as present as someone on a
//! page. A page is here while it is visible; the app is here while its link
//! is up and its lease fresh, its window on screen or not (D16): it holds
//! what she says until they look. Visibility stays as reported. Her body on
//! the site (the face, its rig, touch) reads only pages:
//! [`last_live_presence`]. Her senses (what she sees and hears there:
//! perception, music) are those of the surface they are using her on:
//! [`senses_presence`].

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Duration, Utc};
use once_cell::sync::Lazy;
use serde_json::Value;

use crate::services::agent::presence_window::PRESENCE_WINDOW_SECS;
use crate::services::agent::types::UserRequest;

use super::{LiveSurface, SelfLivePresence};

const MAX_LIVE_USERS: usize = 4096;
const MAX_USER_PAGES: usize = 16;

#[derive(Default)]
struct LivePresenceStore {
    users: HashMap<i32, HashMap<String, SelfLivePresence>>,
}

impl LivePresenceStore {
    fn remember(&mut self, user_id: i32, live: SelfLivePresence, now: DateTime<Utc>) -> bool {
        self.users.retain(|_, pages| {
            pages.retain(|_, value| presence_is_fresh_at(value, now));
            !pages.is_empty()
        });
        if user_id <= 0
            || (!self.users.contains_key(&user_id) && self.users.len() >= MAX_LIVE_USERS)
        {
            return false;
        }
        let was_present = self.here(user_id, now).present;
        let mut live = live;
        let pages = self.users.entry(user_id).or_default();
        let key = live.instance_id.clone().unwrap_or_default();
        let before = pages
            .get(&key)
            .filter(|before| presence_is_fresh_at(before, now));
        // The same as it was: they did not come or go, so it keeps when they did.
        let same = before.is_some_and(|before| {
            before.page_visible == live.page_visible
                && before.panel_visible == live.panel_visible
                && before.face_visible == live.face_visible
        });
        // Its window going off screen is not using it: an app still here
        // keeps when they last came to it.
        let leaving = before.is_some() && !live.page_visible;
        if live.changed_at.is_none() {
            live.changed_at = before
                .filter(|_| same || leaving)
                .and_then(|before| before.changed_at)
                .or(live.captured_at);
        }
        if !pages.contains_key(&key) && pages.len() >= MAX_USER_PAGES {
            // Prefer evicting an old hidden page, not the actively viewed one.
            if let Some(oldest) = pages
                .iter()
                .min_by_key(|(id, value)| (value.page_visible, value.captured_at, *id))
                .map(|(id, _)| id.clone())
            {
                pages.remove(&oldest);
            }
        }
        pages.insert(key, live);
        !was_present && self.here(user_id, now).present
    }

    fn last(&self, user_id: i32, now: DateTime<Utc>) -> SelfLivePresence {
        self.last_on(user_id, LiveSurface::Web, now)
    }

    /// Of the pages and apps they are on, the one they came to or used last;
    /// a page wins a tie.
    fn front(&self, user_id: i32, now: DateTime<Utc>) -> Option<Front> {
        let (key, newest) = self
            .users
            .get(&user_id)?
            .iter()
            .filter(|(_, live)| counts_here(live) && presence_is_fresh_at(live, now))
            .max_by_key(|(key, live)| {
                (
                    live.changed_at.or(live.captured_at),
                    live.surface == LiveSurface::Web,
                    live.panel_visible,
                    *key,
                )
            })?;
        let surface = newest.surface;
        Some(Front {
            surface,
            device_id: match surface {
                LiveSurface::Native => key.strip_prefix(NATIVE_PREFIX).map(str::to_string),
                LiveSurface::Web => None,
            },
            live: self.last_on(user_id, surface, now),
        })
    }

    /// Here on any surface, and looking at her panel on any.
    fn here(&self, user_id: i32, now: DateTime<Utc>) -> Here {
        // The app with its link up is here whatever its window shows (D16).
        let mut here = Here {
            present: self.on(user_id, LiveSurface::Native, now),
            panel_open: false,
        };
        for surface in [LiveSurface::Web, LiveSurface::Native] {
            let live = self.last_on(user_id, surface, now);
            here.present |= live.page_visible;
            here.panel_open |= live.page_visible && live.panel_visible;
        }
        here
    }

    /// Some fresh lease of theirs on this surface.
    fn on(&self, user_id: i32, surface: LiveSurface, now: DateTime<Utc>) -> bool {
        self.users.get(&user_id).is_some_and(|pages| {
            pages
                .values()
                .any(|live| live.surface == surface && presence_is_fresh_at(live, now))
        })
    }

    fn last_on(&self, user_id: i32, surface: LiveSurface, now: DateTime<Utc>) -> SelfLivePresence {
        let Some(pages) = self.users.get(&user_id) else {
            return SelfLivePresence::default();
        };
        let fresh: Vec<_> = pages
            .iter()
            .filter(|(_, live)| live.surface == surface && presence_is_fresh_at(live, now))
            .collect();
        // Select one coherent body/perception/music source. Never splice the rig
        // of one outfit into another page's observations. Visible panel wins,
        // then visible face, then recency; hidden updates cannot displace it.
        let Some((_, selected)) = fresh.iter().max_by_key(|(id, live)| {
            (
                live.page_visible,
                live.page_visible && live.panel_visible,
                live.page_visible && live.face_visible,
                live.captured_at,
                *id,
            )
        }) else {
            return SelfLivePresence::default();
        };
        let mut live = (*selected).clone();
        live.page_visible = fresh.iter().any(|(_, value)| value.page_visible);
        live.panel_visible = fresh
            .iter()
            .any(|(_, value)| value.page_visible && value.panel_visible);
        live.face_visible = fresh
            .iter()
            .any(|(_, value)| value.page_visible && value.face_visible);
        // Hidden audio still owns the mouth. Interruptibility is conservative.
        live.speaking = fresh.iter().any(|(_, value)| value.speaking);
        live.speech_interruptible = live.speaking
            && fresh
                .iter()
                .filter(|(_, value)| value.speaking)
                .all(|(_, value)| value.speech_interruptible);
        age_perception(&mut live, now);
        live
    }
}

/// Whether the person is with her right now, wherever: a visible page of the
/// site, or the native app with its link up (D16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Here {
    pub present: bool,
    /// Her panel is open where they are.
    pub panel_open: bool,
}

/// The surface the person is using her on right now.
#[derive(Debug, Clone, PartialEq)]
pub struct Front {
    pub surface: LiveSurface,
    /// The native child's device, on the app.
    pub device_id: Option<String>,
    /// That surface's presence: its face, rig and perception.
    pub live: SelfLivePresence,
}

const NATIVE_PREFIX: &str = "native:";

/// A lease that counts as them being here: a visible page, or the app with
/// its link up whatever its window shows (D16).
fn counts_here(live: &SelfLivePresence) -> bool {
    live.page_visible || live.surface == LiveSurface::Native
}

/// Something she says to them in person can be said where they are using
/// her: a page with her face on screen, or the app, which holds the line
/// (its voice and acting) until its window is looked at again (D16).
pub fn can_speak_to(live: &SelfLivePresence) -> bool {
    live.surface == LiveSurface::Native || (live.page_visible && live.face_visible)
}

/// The rig she acts with where they are using her. The app plays her acting
/// once its window is on screen again (D16), so its summary is taken as that
/// body seen; the page's is taken as it is.
pub fn acting_rig_state(user_id: i32) -> Option<myriad_merope::RigStateSummary> {
    let live = face_presence(user_id);
    let mut rig = live.rig_state?;
    if live.surface == LiveSurface::Native {
        rig.page_visible = true;
        rig.face_visible = true;
    }
    Some(rig)
}

static LIVE: Lazy<RwLock<LivePresenceStore>> =
    Lazy::new(|| RwLock::new(LivePresenceStore::default()));

/// Returns true when the addressee moved from absent/expired to here (any surface).
pub fn remember_live_presence(user_id: i32, live: SelfLivePresence) -> bool {
    LIVE.write()
        .map(|mut store| store.remember(user_id, live, crate::clock::now()))
        .unwrap_or(false)
}

/// The person is with her right now, on any surface.
pub fn owner_here(user_id: i32) -> Here {
    LIVE.read()
        .map(|store| store.here(user_id, crate::clock::now()))
        .unwrap_or_default()
}

/// People with her right now: a visible page of the site, or the native app.
pub fn present_users() -> Vec<i32> {
    let now = crate::clock::now();
    LIVE.read()
        .map(|store| {
            store
                .users
                .keys()
                .copied()
                .filter(|user_id| store.here(*user_id, now).present)
                .collect()
        })
        .unwrap_or_default()
}

pub fn last_live_presence(user_id: i32) -> SelfLivePresence {
    LIVE.read()
        .map(|store| store.last(user_id, crate::clock::now()))
        .unwrap_or_default()
}

/// What she senses of them now: the surface they are using her on (the one
/// they came to or used last), so her senses follow them to the app; with
/// them on no surface, the pages' as before.
pub fn senses_presence(user_id: i32) -> SelfLivePresence {
    front_presence(user_id)
        .map(|front| front.live)
        .unwrap_or_else(|| last_live_presence(user_id))
}

/// What a turn said on the app is answered with: that device's perception
/// and music, in the shape a page's request carries them. None when it
/// reports nothing of either.
pub fn native_senses(user_id: i32, device_id: &str) -> Option<Value> {
    let live = native_presence(user_id, device_id)?;
    if live.perception_payload.is_empty() && live.music_status.is_none() {
        return None;
    }
    let mut senses = serde_json::Map::new();
    if !live.perception_payload.is_empty() {
        senses.insert("perception".into(), Value::Array(live.perception_payload));
    }
    if let Some(music) = live.music_status {
        senses.insert("musicStatus".into(), music);
    }
    Some(Value::Object(senses))
}

/// The lease key of a native child: a colon never passes the page whitelist,
/// so no page can write over it.
fn native_instance(device_id: &str) -> String {
    format!("{NATIVE_PREFIX}{device_id}")
}

/// Where they are using her now: the page or app they came to or used last.
pub fn front_presence(user_id: i32) -> Option<Front> {
    LIVE.read()
        .ok()
        .and_then(|store| store.front(user_id, crate::clock::now()))
}

/// Her face where they are using her: that surface's presence (a page's when
/// nobody is anywhere).
pub fn face_presence(user_id: i32) -> SelfLivePresence {
    front_presence(user_id)
        .map(|front| front.live)
        .unwrap_or_default()
}

/// They said something to her on the app: it is the surface in use now.
pub fn native_used(user_id: i32, device_id: &str) {
    if let Ok(mut store) = LIVE.write() {
        let now = crate::clock::now();
        if let Some(live) = store
            .users
            .get_mut(&user_id)
            .and_then(|pages| pages.get_mut(&native_instance(device_id)))
            .filter(|live| presence_is_fresh_at(live, now))
        {
            live.changed_at = Some(now);
        }
    }
}

/// What a native child on her link reports, through the same whitelist and
/// lease as a page. It never revives anyone onto the site.
/// Returns true when the person just arrived (on no surface before).
pub fn remember_native_presence(user_id: i32, device_id: &str, data: &Value) -> bool {
    let mut live = live_presence_from_custom_data(data);
    live.instance_id = Some(native_instance(device_id));
    live.surface = LiveSurface::Native;
    remember_live_presence(user_id, live)
}

/// The native child's own lease, while it is fresh: the face her link
/// reports for that device (its window, its rig, whether it is speaking).
pub fn native_presence(user_id: i32, device_id: &str) -> Option<SelfLivePresence> {
    let now = crate::clock::now();
    let store = LIVE.read().ok()?;
    let mut live = store
        .users
        .get(&user_id)?
        .get(&native_instance(device_id))
        .filter(|live| presence_is_fresh_at(live, now))?
        .clone();
    age_perception(&mut live, now);
    Some(live)
}

/// The child's link closed: it is not there any more.
pub fn forget_native_presence(user_id: i32, device_id: &str) {
    if let Ok(mut store) = LIVE.write() {
        let key = native_instance(device_id);
        if let Some(pages) = store.users.get_mut(&user_id) {
            pages.remove(&key);
            if pages.is_empty() {
                store.users.remove(&user_id);
            }
        }
    }
}

fn age_perception(live: &mut SelfLivePresence, now: DateTime<Utc>) {
    // The page-presence lease is longer than individual observations. Age a
    // copy on read, never renew the stored TTL by repeatedly reading it.
    let elapsed = live.captured_at.map_or(0, |at| {
        now.signed_duration_since(at).num_milliseconds().max(0)
    });
    for item in &mut live.perception_payload {
        let remaining = item
            .get("ttlMs")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .saturating_sub(elapsed)
            .max(0);
        item["ttlMs"] = remaining.into();
    }
    live.perception_payload.retain(|item| {
        item.get("ttlMs")
            .and_then(Value::as_i64)
            .is_some_and(|ttl| ttl > 0)
    });
    refresh_perception_text(live);
}

fn refresh_perception_text(live: &mut SelfLivePresence) {
    live.perception = live
        .perception_payload
        .iter()
        .filter_map(Value::as_object)
        .map(crate::services::agent::perception_view::perception_reader_text)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(160).collect())
        .take(crate::services::agent::perception_view::MAX_PERCEPTION_ITEMS)
        .collect();
}

/// Looking at her: her panel is open where they are (a page or the app).
pub fn live_presence_panel_open(user_id: i32) -> bool {
    owner_here(user_id).panel_open
}

fn presence_is_fresh_at(live: &SelfLivePresence, now: DateTime<Utc>) -> bool {
    let Some(captured_at) = live.captured_at else {
        return false;
    };
    now.signed_duration_since(captured_at) <= Duration::seconds(PRESENCE_WINDOW_SECS)
}

/// Whitelist for inbound presence JSON. Unknown keys are ignored; perception
/// keeps only reader-safe text (local privacy never yields `summary`).
pub fn live_presence_from_custom_data(data: &Value) -> SelfLivePresence {
    let mut live = SelfLivePresence::default();
    apply_whitelisted_presence(&mut live, data);
    live.captured_at = Some(crate::clock::now());
    live
}

fn apply_whitelisted_presence(live: &mut SelfLivePresence, data: &Value) {
    if let Some(presence) = data.get("presence").and_then(Value::as_object) {
        live.instance_id = presence
            .get("instanceId")
            .and_then(Value::as_str)
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 64
                    && id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            })
            .map(str::to_owned);
        if let Some(rig) = presence.get("rigState") {
            live.rig_state = myriad_merope::sanitize_rig_state(rig);
        }
        live.speaking = presence
            .get("speaking")
            .and_then(Value::as_bool)
            .unwrap_or(live.speaking);
        live.face_visible = presence
            .get("faceVisible")
            .and_then(Value::as_bool)
            .unwrap_or(live.face_visible);
        live.page_visible = presence
            .get("pageVisible")
            .and_then(Value::as_bool)
            .unwrap_or(live.page_visible);
        live.panel_visible = presence
            .get("panelVisible")
            .and_then(Value::as_bool)
            .unwrap_or(live.panel_visible);
        live.speech_interruptible = presence
            .get("speechInterruptible")
            .and_then(Value::as_bool)
            .unwrap_or(live.speech_interruptible);
        if let Some(mode) = presence.get("visibleMode").and_then(Value::as_str) {
            live.visible_mode = Some(mode.to_string());
        }
        live.motion_intent = presence
            .get("motionIntent")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(live.motion_intent.clone());
        live.speech_intent = presence
            .get("speechIntent")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(live.speech_intent.clone());
    }
    if let Some(items) = data.get("perception").and_then(Value::as_array) {
        live.perception_payload = items
            .iter()
            .filter_map(sanitize_perception)
            .take(crate::services::agent::perception_view::MAX_PERCEPTION_ITEMS)
            .collect();
        refresh_perception_text(live);
    }
    if let Some(music) = data.get("musicStatus") {
        live.music_status = sanitize_music_status(music);
    }
}

fn sanitize_perception(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let source = obj
        .get("sourceId")?
        .as_str()?
        .chars()
        .take(80)
        .collect::<String>();
    let kind = obj.get("kind")?.as_str()?;
    if source.is_empty()
        || !matches!(
            kind,
            "page" | "pointer" | "surface" | "music" | "voice" | "presence" | "screen"
        )
    {
        return None;
    }
    let privacy = obj
        .get("privacy")
        .and_then(Value::as_str)
        .unwrap_or("local");
    if !matches!(privacy, "local" | "consented" | "system") {
        return None;
    }
    let ttl = obj
        .get("ttlMs")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .clamp(0, 90_000);
    if ttl == 0 {
        return None;
    }
    let mut facts = serde_json::Map::new();
    if let Some(input) = obj.get("safeFacts").and_then(Value::as_object) {
        for (key, value) in input.iter().take(12) {
            let value = match value {
                Value::String(text) => Value::String(text.chars().take(120).collect()),
                Value::Bool(_) | Value::Number(_) => value.clone(),
                _ => continue,
            };
            facts.insert(key.chars().take(40).collect(), value);
        }
    }
    Some(serde_json::json!({
        "sourceId": source,
        "kind": kind,
        "ttlMs": ttl,
        "summary": if privacy == "local" { String::new() } else {
            obj.get("summary").and_then(Value::as_str).unwrap_or("").chars().take(400).collect()
        },
        "safeFacts": facts,
        "privacy": privacy,
    }))
}

fn sanitize_music_status(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let song = obj
        .get("currentSong")
        .and_then(Value::as_object)
        .map(|song| {
            let text = |key: &str, max: usize| {
                song.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(max)
                    .collect::<String>()
            };
            serde_json::json!({
                "name": text("name", 80), "artist": text("artist", 80),
                "album": text("album", 80), "source": text("source", 40),
                // Whole seconds; a player may report fractions.
                "duration": song.get("duration").and_then(Value::as_f64).filter(|seconds| seconds.is_finite()).map_or(0, |seconds| seconds.round().clamp(0.0, 86_400.0) as u64),
            })
        });
    Some(serde_json::json!({
        "isPlaying": obj.get("isPlaying").and_then(Value::as_bool).unwrap_or(false),
        "isEnabled": obj.get("isEnabled").and_then(Value::as_bool).unwrap_or(false),
        "currentSong": song,
        "currentSongIndex": obj.get("currentSongIndex").and_then(Value::as_u64).unwrap_or(0).min(10_000),
        "playlistLength": obj.get("playlistLength").and_then(Value::as_u64).unwrap_or(0).min(10_000),
        "currentLyric": obj.get("currentLyric").and_then(Value::as_str).unwrap_or("").chars().take(120).collect::<String>(),
        // Where in the song it is, whole seconds (D41).
        "currentTime": obj.get("currentTime").and_then(Value::as_f64).filter(|seconds| seconds.is_finite()).map(|seconds| seconds.round().clamp(0.0, 86_400.0) as u64),
    }))
}

/// What a turn says about where it was said. A page's request is that page
/// now; a turn said on a native child is not a page at all (its link
/// reports that device's presence, and saying it was using it there).
pub fn remember_turn_presence(request: &UserRequest) {
    if request
        .context
        .as_ref()
        .is_some_and(|context| context.on_device.is_some())
    {
        return;
    }
    remember_live_presence(request.user_id, live_presence_from_request(request));
}

pub fn live_presence_from_request(request: &UserRequest) -> SelfLivePresence {
    let mut live = SelfLivePresence::default();
    let Some(context) = request.context.as_ref() else {
        live.captured_at = Some(crate::clock::now());
        return live;
    };
    live.rig_state = context.rig_state.clone();
    live.visible_mode = Some(context.interaction_mode.as_str().to_string());
    if let Some(rig) = live.rig_state.as_ref() {
        live.speaking = rig.speaking;
        live.face_visible = rig.face_visible;
        live.motion_intent = rig.acting.intent.clone();
        live.speech_intent = if rig.speaking {
            Some("speech".into())
        } else {
            Some("idle".into())
        };
        live.speech_interruptible = rig.speaking;
    }
    if let Some(data) = context.custom_data.as_ref() {
        apply_whitelisted_presence(&mut live, data);
    }
    live.captured_at = Some(crate::clock::now());
    // Saying something there is using it.
    live.changed_at = live.captured_at;
    live
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::AgentInteractionMode;
    use crate::services::agent::types::{RequestContext, UserRequest};

    fn page(id: &str, visible: bool, panel: bool, now: DateTime<Utc>) -> SelfLivePresence {
        let mut live = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "instanceId": id, "pageVisible": visible,
                "panelVisible": panel, "faceVisible": visible,
                "rigState": { "expression": id, "capabilities": ["head-body"] } },
            "perception": [{ "sourceId": "page", "kind": "page", "privacy": "consented",
                "summary": id, "ttlMs": 60000 }]
        }));
        live.captured_at = Some(now);
        live
    }

    #[test]
    fn hidden_tab_cannot_replace_visible_body_or_page_and_revival_is_aggregated() {
        let mut store = LivePresenceStore::default();
        let now = crate::clock::now();
        assert!(store.remember(1, page("front", true, true, now), now));
        assert!(!store.remember(1, page("back", false, false, now), now));
        let live = store.last(1, now + Duration::seconds(9));
        assert!(live.page_visible && live.panel_visible && live.face_visible);
        assert_eq!(live.instance_id.as_deref(), Some("front"));
        assert_eq!(live.perception, vec!["front"]);
        assert_eq!(live.perception_payload[0]["ttlMs"], 51000);
        assert!(!store.remember(1, page("back", true, false, now), now));
        assert!(!store.remember(1, page("front", false, true, now), now));
        let live = store.last(1, now);
        assert!(live.page_visible && !live.panel_visible);
        assert_eq!(live.instance_id.as_deref(), Some("back"));
        assert_eq!(live.perception, vec!["back"]);
        assert!(!store.remember(1, page("back", false, false, now), now));
        assert!(!store.last(1, now).page_visible);
        assert!(store.remember(1, page("front", true, true, now), now));
    }

    #[test]
    fn leases_expire_independently_and_reads_do_not_extend_observation_ttl() {
        let mut store = LivePresenceStore::default();
        let now = crate::clock::now();
        store.remember(1, page("front", true, true, now), now);
        let later = now + Duration::seconds(PRESENCE_WINDOW_SECS + 1);
        store.remember(1, page("back", false, false, later), later);
        assert!(!store.last(1, later).page_visible);
        assert_eq!(store.users[&1].len(), 1);
        assert!(
            store
                .last(1, later + Duration::seconds(61))
                .perception
                .is_empty()
        );
        assert!(
            !store
                .last(1, later + Duration::seconds(PRESENCE_WINDOW_SECS + 1))
                .speaking
        );
    }

    #[test]
    fn revocation_clears_the_selected_page_without_reusing_other_tabs_content() {
        let mut store = LivePresenceStore::default();
        let now = crate::clock::now();
        store.remember(1, page("front", true, true, now), now);
        store.remember(1, page("back", true, false, now), now);
        let mut revoked = page("front", true, true, now);
        revoked.perception_payload.clear();
        revoked.rig_state = None;
        store.remember(1, revoked, now);
        assert!(store.last(1, now).perception.is_empty());
        assert!(store.last(1, now).rig_state.is_none());
        assert!(store.last(2, now).perception.is_empty());
    }

    #[test]
    fn someone_on_the_app_is_here_but_not_on_the_sites_face() {
        let mut store = LivePresenceStore::default();
        let now = crate::clock::now();
        let mut native = page("mac", true, true, now);
        native.instance_id = Some(native_instance("dev_a"));
        native.surface = LiveSurface::Native;
        // Arriving on the Mac app is arriving, and its open panel is her panel open.
        assert!(store.remember(1, native, now));
        assert_eq!(
            store.here(1, now),
            Here {
                present: true,
                panel_open: true
            }
        );
        // The site's face is not on screen: its body, rig and perception stay the pages'.
        assert!(!store.last(1, now).page_visible && !store.last(1, now).face_visible);
        let on_mac = store.last_on(1, LiveSurface::Native, now);
        assert_eq!(on_mac.perception, vec!["mac"]);
        // Already here: a page opening now is no second arrival.
        assert!(!store.remember(1, page("front", true, false, now), now));
        assert_eq!(store.last(1, now).perception, vec!["front"]);
        assert!(!store.last(1, now).panel_visible);
        assert!(
            store.here(1, now).panel_open,
            "the app's panel still counts"
        );
        // The app in the background: here on the page, no panel open.
        let mut hidden = page("mac", false, true, now);
        hidden.instance_id = Some(native_instance("dev_a"));
        hidden.surface = LiveSurface::Native;
        store.remember(1, hidden, now);
        assert_eq!(
            store.here(1, now),
            Here {
                present: true,
                panel_open: false
            }
        );
        // The same lease as a page: once it runs out, nobody is here.
        let later = now + Duration::seconds(PRESENCE_WINDOW_SECS + 1);
        assert_eq!(store.here(1, later), Here::default());

        // Through the link's entry points: a page cannot pose as the child,
        // and the link closing ends it.
        let user = 1_930_777;
        assert!(remember_native_presence(
            user,
            "dev_b",
            &serde_json::json!({
                "presence": { "instanceId": "native:dev_b", "pageVisible": true, "panelVisible": true }
            }),
        ));
        assert!(owner_here(user).present && live_presence_panel_open(user));
        assert!(present_users().contains(&user));
        assert!(!last_live_presence(user).page_visible);
        // Sent again, it is no new arrival.
        assert!(!remember_native_presence(
            user,
            "dev_b",
            &serde_json::json!({ "presence": { "pageVisible": true, "panelVisible": true } }),
        ));
        remember_live_presence(
            user,
            live_presence_from_custom_data(&serde_json::json!({
                "presence": { "instanceId": "native:dev_b", "pageVisible": false }
            })),
        );
        assert!(
            live_presence_panel_open(user),
            "a page cannot close the app's panel"
        );
        forget_native_presence(user, "dev_b");
        assert!(!owner_here(user).present && !live_presence_panel_open(user));
        assert!(!present_users().contains(&user));
    }

    #[test]
    fn the_surface_in_use_is_the_one_they_came_to_or_used_last() {
        let mut store = LivePresenceStore::default();
        let now = crate::clock::now();
        let app = |visible: bool, panel: bool, at| {
            let mut live = page("mac", visible, panel, at);
            live.instance_id = Some(native_instance("dev_front"));
            live.surface = LiveSurface::Native;
            live
        };
        assert!(store.front(1, now).is_none());
        store.remember(1, page("tab", true, false, now), now);
        let later = now + Duration::seconds(5);
        store.remember(1, app(true, false, later), later);
        let front = store.front(1, later).unwrap();
        assert_eq!(front.surface, LiveSurface::Native);
        assert_eq!(front.device_id.as_deref(), Some("dev_front"));
        assert_eq!(front.live.perception, vec!["mac"]);
        // The page reporting the same again is not coming back to it.
        let again = later + Duration::seconds(5);
        store.remember(1, page("tab", true, false, again), again);
        assert_eq!(store.front(1, again).unwrap().surface, LiveSurface::Native);
        // Opening her panel there is.
        store.remember(1, page("tab", true, true, again), again);
        let front = store.front(1, again).unwrap();
        assert_eq!((front.surface, front.device_id), (LiveSurface::Web, None));
        // A tie goes to the page; the app gone, the page is all there is.
        let tie = again + Duration::seconds(5);
        store.remember(1, app(false, false, tie), tie);
        store.remember(1, app(true, true, tie), tie);
        assert_eq!(store.front(1, tie).unwrap().surface, LiveSurface::Native);
        let mut said = page("tab", true, true, tie);
        said.changed_at = Some(tie);
        store.remember(1, said, tie);
        assert_eq!(store.front(1, tie).unwrap().surface, LiveSurface::Web);
        store.remember(1, app(false, false, tie), tie);
        assert_eq!(store.front(1, tie).unwrap().surface, LiveSurface::Web);
    }

    #[test]
    fn the_app_with_its_link_up_is_here_whatever_its_window_shows() {
        let mut store = LivePresenceStore::default();
        let now = crate::clock::now();
        let app = |visible: bool, at| {
            let mut live = page("mac", visible, visible, at);
            live.instance_id = Some(native_instance("dev_d16"));
            live.surface = LiveSurface::Native;
            live
        };
        // Launched behind other windows: arriving all the same, her panel not open.
        assert!(store.remember(1, app(false, now), now));
        assert_eq!(
            store.here(1, now),
            Here {
                present: true,
                panel_open: false
            }
        );
        let front = store.front(1, now).unwrap();
        assert_eq!(front.surface, LiveSurface::Native);
        // What it reported stays as it was reported.
        assert!(!front.live.page_visible && !front.live.face_visible);
        assert!(can_speak_to(&front.live));
        // A hidden page is not here, and cannot be spoken to.
        let hidden_tab = page("tab", false, false, now);
        assert!(!can_speak_to(&hidden_tab));

        // Their window comes up, then a page is opened later: the page is in use.
        let t1 = now + Duration::seconds(1);
        store.remember(1, app(true, t1), t1);
        let t2 = now + Duration::seconds(2);
        store.remember(1, page("tab", true, false, t2), t2);
        assert_eq!(store.front(1, t2).unwrap().surface, LiveSurface::Web);
        // The app's window going away is not using it: the page stays in use.
        let t3 = now + Duration::seconds(3);
        store.remember(1, app(false, t3), t3);
        assert_eq!(store.front(1, t3).unwrap().surface, LiveSurface::Web);
        // Back to the app's window: the app again; hiding it later keeps it.
        let t4 = now + Duration::seconds(4);
        store.remember(1, app(true, t4), t4);
        let t5 = now + Duration::seconds(5);
        store.remember(1, app(false, t5), t5);
        assert_eq!(store.front(1, t5).unwrap().surface, LiveSurface::Native);
        // The lease still ends it: no link refresh, nobody here.
        store.remember(1, page("tab", false, false, t5), t5);
        let later = t5 + Duration::seconds(PRESENCE_WINDOW_SECS + 1);
        assert_eq!(store.here(1, later), Here::default());
        assert!(store.front(1, later).is_none());

        // Through the link's entry points, and the rig she acts with there.
        let user = 1_930_916;
        assert!(remember_native_presence(
            user,
            "dev_d16",
            &serde_json::json!({ "presence": { "pageVisible": false, "faceVisible": false,
                "rigState": { "pageVisible": false, "faceVisible": false, "expression": "steady" } } }),
        ));
        assert!(owner_here(user).present && !live_presence_panel_open(user));
        assert!(present_users().contains(&user));
        let rig = acting_rig_state(user).unwrap();
        assert!(rig.page_visible && rig.face_visible);
        let reported = front_presence(user).unwrap().live;
        let reported_rig = reported.rig_state.unwrap();
        assert!(!reported_rig.page_visible && !reported_rig.face_visible);
        assert_eq!(reported_rig.expression, rig.expression);
        assert!(!reported.page_visible);
        forget_native_presence(user, "dev_d16");
        assert!(!owner_here(user).present && acting_rig_state(user).is_none());
    }

    #[test]
    fn page_keys_are_bounded_and_not_exposed_to_the_decision_model() {
        let now = crate::clock::now();
        let mut store = LivePresenceStore::default();
        for i in 0..100 {
            store.remember(1, page(&format!("page-{i}"), false, false, now), now);
        }
        assert_eq!(store.users[&1].len(), MAX_USER_PAGES);
        let live = page("page-key", true, true, now);
        assert_eq!(live.instance_id.as_deref(), Some("page-key"));
        assert!(
            serde_json::to_value(&live)
                .unwrap()
                .get("instanceId")
                .is_none()
        );
        let invalid = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "instanceId": "x".repeat(65) }
        }));
        assert!(invalid.instance_id.is_none());
    }

    #[test]
    fn inbound_replaces_chat_rig_with_current_sanitized_state() {
        let old_rig = myriad_merope::sanitize_rig_state(&serde_json::json!({
            "capabilities": ["head-body", "cry-eye"], "expression": "warm"
        }))
        .unwrap();
        remember_live_presence(
            906,
            SelfLivePresence {
                rig_state: Some(old_rig),
                captured_at: Some(crate::clock::now()),
                ..Default::default()
            },
        );
        let inbound = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "pageVisible": true, "faceVisible": true,
                "rigState": { "expression": "tense", "posture": "closed",
                    "capabilities": ["head-body"], "driver": { "headX": 99 } }
            }
        }));
        remember_live_presence(906, inbound);
        let rig = last_live_presence(906).rig_state.unwrap();
        assert_eq!(rig.expression, "tense");
        assert_eq!(rig.posture, "closed");
        assert_eq!(rig.capabilities, vec!["head-body"]);
        assert!(!serde_json::to_string(&rig).unwrap().contains("driver"));
        // A new report with no rig is not permission to keep a previous body's capabilities.
        remember_live_presence(
            906,
            live_presence_from_custom_data(&serde_json::json!({
                "presence": { "pageVisible": true, "faceVisible": false, "rigState": null }
            })),
        );
        assert!(last_live_presence(906).rig_state.is_none());
    }

    /// What she sees and hears of them is where they are using her: the app's
    /// when that is where they are, the pages' otherwise; a turn said on the
    /// app is answered with the app's.
    #[test]
    fn her_senses_follow_them_to_the_app() {
        let user = 9_120_301;
        let now = crate::clock::now();
        // Only a hidden page: the pages' senses, as before.
        remember_live_presence(user, page("tab", false, false, now));
        assert_eq!(senses_presence(user).perception, vec!["tab"]);
        assert!(native_senses(user, "dev_senses").is_none());
        remember_native_presence(
            user,
            "dev_senses",
            &serde_json::json!({
                "presence": { "pageVisible": true, "faceVisible": true },
                "perception": [{ "sourceId": "music_track", "kind": "music",
                    "privacy": "consented", "summary": "晴天 — 周杰伦", "ttlMs": 60000 }],
                "musicStatus": { "isPlaying": true, "isEnabled": true,
                    "currentSong": { "name": "晴天", "artist": "周杰伦" } }
            }),
        );
        let senses = senses_presence(user);
        assert_eq!(senses.perception, vec!["晴天 — 周杰伦"]);
        assert_eq!(senses.music_status.unwrap()["currentSong"]["name"], "晴天");
        let turn = native_senses(user, "dev_senses").expect("the app's senses");
        assert_eq!(turn["perception"][0]["sourceId"], "music_track");
        assert_eq!(turn["musicStatus"]["isPlaying"], true);
        let scene = crate::services::agent::chat_prompt::format_chat_scene(
            turn.get("perception"),
            None,
            "在听什么",
        );
        assert!(scene.contains("晴天"), "{scene}");
        // They come back to a page: its senses again.
        let later = now + Duration::seconds(2);
        LIVE.write()
            .unwrap()
            .remember(user, page("tab", true, true, later), later);
        assert_eq!(senses_presence(user).perception, vec!["tab"]);
        forget_native_presence(user, "dev_senses");
        assert!(native_senses(user, "dev_senses").is_none());
    }

    /// A turn said on the app is not a page: it leaves the pages' lease alone.
    #[test]
    fn a_turn_said_on_the_app_is_not_a_page() {
        let user = 9_120_302;
        let mut request = UserRequest {
            raw_input: "hi".into(),
            timestamp: crate::clock::now(),
            user_id: user,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                on_device: Some("dev_turn".into()),
                ..Default::default()
            }),
        };
        remember_turn_presence(&request);
        assert!(last_live_presence(user).captured_at.is_none());
        request.context.as_mut().unwrap().on_device = None;
        remember_turn_presence(&request);
        assert!(last_live_presence(user).captured_at.is_some());
    }

    #[test]
    fn live_presence_does_not_invent_grants() {
        let request = UserRequest {
            raw_input: "hi".into(),
            timestamp: crate::clock::now(),
            user_id: 7,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                custom_data: Some(serde_json::json!({
                    "presence": {
                        "speaking": true,
                        "speechInterruptible": true,
                        "visibleMode": "chat",
                        "speechIntent": "tts"
                    },
                    "perception": [{
                        "sourceId": "music", "kind": "music", "ttlMs": 2000,
                        "privacy": "system", "summary": "music idle"
                    }]
                })),
                ..Default::default()
            }),
        };
        let live = live_presence_from_request(&request);
        assert!(live.speaking);
        assert_eq!(live.visible_mode.as_deref(), Some("chat"));
        assert_eq!(live.perception, vec!["music idle"]);
        remember_live_presence(7, live.clone());
        assert_eq!(last_live_presence(7).speech_intent.as_deref(), Some("tts"));
        assert!(last_live_presence(7).captured_at.is_some());
    }

    #[test]
    fn last_live_presence_expires_stale_facts() {
        let mut live = SelfLivePresence {
            speaking: true,
            perception: vec!["hours-old".into()],
            perception_payload: vec![serde_json::json!({
                "sourceId": "page", "kind": "page", "privacy": "consented",
                "summary": "hours-old", "ttlMs": 90_000,
            })],
            captured_at: Some(crate::clock::now() - Duration::seconds(120)),
            ..Default::default()
        };
        remember_live_presence(91, live.clone());
        let expired = last_live_presence(91);
        assert!(!expired.speaking);
        assert!(expired.perception.is_empty());

        live.captured_at = Some(crate::clock::now() - Duration::seconds(60));
        remember_live_presence(91, live.clone());
        let fresh = last_live_presence(91);
        assert!(fresh.speaking);
        assert_eq!(fresh.perception, vec!["hours-old"]);
    }

    #[test]
    fn live_presence_honors_local_perception_privacy() {
        let request = UserRequest {
            raw_input: "hi".into(),
            timestamp: crate::clock::now(),
            user_id: 93,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                custom_data: Some(serde_json::json!({
                    "perception": [{
                        "sourceId": "pointer",
                        "kind": "pointer",
                        "ttlMs": 2000,
                        "privacy": "local",
                        "summary": "SECRET_PAGE_BODY",
                        "safeFacts": { "route": "/inbox" }
                    }]
                })),
                ..Default::default()
            }),
        };
        let live = live_presence_from_request(&request);
        assert!(
            !live
                .perception
                .iter()
                .any(|line| line.contains("SECRET_PAGE_BODY")),
            "local summary must not reach the autonomy read path"
        );
        assert_eq!(live.perception, vec!["route=/inbox"]);
    }

    #[test]
    fn live_presence_keeps_consented_perception_summary() {
        let request = UserRequest {
            raw_input: "hi".into(),
            timestamp: crate::clock::now(),
            user_id: 94,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                custom_data: Some(serde_json::json!({
                    "perception": [{
                        "sourceId": "page",
                        "kind": "page",
                        "ttlMs": 8000,
                        "privacy": "consented",
                        "summary": "Hello article",
                        "safeFacts": { "title": "Hello" }
                    }]
                })),
                ..Default::default()
            }),
        };
        let live = live_presence_from_request(&request);
        assert_eq!(live.perception, vec!["Hello article"]);
    }

    #[test]
    fn live_presence_keeps_more_than_eight_perception_rows() {
        let rows: Vec<_> = (0..9)
            .map(|i| {
                serde_json::json!({
                    "sourceId": format!("presence-{i}"),
                    "kind": "presence",
                    "ttlMs": 4000,
                    "privacy": "consented",
                    "summary": format!("src-{i}"),
                })
            })
            .collect();
        let request = UserRequest {
            raw_input: "hi".into(),
            timestamp: crate::clock::now(),
            user_id: 95,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                custom_data: Some(serde_json::json!({ "perception": rows })),
                ..Default::default()
            }),
        };
        let live = live_presence_from_request(&request);
        assert_eq!(live.perception.len(), 9);
        assert_eq!(live.perception[8], "src-8");
    }

    #[test]
    fn last_live_presence_treats_missing_timestamp_as_expired() {
        remember_live_presence(
            92,
            SelfLivePresence {
                speaking: true,
                captured_at: None,
                ..Default::default()
            },
        );
        let expired = last_live_presence(92);
        assert!(!expired.speaking);
        assert!(expired.perception.is_empty());
    }

    #[test]
    fn custom_data_whitelist_drops_unknown_keys_and_raw_perception() {
        let live = live_presence_from_custom_data(&serde_json::json!({
            "presence": {
                "speaking": true,
                "pageVisible": true,
                "panelVisible": true,
                "__admin": true
            },
            "perception": [{
                "sourceId": "presence", "kind": "presence", "ttlMs": 4000,
                "privacy": "system", "summary": "x", "raw": "<pixels>"
            }],
            "secret": "nope"
        }));
        assert!(live.speaking);
        assert!(live.page_visible);
        assert!(live.panel_visible);
        let encoded = serde_json::to_value(&live).unwrap();
        assert!(encoded.get("__admin").is_none());
        assert_eq!(
            encoded.get("perception").and_then(|v| v.as_array()),
            Some(&vec![serde_json::json!("x")])
        );
        assert!(encoded.get("secret").is_none());
        assert!(!encoded.to_string().contains("__admin"));
        assert!(!encoded.to_string().contains("<pixels>"));
    }

    #[test]
    fn custom_data_whitelist_truncates_perception_count_and_length() {
        let cap = crate::services::agent::perception_view::MAX_PERCEPTION_ITEMS;
        let long: String = "s".repeat(161);
        let mut items = Vec::new();
        for i in 0..15 {
            items.push(serde_json::json!({
                "sourceId": format!("presence-{i}"),
                "kind": "presence",
                "ttlMs": 4000,
                "summary": format!("{long}-{i}"),
                "privacy": "consented"
            }));
        }
        let live = live_presence_from_custom_data(&serde_json::json!({
            "perception": items
        }));
        assert_eq!(live.perception.len(), cap);
        assert!(live.perception.iter().all(|row| row.chars().count() == 160));
    }

    #[test]
    fn remembering_custom_data_stamps_fresh_captured_at() {
        let live = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "speaking": true }
        }));
        remember_live_presence(95, live);
        let stored = last_live_presence(95);
        assert!(stored.speaking);
        let captured = stored.captured_at.expect("fresh timestamp");
        assert!(crate::clock::now().signed_duration_since(captured) < Duration::seconds(2));
    }

    #[test]
    fn remember_reports_revival_onto_the_page() {
        let live = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "pageVisible": true }
        }));
        assert!(remember_live_presence(96, live.clone()));
        assert!(last_live_presence(96).page_visible);
        assert!(!live_presence_panel_open(96));
        assert!(!remember_live_presence(96, live));
    }

    #[test]
    fn panel_open_requires_the_page_and_the_panel() {
        let on_page = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "pageVisible": true, "panelVisible": true }
        }));
        remember_live_presence(97, on_page);
        assert!(last_live_presence(97).page_visible);
        assert!(live_presence_panel_open(97));
        let page_only = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "pageVisible": true, "panelVisible": false }
        }));
        remember_live_presence(97, page_only);
        assert!(last_live_presence(97).page_visible);
        assert!(!live_presence_panel_open(97));
    }
}
