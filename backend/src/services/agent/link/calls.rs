//! Her calls to a device (contract `calls`), the site being her host.
//!
//! A child that listed `calls` says after welcome what it can be asked to do
//! (`call.offer`: the declared layer, kept with its link only); the owner
//! approves per device and permission group (`calls_store`: the approved
//! layer, kept on the site only); what is granted is worked out from both
//! and the moment: offered, enabled here, of the version the contract names,
//! approved. The host tells it as `call.granted` before anything is asked on
//! that link, and again whenever it changes. Only she asks a device; a
//! device asks only her host, and the site provides nothing to be asked yet,
//! so every ask of a child's is answered failed.

use std::cell::RefCell;

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::calls_store;
use super::connection::Outgoing;

/// The actions she may ask a device to do now. The contract's other child
/// actions are known (a device's offer of them is kept) but not granted.
const ENABLED: &[&str] = &["screen.look"];

thread_local! {
    /// What this test's links enable instead of [`ENABLED`] (each test has
    /// its own thread).
    static ENABLED_IN_TEST: RefCell<Option<Vec<&'static str>>> = const { RefCell::new(None) };
}

/// Enables `actions` instead of [`ENABLED`] on this test's thread.
#[cfg(test)]
pub(super) fn enable_in_test(actions: &[&'static str]) {
    ENABLED_IN_TEST.with(|cell| *cell.borrow_mut() = Some(actions.to_vec()));
}

pub(super) fn enabled(action: &str) -> bool {
    ENABLED_IN_TEST
        .with(|cell| {
            cell.borrow()
                .as_ref()
                .map(|actions| actions.contains(&action))
        })
        .unwrap_or_else(|| ENABLED.contains(&action))
}

/// An action and its version, as `call.offer` and `call.granted` list them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Action {
    pub name: String,
    pub version: u64,
}

impl Action {
    fn json(&self) -> Value {
        json!({ "name": self.name, "version": self.version })
    }
}

/// What a `call.offer` says the device can do: actions of the contract that
/// a child does, each once. Any other is left out.
pub(super) fn offered(body: &Value) -> Vec<Action> {
    let mut offered: Vec<Action> = Vec::new();
    for one in body["actions"].as_array().into_iter().flatten() {
        let (Some(name), Some(version)) = (one["name"].as_str(), one["version"].as_u64()) else {
            continue;
        };
        let by_child = myriad_her_link::call_spec(name).is_some_and(|spec| spec.by == "child");
        if by_child && !offered.iter().any(|known| known.name == name) {
            offered.push(Action {
                name: name.to_string(),
                version,
            });
        }
    }
    offered
}

/// Of what was offered, what is granted by what the owner decided.
pub(super) fn granted(offered: &[Action], decided: &[(String, bool)]) -> Vec<Action> {
    offered
        .iter()
        .filter(|action| {
            enabled(&action.name)
                && myriad_her_link::call_spec(&action.name).is_some_and(|spec| {
                    spec.version == action.version && calls_store::approved(decided, &spec.group)
                })
        })
        .cloned()
        .collect()
}

/// `call.granted`: what it may be asked to do, and what it may ask of the
/// host (nothing yet).
pub(super) fn granted_frame(granted: &[Action]) -> Outgoing {
    Outgoing::latest(
        "call.granted",
        json!({
            "actions": granted.iter().map(Action::json).collect::<Vec<_>>(),
            "providing": [],
        }),
    )
}

/// What is granted on one device's link now, from what it offered there
/// and what the owner decided. Read failing grants nothing.
async fn work_out(db: &DatabaseConnection, device_id: &str, offered: &[Action]) -> Vec<Action> {
    match calls_store::decided(db, device_id).await {
        Ok(decided) => granted(offered, &decided),
        Err(error) => {
            tracing::warn!(%error, device = %device_id, "[link] what the owner approved it to be asked could not be read; nothing is granted");
            Vec::new()
        }
    }
}

/// The child on link `id` said what it can be asked to do: kept with the
/// link, and what is granted is told it.
pub(super) async fn offer(db: &DatabaseConnection, device_id: &str, id: u64, offered: Vec<Action>) {
    let Some(turn) = super::calls_offered(device_id, id, offered.clone()) else {
        return;
    };
    let granted = work_out(db, device_id, &offered).await;
    super::grant_calls(device_id, id, turn, granted);
}

/// What the owner approved for `device_id` changed: what it is granted is
/// worked out again and, when it changed, told it on its link.
pub(crate) async fn grants_changed(db: &DatabaseConnection, owner: i32, device_id: &str) {
    let Some((id, turn, offered)) = super::calls_regrant(owner, device_id) else {
        return;
    };
    let granted = work_out(db, device_id, &offered).await;
    super::grant_calls(device_id, id, turn, granted);
}

// Work asks through these next.
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use super::calls_pending::{Answer, ask, sweep};

