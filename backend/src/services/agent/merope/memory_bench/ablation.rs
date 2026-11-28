//! The P3 ablations: what each layer of her memory buys, found by running
//! the bench with that layer taken out. Each is a switch of the bench's
//! own, read where the bench calls production, and off unless set to `1`:
//! off, the bench calls what it always called, in the same order, and its
//! report is the same, byte for byte. Production reads none of them.
//!
//! - `MEROPE_MEMORY_BENCH_NO_NODES=1`: nothing is put on her nodes. A
//!   stretch is not cut onto them before it is gone over (so going over is
//!   offered no topics), what it kept is not put on them (nor, so, how
//!   their life stands: that is written there), and her upkeep neither
//!   sorts onto them nor goes over them. A private talk is still gone over
//!   from what was said, as production goes over one; answering reads her
//!   nodes as production does, and finds none.
//! - `MEROPE_MEMORY_BENCH_NO_NIGHTLY=1`: her nodes are not gone over at
//!   night (`memory_nodes::upkeep::run`). What going over placed nowhere is
//!   still sorted onto them, as production's hourly upkeep sorts it.
//! - `MEROPE_MEMORY_BENCH_NO_STANDING=1`: she answers without 「现在」, how
//!   their life stands: it is taken off her nodes as they are read for an
//!   answer, so it is neither shown every time nor among a node's lines,
//!   and what it would have stood for is shown as the facts it is
//!   (`MEROPE_MEMORY_BENCH_STANDING` is not read either). It is still
//!   written, and paid for, as production writes it: the bench cannot keep
//!   production from writing it without going around production's own
//!   functions.

use serde::Serialize;

use super::super::memory_nodes::context::NodeTurn;

/// Which layers are taken out of this run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Ablation {
    pub(super) no_nodes: bool,
    pub(super) no_nightly: bool,
    pub(super) no_standing: bool,
}

tokio::task_local! {
    /// What a test sets instead of the environment.
    static SET: Ablation;
}

/// This run's: as a test set it, else as the environment says.
pub(super) fn current() -> Ablation {
    SET.try_with(|set| *set).unwrap_or_else(|_| {
        let on = |name: &str| std::env::var(name).is_ok_and(|value| value.trim() == "1");
        Ablation {
            no_nodes: on("MEROPE_MEMORY_BENCH_NO_NODES"),
            no_nightly: on("MEROPE_MEMORY_BENCH_NO_NIGHTLY"),
            no_standing: on("MEROPE_MEMORY_BENCH_NO_STANDING"),
        }
    })
}

/// `work` run with `set` as this run's, whatever the environment says.
pub(super) async fn with<F: std::future::Future>(set: Ablation, work: F) -> F::Output {
    SET.scope(set, work).await
}

impl Ablation {
    /// Whether any layer is taken out.
    pub(super) fn any(self) -> bool {
        self != Self::default()
    }

    /// What a report says of it: nothing when no layer is taken out, so a
    /// run without one reports as it always did.
    pub(super) fn label(self) -> Option<serde_json::Value> {
        self.any().then(|| serde_json::json!(self))
    }

    /// Her nodes as read for an answer, without how their life stands when
    /// it is taken out: off every node's head, and what was kept beside it.
    pub(super) fn read(self, mut nodes: NodeTurn) -> NodeTurn {
        if self.no_standing {
            for node in &mut nodes.shown.nodes {
                without_standing(&mut node.head);
            }
            nodes.shown.stood.clear();
            nodes.shown.grounds.clear();
            nodes.shown.kept.clear();
        }
        nodes
    }
}

/// A node's head without how things stand on it: everything that reads
/// 「现在」 off her nodes reads it from there.
fn without_standing(head: &mut serde_json::Value) {
    if let Some(head) = head.as_object_mut() {
        head.remove("standing");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// None of them is on unless set to `1`, and a test's setting is what
    /// is read, whatever the environment says.
    #[tokio::test]
    async fn every_layer_is_in_unless_taken_out() {
        let none = with(Ablation::default(), async { current() }).await;
        assert_eq!(none, Ablation::default());
        assert!(!none.any());
        assert_eq!(none.label(), None);
        let standing = Ablation {
            no_standing: true,
            ..Default::default()
        };
        assert_eq!(with(standing, async { current() }).await, standing);
        assert!(standing.any());
        assert_eq!(
            standing.label(),
            Some(serde_json::json!({"noNodes": false, "noNightly": false, "noStanding": true}))
        );
    }

    /// Taken out, how things stand goes from a head and nothing else does.
    #[test]
    fn standing_taken_out_leaves_the_rest_of_a_head() {
        let mut head = serde_json::json!({
            "summary": "常去爬山",
            "standing": {"them-1": {"value": {"text": "每周去两次健身房"}}},
            "open": {"o1": {"text": "膝盖还疼"}},
        });
        without_standing(&mut head);
        assert_eq!(
            head,
            serde_json::json!({"summary": "常去爬山", "open": {"o1": {"text": "膝盖还疼"}}})
        );
        let mut none = serde_json::json!(null);
        without_standing(&mut none);
        assert_eq!(none, serde_json::json!(null));
    }
}
