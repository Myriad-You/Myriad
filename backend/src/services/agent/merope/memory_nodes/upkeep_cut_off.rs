//! What an upkeep cut off at its output limit leaves (节点整理被截断): by
//! node (or the inbox), ids only, what was newest on it then, how many
//! times in a row, and when. Cut off twice with nothing new on it, a node
//! waits for something new or a day, so the same node is not asked night
//! after night for nothing.

use sea_orm::DatabaseConnection;

use crate::services::runtime_registry as registry;

/// Where an upkeep cut off at its output limit is kept, by node (or the
/// inbox), ids only: what was newest on it then, how many times in a row,
/// and when.
pub(crate) const CUT_OFF_NAMESPACE: &str = "merope_memory_node_upkeep_cut_off";
/// Cut off this many times with nothing new on it, a node waits ...
const CUT_OFF_WAITS_AFTER: u32 = 2;
/// ... for something new, or this long.
const CUT_OFF_WAIT: i64 = 86_400;
/// How long what was cut off is kept at all.
const CUT_OFF_KEPT: i64 = 7 * 86_400;

/// A node's upkeep (or the inbox's) cut off at its output limit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct CutOff {
    /// The id of what was newest on it then.
    seen: String,
    /// Cut off this many times in a row with that newest.
    times: u32,
    /// When it last was, Unix seconds.
    at: i64,
}

impl CutOff {
    /// When it is next asked, if not now: how many times it was cut off,
    /// and when the wait is over.
    pub(super) fn waits(&self, seen: &str, now: i64) -> Option<(u32, i64)> {
        let until = self.at + CUT_OFF_WAIT;
        (self.seen == seen && self.times >= CUT_OFF_WAITS_AFTER && now < until)
            .then_some((self.times, until))
    }

    /// What is kept once it is cut off again, `seen` newest on it now.
    pub(super) fn again(kept: Option<Self>, seen: String, now: i64) -> Self {
        let times = match kept {
            Some(kept) if kept.seen == seen => kept.times + 1,
            _ => 1,
        };
        Self {
            seen,
            times,
            at: now,
        }
    }
}

pub(super) fn cut_off_key(user_id: i32, audience: &str, node: Option<&str>) -> String {
    format!("{user_id}:{audience}:{}", node.unwrap_or("inbox"))
}

/// The id of the newest row on `node`: what a change to it changes.
pub(super) fn newest_on(rows: &[myriad_persona_log::Row], node: &str) -> String {
    let object = format!("node:{node}");
    rows.iter()
        .filter(|row| row.object == object)
        .max_by(|left, right| left.hlc.cmp(&right.hlc))
        .map(|row| row.id.clone())
        .unwrap_or_default()
}

/// What `node`'s upkeep (the inbox's, none) was cut off with before, and
/// whether it waits now, told in one line when it does.
pub(super) async fn kept_cut_off(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    node: Option<&str>,
    seen: &str,
) -> (Option<CutOff>, bool) {
    // Unread, it was never cut off: asked as ever, the run not stopped.
    let kept: Option<CutOff> = match registry::get(
        db,
        CUT_OFF_NAMESPACE,
        &cut_off_key(user_id, audience, node),
    )
    .await
    {
        Ok(kept) => kept,
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] what upkeep was cut off with could not be read");
            None
        }
    };
    let now = crate::clock::now().timestamp();
    let waits = kept.as_ref().and_then(|kept| kept.waits(seen, now));
    if let Some((times, until)) = waits {
        tracing::info!(
            node = node.unwrap_or("inbox"),
            times,
            again_at = %chrono::DateTime::from_timestamp(until, 0).unwrap_or_default().to_rfc3339(),
            "[memory-nodes] upkeep waits: cut off at its output limit before"
        );
    }
    (kept, waits.is_some())
}

/// Keeps that `node`'s upkeep (the inbox's, none) was cut off once more.
pub(super) async fn keep_cut_off(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    node: Option<&str>,
    kept: Option<CutOff>,
    seen: String,
) {
    let now = crate::clock::now().timestamp();
    let cut = CutOff::again(kept, seen, now);
    let times = cut.times;
    let written = registry::put(
        db,
        CUT_OFF_NAMESPACE,
        &cut_off_key(user_id, audience, node),
        registry::RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &cut,
        now + CUT_OFF_KEPT,
    )
    .await;
    match written {
        Ok(_) => tracing::info!(
            node = node.unwrap_or("inbox"),
            times,
            "[memory-nodes] upkeep cut off at its output limit: nothing written, asked again later"
        ),
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] upkeep cut off, and that could not be kept")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CutOff;

    #[test]
    fn a_node_cut_off_twice_with_nothing_new_waits_a_day() {
        let now = 1_000_000;
        let once = CutOff::again(None, "r1".into(), now);
        assert_eq!(once.times, 1);
        assert_eq!(
            once.waits("r1", now + 1),
            None,
            "asked again at the next upkeep"
        );
        let twice = CutOff::again(Some(once), "r1".into(), now + 3_600);
        assert_eq!(twice.times, 2);
        assert_eq!(
            twice.waits("r1", now + 3_601),
            Some((2, now + 3_600 + super::CUT_OFF_WAIT))
        );
        assert_eq!(twice.waits("r2", now + 3_601), None, "something new on it");
        assert_eq!(
            twice.waits("r1", now + 3_600 + super::CUT_OFF_WAIT),
            None,
            "a day on, asked again"
        );
        // Cut off again on something new: counted from one.
        assert_eq!(CutOff::again(Some(twice), "r2".into(), now).times, 1);
    }
}
