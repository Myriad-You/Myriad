//! What the host counts of what does not get through on her link, for the
//! status page: keys of a look at the screen it deleted (`screenFields`),
//! per frame type and key, and the frames a child told it dropped
//! (`link.dropped`), per device, frame type and reason, and of those the
//! reliable ones its ack had covered (not read, though acked), the
//! `log.vector` frames of her log that got no page (`log.pages`), per
//! device and why, when each page still on its way was sent, per device
//! and audience kind, and the links closed for a page of the device's that
//! could not be settled, in a row and in all. Counts and when, never a
//! value.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};

use super::contract::CONTRACT;

/// How many times, and the last.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Count {
    pub(crate) count: u64,
    pub(crate) last_at: Option<DateTime<Utc>>,
}

impl Count {
    fn add(&mut self, at: DateTime<Utc>) {
        self.count += 1;
        self.last_at = Some(at);
    }
}

/// A frame type the host does not send: what a child names it by.
pub(crate) const UNNAMED: &str = "unnamed";

/// A cap on the devices counted, so a misbehaving end cannot grow it.
const DEVICES_AT_MOST: usize = 64;

#[derive(Default)]
struct Counts {
    /// (frame type, key) → deleted.
    screen_keys: BTreeMap<(&'static str, String), Count>,
    /// device → (frame type, reason) → told.
    dropped: BTreeMap<String, BTreeMap<(&'static str, &'static str), Count>>,
    /// device → reliable frame type → acked by the child but not read.
    unread: BTreeMap<String, BTreeMap<&'static str, Count>>,
    /// device → why → log.vector frames that got no page.
    pages: BTreeMap<String, BTreeMap<&'static str, Count>>,
    /// device → audience → the link that sent the page of it on its way,
    /// and when.
    open: BTreeMap<String, BTreeMap<String, (u64, DateTime<Utc>)>>,
    /// device → links closed for a page that could not be settled.
    unsettled: BTreeMap<String, Unsettled>,
}

/// Links of a device closed for a page of its that could not be settled:
/// how many in a row (since one was last named back), and in all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Unsettled {
    pub(crate) in_a_row: u64,
    pub(crate) closes: Count,
}

static COUNTS: LazyLock<Mutex<Counts>> = LazyLock::new(Mutex::default);

