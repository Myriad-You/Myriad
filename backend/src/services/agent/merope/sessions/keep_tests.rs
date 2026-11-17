//! Her line on a repository: written again only on something new.

use serde_json::json;

use super::super::Picture;
use super::nothing_new;

/// A session of the repository told at `at` (milliseconds).
fn session(at: i64) -> myriad_her_link::now::Session {
    myriad_her_link::now::read_session(&json!({
        "grant": "g_7777777777",
        "session": "s-1",
        "change": "asked",
        "occurredAt": at,
        "tool": "coding-agent",
        "remote": "github.com/me/repo",
        "title": "Split the store",
        "askedBy": "session",
        "askedByTitle": "Plan the release",
        "fromOwner": "把存储拆开然后提交",
        "fromPeer": "照小林说的改",
        "fromAgent": "Committed locally. Push now? (push / wait)",
        "touched": ["src/a.rs"],
        "checks": [{"kind": "test", "state": "passed", "at": at - 1}],
    }))
    .unwrap()
}

/// Her line on a repository is written again only on something new: a
/// session it did not rest on, or a round of one of its sessions come to
/// an end (finished or failed) in different words. A session asking the
/// owner something has not got further.
#[test]
fn her_line_on_a_repository_waits_for_something_new() {
    let key = myriad_nodes::repo_key("github.com/me/repo").unwrap();
    let standing = json!({
        "author": "her",
        "value": {"text": "拆仓库做到一半", "because": ["grant:a", "session:a"]},
    });
    let line = |text: &str, because: &[&str]| json!({"text": text, "because": because});
    let looked = |remote: &str, change: &str| {
        let mut session = session(1_000);
        session.remote = Some(remote.to_string());
        session.change = serde_json::from_value(json!(change)).unwrap();
        Picture {
            device: "dev_mac".to_string(),
            session,
        }
    };
    let same = ["grant:a", "session:a"];
    let asked = [looked("github.com/me/repo", "asked")];
    assert!(
        nothing_new(&standing, &line("拆完了一半，等你定", &same), &asked, &key),
        "only asking: as it stands"
    );
    let finished = [looked("github.com/me/repo", "finished")];
    assert!(
        !nothing_new(&standing, &line("拆完了", &same), &finished, &key),
        "a round finished: written"
    );
    assert!(
        !nothing_new(
            &standing,
            &line("拆到一半失败了", &same),
            &[looked("github.com/me/repo", "failed")],
            &key
        ),
        "a round failed: written"
    );
    assert!(
        nothing_new(&standing, &line("拆仓库做到一半", &same), &finished, &key),
        "a round finished, the same words: as it stands"
    );
    assert!(
        nothing_new(
            &standing,
            &line("拆完了", &same),
            &[looked("github.com/me/other", "finished")],
            &key
        ),
        "another repository's round: as it stands"
    );
    assert!(
        !nothing_new(
            &standing,
            &line("拆仓库做到一半", &["grant:a", "session:a", "session:b"]),
            &asked,
            &key
        ),
        "a new session: written"
    );
    assert!(
        !nothing_new(&json!(null), &line("拆仓库做到一半", &same), &asked, &key),
        "none of hers yet: written"
    );
}
