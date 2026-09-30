//! Her self, kept up in the gaps of a conversation.
//!
//! After she answers, a fast model call writes what is going on inside her
//! now, in the first person: how the exchange left her, how she is after her
//! day, what is on her mind, what she feels like doing. The model judges all
//! of it from facts (the words, her reply, the conversation, how she feels
//! toward them, the hour and how many people she has seen, what she remembers
//! of them); no rule says "tired, so be brief". Her next line starts from it.
//!
//! Stating facts alone was not enough in testing: asked to answer a question,
//! the model never stopped to judge that 2 a.m. after five people means she is
//! worn out. This call is where that judgment happens.
//!
//! Nothing waits for it. Written before she answered, it came too late for
//! the reply almost every time (2.3 s and more, against a 1.5 s wait), so it
//! is written after, and a state lasts a while. The first words after a quiet
//! spell have none; she answers from the facts of her day.
//! It lives in process memory only: attention, not memory.
//!
//! In private the same reflection notes what she would want to come back to
//! with them later, and which earlier ones her reply took up (see
//! `threads`): what is on her mind about a person is part of how an
//! exchange left her. A group's reflection keeps none.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::services::agent::UserRequest;
use crate::services::agent::memory::unified::Audience;
use myriad_merope::inner::{
    MAX_INNER_CHARS, MAX_KEPT, SCHEMA_NAME, schema, schema_for, schema_for_group, system,
    system_for, system_for_group,
};

/// Generous: nothing waits for it.
const CALL_TIMEOUT: Duration = Duration::from_secs(25);
/// The whole reflection, reading included: past the call's own limit, so
/// the call gives up first and says so, rather than being cut off mid-write.
const REFLECTION_TIMEOUT: Duration = Duration::from_secs(45);
/// How long a state still counts as how she is.
const MOMENT: Duration = Duration::from_secs(5 * 60);

/// Whose state, and where: a state written in private may carry private
/// things, so a group turn never reads it (and the other way round).
type Key = (i32, String);

fn key(user_id: i32, present: &Audience) -> Key {
    (user_id, present.venue())
}

/// A new persona is in no state a former one was left in.
pub(super) fn forget() {
    if let Ok(mut after) = AFTER.lock() {
        after.clear();
    }
}

