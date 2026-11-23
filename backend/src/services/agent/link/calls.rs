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
//!
//! She asks a device from Work (a look at the screen, a notification, a line
//! said aloud: [`WORK_ACTIONS`]) and from a proposal for the owner's Mac
//! (`work.run`, `act`). Nothing of it calls a model on the site: what it
//! costs is the Work step's or the device's own.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::calls_store;
use super::connection::Outgoing;

/// The actions she may ask a device to do now. The contract's other child
/// actions are known (a device's offer of them is kept) but not granted.
const ENABLED: &[&str] = &["screen.look", "notify.post", "voice.speak", "work.run"];

/// The actions Work asks a device for, one step each (`device.<action>`):
/// a whole run (`work.run`) is asked by a proposal, not by a step.
const WORK_ACTIONS: &[&str] = &["screen.look", "notify.post", "voice.speak"];

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

/// What the site provides to be asked of it on a link (`call.granted`'s
/// `providing`), `(name, version)`: nothing yet. What it says it provides and
/// how it answers a device's ask ([`answer_ask`]) come from this one list.
const PROVIDING: &[(&str, u64)] = &[];

/// `call.granted`: what it may be asked to do, and what it may ask of the
/// host ([`PROVIDING`]).
pub(super) fn granted_frame(granted: &[Action]) -> Outgoing {
    Outgoing::latest(
        "call.granted",
        json!({
            "actions": granted.iter().map(Action::json).collect::<Vec<_>>(),
            "providing": PROVIDING
                .iter()
                .map(|(name, version)| json!({ "name": name, "version": version }))
                .collect::<Vec<_>>(),
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

pub(crate) use super::calls_pending::{ask, sweep};

/// A Work capability of a device (`device.<action>`) the person `user_id`
/// can use now: a device of theirs is linked and was told it is granted
/// the action. Apart from the site's own grants, which do not know devices.
/// Read from this process's links: where Work runs in a process apart from
/// the web one that holds them, it is never usable there (closed, not
/// open); the site runs as one process.
pub(crate) fn usable(user_id: i32, capability_id: &str) -> bool {
    work_action(capability_id).is_some_and(|action| {
        enabled(action) && !super::granted_devices(user_id, action).is_empty()
    })
}

/// The action of a Work capability of a device (`device.<action>`).
fn work_action(capability_id: &str) -> Option<&str> {
    capability_id
        .strip_prefix("device.")
        .filter(|action| WORK_ACTIONS.contains(action))
}

/// How long a call asked from Work is given (its deadlineAt): under the
/// step's own time, so the step never gives up first. A line said aloud is
/// given the most a call that is not long may have: the device stops one it
/// cannot finish by then and says it was not heard out.
fn work_deadline_ms(action: &str) -> u64 {
    match action {
        "voice.speak" => myriad_her_link::call_most_ms(action),
        _ => 10_000,
    }
}

/// How much longer than its deadline Work waits for a call. One still
/// running when Work stops waiting ends by itself, its row kept.
const WORK_WAITS_MORE_MS: u64 = 2_000;

/// What Work gives `action`, as its args: only the fields the contract
/// names, each of what it says; the Err is what is missing.
fn work_args(action: &str, params: &HashMap<String, Value>) -> Result<Value, String> {
    let text = |name: &str| -> Result<String, String> {
        params
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("bad_args: {name} is needed, as text"))
    };
    Ok(match action {
        "notify.post" => json!({ "title": text("title")?, "body": text("body")? }),
        "voice.speak" => json!({ "text": text("text")? }),
        _ => json!({}),
    })
}

/// What a call that was not done means, as Work is told it: never the
/// device's words.
fn not_done(code: &str) -> &'static str {
    match code {
        "not_now" => "the device held it back now (its do-not-disturb or its own bounds)",
        "declined" => "the owner said no on the device, or did not answer there",
        "call_not_granted" => "the owner has not let her do this on that device",
        "call_unavailable" => "no device of the owner's is linked and lets her do this now",
        "wrong_audience" => {
            "this is only for the owner or her own life, never for a group or anyone else"
        }
        "call_cancelled" => "it was stopped before it was done",
        _ => "the device does not do this",
    }
}

/// What Work was running for, as `for` says it (`callAudiences`).
pub(crate) struct WorkFor<'a> {
    pub user_id: i32,
    /// The group chat the run is in.
    pub venue: Option<&'a str>,
    /// Her own doing: accepted by her autonomy, or an instruction come due
    /// with no one there. Not the owner asking.
    pub her_own: bool,
}

/// Work asked a device's capability. Only the site owner's devices are
/// asked; she asks the one they are at, for whom the run is: someone
/// else's run is for other, a group's for that group, her own doing own,
/// and only the owner's asking is for the owner. A call not for one the
/// action allows is refused and kept as any other. Work is told the result
/// of one done; otherwise a code and what it means, never the device's
/// words; one that may have happened says so (outcome unknown), so it is
/// not asked again.
pub(crate) async fn work(
    db: &DatabaseConnection,
    capability_id: &str,
    params: &HashMap<String, Value>,
    run: WorkFor<'_>,
) -> Result<Value, String> {
    let unknown = |detail: &str| format!("{} {detail}", myriad_agent_rules::OUTCOME_UNKNOWN_PREFIX);
    let Some(action) = work_action(capability_id).filter(|action| enabled(action)) else {
        return Err("call_unknown: this is not something she asks a device".into());
    };
    let args = work_args(action, params)?;
    let site_owner = match crate::services::principal::site_owner_id(db).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return Err("call_unavailable: the site has no owner".into()),
        Err(_) => return Err("call_unavailable: the site owner could not be read".into()),
    };
    let devices = super::granted_devices(site_owner, action);
    let Some(device_id) = super::pick::device(site_owner, &devices).cloned() else {
        return Err(format!(
            "call_unavailable: {}",
            not_done("call_unavailable")
        ));
    };
    let of = if run.user_id != site_owner {
        json!({ "kind": "other" })
    } else if let Some(group) = run.venue {
        json!({ "kind": "group", "groupId": group })
    } else if run.her_own {
        json!({ "kind": "own" })
    } else {
        json!({ "kind": "owner" })
    };
    let deadline_ms = work_deadline_ms(action);
    let asked = ask(
        db,
        site_owner,
        &device_id,
        action,
        &of,
        args,
        Some(deadline_ms),
    );
    let waits = Duration::from_millis(deadline_ms + WORK_WAITS_MORE_MS);
    let answer = match tokio::time::timeout(waits, asked).await {
        Ok(answer) => answer,
        Err(_) => return Err(unknown("the device has not answered by its deadline")),
    };
    match answer.outcome {
        calls_store::Outcome::Done => Ok(answer.result.unwrap_or_else(|| json!({}))),
        // Sent, and nothing sure came back: it may have happened.
        calls_store::Outcome::Failed(code @ ("call_timeout" | "internal_error")) => Err(unknown(
            &format!("{code}: the device did not say how it went"),
        )),
        calls_store::Outcome::Failed(code) => Err(format!("{code}: {}", not_done(code))),
        calls_store::Outcome::Unknown => Err(unknown(
            "the device lost track midway, so it may have happened; do not ask it again",
        )),
    }
}

