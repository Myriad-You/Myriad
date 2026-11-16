//! Where each of her other memory sources goes on her nodes (decisions D1,
//! N4, N5, N6): one memory row, as it is written today, and what it is on a
//! node. Pure: the row and what came before it in, a home out.
//!
//! | source | where | as |
//! | --- | --- | --- |
//! | `about_me` | 「她自己」 (`her`) of the audience it was said in | an entry: the person's words in private, `other_person` in a group (N4) |
//! | `us` | 「关于你」 (`person:<id>`) in their private talk | her view `us` |
//! | `lands` | 「关于你」 in private; the group's own node in a group | her view `lands` |
//! | `sore` | the person's node where it happened (in a group, their node in the group's audience) | something not wrapped up; let go of, closed |
//! | `bit` | 「关于你」 in private; the group's own node in a group (N6) | an entry |
//! | `group_day` | the group's own node | a span, one day |
//! | `jargon` | the group's own node (N6) | an entry |
//! | `narrative` | 「她自己」 in her own audience | a span, one day |
//!
//! A view changes only on evidence that counts (`trust::view_may_change`):
//! a row is evidence that counts only as her own words in the one person's
//! private talk, or in her own. What does not count is an entry instead,
//! tagged who said it there: so how she comes across in a group, which
//! rests on what people there said, is an entry on the group's node, never
//! a view (N4).
//!
//! Who said it: in a group everyone but her is `other_person`, and what she
//! made of a group's talk carries them both; in a private talk the one
//! person is `partner`. Nothing is ever put in another audience than the
//! one it was said in: the one exception, a person's own words about
//! themselves in a group, is [`super::group_facts`].
//!
//! A row that took the place of a sore spot is the same thing not wrapped
//! up yet (mended), or, retired as forgiven, the spot let go of. Every
//! other source, and every source of no audience she keeps nodes for, has
//! no home here.

use chrono::NaiveDate;
use myriad_nodes::SaidBy;
use serde_json::Value;

use super::audience;
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::OWN_VENUE;

/// The name of 「她自己」 when she starts it.
pub(crate) const HER_NAME: &str = "她自己";

/// What a row is on its node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Hang {
    /// An entry, said by these.
    Entry { said_by: Vec<SaidBy> },
    /// A span of experience, said by these, over one day when it is one.
    Span {
        said_by: Vec<SaidBy>,
        day: Option<NaiveDate>,
    },
    /// Her view of this id; when the row is no evidence that counts, an
    /// entry said by `instead`.
    View {
        view: &'static str,
        instead: Vec<SaidBy>,
    },
    /// Something not wrapped up: the same as the one kept on the row
    /// `follows` (a sore mended, or let go of when `closed`), or a new one.
    Open {
        follows: Option<String>,
        closed: bool,
    },
}

/// Whom a node is, for its name when it is started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Named {
    Person(i32),
    Group(String),
    Her,
}

/// Where one row goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Home {
    /// Whose copy of her, and which audience.
    pub user_id: i32,
    pub audience: String,
    pub key: String,
    pub named: Named,
    pub hang: Hang,
    pub text: String,
}

/// The row a new one took the place of: its id, source and why it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Before {
    pub id: String,
    pub source: String,
    pub reason: Option<String>,
}

/// Where a row was said: one person's private talk, a group, her own.
enum Said {
    Private(i32),
    Group(String),
    Own,
}

fn said_in(row: &agent_memories::Model) -> Option<Said> {
    if row.venue == OWN_VENUE {
        return Some(Said::Own);
    }
    if let Some(group) = row.venue.strip_prefix("group:") {
        return (!group.is_empty()).then(|| Said::Group(group.to_string()));
    }
    row.user_id
        .filter(|user_id| *user_id > 0)
        .map(Said::Private)
}

/// A row's text as a node keeps it: no space at the ends, no control
/// character but a line break.
pub(crate) fn clean(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || *ch == '\n')
        .collect::<String>()
        .trim()
        .to_string()
}

fn evidence(row: &agent_memories::Model) -> Value {
    row.evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or(Value::Null)
}

/// A bit's text: its handle with how it goes, as she would say it again.
fn bit_text(row: &agent_memories::Model) -> String {
    let how = clean(&row.content);
    match evidence(row)["handle"].as_str().map(clean) {
        Some(handle) if !handle.is_empty() && !how.contains(&handle) => {
            format!("{handle}：{how}")
        }
        _ => how,
    }
}

/// The day a day's row is of: its evidence's `day`, or its id (`day_…`).
fn day_of(row: &agent_memories::Model) -> Option<NaiveDate> {
    evidence(row)["day"]
        .as_str()
        .or_else(|| row.id.strip_prefix("day_"))
        .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
}

