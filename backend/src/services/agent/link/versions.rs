//! Which versions each end runs, on her link (`versions`, core #116): the
//! site says what it runs (`link.versions`), an end says what it runs, and
//! the site answers with its reading of it (`link.verdict`): `full`, or
//! `partial` with what this link will not carry between the two.
//!
//! What an end said is kept on its device row (`agent_sync_devices.
//! link_versions`) with the reading, so the owner's devices page shows an
//! end that is behind the site, ahead of it, or apart from the others, after
//! the link has closed too. Its app, from its hello, is kept as well: an end
//! that never says its versions is an app from before they were said.
//!
//! The site's core is its `core/` pointer, stamped at build (`build.rs`);
//! its ABI is the highest of her log's rows it takes (`logN`), which is what
//! an end's ABI is read against here. A site built where the pointer cannot
//! be read says no versions of its own, and still reads an end's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::contract::CONTRACT;

/// The capability both ends list for versions to be said.
pub(super) const CAPABILITY: &str = "versions";

/// The myriad-core commit the site was built with (its `core/` pointer);
/// empty when it could not be read at build.
const SITE_CORE_COMMIT: &str = env!("MYRIAD_SITE_CORE_COMMIT");

/// The site's app, as it names itself.
const SITE_APP: &str = "myriad-site";

/// The highest ABI of her log's rows the site takes: `logN` it accepts.
pub(crate) fn site_abi() -> u64 {
    CONTRACT
        .capabilities
        .iter()
        .filter(|capability| !super::connection::NOT_YET.contains(&capability.as_str()))
        .filter_map(|capability| capability.strip_prefix("log")?.parse::<u64>().ok())
        .max()
        .unwrap_or(1)
}

/// The site's core commit, when it was read at build.
pub(crate) fn site_commit() -> Option<&'static str> {
    Some(SITE_CORE_COMMIT).filter(|commit| !commit.is_empty())
}

/// What the site runs, as the owner's devices page compares ends with it.
pub(crate) fn site_json() -> Value {
    json!({
        "contract": CONTRACT.schema_version,
        "core": { "abi": site_abi(), "commit": site_commit() },
        "app": { "name": SITE_APP, "version": env!("CARGO_PKG_VERSION") },
    })
}

/// The site's `link.versions`; none when its core commit is unknown (the
/// contract has it say one).
pub(super) fn ours() -> Option<Value> {
    let commit = site_commit()?;
    Some(json!({
        "contract": CONTRACT.schema_version,
        "core": { "abi": site_abi(), "commit": commit },
        "app": { "name": SITE_APP, "version": env!("CARGO_PKG_VERSION") },
        // Nothing of the site's is asked of it by an end yet (`call.granted`
        // providing is empty).
        "actions": {},
    }))
}

/// The shared core an end runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Core {
    pub abi: u64,
    pub commit: String,
}

/// An app and its version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct App {
    pub name: String,
    pub version: String,
}

/// What an end said it runs (`link.versions`), as the core checked it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Said {
    pub contract: u64,
    pub core: Core,
    pub app: App,
    #[serde(default)]
    pub actions: BTreeMap<String, u64>,
}

/// What its hello listed: what it would do, and what the site took of it.
#[derive(Debug, Clone, Default)]
pub(super) struct Listed {
    pub declared: Vec<String>,
    pub granted: Vec<String>,
}

/// The site's reading of what an end said: `full`, or `partial` with what
/// this link will not carry between the two: a capability it listed that
/// the site does not take, rows of her log of a later ABI than it reads
/// (`logN`, only for an end that syncs her log), and a call it knows that
/// the site does not, or knows at another version. Never `refused`: an end
/// on another protocol is closed before its versions are heard.
pub(super) fn verdict(said: &Said, listed: &Listed) -> (&'static str, Vec<String>) {
    let mut stopped: Vec<String> = listed
        .declared
        .iter()
        .filter(|capability| !listed.granted.contains(capability))
        .cloned()
        .collect();
    let log_abi = |capability: &str| capability.strip_prefix("log")?.parse::<u64>().ok();
    let reads = listed
        .granted
        .iter()
        .filter_map(|capability| log_abi(capability))
        .max();
    if let Some(reads) = reads {
        stopped.extend(((reads + 1)..=site_abi()).map(|abi| format!("log{abi}")));
    }
    for (name, version) in &said.actions {
        let alike = myriad_her_link::call_spec(name).is_some_and(|spec| spec.version == *version);
        if !alike {
            stopped.push(name.clone());
        }
    }
    stopped.sort();
    stopped.dedup();
    let usable = if stopped.is_empty() {
        "full"
    } else {
        "partial"
    };
    (usable, stopped)
}

/// How an end's versions stand against the site's: `same` (the same core
/// commit and contract), `behind` (an older core ABI or contract), `ahead`
/// (a newer one: the site is to be updated), or `apart` (another commit of
/// the same ones, or older in one and newer in the other).
pub(crate) fn compared(said: &Said) -> &'static str {
    let contract = said.contract.cmp(&u64::from(CONTRACT.schema_version));
    let abi = said.core.abi.cmp(&site_abi());
    use std::cmp::Ordering::{Equal, Greater, Less};
    match (contract, abi) {
        (Equal, Equal) if site_commit() == Some(said.core.commit.as_str()) => "same",
        (Less, Less | Equal) | (Equal, Less) => "behind",
        (Greater, Greater | Equal) | (Equal, Greater) => "ahead",
        _ => "apart",
    }
}

/// The body of `link.verdict`.
pub(super) fn verdict_body(usable: &str, stopped: &[String]) -> Value {
    json!({ "usable": usable, "stopped": stopped })
}

