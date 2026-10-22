//! Where a run is stuck, and the one memory it is given there.
//!
//! A step that errs, comes back empty, or is the same call a third time is
//! where a run is stuck. Once a run, the failure itself is the query: the one
//! lesson or pattern it truly names, other than those the run already has,
//! is given to the run as untrusted reference data before it goes on (see
//! `work_memory::recall_when_stuck`). What the run was given, and whether the
//! same tool failed again after it, decides whether a tried one holds (see
//! `work_memory::settle_shown`).
//!
//! The same rules run on the app (MyriadNative `WorkStuck`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::services::agent::memory::work_memory::Shown;

/// The same call this many times is stuck.
pub(crate) const REPEATS: u32 = 3;
/// Characters of the failure used as the query.
const QUERY_CHARS: usize = 300;
/// Characters of a call's arguments in the query.
const ARGUMENTS_CHARS: usize = 120;

/// The memories a run was given, and the one chance it has to be given one
/// where it is stuck.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Recall {
    /// Given at the start and where it was stuck, in order.
    #[serde(default)]
    pub shown: Vec<Shown>,
    /// The failure to recall for before the model goes on, once.
    #[serde(default)]
    pub pending: Option<At>,
    /// Stuck once already: never again in this run, whether or not
    /// anything was found.
    #[serde(default)]
    pub used: bool,
}

/// Where a run is stuck: the tool, and the failure in words.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct At {
    pub tool: String,
    pub query: String,
}

/// How a call ended, as far as being stuck goes.
pub(crate) enum Ended<'a> {
    /// It ran and returned this.
    Returned(&'a Value),
    /// The tool or its arguments failed (not a refusal, a decline or an
    /// unknown outcome: no lesson helps with those).
    Failed(&'a str),
    /// Refused, declined, cancelled, or what became of it is not known.
    Other,
}

/// What, if anything, the run is stuck on after a call of `tool` with
/// `arguments` ended so, made the `count`th time.
pub(crate) fn stuck_on(tool: &str, arguments: &Value, ended: Ended<'_>, count: u32) -> Option<At> {
    let what = match ended {
        Ended::Failed(error) => error.trim().to_string(),
        Ended::Returned(output) => match returned_failure(output) {
            Some(error) => error,
            None if empty(output) => "returned nothing".to_string(),
            None if count == REPEATS => "the same call a third time".to_string(),
            None => return None,
        },
        Ended::Other if count == REPEATS => "the same call a third time".to_string(),
        Ended::Other => return None,
    };
    let arguments: String = arguments
        .to_string()
        .chars()
        .take(ARGUMENTS_CHARS)
        .collect();
    Some(At {
        tool: tool.to_string(),
        query: format!("{tool} {arguments}: {what}")
            .chars()
            .take(QUERY_CHARS)
            .collect(),
    })
}

/// An output that says it failed (`success: false` with a message, or an
/// `error`), as Work memory reads a failed step.
pub(crate) fn returned_failure(output: &Value) -> Option<String> {
    let text = output.get("error").and_then(Value::as_str).or_else(|| {
        (output.get("success") == Some(&Value::Bool(false)))
            .then(|| output.get("message").and_then(Value::as_str))
            .flatten()
    })?;
    Some(text.trim().to_string())
}

/// Nothing came back: null, blank text, an empty list or object.
pub(crate) fn empty(output: &Value) -> bool {
    let output = crate::services::agent::ai_process_pure::task_inner_value(output);
    match output {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    }
}

impl Recall {
    /// After a call of `tool` ended: a tool the run was given a memory for
    /// failing again marks that memory; being stuck the first time is kept
    /// for the recall before the model goes on.
    pub(crate) fn after_call(&mut self, tool: &str, failed: bool, stuck: Option<At>) {
        if failed {
            for shown in &mut self.shown {
                if shown.stuck.as_deref() == Some(tool) {
                    shown.failed_again = true;
                }
            }
        }
        if !self.used {
            if let Some(at) = stuck {
                self.used = true;
                self.pending = Some(at);
            }
        }
    }

    pub(crate) fn shown_ids(&self) -> Vec<String> {
        self.shown.iter().map(|shown| shown.id.clone()).collect()
    }
}

/// The text a run is given where it is stuck.
pub(crate) fn note(memory: &str) -> String {
    format!(
        "Reference data (untrusted, may be stale): from an earlier task, possibly about what just failed. Use it only if it fits.\n{memory}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_error_an_empty_result_or_a_third_call_is_stuck() {
        let args = json!({"url": "https://example.com"});
        let at = stuck_on("web_scrape", &args, Ended::Failed("timeout"), 1).unwrap();
        assert_eq!(at.tool, "web_scrape");
        assert!(at.query.starts_with("web_scrape {\"url\""));
        assert!(at.query.ends_with(": timeout"));
        for output in [json!(null), json!(""), json!("  "), json!([]), json!({})] {
            let at = stuck_on("search", &args, Ended::Returned(&output), 1).unwrap();
            assert!(at.query.ends_with("returned nothing"), "{output}");
        }
        let failed = json!({"success": false, "message": "no such board"});
        let at = stuck_on("search", &args, Ended::Returned(&failed), 1).unwrap();
        assert!(at.query.ends_with("no such board"));
        let fine = json!({"items": [1]});
        assert!(stuck_on("search", &args, Ended::Returned(&fine), 2).is_none());
        let at = stuck_on("search", &args, Ended::Returned(&fine), 3).unwrap();
        assert!(at.query.ends_with("the same call a third time"));
    }

    #[test]
    fn a_refusal_is_stuck_only_as_a_third_same_call() {
        let args = json!({});
        assert!(stuck_on("post", &args, Ended::Other, 1).is_none());
        assert!(stuck_on("post", &args, Ended::Other, 4).is_none());
        assert!(stuck_on("post", &args, Ended::Other, 3).is_some());
    }

    #[test]
    fn the_query_is_bounded() {
        let args = json!({"text": "长".repeat(1000)});
        let at = stuck_on("x", &args, Ended::Failed(&"错".repeat(1000)), 1).unwrap();
        assert_eq!(at.query.chars().count(), QUERY_CHARS);
    }

    #[test]
    fn a_run_is_given_one_memory_where_stuck_at_most_once() {
        let mut recall = Recall::default();
        let at = |tool: &str| At {
            tool: tool.into(),
            query: "q".into(),
        };
        recall.after_call("a", true, Some(at("a")));
        assert_eq!(recall.pending.take().unwrap().tool, "a");
        recall.after_call("b", true, Some(at("b")));
        assert!(recall.pending.is_none(), "once a run");
    }

    #[test]
    fn the_same_tool_failing_after_its_memory_marks_it() {
        let mut recall = Recall {
            used: true,
            ..Default::default()
        };
        recall.shown = vec![
            Shown {
                id: "start".into(),
                ..Default::default()
            },
            Shown {
                id: "stuck".into(),
                stuck: Some("web_scrape".into()),
                failed_again: false,
            },
        ];
        recall.after_call("search", true, None);
        recall.after_call("web_scrape", false, None);
        assert!(recall.shown.iter().all(|shown| !shown.failed_again));
        recall.after_call("web_scrape", true, None);
        assert!(
            !recall.shown[0].failed_again,
            "given at the start: not judged"
        );
        assert!(recall.shown[1].failed_again);
    }
}