/// Where `row` goes on her nodes, if anywhere; `before` is the row it took
/// the place of, if any. Groups and her own are kept with the site owner,
/// so none of them without one.
pub(crate) fn home(
    row: &agent_memories::Model,
    before: Option<&Before>,
    site_owner: Option<i32>,
) -> Option<Home> {
    let said = said_in(row)?;
    let (user_id, audience) = match &said {
        Said::Private(user_id) => (*user_id, audience::private_talk(*user_id, site_owner)),
        Said::Group(group) => (site_owner?, audience::group(group)),
        Said::Own => (site_owner?, audience::OWN.to_string()),
    };
    let them = || row.user_id.filter(|user_id| *user_id > 0);
    let person = |user_id: i32| (audience::person_key(&audience, user_id), Named::Person(user_id));
    let the_group = |group: &str| (format!("group:{group}"), Named::Group(group.to_string()));
    let her_in_group = vec![SaidBy::Her, SaidBy::OtherPerson];
    let mut text = clean(&row.content);
    let ((key, named), hang) = match (row.source.as_str(), &said) {
        ("about_me", Said::Private(_)) => (
            ("her".to_string(), Named::Her),
            // What they told her about herself.
            Hang::Entry {
                said_by: vec![SaidBy::Partner],
            },
        ),
        ("about_me", Said::Group(_)) => (
            ("her".to_string(), Named::Her),
            Hang::Entry {
                said_by: vec![SaidBy::OtherPerson],
            },
        ),
        ("us", Said::Private(user_id)) => (
            person(*user_id),
            Hang::View {
                view: "us",
                instead: vec![SaidBy::Her],
            },
        ),
        ("lands", Said::Private(user_id)) => (
            person(*user_id),
            Hang::View {
                view: "lands",
                instead: vec![SaidBy::Her],
            },
        ),
        ("lands", Said::Group(group)) => (
            the_group(group),
            Hang::View {
                view: "lands",
                instead: vec![SaidBy::OtherPerson],
            },
        ),
        ("sore", Said::Private(_) | Said::Group(_)) => (
            person(them()?),
            Hang::Open {
                follows: before
                    .filter(|before| before.source == "sore")
                    .map(|before| before.id.clone()),
                closed: false,
            },
        ),
        // A sore spot let go of: what happened, behind them now.
        ("chat", Said::Private(_) | Said::Group(_))
            if before.is_some_and(|before| {
                before.source == "sore" && before.reason.as_deref() == Some("forgiven")
            }) =>
        {
            (
                person(them()?),
                Hang::Open {
                    follows: before.map(|before| before.id.clone()),
                    closed: true,
                },
            )
        }
        ("bit", Said::Private(user_id)) => {
            text = bit_text(row);
            (
                person(*user_id),
                Hang::Entry {
                    said_by: vec![SaidBy::Her, SaidBy::Partner],
                },
            )
        }
        ("bit", Said::Group(group)) => {
            text = bit_text(row);
            (
                the_group(group),
                Hang::Entry {
                    said_by: her_in_group,
                },
            )
        }
        ("group_day", Said::Group(group)) => (
            the_group(group),
            Hang::Span {
                said_by: her_in_group,
                day: day_of(row),
            },
        ),
        ("jargon", Said::Group(group)) => (
            the_group(group),
            Hang::Entry {
                // As someone there told it, or as she made it out there.
                said_by: if evidence(row).get("told").is_some() {
                    vec![SaidBy::OtherPerson]
                } else {
                    her_in_group
                },
            },
        ),
        ("narrative", Said::Own) => (
            ("her".to_string(), Named::Her),
            Hang::Span {
                said_by: vec![SaidBy::Her],
                day: day_of(row),
            },
        ),
        _ => return None,
    };
    (!text.is_empty()).then_some(Home {
        user_id,
        audience,
        key,
        named,
        hang,
        text,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(
        source: &str,
        venue: &str,
        user_id: Option<i32>,
        evidence: Value,
    ) -> agent_memories::Model {
        let at = crate::clock::now().fixed_offset();
        agent_memories::Model {
            id: "mem_1".into(),
            user_id,
            kind: "fact".into(),
            content: " 叫他小笼包 ".into(),
            evidence: (!evidence.is_null()).then(|| evidence.to_string()),
            speaker: "agent".into(),
            source: source.into(),
            venue: venue.into(),
            audience: json!([]),
            concepts: json!([]),
            importance: 0.5,
            access_count: 0,
            last_accessed_at: None,
            valid_from: at,
            invalid_at: None,
            invalid_reason: None,
            created_at: at,
            updated_at: at,
            superseded_by: None,
            origin: "site".into(),
        }
    }

    #[test]
    fn each_source_goes_to_its_node_in_the_audience_it_was_said_in() {
        let owner = Some(1);
        let us = home(&row("us", "private", Some(7), Value::Null), None, owner).unwrap();
        assert_eq!(
            (us.user_id, us.audience.as_str(), us.key.as_str()),
            (7, "personal:7", "person:7")
        );
        assert!(matches!(us.hang, Hang::View { view: "us", .. }));
        assert_eq!(us.text, "叫他小笼包");
        // The site owner's own talk is under the core's one name.
        let mine = home(&row("us", "private", Some(1), Value::Null), None, owner).unwrap();
        assert_eq!(mine.audience, "personal:local-owner");

        let lands = home(
            &row("lands", "group:qq:5", Some(7), Value::Null),
            None,
            owner,
        )
        .unwrap();
        assert_eq!(
            (lands.user_id, lands.audience.as_str(), lands.key.as_str()),
            (1, "group:qq:5", "group:qq:5")
        );
        assert_eq!(
            lands.hang,
            Hang::View {
                view: "lands",
                instead: vec![SaidBy::OtherPerson]
            }
        );
        // No site owner, no group.
        assert!(
            home(
                &row("lands", "group:qq:5", Some(7), Value::Null),
                None,
                None
            )
            .is_none()
        );

        let told = home(
            &row("about_me", "group:qq:5", None, Value::Null),
            None,
            owner,
        )
        .unwrap();
        assert_eq!(
            (told.key.as_str(), told.audience.as_str()),
            ("her", "group:qq:5")
        );
        assert_eq!(
            told.hang,
            Hang::Entry {
                said_by: vec![SaidBy::OtherPerson]
            }
        );
        let told = home(
            &row("about_me", "private", Some(7), Value::Null),
            None,
            owner,
        )
        .unwrap();
        assert_eq!(
            (told.key.as_str(), told.audience.as_str()),
            ("her", "personal:7")
        );

        let bit = home(
            &row("bit", "private", Some(7), json!({"handle": "包子"})),
            None,
            owner,
        )
        .unwrap();
        assert_eq!(bit.text, "包子：叫他小笼包");
        assert_eq!(
            bit.hang,
            Hang::Entry {
                said_by: vec![SaidBy::Her, SaidBy::Partner]
            }
        );
        let bit = home(
            &row("bit", "group:qq:5", Some(1), json!({"handle": "小笼包"})),
            None,
            owner,
        )
        .unwrap();
        assert_eq!(
            (bit.key.as_str(), bit.text.as_str()),
            ("group:qq:5", "叫他小笼包")
        );

        let day = home(
            &row(
                "group_day",
                "group:qq:5",
                Some(1),
                json!({"day": "2026-11-05"}),
            ),
            None,
            owner,
        )
        .unwrap();
        assert_eq!(
            day.hang,
            Hang::Span {
                said_by: vec![SaidBy::Her, SaidBy::OtherPerson],
                day: NaiveDate::from_ymd_opt(2026, 11, 5)
            }
        );
        let mut narrative = row("narrative", OWN_VENUE, None, Value::Null);
        narrative.id = "day_2026-11-04".into();
        let narrative = home(&narrative, None, owner).unwrap();
        assert_eq!(
            (narrative.audience.as_str(), narrative.key.as_str()),
            ("own:", "her")
        );
        assert!(matches!(narrative.hang, Hang::Span { day: Some(_), .. }));

        let jargon = home(
            &row(
                "jargon",
                "group:qq:5",
                None,
                json!({"term": "x", "told": "A"}),
            ),
            None,
            owner,
        )
        .unwrap();
        assert_eq!(
            jargon.hang,
            Hang::Entry {
                said_by: vec![SaidBy::OtherPerson]
            }
        );

        // Sources that stay rows.
        for source in [
            "day_with",
            "stranger",
            "maybe_is",
            "spoke_up",
            "lesson",
            "chat_note",
            "game",
            "thread",
        ] {
            assert!(
                home(&row(source, "private", Some(7), Value::Null), None, owner).is_none(),
                "{source}"
            );
        }
    }

    #[test]
    fn a_sore_spot_follows_the_one_it_took_the_place_of() {
        let owner = Some(1);
        let sore = home(
            &row("sore", "group:qq:5", Some(7), Value::Null),
            None,
            owner,
        )
        .unwrap();
        assert_eq!(
            (sore.user_id, sore.audience.as_str(), sore.key.as_str()),
            (1, "group:qq:5", "person:7")
        );
        assert_eq!(
            sore.hang,
            Hang::Open {
                follows: None,
                closed: false
            }
        );
        let was = Before {
            id: "mem_0".into(),
            source: "sore".into(),
            reason: Some("superseded".into()),
        };
        let mended = home(
            &row("sore", "private", Some(7), Value::Null),
            Some(&was),
            owner,
        )
        .unwrap();
        assert_eq!(
            mended.hang,
            Hang::Open {
                follows: Some("mem_0".into()),
                closed: false
            }
        );
        // An ordinary fact is nothing here; one that let a sore go closes it.
        assert!(
            home(
                &row("chat", "private", Some(7), Value::Null),
                Some(&was),
                owner
            )
            .is_none()
        );
        let forgiven = Before {
            reason: Some("forgiven".into()),
            ..was
        };
        let let_go = home(
            &row("chat", "private", Some(7), Value::Null),
            Some(&forgiven),
            owner,
        )
        .unwrap();
        assert_eq!(
            let_go.hang,
            Hang::Open {
                follows: Some("mem_0".into()),
                closed: true
            }
        );
    }
}