/// What her calls of `action` are counted as on her status page.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn kind(action: &str) -> &'static str {
    ENABLED
        .iter()
        .copied()
        .find(|enabled| *enabled == action)
        .unwrap_or("call.other")
}

/// The permission groups the owner is shown for a device: those of an
/// action enabled here, each once.
pub(crate) fn shown_groups() -> Vec<&'static str> {
    let mut groups: Vec<&'static str> = Vec::new();
    for action in ENABLED {
        if let Some(spec) = myriad_her_link::call_spec(action)
            && enabled(action)
            && !groups.contains(&spec.group.as_str())
        {
            groups.push(spec.group.as_str());
        }
    }
    groups
}

/// The groups the owner is shown for a device, and whether each is
/// approved: `[{group, approved}]`.
pub(crate) async fn groups_json(
    db: &DatabaseConnection,
    device_id: &str,
) -> Result<Value, sea_orm::DbErr> {
    let decided = calls_store::decided(db, device_id).await?;
    Ok(shown_groups()
        .into_iter()
        .map(|group| json!({ "group": group, "approved": calls_store::approved(&decided, group) }))
        .collect())
}

/// The owner approves or takes back `group` for a live device of theirs,
/// and the device is told what it is granted now. `false` when it is not
/// one.
pub(crate) async fn set_approved(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    group: &str,
    approved: bool,
) -> Result<bool, sea_orm::DbErr> {
    if !calls_store::set_approved(db, owner, device_id, group, approved).await? {
        return Ok(false);
    }
    grants_changed(db, owner, device_id).await;
    Ok(true)
}

/// A child asked the host to do something (`call.ask` up). The site provides
/// nothing yet: an action the host does is not granted it, anything else
/// the host does not do. Answered with the one `call.result` every ask gets.
pub(super) fn answer_ask(body: &Value) -> Option<Outgoing> {
    let call_id = body["callId"].as_str()?;
    let action = body["action"].as_str()?;
    let version = body["version"].as_u64()?;
    let code = match myriad_her_link::call_spec(action) {
        Some(spec) if spec.by == "host" => "call_not_granted",
        _ => "call_unknown",
    };
    Some(Outgoing::reliable(
        "call.result",
        format!("{call_id}.result"),
        json!({
            "callId": call_id,
            "action": action,
            "version": version,
            "outcome": "failed",
            "error": { "code": code },
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(name: &str, version: u64) -> Action {
        Action {
            name: name.into(),
            version,
        }
    }

    #[test]
    fn an_offer_keeps_the_child_actions_of_the_contract_once_each() {
        let offered = offered(&json!({ "actions": [
            { "name": "screen.look", "version": 1 },
            { "name": "screen.look", "version": 2 },
            { "name": "work.run", "version": 1 },
            // The host's own, and one no contract names: left out.
            { "name": "model.chat", "version": 1 },
            { "name": "fridge.open", "version": 1 },
        ]}));
        assert_eq!(
            offered,
            vec![action("screen.look", 1), action("work.run", 1)]
        );
    }

    #[test]
    fn granted_is_offered_enabled_of_the_contract_version_and_approved() {
        let offered = vec![
            action("screen.look", 1),
            action("notify.post", 1),
            action("work.run", 1),
        ];
        // Only screen.look is enabled; screen is approved until turned off.
        assert_eq!(granted(&offered, &[]), vec![action("screen.look", 1)]);
        assert!(granted(&offered, &[("screen".into(), false)]).is_empty());
        // Another version than the contract's is not asked for.
        assert!(granted(&[action("screen.look", 2)], &[]).is_empty());
        // Enabled but not approved: work waits for the owner.
        enable_in_test(&["screen.look", "work.run"]);
        assert_eq!(granted(&offered, &[]), vec![action("screen.look", 1)]);
        assert_eq!(
            granted(&offered, &[("work".into(), true)]),
            vec![action("screen.look", 1), action("work.run", 1)]
        );
    }

    #[test]
    fn a_childs_ask_is_answered_failed_with_only_a_code() {
        let host = answer_ask(&json!({
            "callId": "c-up-1", "action": "model.chat", "version": 1,
            "args": { "messages": [] }, "for": { "kind": "owner" }, "deadlineAt": 1_900_000_000_000u64,
        }))
        .unwrap();
        assert_eq!(host.id.as_deref(), Some("c-up-1.result"));
        assert_eq!(
            host.body,
            json!({ "callId": "c-up-1", "action": "model.chat", "version": 1, "outcome": "failed", "error": { "code": "call_not_granted" } })
        );
        let child = answer_ask(&json!({
            "callId": "c-up-2", "action": "screen.look", "version": 1,
            "args": {}, "for": { "kind": "owner" }, "deadlineAt": 1_900_000_000_000u64,
        }))
        .unwrap();
        assert_eq!(child.body["error"], json!({ "code": "call_unknown" }));
    }
}