/// What is kept on the device row once it said its versions, with the
/// site's reading.
pub(super) fn kept(said: &Said, usable: &str, stopped: &[String]) -> Value {
    json!({
        "said": said,
        "usable": usable,
        "stopped": stopped,
        "saidAt": crate::clock::now().to_rfc3339(),
    })
}

/// What the owner's devices page shows of a device's versions, from what
/// is kept on its row: its app (from its hello), and, once it said them,
/// its core and contract, how they stand against the site's, and the
/// site's reading. None for a device whose link never opened since.
pub(crate) fn shown(kept: Option<&Value>) -> Value {
    let Some(kept) = kept else {
        return Value::Null;
    };
    let said = serde_json::from_value::<Said>(kept["said"].clone()).ok();
    let app = said
        .as_ref()
        .map(|said| json!(said.app))
        .or_else(|| kept.get("app").filter(|app| app.is_object()).cloned());
    json!({
        "app": app,
        "said": said.is_some(),
        "contract": said.as_ref().map(|said| said.contract),
        "core": said.as_ref().map(|said| json!(said.core)),
        "compared": said.as_ref().map(compared),
        "usable": said.as_ref().and(kept.get("usable").cloned()),
        "stopped": said.as_ref().and(kept.get("stopped").cloned()),
        "saidAt": said.as_ref().and(kept.get("saidAt").cloned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(abi: u64, contract: u64, commit: &str, actions: &[(&str, u64)]) -> Said {
        Said {
            contract,
            core: Core {
                abi,
                commit: commit.into(),
            },
            app: App {
                name: "MyriadNative".into(),
                version: "1.2.3".into(),
            },
            actions: actions
                .iter()
                .map(|(name, version)| (name.to_string(), *version))
                .collect(),
        }
    }

    fn listed(declared: &[&str], granted: &[&str]) -> Listed {
        Listed {
            declared: declared.iter().map(|one| one.to_string()).collect(),
            granted: granted.iter().map(|one| one.to_string()).collect(),
        }
    }

    const OTHER: &str = "1111111111111111111111111111111111111111";

    #[test]
    fn the_site_reads_its_abi_off_the_log_rows_it_takes() {
        assert!(site_abi() >= 4, "{}", site_abi());
        assert!(
            CONTRACT
                .capabilities
                .contains(&format!("log{}", site_abi()))
        );
    }

    #[test]
    fn what_a_link_will_not_carry_is_named() {
        let contract = u64::from(CONTRACT.schema_version);
        let top = site_abi();
        let all_logs: Vec<String> = (2..=top).map(|abi| format!("log{abi}")).collect();
        let logs: Vec<&str> = all_logs.iter().map(String::as_str).collect();
        // Reads every row the site takes, knows the call the site knows.
        let spec = myriad_her_link::call_spec("screen.look").expect("the contract has it");
        let full = said(top, contract, OTHER, &[("screen.look", spec.version)]);
        assert_eq!(verdict(&full, &listed(&logs, &logs)), ("full", vec![]));
        // An end of an older core: the rows it cannot read do not go.
        let older = said(2, contract, OTHER, &[]);
        let (usable, stopped) = verdict(&older, &listed(&["log2"], &["log2"]));
        assert_eq!(usable, "partial");
        assert_eq!(stopped, all_logs[1..].to_vec());
        // One that syncs no log is not short of rows; a capability the site
        // does not take, and a call at another version or unknown, are.
        let (usable, stopped) = verdict(
            &said(
                top,
                contract,
                OTHER,
                &[("screen.look", spec.version + 1), ("made.up", 1)],
            ),
            &listed(&["calls", "teleport"], &["calls"]),
        );
        assert_eq!(usable, "partial");
        assert_eq!(stopped, ["made.up", "screen.look", "teleport"]);
    }

    #[test]
    fn an_end_stands_behind_ahead_or_apart_from_the_site() {
        let contract = u64::from(CONTRACT.schema_version);
        let top = site_abi();
        assert_eq!(compared(&said(top - 1, contract, OTHER, &[])), "behind");
        assert_eq!(compared(&said(top, contract - 1, OTHER, &[])), "behind");
        assert_eq!(compared(&said(top + 1, contract, OTHER, &[])), "ahead");
        assert_eq!(compared(&said(top, contract + 1, OTHER, &[])), "ahead");
        assert_eq!(compared(&said(top - 1, contract + 1, OTHER, &[])), "apart");
        let mine = site_commit().unwrap_or(OTHER);
        let same = compared(&said(top, contract, mine, &[]));
        assert_eq!(
            same,
            if site_commit().is_some() {
                "same"
            } else {
                "apart"
            }
        );
        if site_commit().is_some() {
            assert_eq!(compared(&said(top, contract, OTHER, &[])), "apart");
        }
    }

    #[test]
    fn what_is_shown_comes_from_what_is_kept() {
        assert_eq!(shown(None), Value::Null);
        // Its hello only: the app, nothing said.
        let hello = json!({ "app": { "name": "MyriadNative", "version": "1.0.0" } });
        let only = shown(Some(&hello));
        assert_eq!(only["said"], false);
        assert_eq!(only["app"]["version"], "1.0.0");
        assert!(only["compared"].is_null() && only["usable"].is_null());
        let older = said(2, u64::from(CONTRACT.schema_version), OTHER, &[]);
        let kept = kept(&older, "partial", &["log3".to_string()]);
        let shown = shown(Some(&kept));
        assert_eq!(shown["said"], true);
        assert_eq!(shown["core"]["abi"], 2);
        assert_eq!(shown["compared"], "behind");
        assert_eq!(shown["usable"], "partial");
        assert_eq!(shown["stopped"], json!(["log3"]));
        assert_eq!(shown["app"]["name"], "MyriadNative");
    }
}