/// What her calls of `action` are counted as on her status page.
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
    // Taken back at once, before it is written: nothing of it is asked in
    // between. Given, it is told once written.
    if !approved {
        super::withhold_calls(owner, device_id, group);
    }
    let set = calls_store::set_approved(db, owner, device_id, group, approved).await;
    // Told what stands, written or not (a write that failed gives back what
    // was withheld).
    grants_changed(db, owner, device_id).await;
    set
}

/// A child asked the host to do something (`call.ask` up), taken at
/// `taken_at` (milliseconds, the host's clock). Whether it is done is the
/// shared core's to say (`myriad_her_link::host_call_receive`): the site
/// provides nothing yet ([`PROVIDING`]), so every ask is answered failed, with
/// the one `call.result` every ask gets: a device's action or one no contract
/// registers is `call_unknown`; a host action at a version the contract does
/// not have, `version_unsupported` with the host's; one not for whom it is
/// asked for, `wrong_audience`; any other host action, `call_not_granted`.
pub(super) fn answer_ask(body: &Value, taken_at: u64) -> Option<Outgoing> {
    let call_id = body["callId"].as_str()?;
    let action = body["action"].as_str()?;
    let version = body["version"].as_u64()?;
    let deadline_at = body["deadlineAt"].as_u64()?;
    let providing: Vec<(String, u64)> = PROVIDING
        .iter()
        .map(|(name, version)| (name.to_string(), *version))
        .collect();
    let received = myriad_her_link::host_call_receive(
        action,
        version,
        &body["for"],
        deadline_at,
        taken_at,
        &providing,
    );
    let code = match received.verdict {
        myriad_her_link::CallVerdict::Fail(code) => code,
        // Nothing the site provides runs here yet: one it may be asked to do
        // can only be one it says it provides, and that list is empty.
        myriad_her_link::CallVerdict::Do | myriad_her_link::CallVerdict::Hold => {
            "call_unavailable"
        }
    };
    Some(Outgoing::reliable(
        "call.result",
        format!("{call_id}.result"),
        json!({
            "callId": call_id,
            "action": action,
            "version": received.version,
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
        // screen and notify are approved until turned off; work waits for
        // the owner.
        assert_eq!(
            granted(&offered, &[]),
            vec![action("screen.look", 1), action("notify.post", 1)]
        );
        assert_eq!(
            granted(&offered, &[("screen".into(), false)]),
            vec![action("notify.post", 1)]
        );
        assert_eq!(
            granted(&offered, &[("work".into(), true)]),
            vec![
                action("screen.look", 1),
                action("notify.post", 1),
                action("work.run", 1)
            ]
        );
        // Another version than the contract's is not asked for.
        assert!(granted(&[action("screen.look", 2)], &[]).is_empty());
        // Not enabled here: not granted, whatever the owner decided.
        enable_in_test(&["screen.look"]);
        assert_eq!(
            granted(&offered, &[("work".into(), true)]),
            vec![action("screen.look", 1)]
        );
    }

    #[test]
    fn work_gives_each_action_only_its_contract_args() {
        let params = |pairs: &[(&str, Value)]| -> HashMap<String, Value> {
            pairs
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect()
        };
        assert_eq!(work_args("screen.look", &params(&[])).unwrap(), json!({}));
        assert_eq!(
            work_args(
                "notify.post",
                &params(&[
                    ("title", json!(" Myriad ")),
                    ("body", json!("The build finished.")),
                    ("kind", json!("urgent")),
                ])
            )
            .unwrap(),
            json!({ "title": "Myriad", "body": "The build finished." })
        );
        assert!(
            work_args("notify.post", &params(&[("title", json!("Myriad"))]))
                .unwrap_err()
                .starts_with("bad_args: body")
        );
        assert_eq!(
            work_args("voice.speak", &params(&[("text", json!("Tea is ready."))])).unwrap(),
            json!({ "text": "Tea is ready." })
        );
        assert!(work_args("voice.speak", &params(&[("text", json!("  "))])).is_err());
        // A line said aloud is given what a call that is not long may have.
        assert_eq!(work_deadline_ms("voice.speak"), 60_000);
        assert_eq!(work_deadline_ms("notify.post"), 10_000);
    }

    #[test]
    fn a_whole_run_is_not_a_step_of_work() {
        assert_eq!(work_action("device.notify.post"), Some("notify.post"));
        assert_eq!(work_action("device.work.run"), None);
        assert_eq!(work_action("screen.look"), None);
    }

    #[test]
    fn a_childs_ask_is_answered_failed_with_only_a_code() {
        let ask = |call_id: &str, action: &str, version: u64, of: Value| {
            json!({
                "callId": call_id, "action": action, "version": version,
                "args": {}, "for": of, "deadlineAt": 1_900_000_000_000u64,
            })
        };
        let owner = json!({ "kind": "owner" });
        let answer = |body: Value| answer_ask(&body, 1_899_999_990_000).unwrap();
        let host = answer(ask("c-up-1", "model.chat", 1, owner.clone()));
        assert_eq!(host.id.as_deref(), Some("c-up-1.result"));
        assert_eq!(
            host.body,
            json!({ "callId": "c-up-1", "action": "model.chat", "version": 1, "outcome": "failed", "error": { "code": "call_not_granted" } })
        );
        let child = answer(ask("c-up-2", "screen.look", 1, owner.clone()));
        assert_eq!(child.body["error"], json!({ "code": "call_unknown" }));
        // What the shared core holds every receiver to, the site too: the
        // version first, then for whom it is.
        let newer = answer(ask("c-up-3", "group.read", 2, json!({ "kind": "group", "groupId": "g42" })));
        assert_eq!(newer.body["error"], json!({ "code": "version_unsupported" }));
        assert_eq!(newer.body["version"], 1);
        let for_owner = answer(ask("c-up-4", "group.read", 1, owner));
        assert_eq!(for_owner.body["error"], json!({ "code": "wrong_audience" }));
        // Nothing is provided to be asked of it, and call.granted says so.
        assert_eq!(granted_frame(&[]).body["providing"], json!([]));
    }
}
