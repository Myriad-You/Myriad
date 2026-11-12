//! What she is shown of the changes that came together: each session as it
//! stands, the others of its repository, how the owner decided before, and
//! what she made of it last time; then what she already told the owner of
//! their sessions.

use myriad_her_link::now::{AskedBy, Change, CheckKind, CheckState, Session, WaitingOn};
use myriad_nodes::sessions;
use sea_orm::DatabaseConnection;

use super::Picture;
use super::judge::{self, Material, Said};

/// What she recalls of the owner, for each change.
const MEMORIES: usize = 8;
/// How many of the repository's other sessions she is shown, for each.
const OTHERS: usize = 8;

/// One change and what is around it.
pub(super) struct Shown<'a> {
    pub picture: &'a Picture,
    /// The owner's other sessions as they last changed.
    pub others: &'a [Picture],
    /// What she recalls of how the owner decided before: id and text.
    pub memories: Vec<(String, String)>,
    /// What she made of this session last time.
    pub before: Option<String>,
}

fn check_word(kind: CheckKind) -> &'static str {
    match kind {
        CheckKind::Test => "tests",
        CheckKind::Build => "build",
        CheckKind::Other => "check",
    }
}

fn checks_line(session: &Session) -> String {
    let mut seen = Vec::new();
    let mut parts = Vec::new();
    for check in &session.checks {
        if seen.contains(&check.kind) {
            continue;
        }
        seen.push(check.kind);
        let state = match check.state {
            CheckState::Running => "running",
            CheckState::Passed => "passed",
            CheckState::Failed => "failed",
            CheckState::Ended => "ended, outcome not known",
        };
        parts.push(format!("{} {state}", check_word(check.kind)));
    }
    parts.join("; ")
}

/// Where a session stands, in a line.
pub(super) fn now_line(session: &Session) -> String {
    let change = match session.change {
        Change::Started => "started",
        Change::Asked => "stopped and asks the owner",
        Change::Finished => "finished a turn, nothing to answer",
        Change::Failed => "failed (kept erring, or its checks failed)",
        Change::Quiet => "quiet for long",
    };
    let waiting = match session.waiting_on {
        Some(WaitingOn::Owner) => "; waits on the owner",
        Some(WaitingOn::Session) => "; waits to report back to the session that sent it on",
        Some(WaitingOn::Checks) => "; waits on its tests or builds",
        Some(WaitingOn::Program) => "; waits on another program",
        Some(WaitingOn::Unknown) | None => "",
    };
    format!("{change}{waiting}")
}

/// Where a session runs: its repository's name, or its folder.
fn place(session: &Session) -> &str {
    match (&session.remote, &session.folder) {
        (Some(remote), _) => remote.rsplit('/').next().unwrap_or(remote),
        (None, Some(folder)) => folder,
        (None, None) => "",
    }
}

fn title(session: &Session) -> String {
    let repo = place(session);
    match &session.title {
        Some(title) => format!("{title} ({} in {repo})", session.tool),
        None => format!("a {} session in {repo}", session.tool),
    }
}

/// Everything she is shown, numbered: each change under its heading, then
/// what she already told the owner of their sessions.
pub(super) fn material(changes: &[Shown], said: &[Said]) -> Material {
    let mut material = Material::default();
    for shown in changes {
        one(&mut material, shown);
    }
    material.add_said(said);
    material
}