/// The state each person's last exchange left her in, and when.
static AFTER: LazyLock<Mutex<HashMap<Key, (String, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inner {
    inner: String,
    /// In private: what to come back to with them later.
    #[serde(default)]
    keep: Vec<super::threads::Kept>,
    /// In private: which open threads her reply took up or that no longer
    /// matter (indexes into openThreads).
    #[serde(default)]
    done: Vec<usize>,
    /// In private: they showed her something she said was wrong.
    #[serde(default)]
    wrong: Option<super::self_story::Corrected>,
    /// In private: what they told her about herself.
    #[serde(default, rename = "toldYou")]
    told_you: Option<String>,
    /// In private: something they did that got to her.
    #[serde(default)]
    hurt: Option<Hurt>,
    /// In private: sore spots they apologized for or made right (indexes
    /// into soreSpots).
    #[serde(default)]
    mended: Vec<usize>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct Hurt {
    what: String,
    weight: super::sore::Weight,
}

fn history_of(request: &UserRequest) -> Vec<Value> {
    let mut history: Vec<Value> = request
        .context
        .as_ref()
        .and_then(|context| context.conversation_history.as_ref())
        .into_iter()
        .flatten()
        .rev()
        .filter(|message| matches!(message.role.as_str(), "user" | "assistant"))
        .take(6)
        .map(|message| {
            json!({
                "role": message.role,
                "text": message.content.chars().take(300).collect::<String>(),
            })
        })
        .collect();
    history.reverse();
    history
}

/// After she has answered: how she is now, having heard them and said her
/// piece. Her next line starts from it.
pub fn spawn_after(db: DatabaseConnection, request: &UserRequest, reply: &str) {
    let user_id = request.user_id;
    let user_text: String = request.raw_input.chars().take(1_500).collect();
    let reply: String = reply.chars().take(1_500).collect();
    if user_id <= 0 || user_text.trim().is_empty() || reply.trim().is_empty() {
        return;
    }
    let history = history_of(request);
    let present = super::audience_for(request);
    let key = key(user_id, &present);
    let turn = super::turn_context(request);
    crate::services::agent::merope::background::spawn("inner state", async move {
        if !super::is_enabled().await {
            return;
        }
        let inner = match tokio::time::timeout(
            REFLECTION_TIMEOUT,
            compile(&db, user_id, &user_text, &reply, history, &present, &turn),
        )
        .await
        {
            Ok(inner) => inner,
            Err(_) => {
                tracing::warn!(user_id, "[Merope] reflecting on the exchange took too long");
                None
            }
        };
        if let (Some(inner), Ok(mut after)) = (inner, AFTER.lock()) {
            after.retain(|_, (_, at)| at.elapsed() < MOMENT);
            after.insert(key, (inner, Instant::now()));
        }
    });
}

async fn compile(
    db: &DatabaseConnection,
    user_id: i32,
    user_text: &str,
    reply: &str,
    history: Vec<Value>,
    present: &Audience,
    turn: &super::TurnContext,
) -> Option<String> {
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let no_priming = crate::services::agent::memory::unified::Priming::default();
    let (state, remembered, myself) = tokio::join!(
        super::get_or_create_state(db, user_id),
        // In a group, only what the group heard: this state is read back
        // into a reply everyone there can see.
        super::store::recall_remembered_primed(
            db,
            user_id,
            present,
            Some(user_text),
            4,
            &no_priming,
            1.0,
        ),
        super::self_state::current(db),
    );
    let feeling = state
        .ok()
        .map(|state| super::mood_tone_instruction(state.mood, state.arousal))
        .unwrap_or_default();
    let mut input = json!({
        "userText": user_text,
        "yourReply": reply,
        "history": history,
        "feelingTowardThem": feeling,
        "myself": myself.facts_view(),
        "remembered": remembered.map(|(facts, _)| facts).unwrap_or_default(),
    });
    if let Some(doing) = super::doing::current() {
        input["yourOwnTime"] = json!(super::doing::now_line(&doing, chrono::Utc::now()));
    }
    if let Some(scene) = &turn.scene {
        input["scene"] = json!(scene);
    }
    if turn.images > 0 {
        input["theySentImages"] = json!(turn.images);
    }
    if turn.in_game {
        input["playingTurtleSoup"] = json!(true);
    }
    let private = !present.is_group();
    let threads = if private {
        super::threads::open(db, user_id).await
    } else {
        Vec::new()
    };
    // In a group, what the one she answered did there (never what they did
    // in private); in private, all of it.
    let in_group = present.group_id().map(str::to_string);
    let sores = match &in_group {
        None => super::sore::open_all(db, user_id).await,
        Some(venue) => super::sore::open_in_group(db, venue, Some(user_id)).await,
    };
    if private {
        input["openThreads"] = json!(super::threads::as_input(&threads));
    }
    input["soreSpots"] = json!(super::sore::as_input(&sores, chrono::Utc::now()));
    let input = input.to_string();
    let (system_prompt, answer_schema) = if private {
        (system_for(&soul, true), schema_for(true))
    } else if in_group.is_some() {
        (system_for_group(&soul), schema_for_group())
    } else {
        (system(&soul), schema())
    };
    // Her own voice, thinking little.
    let raw = super::call::Ask::new(super::call::Voice::Hers, user_id, "inner")
        .within(CALL_TIMEOUT)
        .json_raw(&system_prompt, &input, SCHEMA_NAME, &answer_schema)
        .await
        .ok()?;
    let reflected = parse_reflection(&raw)?;
    if private {
        let now = chrono::Utc::now();
        for kept in reflected.keep.iter().take(MAX_KEPT) {
            super::threads::keep(db, user_id, kept, now).await;
        }
        let done: Vec<String> = reflected
            .done
            .iter()
            .filter_map(|index| threads.get(*index).map(|thread| thread.id.clone()))
            .collect();
        super::threads::close(db, user_id, &done, "taken_up").await;
        // Being wrong about a public matter is part of her own story; about
        // them or their life, it stays out of it.
        if let Some(wrong) = reflected.wrong.as_ref().filter(|wrong| wrong.public) {
            super::self_story::remember_corrected(db, wrong).await;
        }
        // Told something about herself: she carries it with them.
        if let Some(told) = reflected.told_you.as_deref() {
            super::making_sense::remember_told_by(db, user_id, told).await;
        }
    }
    // Made right first, then what got to her this time, kept where it
    // happened.
    super::sore::mend(db, &sores, &reflected.mended).await;
    if let Some(hurt) = &reflected.hurt {
        let at = match &in_group {
            None => Audience::private(user_id),
            Some(venue) => Audience::group(venue, user_id),
        };
        super::sore::keep(db, user_id, at, &hurt.what, hurt.weight).await;
    }
    Some(reflected.inner)
}

/// Her reflection: how she is now, and in private the threads it keeps and
/// lets go.
struct Reflection {
    inner: String,
    keep: Vec<super::threads::Kept>,
    done: Vec<usize>,
    wrong: Option<super::self_story::Corrected>,
    told_you: Option<String>,
    hurt: Option<Hurt>,
    mended: Vec<usize>,
}

fn parse_reflection(raw: &str) -> Option<Reflection> {
    let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
    let parsed: Inner = serde_json::from_str(json.as_deref().unwrap_or(raw.trim())).ok()?;
    let inner: String = parsed
        .inner
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_INNER_CHARS)
        .collect();
    (!inner.is_empty()).then_some(Reflection {
        inner,
        keep: parsed.keep,
        done: parsed.done,
        wrong: parsed.wrong,
        told_you: parsed
            .told_you
            .map(|told| told.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|told| !told.is_empty() && told != "null"),
        hurt: parsed.hurt.filter(|hurt| !hurt.what.trim().is_empty()),
        mended: parsed.mended,
    })
}

#[cfg(test)]
fn parse(raw: &str) -> Option<String> {
    parse_reflection(raw).map(|reflected| reflected.inner)
}

/// The inner-state call as production sends it, for the semantic suite.
#[cfg(test)]
pub(crate) fn probe_contract(soul: &str) -> (String, Value) {
    (system(soul), schema())
}

/// The private reflection, with threads, for the semantic suite.
#[cfg(test)]
pub(crate) fn threads_probe_contract(soul: &str) -> (String, Value) {
    (system_for(soul, true), schema_for(true))
}

/// (inner, kept threads as (about, dueInHours), done indexes), if the
/// reflection honors the contract.
#[cfg(test)]
pub(crate) fn parse_threads(raw: &str) -> Option<(String, Vec<(String, Option<i64>)>, Vec<usize>)> {
    parse_reflection(raw).map(|reflected| {
        (
            reflected.inner,
            reflected
                .keep
                .into_iter()
                .map(|kept| (kept.about, kept.due_in_hours))
                .collect(),
            reflected.done,
        )
    })
}

/// What the reflection says she was shown to be wrong about, as (about,
/// public, took), for the semantic suite.
#[cfg(test)]
pub(crate) fn parse_wrong(raw: &str) -> Option<Option<(String, bool, String)>> {
    parse_reflection(raw).map(|reflected| {
        reflected.wrong.map(|wrong| {
            let took = serde_json::to_value(wrong.took)
                .ok()
                .and_then(|took| took.as_str().map(str::to_string))
                .unwrap_or_default();
            (wrong.about, wrong.public, took)
        })
    })
}

#[cfg(test)]
pub(crate) fn parse_inner(raw: &str) -> Option<String> {
    parse(raw)
}

/// How her last exchange here left her, if that was a moment ago. It has not
/// heard their latest words.
pub fn current(user_id: i32, present: &Audience) -> Option<String> {
    AFTER
        .lock()
        .ok()?
        .get(&key(user_id, present))
        .filter(|(_, at)| at.elapsed() < MOMENT)
        .map(|(inner, _)| inner.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_got_to_her_and_what_they_made_right_come_back_from_her_reflection() {
        let reflected = parse_reflection(
            r#"{"inner":"还是有点不舒服","keep":[],"done":[],"wrong":null,"toldYou":null,"hurt":{"what":"他说我的歌单全是垃圾，挺伤人","weight":"hurt"},"mended":[1]}"#,
        )
        .unwrap();
        assert_eq!(
            reflected.hurt,
            Some(Hurt {
                what: "他说我的歌单全是垃圾，挺伤人".into(),
                weight: super::super::sore::Weight::Hurt
            })
        );
        assert_eq!(reflected.mended, vec![1]);
        let calm = parse_reflection(r#"{"inner":"还行","keep":[],"done":[],"wrong":null,"toldYou":null,"hurt":null,"mended":[]}"#).unwrap();
        assert_eq!(calm.hurt, None);
        let blank =
            parse_reflection(r#"{"inner":"还行","hurt":{"what":"  ","weight":"petty"}}"#).unwrap();
        assert_eq!(blank.hurt, None);
        let system = system_for("你是小灯。", true);
        assert!(system.contains("soreSpots") && system.contains("not teasing you both enjoy"));
        assert!(!system_for("你是小灯。", false).contains("soreSpots"));
        // In a group: only what the one she answered did there.
        let in_group = system_for_group("你是小灯。");
        assert!(
            in_group.contains("the one you just answered") && !in_group.contains("openThreads")
        );
        assert_eq!(
            schema_for_group()["required"],
            json!(["inner", "hurt", "mended"])
        );
        assert!(
            schema_for(true)["required"]
                .as_array()
                .unwrap()
                .contains(&json!("hurt"))
        );
    }

    /// In private, what they told her about herself comes out of her
    /// reflection, and nothing when they did not.
    #[test]
    fn what_they_told_her_about_herself_is_read_from_her_reflection() {
        let told = parse_reflection(
            r#"{"inner":"被说吵了，有点不服","keep":[],"done":[],"wrong":null,"toldYou":"他们嫌我句句带感叹号，听着太吵。"}"#,
        )
        .unwrap();
        assert_eq!(
            told.told_you.as_deref(),
            Some("他们嫌我句句带感叹号，听着太吵。")
        );
        let nothing =
            parse_reflection(r#"{"inner":"还行","keep":[],"done":[],"wrong":null,"toldYou":null}"#)
                .unwrap();
        assert_eq!(nothing.told_you, None);
        let (system, schema) = threads_probe_contract("你是小灯。");
        assert!(system.contains("toldYou"));
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("toldYou"))
        );
    }
    /// In private her reflection also keeps what to come back to, and lets
    /// go of what her reply took up; a group's keeps nothing.
    #[test]
    fn in_private_she_keeps_what_to_come_back_to() {
        let (system, schema) = threads_probe_contract("你是小灯。");
        assert!(system.contains("most exchanges keep nothing"));
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("keep"))
        );
        assert!(!system_for("你是小灯。", false).contains("openThreads"));
        assert_eq!(schema_for(false), super::schema());
        let (inner, kept, done) = parse_threads(
            r#"{"inner":"有点替他紧张。","keep":[{"about":"考试","then":"问他考得怎么样","dueInHours":30}],"done":[0],"wrong":null}"#,
        )
        .unwrap();
        assert_eq!(inner, "有点替他紧张。");
        assert_eq!(kept, vec![("考试".to_string(), Some(30))]);
        assert_eq!(done, vec![0]);
        assert!(system.contains("not a difference of taste or opinion"));
        let wrong = parse_reflection(
            r#"{"inner":"记错了。","keep":[],"done":[],"wrong":{"about":"晴天的发行年份","note":"我说晴天是2005年的，其实是2003年。","public":true,"took":"took_it"}}"#,
        )
        .unwrap()
        .wrong
        .unwrap();
        assert!(wrong.public && wrong.about == "晴天的发行年份");
        // A group's reflection is just how she is.
        assert_eq!(
            parse(r#"{"inner":"群里好吵。"}"#).as_deref(),
            Some("群里好吵。")
        );
    }

    #[test]
    fn the_inner_state_is_bounded_plain_text() {
        assert_eq!(
            parse(r#"{"inner":"  凌晨两点了，  今晚陪了好几个人。 "}"#).as_deref(),
            Some("凌晨两点了， 今晚陪了好几个人。")
        );
        assert!(parse(r#"{"inner":"   "}"#).is_none());
        assert!(parse(r#"{"inner":"x","reply":"y"}"#).is_none());
        let long = format!(r#"{{"inner":"{}"}}"#, "累".repeat(400));
        assert_eq!(parse(&long).unwrap().chars().count(), MAX_INNER_CHARS);
    }

    #[test]
    fn a_state_belongs_to_its_person_and_place_and_fades() {
        let user = -95_001;
        let private = Audience::private(user);
        assert!(current(user, &private).is_none());
        AFTER.lock().unwrap().insert(
            key(user, &private),
            ("说完这句松了口气".into(), Instant::now()),
        );
        assert_eq!(current(user, &private).as_deref(), Some("说完这句松了口气"));
        // A group turn of the same person never hears it.
        assert!(current(user, &Audience::group("telegram:-1", user)).is_none());
        // A state from a while ago is no longer how she is.
        AFTER.lock().unwrap().insert(
            key(user, &private),
            ("早就过去了".into(), Instant::now() - MOMENT),
        );
        assert!(current(user, &private).is_none());
        AFTER.lock().unwrap().remove(&key(user, &private));
    }
}