fn with<T>(f: impl FnOnce(&mut Counts) -> T) -> T {
    match COUNTS.lock() {
        Ok(mut counts) => f(&mut counts),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
}

/// Whether a look at the screen may say this key (`screenFields.facts`).
pub(super) fn screen_fact(key: &str) -> bool {
    CONTRACT.screen_fields.facts.iter().any(|fact| fact == key)
}

/// A key of a look at the screen was deleted from a frame of type `t`.
/// Only keys the contract could name are kept as themselves, so a child
/// cannot grow this with keys of its own making. The first time for a
/// frame type and key is logged (its name, never its value).
pub(super) fn screen_key_deleted(t: &'static str, key: &str) {
    let key = if super::contract::valid_id(key) && key.len() <= 40 {
        key
    } else {
        UNNAMED
    };
    let now = crate::clock::now();
    let first = with(|counts| {
        let count = counts.screen_keys.entry((t, key.to_string())).or_default();
        count.add(now);
        count.count == 1
    });
    // Once per frame type and key while the host runs: that it happens, never
    // the value.
    if first {
        tracing::info!(t, key, "[link] a key of a look at the screen was deleted");
    }
}

/// A child dropped a frame (`link.dropped`): `t` counted as itself only
/// when it is a frame type the host sends, `reason` only when it is one of
/// `dropReasons`.
pub(super) fn dropped(device_id: &str, t: &str, reason: &str) {
    let Some(reason) = CONTRACT
        .drop_reasons
        .iter()
        .find(|known| *known == reason)
        .map(String::as_str)
    else {
        return;
    };
    let t = CONTRACT
        .frames
        .get_key_value(t)
        .filter(|(_, spec)| matches!(spec.from.as_str(), "host" | "either"))
        .map_or(UNNAMED, |(name, _)| name.as_str());
    let now = crate::clock::now();
    with(|counts| {
        if !counts.dropped.contains_key(device_id) && counts.dropped.len() >= DEVICES_AT_MOST {
            return;
        }
        counts
            .dropped
            .entry(device_id.to_string())
            .or_default()
            .entry((t, reason))
            .or_default()
            .add(now);
    });
}

/// A reliable frame of type `t` the child's ack covered was one it could
/// not read (`link.dropped` named its type right after).
pub(super) fn unread(device_id: &str, t: &'static str) {
    let now = crate::clock::now();
    with(|counts| {
        if !counts.unread.contains_key(device_id) && counts.unread.len() >= DEVICES_AT_MOST {
            return;
        }
        counts
            .unread
            .entry(device_id.to_string())
            .or_default()
            .entry(t)
            .or_default()
            .add(now);
    });
}

/// A `log.vector` from a device got no page of her log: it named a page
/// not on its way (`no_such_page`), or the page it named brought nothing it
/// did not hold (`no_headway`); or rows it asked again for were past what
/// a link keeps (`ask_dropped`). Only those words are counted.
pub(crate) fn page_passed(device_id: &str, why: &'static str) {
    if !matches!(why, "no_such_page" | "no_headway" | "ask_dropped") {
        return;
    }
    let now = crate::clock::now();
    with(|counts| {
        if !counts.pages.contains_key(device_id) && counts.pages.len() >= DEVICES_AT_MOST {
            return;
        }
        counts
            .pages
            .entry(device_id.to_string())
            .or_default()
            .entry(why)
            .or_default()
            .add(now);
    });
}

/// A page of `audience` went to a device on the link numbered `link`: on
/// its way until it is named, let go, or that link closes.
pub(crate) fn page_opened(device_id: &str, link: u64, audience: &str) {
    let now = crate::clock::now();
    with(|counts| {
        if !counts.open.contains_key(device_id) && counts.open.len() >= DEVICES_AT_MOST {
            return;
        }
        counts
            .open
            .entry(device_id.to_string())
            .or_default()
            .insert(audience.to_string(), (link, now));
    });
}

/// The page of `audience` the link numbered `link` sent a device is not on
/// its way any more. One another link of the device sent stays.
pub(crate) fn page_closed(device_id: &str, link: u64, audience: &str) {
    forget_open(device_id, |at, (by, _)| by == link && at == audience);
}

/// The link numbered `link` of a device closed: no page it sent is on its
/// way. Those of a link that took over from it stay.
pub(crate) fn pages_closed(device_id: &str, link: u64) {
    forget_open(device_id, |_, (by, _)| by == link);
}

fn forget_open(device_id: &str, gone: impl Fn(&str, (u64, DateTime<Utc>)) -> bool) {
    with(|counts| {
        if let Some(open) = counts.open.get_mut(device_id) {
            open.retain(|audience, page| !gone(audience, *page));
            if open.is_empty() {
                counts.open.remove(device_id);
            }
        }
    });
}

/// A link of a device is to close: a page of the device's could not be
/// settled.
pub(crate) fn page_unsettled(device_id: &str) {
    let now = crate::clock::now();
    with(|counts| {
        if !counts.unsettled.contains_key(device_id) && counts.unsettled.len() >= DEVICES_AT_MOST {
            return;
        }
        let unsettled = counts.unsettled.entry(device_id.to_string()).or_default();
        unsettled.in_a_row += 1;
        unsettled.closes.add(now);
    });
}

/// A page of a device's was named back: closes since are a new row.
pub(crate) fn page_settled(device_id: &str) {
    with(|counts| {
        if let Some(unsettled) = counts.unsettled.get_mut(device_id) {
            unsettled.in_a_row = 0;
        }
    });
}

/// The links of a device closed for a page that could not be settled.
pub(crate) fn pages_unsettled_by(device_id: &str) -> Option<Unsettled> {
    with(|counts| counts.unsettled.get(device_id).copied())
}

/// When each page still on its way to a device was sent, by the kind of
/// its audience (`personal`, `own`, `group`), never whose.
pub(crate) fn pages_open_by(device_id: &str) -> Vec<(String, DateTime<Utc>)> {
    with(|counts| {
        counts
            .open
            .get(device_id)
            .map(|open| {
                open.iter()
                    .map(|(audience, (_, at))| {
                        (
                            audience.split(':').next().unwrap_or_default().to_string(),
                            *at,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The `log.vector` frames of one device that got no page, by why.
pub(crate) fn pages_passed_by(device_id: &str) -> Vec<(&'static str, Count)> {
    with(|counts| {
        counts
            .pages
            .get(device_id)
            .map(|kinds| kinds.iter().map(|(why, count)| (*why, *count)).collect())
            .unwrap_or_default()
    })
}

/// What one device acked of her reliable frames without reading it.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn unread_by(device_id: &str) -> Vec<(&'static str, Count)> {
    with(|counts| {
        counts
            .unread
            .get(device_id)
            .map(|kinds| kinds.iter().map(|(t, count)| (*t, *count)).collect())
            .unwrap_or_default()
    })
}

/// The screen keys deleted so far, per frame type and key.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn screen_keys() -> Vec<(&'static str, String, Count)> {
    with(|counts| {
        counts
            .screen_keys
            .iter()
            .map(|((t, key), count)| (*t, key.clone(), *count))
            .collect()
    })
}

/// What one device told it dropped, per frame type and reason.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn dropped_by(device_id: &str) -> Vec<(&'static str, &'static str, Count)> {
    with(|counts| {
        counts
            .dropped
            .get(device_id)
            .map(|kinds| {
                kinds
                    .iter()
                    .map(|((t, reason), count)| (*t, *reason, *count))
                    .collect()
            })
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_frame_is_counted_by_what_the_host_sends_and_the_contract_names() {
        let device = "counts_device_a";
        dropped(device, "say", "unknown_frame");
        dropped(device, "say", "unknown_frame");
        // Not a frame type the host sends, nor one at all: unnamed.
        dropped(device, "input", "bad_frame");
        dropped(device, "what_she_said_to_bob", "bad_frame");
        // A reason the contract does not name is not counted.
        dropped(device, "say", "tired");
        let told = dropped_by(device);
        let of = |t: &str, reason: &str| {
            told.iter()
                .find(|(at, why, _)| *at == t && *why == reason)
                .map(|(_, _, count)| count.count)
        };
        assert_eq!(of("say", "unknown_frame"), Some(2));
        assert_eq!(of(UNNAMED, "bad_frame"), Some(2));
        assert_eq!(told.len(), 2);
        assert!(dropped_by("counts_device_never").is_empty());
    }

    #[test]
    fn a_deleted_screen_key_is_counted_by_its_name_never_its_value() {
        screen_key_deleted("presence", "others_text");
        screen_key_deleted("presence", "a key with spaces");
        let keys = screen_keys();
        assert!(
            keys.iter().any(|(t, key, count)| *t == "presence"
                && key == "others_text"
                && count.count >= 1)
        );
        assert!(keys.iter().any(|(_, key, _)| key == UNNAMED));
        assert!(!keys.iter().any(|(_, key, _)| key.contains(' ')));
    }
}