fn one(material: &mut Material, shown: &Shown) {
    let picture = shown.picture;
    let session = &picture.session;
    let reference = sessions::session_ref(&picture.device, &session.grant, &session.session);
    material.change(now_line(session));
    material.add("the session", title(session), &reference);
    let given = match (session.asked_by, &session.asked_by_title) {
        (Some(AskedBy::Owner), _) => "the owner, in person".to_string(),
        (Some(AskedBy::Session), Some(title)) => {
            format!("another of the owner's sessions, «{title}», sent it on; it reports back there")
        }
        (Some(AskedBy::Session), None) => {
            "another of the owner's sessions sent it on; it reports back there".to_string()
        }
        (Some(AskedBy::None) | None, _) => "not known".to_string(),
    };
    material.add("who gave it this work", given, &reference);
    if let Some(words) = &session.from_peer {
        material.add(
            "what the session that sent it on told it",
            judge::quoted(words),
            &reference,
        );
    }
    if let Some(words) = &session.from_owner {
        material.add(
            "what the owner last told it",
            judge::quoted(words),
            &reference,
        );
    }
    if let Some(words) = &session.from_agent {
        material.add(
            "what it last said or asked",
            judge::quoted(words),
            &reference,
        );
    }
    if !session.touched.is_empty() {
        material.add("files it changed", session.touched.join(", "), &reference);
    }
    material.add("its tests and builds", checks_line(session), &reference);
    for other in shown
        .others
        .iter()
        .filter(|other| {
            other.session.remote == session.remote
                && (session.remote.is_some()
                    || (other.device == picture.device && other.session.folder == session.folder))
                && !(other.device == picture.device
                    && other.session.grant == session.grant
                    && other.session.session == session.session)
        })
        .take(OTHERS)
    {
        let shared: Vec<&String> = other
            .session
            .touched
            .iter()
            .filter(|path| session.touched.contains(path))
            .collect();
        let mut text = format!("{}: {}", title(&other.session), now_line(&other.session));
        if !shared.is_empty() {
            text.push_str(&format!(
                "; also changed {}",
                shared
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        material.add(
            "another session in this repository",
            text,
            &sessions::session_ref(&other.device, &other.session.grant, &other.session.session),
        );
    }
    for (id, text) in &shown.memories {
        material.add(
            "what you know of the owner (how they decided before)",
            text.clone(),
            &format!("memory:{id}"),
        );
    }
    if let Some(before) = &shown.before {
        material.add(
            "what you made of this session last time",
            before.clone(),
            &reference,
        );
    }
}

/// What she recalls of the owner for this session: id and text.
pub(super) async fn memories(
    db: &DatabaseConnection,
    owner: i32,
    session: &Session,
) -> Vec<(String, String)> {
    let query = format!(
        "{} {}",
        session.title.as_deref().unwrap_or_default(),
        session.from_agent.as_deref().unwrap_or_default()
    );
    let present = crate::services::agent::memory::unified::Audience::private(owner);
    match crate::services::agent::memory::unified::recall_primed(
        db,
        owner,
        &present,
        Some(query.trim()).filter(|query| !query.is_empty()),
        &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
        MEMORIES,
        &Default::default(),
        1.0,
    )
    .await
    {
        Ok((recalled, _)) => recalled
            .iter()
            .map(|record| {
                (
                    record.id.clone(),
                    crate::services::agent::merope::store::as_known(record),
                )
            })
            .filter(|(_, text)| !text.is_empty())
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "[Merope] no memories for a session's look");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn picture(session: &str, change: &str, touched: &[&str]) -> Picture {
        Picture {
            device: "dev_mac".into(),
            session: myriad_her_link::now::read_session(&json!({
                "grant": "g_0123456789",
                "session": session,
                "change": change,
                "occurredAt": 1_760_000_000_000_i64,
                "tool": "coding-agent",
                "remote": "github.com/me/repo",
                "title": "Split the store",
                "fromOwner": "split the store and commit it",
                "fromAgent": "Committed locally. Push now? (push / wait)",
                "touched": touched,
                "checks": [{"kind": "test", "state": "passed", "at": 1}],
            }))
            .unwrap(),
        }
    }

    #[test]
    fn she_is_shown_each_change_whole_and_what_is_around_it() {
        let asking = picture("s-1", "asked", &["src/a.rs", "src/b.rs"]);
        let other = picture("s-2", "quiet", &["src/b.rs"]);
        let done = picture("s-3", "finished", &[]);
        let others = [asking.clone(), other];
        let material = material(
            &[
                Shown {
                    picture: &asking,
                    others: &others,
                    memories: vec![("m1".into(), "pushes need their own word".into())],
                    before: Some("你点头就行：上次也问过".into()),
                },
                Shown {
                    picture: &done,
                    others: &[],
                    memories: Vec::new(),
                    before: None,
                },
            ],
            &[Said {
                id: 7,
                minutes_ago: 3,
                text: "测试都过了".into(),
            }],
        );
        let numbered = material.numbered();
        for line in [
            "Change 1 of 2: stopped and asks the owner",
            "[1] the session: Split the store (coding-agent in repo)",
            "[2] who gave it this work: not known",
            "[3] what the owner last told it: split the store and commit it",
            "[4] what it last said or asked: Committed locally. Push now? (push / wait)",
            "[5] files it changed: src/a.rs, src/b.rs",
            "[6] its tests and builds: tests passed",
            "[7] another session in this repository: Split the store (coding-agent in repo): quiet for long; also changed src/b.rs",
            "[8] what you know of the owner (how they decided before): pushes need their own word",
            "[9] what you made of this session last time: 你点头就行：上次也问过",
            "Change 2 of 2: finished a turn, nothing to answer",
            "[10] the session: Split the store (coding-agent in repo)",
            "Around them:",
            "[15] what you already told the owner of their sessions: 3 minutes ago: 测试都过了",
        ] {
            assert!(numbered.contains(line), "{line}\n{numbered}");
        }
        assert_eq!(material.changes.len(), 2);
        assert_eq!(material.pieces.len(), 15, "itself is not another session");
        let evidence = material.evidence(&[4, 7, 8, 15]);
        assert_eq!(evidence.len(), 4);
        assert!(evidence.contains(&"memory:m1".to_string()));
        assert!(evidence.contains(&"proactive:7".to_string()));
    }
}
