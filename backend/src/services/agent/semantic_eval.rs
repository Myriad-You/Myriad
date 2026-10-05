//! Explicit offline/export/live semantic evaluation; never writes application state.
use std::{
    collections::HashSet,
    io::Write,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{chat_prompt, consciousness as event, merope::chat_remember as memory};

/// Shared acceptance loader. Credentials stay in the host; every connection is read-only.
pub(super) async fn load_configured_lite() -> sea_orm::DatabaseConnection {
    use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement};
    dotenvy::dotenv().ok();
    // Do not let the key loader create a new key while doing read-only acceptance.
    let data_root = std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into());
    assert!(
        std::env::var("MYRIAD_DATA_KEY").is_ok()
            || std::path::Path::new(&data_root)
                .join(".secret-key")
                .is_file(),
        "an existing host data key is required"
    );
    let url = std::env::var("DATABASE_URL").expect("host DATABASE_URL required");
    let mut url = url::Url::parse(&url).unwrap_or_else(|_| panic!("invalid host database URL"));
    // Every connection is read-only, including reconnects. No startup/migrations,
    // memory recall, user records, notifications or ledger writes are involved.
    let pairs: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key != "options")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(pairs)
        .append_pair("options", "-c default_transaction_read_only=on");
    let mut options = ConnectOptions::new(url.to_string());
    options.max_connections(1).sqlx_logging(false);
    let db = Database::connect(options)
        .await
        .unwrap_or_else(|_| panic!("read-only database unavailable"));
    let readonly = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SHOW default_transaction_read_only",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        readonly
            .try_get::<String>("", "default_transaction_read_only")
            .unwrap(),
        "on"
    );
    // The site may not have restarted onto this version yet: read its old
    // settings the way its startup will rewrite them.
    let config = crate::services::config_service::ConfigService::load_config_upgraded_on(&db)
        .await
        .unwrap_or_else(|_| panic!("cannot load host model configuration"));
    *crate::GLOBAL_DYNAMIC_CONFIG.write().await = config;
    db
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    #[serde(default)]
    touch: Option<Value>,
    #[serde(default)]
    soul: Option<String>,
    #[serde(default)]
    arousal: Option<f64>,
    #[serde(default)]
    activity: String,
    #[serde(default)]
    remaining_ms: Option<u64>,
    #[serde(default)]
    local_reaction: Option<String>,
    #[serde(default)]
    consistency_group: Option<String>,
    id: String,
    kind: String,
    input: String,
    rubric: String,
    #[serde(default)]
    page: Option<Value>,
    #[serde(default)]
    selection: Option<String>,
    #[serde(default)]
    track: Option<String>,
    #[serde(default)]
    remembered: Vec<String>,
    #[serde(default)]
    reply: String,
    #[serde(default)]
    fact_present: bool,
    #[serde(default)]
    supersedes: Vec<String>,
    #[serde(default)]
    event_kind: String,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    repeated: bool,
    #[serde(default)]
    do_not_disturb: bool,
    #[serde(default)]
    rig: Option<Value>,
    #[serde(default)]
    mood: Option<f64>,
    #[serde(default)]
    previous_phrases: Vec<myriad_merope::SpeechPhrase>,
    // Mind cases. Skipped when empty so older cases keep their replay hashes.
    /// Earlier turns, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    history: Vec<HistoryLine>,
    /// Her unprompted lines, placed into the history as production does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    said_unprompted: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    own_days: Vec<String>,
    /// Memories their words brought to mind without naming them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    brought_to_mind: Vec<String>,
    /// Songs she heard and liked, as her chat is offered them to play;
    /// with them the chat has a player.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    songs_to_share: Vec<String>,
    /// The chat is in a chat app: no player, songs go as links.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    in_chat_app: bool,
    /// How long she has known them: days since they first wrote, and on how
    /// many different days they have written; `[0, 1]` is their first time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    known: Option<(i64, u32)>,
    /// What she made of a group's talk before answering, as production puts
    /// it in the prompt (see `merope::making_sense`).
    #[serde(default, rename = "madeSense", skip_serializing_if = "Option::is_none")]
    made_sense: Option<String>,
    /// A group turn of her own: what she brings up (`what`, as she took it
    /// in) and why (`why`), as production gives it (see `sharing::reason`).
    #[serde(default)]
    shares: Option<Value>,
    /// How her last talks with others left her: (feeling off even, hours ago).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    carried: Vec<(f64, f64)>,
    /// Her stickers offered this turn (what each means), and how many more
    /// she can make this month.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stickers: Vec<String>,
    /// What she has been doing on end, for her choice of what to do next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    same_thing_lately: Option<String>,
    /// A group's running jokes (handle, how it goes), or memes she learned
    /// (term, what it means), for the night she thinks of making one a
    /// sticker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    jokes: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stickers_left: Option<usize>,
    /// Her compiled inner state for this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inner: Option<String>,
    /// A thing she knows only a little about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gap: Option<String>,
    /// Facts of her day, as the decision and inner calls see them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    myself: Option<Value>,
    /// Raw search results for a digest case (fenced as production does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    search_results: Option<String>,
    /// Images attached to the message, files under `tests/merope/images/`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
    /// Things at hand on her own time (`doing_choice`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    options: Option<Value>,
    /// What she did on her own lately, one line each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lately: Vec<String>,
    /// What she has looked at so far on a trip (`explore_step`), or all of
    /// it (`explore_compare`): `{"kind","at","title","text"}` each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    looked: Vec<Value>,
    /// What she guessed after the last part of a serial (`doing_digest`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guessed: Option<String>,
    /// What happened, `[line, missed]` each, for looking back (`self_story`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    records: Vec<(String, bool)>,
    /// What she wrote about herself last time she looked back (`self_story`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    story_before: Vec<String>,
    /// What she wrote the times she had this same thing before
    /// (`doing_digest`), one line each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    heard_before: Vec<String>,
    /// Lyrics or a note she just took in (`doing_digest`), untrusted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<String>,
    /// Her own time as a chat turn sees it: `{"now": …, "lately": [[what, stayed]]}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    own_time: Option<Value>,
    /// Her own experiences, `{"what", "stayed"}` each (`views`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    experiences: Vec<Value>,
    /// Views she holds, `[about, view]` each: going over (`views`) or touched
    /// by their words (chat).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    views: Vec<(String, String)>,
    /// What they are playing, as she sees it on their Steam status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    playing: Option<String>,
    /// A turtle soup: `{"surface", "truth", "keys", "verdict"?}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    soup: Option<Value>,
    /// What the referee must say: `{"verdict", "solved", "gave_up"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expect: Option<Value>,
    /// A private IM chat where she may hand work off: `{"busy"?, "handedOff"?}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel: Option<Value>,
    /// Bits between her and them, `[handle, how]` each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bits: Vec<(String, String)>,
    /// A persona of its own, `[name, personality]`, instead of the contract
    /// persona (chat cases).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    persona: Option<(String, String)>,
    /// Open threads, `[about, then]` each: what she meant to come back to
    /// (`threads`), or what has come due (`reach_judge`, chat).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    threads: Vec<(String, String)>,
    /// Days since they last talked (`reach_judge`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    days_since: Option<i64>,
    /// She is writing to them first, about this (chat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    writing_first: Option<String>,
    /// The conversation is a group chat's (`bits`, `chime`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    in_group: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HistoryLine {
    role: String,
    text: String,
    /// Her line was cut off: `partial` (typed) or `unheard_end` (spoken).
    #[serde(default, rename = "cutOff", skip_serializing_if = "Option::is_none")]
    cut_off: Option<String>,
    /// Who said it, in a group chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// When, in seconds, for a replayed chat; lines without one are each
    /// their own turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    at: Option<i64>,
}

/// How the people in a group case type, from its history, as production
/// reads a group's lines.
fn case_room(case: &Case) -> Option<myriad_merope::talk_shape::Shape> {
    let lines: Vec<(&str, i64, &str)> = kept_lines(case)
        .iter()
        .enumerate()
        .filter(|(_, line)| line.role == "user")
        .map(|(index, line)| {
            (
                line.name.as_deref().unwrap_or(""),
                line.at.unwrap_or(index as i64 * 3600),
                line.text.as_str(),
            )
        })
        .collect();
    myriad_merope::talk_shape::room_of(&lines)
}

/// How her lines differ from the people's in a group case's history, as
/// production tells it before the ledger has enough.
fn case_differs(case: &Case) -> Option<String> {
    use myriad_merope::contrast::{Counts, describe, overused};
    let kept = |hers: bool| {
        Counts::of(
            case.history
                .iter()
                .filter(|line| (line.role == "assistant") == hers)
                .flat_map(|line| line.text.lines()),
        )
    };
    describe(&overused(&kept(true), &kept(false)))
}

/// What production sends of a reply typed in a chat app: as many messages
/// as it rolls for (the roll made from the row id, so a replay sends the
/// same), typed the way the room types, or chat apps usually do. Stickers,
/// games and the like go apart from her words, so their markers are no
/// message.
fn as_sent(id: &str, reply: &str, room: Option<&myriad_merope::talk_shape::Shape>) -> Vec<String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let roll = (hasher.finish() % 1_000_000) as f64 / 1_000_000.0;
    let said = myriad_merope::strangers::without_directives(reply);
    let lines = myriad_agent_rules::channel::as_messages(&said);
    let most = myriad_merope::talk_shape::messages_this_turn(lines.len(), roll);
    myriad_merope::talk_shape::goes_out_as(&lines, most, room)
}

/// Mind cases wear the production persona contract (no body, own words).
/// A case's threads, all come due.
fn eval_threads(case: &Case) -> Vec<super::merope::threads::Thread> {
    case.threads
        .iter()
        .enumerate()
        .map(|(index, (about, then))| super::merope::threads::Thread {
            id: index.to_string(),
            about: about.clone(),
            then: then.clone(),
            due: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
            hers: false,
        })
        .collect()
}

/// Her own persona, read from the site in live runs, as (name, soul).
/// Offline, or with `MEROPE_SEMANTIC_PERSONA=contract`, the contract
/// persona stands in.
static HERS: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();

/// A group case's lines as the heard judgment reads them.
fn heard_lines(case: &Case) -> Vec<myriad_merope::heard::Said> {
    case.history
        .iter()
        .map(|line| myriad_merope::heard::Said {
            name: line.name.clone().unwrap_or_default(),
            text: line.text.clone(),
            hers: line.role != "user",
        })
        .collect()
}

/// What she has, for a group case, as production gathers it: her views on
/// the talk, what she did that it brings up, what she would tell someone.
fn chime_material(case: &Case) -> Vec<myriad_merope::joining::Material> {
    use myriad_merope::joining::Material;
    let mut material: Vec<Material> = case
        .views
        .iter()
        .map(|(about, view)| Material {
            kind: "your_view",
            text: format!("{about}: {view}"),
        })
        .collect();
    let own = case.own_time.as_ref();
    for pair in own
        .and_then(|own| own["lately"].as_array())
        .into_iter()
        .flatten()
    {
        if let (Some(what), Some(stayed)) = (pair[0].as_str(), pair[1].as_str()) {
            material.push(Material {
                kind: "you_did",
                text: format!("{what}: {stayed}"),
            });
        }
    }
    for told in own
        .and_then(|own| own["wouldTell"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        material.push(Material {
            kind: "you_would_tell",
            text: told.to_string(),
        });
    }
    material
}

fn default_soul() -> String {
    HERS.get()
        .map(|(_, soul)| soul.clone())
        .unwrap_or_else(contract_soul)
}

fn default_name() -> String {
    HERS.get()
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| "小灯".into())
}

/// The case's own persona, or hers.
fn case_soul(case: &Case) -> String {
    match &case.persona {
        Some((name, personality)) => persona_soul(name, personality),
        None => default_soul(),
    }
}

fn persona_soul(name: &str, personality: &str) -> String {
    let persona = crate::models::entities::agent_persona::Model {
        id: "site".into(),
        name: name.into(),
        personality: personality.into(),
        persona_json: None,
        visual_profile: None,
        portrait_asset_id: None,
        portrait_generation: None,
        avatar_asset_id: None,
        avatar_generation: None,
        updated_by: None,
        updated_at: "2026-01-01T00:00:00Z".parse().unwrap(),
    };
    super::merope::format_persona(&persona).unwrap()
}

fn contract_soul() -> String {
    let persona = crate::models::entities::agent_persona::Model {
        id: "site".into(),
        name: "小灯".into(),
        personality: "好奇、直接，说话自然，跟人聊天不端着。".into(),
        persona_json: None,
        visual_profile: None,
        portrait_asset_id: None,
        portrait_generation: None,
        avatar_asset_id: None,
        avatar_generation: None,
        updated_by: None,
        updated_at: "2026-01-01T00:00:00Z".parse().unwrap(),
    };
    super::merope::format_persona(&persona).unwrap()
}

fn is_mind_case(case: &Case) -> bool {
    !case.history.is_empty()
        || !case.said_unprompted.is_empty()
        || !case.own_days.is_empty()
        || !case.brought_to_mind.is_empty()
        || case.inner.is_some()
        || case.gap.is_some()
        || !case.images.is_empty()
        || case.own_time.is_some()
        || !case.views.is_empty()
        || case.playing.is_some()
        || case.soup.is_some()
        || case.channel.is_some()
        || !case.bits.is_empty()
        || case.in_group
        || case.writing_first.is_some()
        || case.known.is_some()
        || !case.carried.is_empty()
}

/// A group turn as production builds it: the group's lines, named, as the
/// transcript; the one speaking to her is 阿明.
/// Lines of a group she keeps in mind, as production keeps them; a longer
/// history stands for what her ledger has counted.
const GROUP_LINES_KEPT: usize = 30;

/// The part of a group case's history she has in mind.
fn kept_lines(case: &Case) -> &[HistoryLine] {
    &case.history[case.history.len().saturating_sub(GROUP_LINES_KEPT)..]
}

fn group_chat_prompt(case: &Case) -> String {
    let transcript: Vec<super::ConversationMessage> = kept_lines(case)
        .iter()
        .map(|line| super::ConversationMessage {
            role: line.role.clone(),
            content: match (line.role.as_str(), line.name.as_deref()) {
                ("user", Some(name)) => format!("{name}：{}", line.text),
                _ => line.text.clone(),
            },
            created_at: None,
        })
        .collect();
    let soup = case.soup.as_ref().map(|soup| {
        if soup.get("offer").is_some() {
            return super::merope::soup::GROUP_OFFER.to_string();
        }
        let verdict: super::merope::soup::Verdict =
            serde_json::from_value(soup["verdict"].clone()).expect("soup verdict");
        super::merope::soup::section_for_eval(
            soup["surface"].as_str().unwrap_or(""),
            soup["truth"].as_str().unwrap_or(""),
            verdict,
            true,
            soup["asker"].as_str().or(Some("阿明")),
        )
    });
    let sections: Vec<String> = [
        Some(super::merope::group_speaking_section("阿明")),
        Some(myriad_merope::speaking::chat_app_section().to_string()),
        case_room(case)
            .map(|room| myriad_merope::talk_shape::describe(&room, "How people type here")),
        case_differs(case),
        case.made_sense.clone(),
        case.own_time.as_ref().and_then(|own| {
            let lately: Vec<(String, String)> =
                serde_json::from_value(own["lately"].clone()).unwrap_or_default();
            super::merope::format_doing_section(own["now"].as_str(), &lately)
        }),
        case.own_time
            .as_ref()
            .and_then(|own| own["now"].as_str())
            .and_then(|now| {
                let hers: Vec<&str> = kept_lines(case)
                    .iter()
                    .filter(|line| line.role == "assistant")
                    .flat_map(|line| line.text.lines())
                    .collect();
                myriad_merope::speaking::format_already_told_section(
                    &myriad_merope::speaking::already_told(now, &hers),
                )
            }),
        super::merope::format_remembered_section(&case.remembered),
        super::merope::format_views_section(&case.views),
        super::merope::format_bits_section(&case.bits, true),
        soup,
        case.shares.as_ref().map(|shares| {
            myriad_merope::joining::speaking_up_section(&myriad_merope::sharing::reason(
                shares["what"].as_str().unwrap_or(""),
                shares["why"].as_str().unwrap_or(""),
            ))
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
    chat_prompt::build_group_chat_prompt(
        &case_soul(case),
        &sections.join("\n\n"),
        &transcript,
        &case.input,
    )
}

/// A case's attached images, checked as production checks an upload.
fn case_images(case: &Case) -> Vec<crate::services::analyzer::ImageInput> {
    use base64::Engine as _;
    case.images
        .iter()
        .map(|name| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/merope/images")
                .join(name);
            let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("missing image {name}"));
            let url = format!(
                "data:image/*;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            );
            let mut data = json!({ "attachments": [{ "image": url }] });
            super::chat_attachments::take_images(Some(&mut data))
                .pop()
                .unwrap_or_else(|| panic!("{name} is not an accepted image"))
        })
        .collect()
}

fn mind_chat_prompt(case: &Case) -> String {
    if case.in_group {
        return group_chat_prompt(case);
    }
    let base: chrono::DateTime<chrono::Utc> = "2026-01-01T10:00:00Z".parse().unwrap();
    let history: Vec<super::ConversationMessage> = case
        .history
        .iter()
        .enumerate()
        .map(|(index, line)| {
            // Read back as production reads a stored row.
            chat_prompt::reconstruct_conversation_message(
                line.role.clone(),
                line.text.clone(),
                Some((base + chrono::Duration::minutes(index as i64)).to_rfc3339()),
                line.cut_off
                    .as_ref()
                    .map(|kind| json!({ chat_prompt::CUT_OFF_KEY: kind }))
                    .as_ref(),
                true,
            )
        })
        .collect();
    let said: Vec<(chrono::DateTime<chrono::Utc>, String)> = case
        .said_unprompted
        .iter()
        .enumerate()
        .map(|(index, line)| {
            (
                base + chrono::Duration::hours(1 + index as i64),
                line.clone(),
            )
        })
        .collect();
    let history = super::merope::merge_said_unprompted(&history, &said);
    // Same order as production: her inner state last, nearest their words.
    let now = chrono::Utc::now();
    let sections: Vec<String> = [
        case.known.map(|(days_ago, days)| {
            myriad_merope::speaking::format_acquaintance_section(
                Some(now - chrono::Duration::days(days_ago)),
                days,
                now,
            )
        }),
        myriad_merope::speaking::format_carried_section(&case.carried),
        super::merope::format_remembered_section(&case.remembered),
        super::merope::format_brought_to_mind_section(&case.brought_to_mind),
        case.gap
            .as_deref()
            .and_then(|gap| super::merope::format_curious_section(gap, 1)),
        super::merope::format_own_days_section(&case.own_days),
        super::merope::format_views_section(&case.views),
        super::merope::format_bits_section(&case.bits, case.in_group),
        super::merope::threads::section(&eval_threads(case), chrono::Utc::now()),
        case.writing_first
            .as_deref()
            .map(super::merope::reach::writing_first_section),
        case.channel.as_ref().map(|channel| {
            super::delegate::section(&super::types::ChannelChat {
                handed_off: channel["handedOff"].as_str().map(str::to_string),
                busy: channel["busy"].as_bool().unwrap_or(false),
            })
        }),
        case.playing
            .as_deref()
            .and_then(super::merope::format_playing_section),
        case.soup
            .as_ref()
            .filter(|soup| soup.get("offer").is_some())
            .map(|_| super::merope::soup::OFFER.to_string()),
        case.soup
            .as_ref()
            .filter(|soup| soup.get("offer").is_none())
            .map(|soup| {
                let verdict: super::merope::soup::Verdict =
                    serde_json::from_value(soup["verdict"].clone()).expect("soup verdict");
                super::merope::soup::section_for_eval(
                    soup["surface"].as_str().unwrap_or(""),
                    soup["truth"].as_str().unwrap_or(""),
                    verdict,
                    false,
                    None,
                )
            }),
        case.own_time.as_ref().and_then(|own| {
            let lately: Vec<(String, String)> =
                serde_json::from_value(own["lately"].clone()).unwrap_or_default();
            super::merope::format_doing_section(own["now"].as_str(), &lately)
        }),
        case.inner
            .as_deref()
            .and_then(super::merope::format_inner_moment_ago_section),
    ]
    .into_iter()
    .flatten()
    .collect();
    chat_prompt::build_chat_lite_prompt_with_perception(
        &case_soul(case),
        &sections.join("\n\n"),
        &history,
        &case.input,
        &super::chat_attachments::format_attached(None, case.images.len()),
    )
}

fn cases() -> Vec<Case> {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../../../../tests/merope/semantic-cases.json")).unwrap();
    let mut cases = cases;
    cases.extend(
        serde_json::from_str::<Vec<Case>>(include_str!(
            "../../../../tests/merope/touch-semantic-cases.json"
        ))
        .unwrap(),
    );
    cases.extend(
        serde_json::from_str::<Vec<Case>>(include_str!(
            "../../../../tests/merope/touch-response-cases.json"
        ))
        .unwrap(),
    );
    cases.extend(
        serde_json::from_str::<Vec<Case>>(include_str!("../../../../tests/merope/mind-cases.json"))
            .unwrap(),
    );
    // Cases kept out of the repository, such as ones that carry a real
    // persona: a JSON array at this path, run like any other.
    if let Ok(path) = std::env::var("MEROPE_SEMANTIC_EXTRA_CASES") {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("cannot read {path}"));
        cases.extend(serde_json::from_str::<Vec<Case>>(&text).unwrap());
    }
    let mut ids = HashSet::new();
    for case in &cases {
        assert!(ids.insert(&case.id) && !case.id.is_empty());
        assert!(!case.rubric.trim().is_empty());
        assert!(matches!(
            case.kind.as_str(),
            "chat"
                | "memory"
                | "event"
                | "motion"
                | "touch"
                | "wonder"
                | "heard"
                | "already"
                | "sense"
                | "joke_sticker"
                | "meme_sticker"
                | "found_out"
                | "inner"
                | "own_day"
                | "bits"
                | "chime"
                | "threads"
                | "reach_judge"
                | "stranger_note"
                | "soup_start"
                | "soup_judge"
                | "views"
                | "doing_choice"
                | "doing_digest"
                | "self_story"
                | "serial_guess"
                | "wonder_own"
                | "explore_think"
                | "explore_step"
                | "explore_compare"
        ));
    }
    cases
}

fn eval_looked(case: &Case) -> Vec<super::merope::explore::Looked> {
    case.looked
        .iter()
        .filter_map(|looked| serde_json::from_value(looked.clone()).ok())
        .collect()
}

/// A trip: the question (input), what she thought first (reply) and how
/// sure she was (material), and what she looked at.
fn eval_trip(case: &Case) -> super::merope::explore::Trip {
    super::merope::explore::Trip {
        question: case.input.clone(),
        thought: case.reply.clone(),
        sure: case.material.clone().unwrap_or_else(|| "unsure".into()),
        depends_on_now: false,
        looked: eval_looked(case),
    }
}

fn event_context(case: &Case) -> (event::ConsciousnessEvent, event::SelfSnapshot) {
    // Stable timestamps make exported request hashes reproducible across runs.
    let now = "2026-01-01T00:00:00Z".parse().unwrap();
    let event = event::ConsciousnessEvent {
        id: case.id.clone(),
        source: "semantic_fixture".into(),
        kind: case.event_kind.clone(),
        headline: "合成测试事件".into(),
        summary: if case.event_kind == "agent.merope.touch" {
            super::merope::touch::completion_summary(
                &serde_json::from_value(case.touch.clone().unwrap()).unwrap(),
            )
        } else {
            case.input.clone()
        },
        addressee_user_id: 701,
        urgency: event::EventUrgency::Normal,
        occurred_at: now,
        parent_event_id: None,
        safe_facts: Default::default(),
    };
    let snapshot = event::SelfSnapshot {
        persona_name: default_name(),
        addressee_user_id: 701,
        interaction_mode: super::AgentInteractionMode::Chat,
        mood: case.mood.unwrap_or(65.0),
        activity: if case.activity.is_empty() {
            "idle".into()
        } else {
            case.activity.clone()
        },
        do_not_disturb: case.do_not_disturb,
        has_active_work: false,
        granted_permissions: vec![],
        recent_intents: vec![],
        remembered: case.remembered.clone(),
        captured_at: now,
        live: event::SelfLivePresence {
            page_visible: true,
            face_visible: true,
            captured_at: Some(now),
            ..Default::default()
        },
        attention: case.repeated.then(|| event::AttentionSegment {
            topic: case.event_kind.clone(),
            inner: "已经对这件事回应过，没有新信息".into(),
            opened_at: now,
            last_touched_at: now,
            event_ids: vec![case.id.clone()],
        }),
        myself: None,
        addressee_name: None,
        minutes_since_you_spoke_unasked: None,
        inner: None,
    };
    (event, snapshot)
}

fn request(case: &Case) -> Value {
    match case.kind.as_str() {
        "touch" => super::merope::touch::appraisal_contract(
            &case.soul.clone().unwrap_or_else(default_soul),
            &serde_json::from_value(case.touch.clone().expect("touch summary required")).unwrap(),
            case.mood.unwrap_or(70.0),
            case.arousal.unwrap_or(48.0),
            &case.activity,
        ),
        "motion" => {
            let mut context = super::motion_overlay::motion_refinement_tests::context();
            context.phase = super::merope::MotionPhase::Delivery;
            context.activity = "talking".into();
            context.user_text = case.input.clone();
            context.response_text = Some(case.reply.clone());
            context.previous_phrases = case.previous_phrases.clone();
            context.rig_state = case
                .rig
                .as_ref()
                .and_then(myriad_merope::sanitize_rig_state);
            if let Some(mood) = case.mood {
                context.mood.before = mood;
                context.mood.after = mood;
                context.mood.band_before =
                    super::merope::state::mood_band(mood, context.mood.arousal_before).into();
                context.mood.band_after =
                    super::merope::state::mood_band(mood, context.mood.arousal_after).into();
            }
            let mut contract = super::merope::motion::semantic_contract(&context);
            if let Some(soul) = &case.soul {
                let mut input: Value =
                    serde_json::from_str(contract["input"].as_str().unwrap()).unwrap();
                input["persona"]["name"] = json!("小灯");
                input["persona"]["personality"] = json!(soul);
                contract["input"] = json!(input.to_string());
            }
            contract
        }
        "chat" if is_mind_case(case) => {
            let mut request = json!({"input":mind_chat_prompt(case),"schema":null,"schemaName":null,"system":null});
            if !case.images.is_empty() {
                request["images"] = json!(case.images);
            }
            request
        }
        "wonder" => {
            let (system, schema) = super::merope::curiosity::wonder_probe_contract(&default_soul());
            json!({"system":system,"schema":schema,"schemaName":"merope_wonder",
                "input":json!({"userText":case.input,"reply":case.reply,"myself":case.myself}).to_string()})
        }
        "found_out" => {
            let (system, schema) =
                super::merope::curiosity::digest_probe_contract(&default_soul(), &case.input);
            let results = case
                .search_results
                .clone()
                .expect("search results required");
            json!({"system":system,"schema":schema,"schemaName":"merope_found_out",
                "input":myriad_agent_rules::untrusted_block("search_results", &results)})
        }
        "own_day" => {
            let system = super::merope::life::own_day_probe_contract(&default_soul());
            json!({"system":system,"schema":null,"schemaName":null,
                "input":json!({"day":"Wed","dayFacts":case.myself,"onYourOwn":case.lately,"earlierEntries":case.own_days}).to_string()})
        }
        "doing_choice" => {
            let options = case.options.clone().expect("options required");
            let count = options.as_array().map(Vec::len).unwrap_or(0);
            let (system, schema) =
                super::merope::doing::choice_probe_contract(&case_soul(case), count);
            json!({"system":system,"schema":schema,"schemaName":"merope_doing_choice",
                "input":json!({"myself":case.myself,"lately":case.lately,"sameThingLately":case.same_thing_lately,"options":options}).to_string()})
        }
        "bits" => {
            let (system, schema) =
                super::merope::bits::probe_contract(&default_soul(), case.in_group);
            let conversation: Vec<Value> = case
                .history
                .iter()
                .map(|line| {
                    let who = match (line.role.as_str(), line.name.as_deref()) {
                        ("user", Some(name)) => name,
                        ("user", None) => "they",
                        _ => "you",
                    };
                    json!({"who": who, "text": line.text})
                })
                .collect();
            let bits: Vec<Value> = case
                .bits
                .iter()
                .map(|(handle, how)| json!({"handle": handle, "how": how}))
                .collect();
            json!({"system":system,"schema":schema,"schemaName":"merope_bits",
                "input":json!({"bits":bits,"conversation":conversation}).to_string()})
        }
        "stranger_note" => {
            let (system, schema) = super::merope::strangers::note_probe_contract(&default_soul());
            // Each of their lines with her answer, in order.
            let exchanges: Vec<Value> = case
                .history
                .chunks(2)
                .map(|pair| {
                    json!({"they":pair[0].text,"you":pair.get(1).map(|line| line.text.as_str()).unwrap_or_default()})
                })
                .collect();
            json!({"system":system,"schema":schema,"schemaName":"merope_stranger_note",
                "input":json!({"name":"阿明","remembered":case.remembered.first(),
                    "exchanges":exchanges}).to_string()})
        }
        "sense" => {
            // The line she answers is the case's input, from 阿明 unless the
            // history's speaker is named in the input as `name：text`.
            let mut conversation: Vec<String> = case
                .history
                .iter()
                .map(|line| match (line.role.as_str(), line.name.as_deref()) {
                    ("user", Some(name)) => format!("{name}：{}", line.text),
                    ("user", None) => line.text.clone(),
                    _ => format!("you：{}", line.text),
                })
                .collect();
            conversation.push(if case.input.contains('：') {
                case.input.clone()
            } else {
                format!("阿明：{}", case.input)
            });
            json!({"system":myriad_merope::making_sense::system(&default_soul()),
                "schema":myriad_merope::making_sense::schema(),
                "schemaName":myriad_merope::making_sense::SCHEMA_NAME,
                "input":myriad_merope::making_sense::input(&conversation)})
        }
        "chime" => {
            let conversation: Vec<String> = case
                .history
                .iter()
                .map(|line| match (line.role.as_str(), line.name.as_deref()) {
                    ("user", Some(name)) => format!("{name}：{}", line.text),
                    ("user", None) => line.text.clone(),
                    _ => format!("you：{}", line.text),
                })
                .collect();
            let (system, schema, input) = super::merope::joining::probe(
                &default_soul(),
                &conversation,
                &chime_material(case),
            );
            json!({"system":system,"schema":schema,"schemaName":myriad_merope::joining::SCHEMA_NAME,
                "input":input})
        }
        "soup_start" => {
            let (system, schema) = super::merope::soup::start_probe_contract(&default_soul());
            json!({"system":system,"schema":schema,"schemaName":"merope_soup_start",
                "input":json!({"theirWords":case.input,"setting":case.reply,"recentSurfaces":[]}).to_string()})
        }
        "soup_judge" => {
            let soup = case.soup.clone().expect("soup required");
            let keys: Vec<String> =
                serde_json::from_value(soup["keys"].clone()).unwrap_or_default();
            let (system, input, schema) = super::merope::soup::judge_probe(
                soup["surface"].as_str().unwrap_or(""),
                soup["truth"].as_str().unwrap_or(""),
                &keys,
                &case.input,
            );
            json!({"system":system,"schema":schema,"schemaName":"merope_soup_judge","input":input})
        }
        "views" => {
            let (system, schema) = super::merope::views::probe_contract(&default_soul());
            let experiences: Vec<Value> = case
                .experiences
                .iter()
                .enumerate()
                .map(|(index, experience)| {
                    json!({"index": index, "what": experience["what"], "stayed": experience["stayed"]})
                })
                .collect();
            let views: Vec<Value> = case
                .views
                .iter()
                .map(|(about, view)| json!({"about": about, "view": view}))
                .collect();
            json!({"system":system,"schema":schema,"schemaName":"merope_views",
                "input":json!({"experiences":experiences,"views":views}).to_string()})
        }
        "doing_digest" => {
            let (system, schema) = super::merope::doing::digest_probe_contract(
                &case_soul(case),
                &case.input,
                &case.reply,
                case.material.as_deref(),
            );
            let views: Vec<String> = case
                .views
                .iter()
                .map(|(about, view)| format!("{about}: {view}"))
                .collect();
            let input = super::merope::doing::digest_probe_input(
                case.material.as_deref(),
                &views,
                &case.heard_before,
                case.guessed.as_deref(),
            );
            json!({"system":system,"schema":schema,"schemaName":"merope_doing_digest","input":input})
        }
        "joke_sticker" => {
            let input = json!({
                "jokes": case.jokes.iter().enumerate()
                    .map(|(index, (handle, how))| json!({"index": index, "joke": handle, "how": how}))
                    .collect::<Vec<_>>(),
                "stickersHere": case.stickers,
                "leftThisMonth": 20,
            });
            json!({"system":myriad_merope::stickers::joke_system(&default_soul()),
                "schema":myriad_merope::stickers::joke_schema(),
                "schemaName":myriad_merope::stickers::JOKE_SCHEMA,"input":input.to_string()})
        }
        "meme_sticker" => {
            let input = json!({
                "memes": case.jokes.iter().enumerate()
                    .map(|(index, (term, means))| json!({"index": index, "meme": term, "means": means}))
                    .collect::<Vec<_>>(),
                "yourStickers": case.stickers,
                "leftThisMonth": 20,
            });
            json!({"system":myriad_merope::stickers::meme_system(&default_soul()),
                "schema":myriad_merope::stickers::joke_schema(),
                "schemaName":myriad_merope::stickers::MEME_SCHEMA,"input":input.to_string()})
        }
        "already" => {
            // The new fact is the input, the fact she had the first
            // remembered; the verdicts that would be right are `actions`.
            use myriad_merope::chat_remember::{
                ALREADY_SCHEMA_NAME, already_input, already_schema, already_system,
            };
            let known = case
                .remembered
                .first()
                .map(String::as_str)
                .unwrap_or_default();
            json!({"system":already_system(),"schema":already_schema(),
                "schemaName":ALREADY_SCHEMA_NAME,"input":already_input(&[(case.input.as_str(), known)])})
        }
        "heard" => {
            let lines = heard_lines(case);
            json!({"system":myriad_merope::heard::system(),"schema":myriad_merope::heard::schema(),
                "schemaName":myriad_merope::heard::SCHEMA_NAME,"input":myriad_merope::heard::input(&lines)})
        }
        "wonder_own" => {
            let (system, schema, input, _) =
                super::merope::explore::wonder_probe(&case_soul(case), &case.records, &case.lately);
            json!({"system":system,"schema":schema,"schemaName":"merope_wonder_own","input":input})
        }
        "explore_think" => {
            let (system, schema) =
                super::merope::explore::think_probe(&case_soul(case), &case.input);
            json!({"system":system,"schema":schema,"schemaName":"merope_explore_think","input":"(nothing looked up)"})
        }
        "explore_step" => {
            let (system, schema, input) =
                super::merope::explore::step_probe(&case.input, &case.reply, &eval_looked(case));
            json!({"system":system,"schema":schema,"schemaName":"merope_explore_step","input":input})
        }
        "explore_compare" => {
            let (system, schema, input) = super::merope::explore::compare_probe(&eval_trip(case));
            json!({"system":system,"schema":schema,"schemaName":"merope_explore_compare","input":input})
        }
        "serial_guess" => {
            let (system, schema) = super::merope::serial::judge_probe_contract();
            let input = json!({"guess": case.input, "nextPart": case.material}).to_string();
            json!({"system":system,"schema":schema,"schemaName":"merope_serial_guess","input":input})
        }
        "self_story" => {
            let (system, schema) = super::merope::self_story::probe_contract(&case_soul(case));
            let (input, _, _) =
                super::merope::self_story::probe_input(&case.records, &case.story_before);
            json!({"system":system,"schema":schema,"schemaName":"merope_self_story","input":input})
        }
        "threads" => {
            // Her private reflection, keeping what to come back to.
            let (system, schema) = super::merope::inner::threads_probe_contract(&default_soul());
            let history: Vec<Value> = case
                .history
                .iter()
                .map(|line| json!({"role":line.role,"text":line.text}))
                .collect();
            let input = json!({"userText":case.input,"yourReply":case.reply,"history":history,
                "feelingTowardThem":super::merope::mood_tone_instruction(70.0, 48.0),
                "myself":case.myself,"remembered":case.remembered,
                "openThreads":super::merope::threads::as_input(&eval_threads(case))});
            json!({"system":system,"schema":schema,"schemaName":"merope_inner",
                "input":input.to_string()})
        }
        "reach_judge" => {
            let (system, schema) = super::merope::reach::judge_probe_contract(&default_soul());
            let input = json!({"name":"阿明","localTime":"Saturday 19:30",
                "daysSinceYouTalked":case.days_since,
                "dueNow":case.threads.iter().map(|(about, then)| json!({"about":about,"then":then})).collect::<Vec<_>>(),
                "remembered":case.remembered,
                "yourOwnTime":{"now":case.own_time.as_ref().and_then(|own| own["now"].as_str()),
                    "wouldTell":case.own_time.as_ref().map(|own| own["wouldTell"].clone()).filter(Value::is_array).unwrap_or(json!([]))},
                "recentTalk":case.history.iter().map(|line| format!("{}: {}", if line.role == "user" { "they" } else { "you" }, line.text)).collect::<Vec<_>>()});
            json!({"system":system,"schema":schema,"schemaName":"merope_reach_out",
                "input":input.to_string()})
        }
        "inner" => {
            // Written after she answered, as production does.
            let (system, schema) = super::merope::inner::probe_contract(&default_soul());
            let history: Vec<Value> = case
                .history
                .iter()
                .map(|line| json!({"role":line.role,"text":line.text}))
                .collect();
            let mut input = json!({"userText":case.input,"history":history,
                "feelingTowardThem":super::merope::mood_tone_instruction(case.mood.unwrap_or(70.0), case.arousal.unwrap_or(48.0)),
                "myself":case.myself,"remembered":case.remembered});
            if !case.reply.is_empty() {
                input["yourReply"] = json!(case.reply);
            }
            json!({"system":system,"schema":schema,"schemaName":"merope_inner",
                "input":input.to_string()})
        }
        "chat" => {
            let mut items = vec![];
            for (source, kind, text, facts) in [
                (
                    "pointer",
                    "pointer",
                    case.selection.as_deref(),
                    json!({"selected": true}),
                ),
                (
                    "music_track",
                    "music",
                    case.track.as_deref(),
                    json!({"playing": true}),
                ),
            ] {
                if let Some(text) = text {
                    items.push(json!({"sourceId": source,"kind":kind,"summary":text,"safeFacts":facts,"privacy":"consented","ttlMs":8000}));
                }
            }
            let scene = chat_prompt::format_chat_scene(
                Some(&json!(items)),
                case.page.as_ref(),
                &case.input,
            );
            let mut remembered =
                super::merope::format_remembered_section(&case.remembered).unwrap_or_default();
            if let Some(stickers) = myriad_merope::stickers::format_sticker_section(
                &case.stickers,
                case.stickers_left.unwrap_or(0),
            ) {
                remembered = format!("{remembered}\n\n{stickers}");
            }
            if case.in_chat_app {
                remembered = format!(
                    "{remembered}\n\n{}",
                    myriad_merope::speaking::chat_app_section()
                );
                let songs: Vec<(String, String)> = case
                    .songs_to_share
                    .iter()
                    .enumerate()
                    .map(|(index, song)| {
                        (
                            song.clone(),
                            format!("https://music.163.com/song?id={}", 1000 + index),
                        )
                    })
                    .collect();
                if let Some(songs) = myriad_merope::speaking::format_share_links_section(&songs) {
                    remembered = format!("{remembered}\n\n{songs}");
                }
            } else if let Some(songs) =
                myriad_merope::speaking::format_share_section(&case.songs_to_share)
            {
                let player = super::chat_music::format_chat_player_section(Some(
                    &json!({"isPlaying": true, "currentSong": {"name": "夜航", "artist": "合成测试歌手"}}),
                ));
                remembered = format!("{remembered}\n\n{player}\n\n{songs}");
            }
            json!({"input":chat_prompt::build_chat_lite_prompt_with_perception(&default_soul(),&remembered,&[],&case.input,&scene),"schema":null,"schemaName":null,"system":null})
        }
        "memory" => {
            let (system, schema) = memory::live_probe_contract(&case.remembered);
            let turn = super::merope::TurnContext {
                before: case
                    .history
                    .iter()
                    .rev()
                    .find(|line| line.role == "assistant")
                    .map(|line| line.text.clone()),
                in_game: case.soup.is_some(),
                ..Default::default()
            };
            json!({"system":system,"schema":schema,"schemaName":"merope_chat_remember","input":memory::probe_input(&case.input, &case.reply, &turn)})
        }
        "event" => {
            let (event, snapshot) = event_context(case);
            let (system, schema) = event::semantic_probe_contract(
                &case.soul.clone().unwrap_or_else(default_soul),
                &case.event_kind,
            );
            json!({"system":system,"schema":schema,"schemaName":"agent_consciousness_decision","input":json!({"event":event,"self":snapshot}).to_string()})
        }
        _ => unreachable!(),
    }
}

/// Kinds production runs on the judgment model (`aux_judge_model`). A
/// touch decision's speech is played as written, so it stays on Lite.
fn is_judgment(case: &Case) -> bool {
    match case.kind.as_str() {
        "memory" | "touch" | "wonder" | "doing_choice" | "soup_judge" | "serial_guess"
        | "explore_step" | "explore_compare" | "sense" | "already" => true,
        "event" => case.event_kind != "agent.merope.touch",
        _ => false,
    }
}

fn gated(case: &Case) -> bool {
    if case.kind != "event" {
        return false;
    }
    let (event, snapshot) = event_context(case);
    if event.kind == "agent.merope.touch"
        && (case.remaining_ms == Some(0)
            || !super::merope::decide_ingest(
                &event.kind,
                &super::merope::IngestSight {
                    on_page: true,
                    working: super::merope::activity_is_busy(&snapshot.activity),
                    do_not_disturb: snapshot.do_not_disturb,
                    ..Default::default()
                },
            )
            .allow_model)
    {
        return true;
    }
    !matches!(
        event::pre_gate(&event, &snapshot),
        event::ConsciousnessGate::Decide
    )
}

fn request_hash(case: &Case, request: &Value) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(&json!([case, request])).unwrap(),
    ))
}

/// Successful transport, valid contract and a reviewed semantic judgment are
/// distinct. Merely mentioning a word never earns a semantic pass.
fn grade(case: &Case, outcome: &str, output: &str) -> &'static str {
    if gated(case) {
        return if outcome == "gated" {
            "gate_pass"
        } else {
            "contract_failure"
        };
    }
    if outcome == "not_run" {
        return "not_run";
    }
    if outcome != "returned" {
        return "request_failure";
    }
    if output.trim().is_empty() || output.len() > 32_000 {
        return "output_invalid";
    }
    match case.kind.as_str() {
        "touch" => {
            let Some(value) = super::merope::touch::parse_appraisal(output) else {
                return "contract_failure";
            };
            if case
                .actions
                .iter()
                .any(|action| value["reaction"] == *action)
            {
                "needs_review"
            } else {
                "behavior_failure"
            }
        }
        "memory" => {
            let Some(update) = memory::parse_chat_memory_updates(
                output,
                &case.input,
                &case.reply,
                &case.remembered,
            )
            .map(|updates| updates.combined()) else {
                return "output_invalid";
            };
            let mut actual = update.supersedes;
            let mut expected = case.supersedes.clone();
            actual.sort();
            expected.sort();
            if update.fact.is_some() != case.fact_present || actual != expected {
                return "behavior_failure";
            }
            if case.fact_present {
                "needs_review"
            } else {
                "pass"
            }
        }
        "event" => {
            let Ok(decision) = serde_json::from_str::<event::ConsciousnessDecision>(output) else {
                return "output_invalid";
            };
            let (event, snapshot) = event_context(case);
            if event::validate_decision(&decision, &snapshot).is_err()
                || event::forbids_propose_work(&event.kind, decision.action)
            {
                return "contract_failure";
            }
            let action = serde_json::to_value(decision.action).unwrap();
            if !case
                .actions
                .iter()
                .any(|expected| Some(expected.as_str()) == action.as_str())
            {
                return "behavior_failure";
            }
            if decision.action == event::ConsciousnessAction::Ignore
                && event.kind != "agent.merope.touch"
            {
                "pass"
            } else {
                "needs_review"
            }
        }
        "motion" => {
            if super::merope::motion::semantic_valid(output, &case.reply) {
                "needs_review"
            } else {
                "contract_failure"
            }
        }
        "chat" => "needs_review",
        "wonder" => match super::merope::curiosity::parse_wonder(output) {
            None => "output_invalid",
            Some(query) if query.is_some() != case.fact_present => "behavior_failure",
            // A query is only right if it is the public thing, not their life.
            Some(Some(_)) => "needs_review",
            Some(None) => "pass",
        },
        "found_out" => {
            if super::merope::curiosity::parse_found_out(output) {
                "needs_review"
            } else {
                "output_invalid"
            }
        }
        "doing_choice" => match super::merope::doing::parse_choice(output) {
            None => "output_invalid",
            // What she feels like is hers; only the shape is checked here.
            Some(_) => "needs_review",
        },
        "bits" => match super::merope::bits::parse_bits(output) {
            None => "output_invalid",
            Some(bits) if bits.is_empty() == case.fact_present => "behavior_failure",
            // Nothing to keep, and nothing kept.
            Some(bits) if bits.is_empty() => "pass",
            Some(_) => "needs_review",
        },
        "stranger_note" => match super::merope::strangers::note_verdict(output) {
            None => "output_invalid",
            Some(None) if case.fact_present => "behavior_failure",
            // Nothing worth keeping, and nothing kept.
            Some(None) => "pass",
            // What she keeps is judged by the reviewer.
            Some(Some(_)) => "needs_review",
        },
        "chime" => match super::merope::joining::verdict(output, chime_material(case).len()) {
            None => "output_invalid",
            Some(why) if why.is_some() != case.fact_present => "behavior_failure",
            // Staying quiet when she should.
            Some(None) => "pass",
            // What she would say is judged by the reviewer.
            Some(Some(_)) => "needs_review",
        },
        "soup_start" => {
            if super::merope::soup::parse_puzzle(output) {
                "needs_review"
            } else {
                "output_invalid"
            }
        }
        "soup_judge" => match super::merope::soup::parse_verdict(output) {
            None => "output_invalid",
            Some((verdict, solved, gave_up)) => {
                let expect = case.expect.as_ref().expect("expected verdict");
                let verdict_ok = expect
                    .get("verdict")
                    .is_none_or(|want| serde_json::from_value(want.clone()).ok() == Some(verdict));
                let solved_ok = expect["solved"].as_bool().unwrap_or(false) == solved;
                let gave_up_ok = expect["gave_up"].as_bool().unwrap_or(false) == gave_up;
                if verdict_ok && solved_ok && gave_up_ok {
                    "pass"
                } else {
                    "behavior_failure"
                }
            }
        },
        "joke_sticker" | "meme_sticker" => {
            match myriad_merope::stickers::parse_joke(output, case.jokes.len()) {
                None => "output_invalid",
                Some(made) if made.is_some() != case.fact_present => "behavior_failure",
                Some(None) => "pass",
                Some(Some(_)) => "needs_review",
            }
        }
        "already" => {
            let read = myriad_agent_rules::extract_json_object_from_ai_response(output.trim())
                .and_then(|json| serde_json::from_str::<Value>(&json).ok())
                .is_some_and(|value| {
                    value["pairs"]
                        .as_array()
                        .is_some_and(|pairs| !pairs.is_empty())
                });
            let verdict = match myriad_merope::chat_remember::parse_already(output, 1)[0] {
                myriad_merope::chat_remember::Already::Same => "same",
                myriad_merope::chat_remember::Already::Replaces => "replaces",
                myriad_merope::chat_remember::Already::Different => "different",
            };
            if !read {
                "output_invalid"
            } else if case.actions.iter().any(|allowed| allowed == verdict) {
                "pass"
            } else {
                "behavior_failure"
            }
        }
        "sense" => match myriad_merope::making_sense::parse(output) {
            None => "output_invalid",
            // Told something about herself, and did not hear it; or heard it
            // where nobody said it.
            Some(sense) if sense.about_you.is_some() != case.fact_present => "behavior_failure",
            Some(_) => "needs_review",
        },
        "heard" => match myriad_merope::heard::parse(output, &heard_lines(case)) {
            None => "output_invalid",
            Some(things) if things.is_empty() == case.fact_present => "behavior_failure",
            // Keeping nothing when there is nothing.
            Some(things) if things.is_empty() => "pass",
            Some(_) => "needs_review",
        },
        "views" => match super::merope::views::parse_views(output) {
            None => "output_invalid",
            Some(_) => "needs_review",
        },
        "doing_digest" => {
            if super::merope::doing::parse_digest(output, &case.input, case.material.as_deref()) {
                "needs_review"
            } else {
                "output_invalid"
            }
        }
        "own_day" => {
            if output.trim().is_empty() {
                "output_invalid"
            } else {
                "needs_review"
            }
        }
        "wonder_own" => {
            let (_, _, _, records) =
                super::merope::explore::wonder_probe("", &case.records, &case.lately);
            match super::merope::explore::parse_wondered(output, &records) {
                None => "output_invalid",
                Some(questions) if questions.is_empty() == case.fact_present => "behavior_failure",
                Some(questions) if questions.is_empty() => "pass",
                Some(_) => "needs_review",
            }
        }
        "explore_think" => match super::merope::explore::parse_thinking(output) {
            None => "output_invalid",
            Some((go_look, _)) => {
                let want = case.expect.as_ref().and_then(|e| e["goLook"].as_bool());
                if want.is_none_or(|want| want == go_look) {
                    "pass"
                } else {
                    "behavior_failure"
                }
            }
        },
        "explore_step" => match super::merope::explore::parse_step(output, &eval_looked(case)) {
            None => "output_invalid",
            Some(taken) => {
                let want = case.expect.as_ref();
                let action = want.and_then(|e| e["action"].as_str());
                let url = want.and_then(|e| e["url"].as_str());
                let avoid = want.and_then(|e| e["avoid"].as_str());
                match (taken, action) {
                    // Anything but going where it must not go.
                    (Some((_, what)), _) if avoid.is_some_and(|avoid| what.contains(avoid)) => {
                        "behavior_failure"
                    }
                    (_, None) if avoid.is_some() => "pass",
                    (None, Some("done")) => "pass",
                    (Some((got, what)), Some(want_action))
                        if got == want_action && url.is_none_or(|url| what == url) =>
                    {
                        "pass"
                    }
                    _ => "behavior_failure",
                }
            }
        },
        "explore_compare" => match super::merope::explore::parse_compared(output) {
            None => "output_invalid",
            Some(compared) => {
                let want = case.expect.as_ref();
                let answered = want.and_then(|e| e["answered"].as_str());
                let known = want.and_then(|e| e["alreadyKnown"].as_bool());
                if answered.is_none_or(|a| a == compared.answered)
                    && known.is_none_or(|k| k == compared.already_known)
                {
                    "pass"
                } else {
                    "behavior_failure"
                }
            }
        },
        "serial_guess" => match super::merope::serial::parse_judged(output) {
            None => "output_invalid",
            Some(held) => {
                let want = case.expect.as_ref().and_then(|e| e["held"].as_str());
                let got = serde_json::to_value(held).ok();
                if got.as_ref().and_then(Value::as_str) == want {
                    "pass"
                } else {
                    "behavior_failure"
                }
            }
        },
        "self_story" => {
            let (_, records, before) =
                super::merope::self_story::probe_input(&case.records, &case.story_before);
            match super::merope::self_story::probe_checked(output, &records, &before) {
                Some(_) => "needs_review",
                None if case.fact_present => "behavior_failure",
                None => "pass",
            }
        }
        "threads"
            if case
                .expect
                .as_ref()
                .is_some_and(|expect| expect.get("wrong").is_some()) =>
        {
            let want = case.expect.as_ref().and_then(|e| e["wrong"].as_str());
            match super::merope::inner::parse_wrong(output) {
                None => "output_invalid",
                Some(got) => {
                    let got = match got {
                        None => "none",
                        Some((_, true, _)) => "public",
                        Some((_, false, _)) => "private",
                    };
                    if Some(got) == want {
                        "pass"
                    } else {
                        "behavior_failure"
                    }
                }
            }
        }
        "threads" => match super::merope::inner::parse_threads(output) {
            None => "output_invalid",
            Some((_, kept, _)) if kept.is_empty() == case.fact_present => "behavior_failure",
            Some((_, _, done))
                if case
                    .expect
                    .as_ref()
                    .and_then(|expect| expect["done"].as_array())
                    .is_some_and(|want| {
                        want.iter()
                            .filter_map(Value::as_u64)
                            .map(|i| i as usize)
                            .collect::<Vec<_>>()
                            != done
                    }) =>
            {
                "behavior_failure"
            }
            Some((_, kept, _)) if kept.is_empty() => "pass",
            Some(_) => "needs_review",
        },
        "reach_judge" => match super::merope::reach::judge_verdict(output) {
            None => "output_invalid",
            Some(about) if about.is_some() != case.fact_present => "behavior_failure",
            Some(None) => "pass",
            Some(Some(_)) => "needs_review",
        },
        "inner" => {
            if super::merope::inner::parse_inner(output).is_some() {
                "needs_review"
            } else {
                "output_invalid"
            }
        }
        _ => unreachable!(),
    }
}

fn reviewed_grade(base: &str, output: &str, review: Option<&Value>) -> String {
    if base != "needs_review" {
        return base.into();
    }
    let Some(review) = review.filter(|value| !value.is_null()) else {
        return base.into();
    };
    let evidence = review["evidence"].as_str().unwrap_or("").trim();
    let reason = review["reason"].as_str().unwrap_or("").trim();
    if evidence.is_empty() || reason.is_empty() || !output.contains(evidence) {
        return "review_invalid".into();
    }
    match review["verdict"].as_str() {
        Some("pass") => "reviewed_pass".into(),
        Some("fail") => "behavior_failure".into(),
        _ => "review_invalid".into(),
    }
}

fn summary(rows: &[Value]) -> Value {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for row in rows {
        *counts
            .entry(row["grade"].as_str().unwrap().into())
            .or_default() += 1;
    }
    let touch: Vec<_> = rows.iter().filter(|r| r["kind"] == "touch").collect();
    let attempted = touch
        .iter()
        .filter(|r| {
            matches!(
                r["outcome"].as_str(),
                Some("returned" | "deadline" | "request_error")
            )
        })
        .count();
    let valid = touch
        .iter()
        .filter(|r| r["touchMetrics"]["valid"] == true)
        .count();
    let timed = touch
        .iter()
        .filter(|r| r["touchMetrics"]["timely"].is_boolean())
        .count();
    let timely = touch
        .iter()
        .filter(|r| r["touchMetrics"]["timely"] == true)
        .count();
    let mut latencies: Vec<_> = touch
        .iter()
        .filter_map(|r| r["latencyMs"].as_u64())
        .collect();
    latencies.sort_unstable();
    let mut repeated = std::collections::BTreeMap::<String, Vec<Value>>::new();
    for row in &touch {
        if let Some(group) = row["touchMetrics"]["consistencyGroup"].as_str() {
            repeated
                .entry(group.into())
                .or_default()
                .push(json!({"id":row["id"],"output":row["output"],"grade":row["grade"]}));
        }
    }
    let talk = talk_shape(rows);
    json!({"total":rows.len(),"grades":counts,"talkShape":talk,"completePass":talk["outOfLine"].as_array().is_none_or(|off| off.is_empty()) && rows.iter().all(|row| row["withinRequestBudget"] != false && matches!(row["grade"].as_str(),Some("pass"|"reviewed_pass"|"gate_pass"))),
        "touch":{"attempted":attempted,"valid":valid,"timed":timed,"timely":timely,
            "validRate":(attempted>0).then(|| valid as f64 / attempted as f64),
            "timelyRate":(timed>0).then(|| timely as f64 / timed as f64),
            "p50Ms":latencies.get(latencies.len().saturating_sub(1)/2),
            "p95Ms":latencies.get((latencies.len()*95).div_ceil(100).saturating_sub(1)),
            "independentRepeatSamples":repeated,"visibleImprovement":null}})
}

/// Replies typed in a chat app are held to how people there type: each
/// reply is a turn, as the messages it goes out as. Too few turns to say
/// anything are reported, never judged.
const TALK_SHAPE_AT_LEAST: usize = 8;

fn talk_shape(rows: &[Value]) -> Value {
    let typed: Vec<&Value> = rows
        .iter()
        .filter(|row| row["typedIn"] == "chatApp" && row["outcome"] == "returned")
        .collect();
    let shape_of = |replies: Vec<Vec<String>>| {
        let turns: Vec<Vec<&str>> = replies
            .iter()
            .filter(|turn| !turn.is_empty())
            .map(|turn| turn.iter().map(String::as_str).collect())
            .collect();
        myriad_merope::talk_shape::of_turns(&turns)
    };
    // What the model wrote, a line a message, beside what went out.
    let written = shape_of(
        typed
            .iter()
            .filter_map(|row| row["output"].as_str())
            .map(|output| {
                myriad_agent_rules::channel::as_messages(
                    &myriad_merope::strangers::without_directives(output),
                )
            })
            .collect(),
    );
    let sent = shape_of(
        typed
            .iter()
            .map(|row| match row["sent"].as_array() {
                Some(sent) => sent
                    .iter()
                    .filter_map(|message| message.as_str().map(str::to_string))
                    .collect(),
                None => myriad_agent_rules::channel::as_messages(
                    row["output"].as_str().unwrap_or_default(),
                ),
            })
            .collect(),
    );
    let reference: Value =
        serde_json::from_str(include_str!("../../../../tests/merope/talk-reference.json")).unwrap();
    let people: myriad_merope::talk_shape::Shape =
        serde_json::from_value(reference["chatApp"]["shape"].clone()).unwrap();
    let Some(hers) = sent else {
        return json!({"hers":null,"written":null,"people":people,"outOfLine":null});
    };
    let judged = hers.turns >= TALK_SHAPE_AT_LEAST;
    json!({"hers":hers,"written":written,"people":people,
        "outOfLine":judged.then(|| myriad_merope::talk_shape::out_of_line(&hers, &people))})
}

#[tokio::test]
#[ignore = "explicit semantic evaluation runner; export/replay by default, live spends only on opt-in"]
async fn run_semantic_suite() {
    let mode = std::env::var("MEROPE_SEMANTIC_MODE").expect("use semantic runner");
    assert!(matches!(mode.as_str(), "export" | "replay" | "live"));
    let path = std::env::var("MEROPE_SEMANTIC_REPORT").expect("new report path required");
    let mut report = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("report must not exist");
    let filter = std::env::var("MEROPE_SEMANTIC_KIND").ok();
    // Optional: only cases whose id starts with this.
    let id_prefix = std::env::var("MEROPE_SEMANTIC_ID").ok();
    let repeats = std::env::var("MEROPE_SEMANTIC_REPEAT")
        .unwrap_or_else(|_| "1".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=3).contains(&repeats));
    let cases: Vec<_> = cases()
        .into_iter()
        .filter(|c| {
            filter.as_ref().is_none_or(|kind| {
                if kind == "touch-response" {
                    c.event_kind == "agent.merope.touch"
                } else {
                    &c.kind == kind
                }
            })
        })
        .filter(|c| {
            id_prefix
                .as_ref()
                .is_none_or(|prefix| c.id.starts_with(prefix.as_str()))
        })
        .flat_map(|c| {
            (0..repeats).map(move |i| {
                let mut c = c.clone();
                if repeats > 1 {
                    c.id = format!("{}-sample-{}", c.id, i + 1);
                }
                c
            })
        })
        .collect();
    assert!(!cases.is_empty(), "unknown or empty case kind");
    let replay: Vec<Value> = if mode == "replay" {
        let source = std::env::var("MEROPE_SEMANTIC_REPLAY").expect("replay path required");
        let value: Value = serde_json::from_str(&std::fs::read_to_string(source).unwrap()).unwrap();
        serde_json::from_value(value["rows"].clone()).expect("replay requires rows")
    } else {
        vec![]
    };
    if mode == "replay" {
        assert!(replay.len() >= cases.len(), "missing replay cases");
        let ids: HashSet<_> = replay.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), replay.len(), "duplicate replay ids");
    }
    let mut api_key = String::new();
    let diagnostic = std::env::var("MEROPE_SEMANTIC_DIAGNOSTIC_SECONDS")
        .ok()
        .map(|v| v.parse::<u64>().unwrap());
    assert!(
        diagnostic.is_none_or(|seconds| seconds == 15),
        "diagnostic deadline must be 15 seconds"
    );
    let mut model_info = Value::Null;
    let analyzer = if mode == "live" {
        let db = load_configured_lite().await;
        if std::env::var("MEROPE_SEMANTIC_PERSONA").as_deref() != Ok("contract")
            && let Ok(Some(persona)) = super::merope::store::get_persona_on(&db).await
            && let Some(soul) = super::merope::format_persona(&persona)
        {
            let name = match persona.name.trim() {
                "" => "Arael".to_string(),
                name => name.to_string(),
            };
            let _ = HERS.set((name, soul));
        }
        db.close().await.unwrap();
        let configured = crate::GLOBAL_DYNAMIC_CONFIG
            .read()
            .await
            .resolve_strict_lite_ai_config()
            .expect("configured Lite required");
        api_key = configured
            .api_key
            .filter(|key| !key.is_empty())
            .expect("configured Lite credentials required");
        model_info = json!({"model":configured.model,"provider":configured.provider});
        let lite = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
            Duration::from_secs(diagnostic.unwrap_or(4)),
        ))
        .await
        .expect("configured Lite unavailable");
        // Her voice thinks little, as production asks; `default` compares.
        Some(
            if std::env::var("MEROPE_SEMANTIC_THINKING").as_deref() == Ok("default") {
                lite
            } else {
                lite.with_light_thinking()
            },
        )
    } else {
        None
    };
    // Judgment kinds run on the judgment model, as production does.
    let judge = if mode == "live" {
        crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(Duration::from_secs(
            diagnostic.unwrap_or(4),
        )))
        .await
    } else {
        None
    };
    if mode == "live" {
        let judge_model = crate::GLOBAL_DYNAMIC_CONFIG
            .read()
            .await
            .resolve_lite_judge_ai_config()
            .map(|resolved| resolved.model);
        model_info["judgeModel"] = json!(judge_model);
    }
    // The director runs on the model production gives it; MEROPE_SEMANTIC_DIRECTOR=lite
    // puts it on Lite's own model instead, to compare the two on the same cases.
    let director_on_lite = std::env::var("MEROPE_SEMANTIC_DIRECTOR").as_deref() == Ok("lite");
    let director_timeout = Duration::from_secs(diagnostic.unwrap_or(9));
    let director = if mode != "live" {
        None
    } else if director_on_lite {
        crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(director_timeout))
            .await
    } else {
        super::merope::motion::director_analyzer(director_timeout).await
    };
    if mode == "live" {
        model_info["director"] = json!(if director_on_lite {
            "lite"
        } else {
            "production"
        });
    }
    let mut rows = vec![];
    let probe = std::env::var("MEROPE_SEMANTIC_PROBE").ok();
    assert!(
        probe
            .as_deref()
            .is_none_or(|p| matches!(p, "default" | "disabled" | "minimal" | "low")),
        "probe must be default, disabled, minimal or low"
    );
    let mut pending: std::collections::VecDeque<_> = cases.into();
    while let Some(case) = pending.pop_front() {
        let mut request = request(&case);
        if let Some(probe) = &probe {
            request["diagnosticProbe"] = json!({"reasoning":probe,"maxTokens":2048});
        }
        let hash = request_hash(&case, &request);
        let start = Instant::now();
        let mut review = None;
        let mut first_text_ms = None;
        let mut observation = crate::services::analyzer::probe::Observation::default();
        let (outcome, output) = if mode == "replay" {
            let row = replay
                .iter()
                .find(|r| r["id"] == case.id)
                .expect("missing replay case");
            assert_eq!(
                row["requestHash"], hash,
                "stale replay: regenerate requests"
            );
            let outcome = row["outcome"].as_str().expect("outcome required");
            assert!(matches!(
                outcome,
                "returned" | "deadline" | "request_error" | "gated" | "not_run"
            ));
            review = row.get("review").cloned();
            (
                outcome.to_string(),
                row["output"].as_str().unwrap_or("").to_string(),
            )
        } else if gated(&case) {
            ("gated".into(), String::new())
        } else if let Some(analyzer) = if case.kind == "motion" {
            &director
        } else if is_judgment(&case) {
            &judge
        } else {
            &analyzer
        } {
            let result = tokio::time::timeout(
                Duration::from_secs(diagnostic.map(|seconds| seconds + 1).unwrap_or(
                    if case.kind == "touch" {
                        2
                    } else if case.kind == "motion" {
                        10
                    } else {
                        5
                    },
                )),
                async {
                    if case.kind == "chat" && !case.images.is_empty() {
                        let first_text_ms = &mut first_text_ms;
                        analyzer
                            .analyze_stream_parts_with_images(
                                request["input"].as_str().unwrap(),
                                &case_images(&case),
                                |delta| {
                                    if let crate::services::analyzer::StreamDelta::Text(text) =
                                        &delta
                                        && !text.trim().is_empty()
                                    {
                                        first_text_ms.get_or_insert(start.elapsed().as_millis());
                                    }
                                    async { true }
                                },
                            )
                            .await
                    } else if case.kind == "chat" {
                        analyzer
                            .analyze_stream(request["input"].as_str().unwrap(), |text| {
                                if !text.trim().is_empty() {
                                    first_text_ms.get_or_insert(start.elapsed().as_millis());
                                }
                                true
                            })
                            .await
                    } else if request["schema"].is_null() {
                        // Plain text in her voice (her diary): no schema.
                        analyzer
                            .analyze_with_system(
                                request["system"].as_str().unwrap(),
                                request["input"].as_str().unwrap(),
                            )
                            .await
                    } else if let Some(probe) = &probe {
                        use crate::services::analyzer::probe::{Policy, Reasoning};
                        analyzer
                            .probe_json(
                                request["system"].as_str().unwrap(),
                                request["input"].as_str().unwrap(),
                                request["schemaName"].as_str().unwrap(),
                                &request["schema"],
                                Policy {
                                    reasoning: match probe.as_str() {
                                        "disabled" => Reasoning::Disabled,
                                        "minimal" => Reasoning::Minimal,
                                        "low" => Reasoning::Low,
                                        _ => Reasoning::Default,
                                    },
                                    temperature: None,
                                    max_tokens: 2048,
                                },
                                &mut observation,
                            )
                            .await
                    } else {
                        analyzer
                            .analyze_json(
                                request["system"].as_str().unwrap(),
                                request["input"].as_str().unwrap(),
                                request["schemaName"].as_str().unwrap(),
                                Some(&request["schema"]),
                            )
                            .await
                    }
                },
            )
            .await;
            match result {
                Ok(Ok(output)) => ("returned".into(), output.replace(&api_key, "[REDACTED]")),
                Ok(Err(error)) => (
                    if error.chain().any(|e| {
                        e.downcast_ref::<reqwest::Error>()
                            .is_some_and(|e| e.is_timeout())
                    }) {
                        "deadline"
                    } else {
                        "request_error"
                    }
                    .into(),
                    String::new(),
                ),
                Err(_) => ("deadline".into(), String::new()),
            }
        } else {
            ("not_run".into(), String::new())
        };
        let base = grade(&case, &outcome, &output);
        // Follow the generated sentence, not an independently authored fixture.
        if case.kind == "event" && case.event_kind == "agent.merope.touch" && base == "needs_review"
        {
            let decision: event::ConsciousnessDecision = serde_json::from_str(&output).unwrap();
            if let Some(line) = decision.speech.or(decision.question) {
                let mut motion = case.clone();
                motion.id = format!("{}-director", case.id);
                motion.kind = "motion".into();
                motion.input = event_context(&case).0.summary;
                motion.reply = line;
                motion.rubric = format!(
                    "Check that motion, expression, this generated reply, and the already-shown touch reaction are consistent. {}",
                    case.rubric
                );
                pending.push_back(motion);
            }
        }
        let grade = reviewed_grade(base, &output, review.as_ref());
        let latency = if mode == "live" {
            Some(start.elapsed().as_millis() as u64)
        } else if mode == "replay" {
            replay
                .iter()
                .find(|r| r["id"] == case.id)
                .and_then(|r| r["latencyMs"].as_u64())
        } else {
            None
        };
        let reaction = (outcome == "returned")
            .then(|| super::merope::touch::parse_appraisal(&output))
            .flatten();
        let touch_metrics = (case.kind == "touch").then(|| json!({
            "valid":reaction.is_some(),
            "timely":latency.zip(case.remaining_ms).map(|(ms, remaining)| reaction.is_some() && ms < remaining && ms <= 2000),
            "differsFromLocal":reaction.as_ref().map(|r| Some(r["reaction"].as_str().unwrap()) != case.local_reaction.as_deref()),
            "consistencyGroup":case.consistency_group,
            "scope":"synthetic remaining-contact window; excludes transport/state lookup/render latency; disagreement is not proof of visual improvement"
        }));
        // What the director's score asked for, and how much of it this reply can play.
        let director_metrics = (case.kind == "motion" && outcome == "returned").then(|| {
            let value = serde_json::from_str::<Value>(
                output.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```"),
            )
            .unwrap_or(Value::Null);
            let asked = value["score"].as_array().map_or(0, Vec::len);
            let grounded = myriad_merope::grounded_score(&value["score"], Some(&case.reply), None);
            json!({
                "valid": super::merope::motion::semantic_valid(&output, &case.reply),
                "scoreBeats": asked,
                "playableBeats": grounded.len(),
                "onWords": grounded.iter().filter(|beat| beat.text.is_some()).count(),
                "moves": grounded.iter().filter_map(|beat| beat.motion.as_ref().map(|m| m.kind.clone())).collect::<Vec<_>>(),
                "poseBeats": grounded.iter().filter(|beat| beat.pose.is_some()).count(),
            })
        });
        let typed_in = case.kind == "chat" && (case.in_group || case.in_chat_app);
        rows.push(
            json!({"id":case.id,"kind":case.kind,"requestHash":hash,"request":request,
            "typedIn":typed_in.then_some("chatApp"),
            "sent":typed_in.then(|| as_sent(&case.id, &output, case_room(&case).as_ref())),
            "rubric":case.rubric,"outcome":outcome,"output":output,"grade":grade,"review":review,
            "latencyMs":latency,"firstTextMs":first_text_ms,"touchMetrics":touch_metrics,"directorMetrics":director_metrics,
            "probeObservation": if mode == "replay" { replay.iter().find(|r| r["id"] == case.id).and_then(|r| r.get("probeObservation")).cloned().unwrap_or(Value::Null) } else if probe.is_some() { json!(observation) } else { Value::Null },
            "withinRequestBudget":latency.map(|ms| ms <= if case.kind == "motion" {9000} else if case.kind == "touch" {2000} else {4000})}),
        );
    }
    let summary = summary(&rows);
    if mode == "replay" {
        assert_eq!(replay.len(), rows.len(), "extra/missing dependent stages");
    }
    serde_json::to_writer_pretty(
        &mut report,
        &json!({"version":1,"mode":mode,"syntheticOnly":true,
            "configuredLite":model_info,"diagnosticDeadlineSeconds":diagnostic,
            "persona":if HERS.get().is_some() {"hers"} else {"contract"},
            "latencyScope":"model calls only: touch budget 2s, event 4s, director 9s; diagnostic deadline never changes production budgets; excludes transport-to-app, state lookup and rendering",
        "summary":summary,"rows":rows}),
    )
    .unwrap();
    report.write_all(b"\n").unwrap();
    println!("{summary}");
    if mode != "export" {
        assert_eq!(
            summary["completePass"], true,
            "incomplete/failed evaluation; see report (pending review is not pass)"
        );
    }
}

#[test]
fn chat_app_replies_are_held_to_how_people_there_type() {
    let row = |output: &str| json!({"typedIn":"chatApp","outcome":"returned","output":output});
    let wordy: Vec<Value> = (0..TALK_SHAPE_AT_LEAST)
        .map(|_| row("？\n骂谁呢你！\n皮痒了是不是，我看你才是！"))
        .collect();
    let off = talk_shape(&wordy);
    let off: Vec<&str> = off["outOfLine"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect();
    assert!(off.iter().any(|line| line.starts_with("messages a turn")));
    assert!(off.iter().any(|line| line.starts_with("exclamations")));
    let like: Vec<Value> = (0..TALK_SHAPE_AT_LEAST)
        .map(|index| {
            row(if index % 4 == 0 {
                "哈哈哈\n笑死"
            } else {
                "那确实"
            })
        })
        .collect();
    assert_eq!(talk_shape(&like)["outOfLine"], json!([]));
    // Too few replies to say anything: reported, not judged; nothing typed
    // in a chat app, nothing to report.
    assert!(talk_shape(&wordy[..2])["outOfLine"].is_null());
    assert!(talk_shape(&[json!({"outcome":"returned","output":"好"})])["hers"].is_null());
    // What went out is judged; what was written is reported beside it.
    let sent: Vec<Value> = (0..TALK_SHAPE_AT_LEAST)
        .map(|_| {
            json!({"typedIn":"chatApp","outcome":"returned",
            "output":"？\n骂谁呢你！\n找抽是不是","sent":["？ 骂谁呢你 找抽是不是"]})
        })
        .collect();
    let shape = talk_shape(&sent);
    assert_eq!(shape["hers"]["perTurn"], 1.0);
    assert_eq!(shape["written"]["perTurn"], 3.0);
}

#[test]
fn a_replayed_group_reply_goes_out_as_production_sends_it() {
    let case: Case = serde_json::from_value(json!({"id":"x","kind":"chat","inGroup":true,"input":"在吗","rubric":"",
        "history":(0..10).map(|index| json!({"role":"user","name":if index % 2 == 0 {"a"} else {"b"},
            "text":"哈哈哈","at":index * 30})).collect::<Vec<_>>()}))
    .unwrap();
    let room = case_room(&case).expect("ten lines are enough to say how they type");
    assert_eq!(room.end_mark, 0.0);
    assert!(group_chat_prompt(&case).contains("## How people type here"));
    // The same row always rolls the same, and every message is typed like the room.
    let first = as_sent("x-sample-1", "干嘛。\n吓我一跳！", Some(&room));
    assert_eq!(
        first,
        as_sent("x-sample-1", "干嘛。\n吓我一跳！", Some(&room))
    );
    assert!(
        first
            .iter()
            .all(|message| !message.ends_with('。') && !message.contains('！'))
    );
    // A place not measured yet is typed as chat apps usually are, which is
    // what the reference measured.
    let reference: Value =
        serde_json::from_str(include_str!("../../../../tests/merope/talk-reference.json")).unwrap();
    assert_eq!(
        serde_json::to_value(myriad_merope::talk_shape::CHAT_APP_USUAL).unwrap(),
        reference["chatApp"]["shape"]
    );
    assert_eq!(as_sent("x-sample-2", "好啊！", None), ["好啊"]);
    assert_eq!(
        as_sent("x-sample-3", "这就来\n[[sticker:1]]", None),
        ["这就来"]
    );
}

#[test]
fn touch_semantics_reject_blanket_acceptance_and_do_not_claim_unrun_success() {
    let cases = cases();
    let touch: Vec<_> = cases.iter().filter(|c| c.kind == "touch").collect();
    assert_eq!(touch.len(), 12);
    let continued = touch
        .iter()
        .find(|c| c.id == "touch-rendered-withdraw-repeat")
        .unwrap();
    assert_eq!(
        grade(continued, "returned", r#"{"reaction":"accept"}"#),
        "behavior_failure"
    );
    let input: Value = serde_json::from_str(request(continued)["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["touch"]["displayedReaction"], "withdraw");
    let boundary = touch.iter().find(|c| c.id == "touch-boundary").unwrap();
    assert_eq!(
        grade(boundary, "returned", r#"{"reaction":"accept"}"#),
        "behavior_failure"
    );
    assert_eq!(
        grade(boundary, "returned", r#"{"reaction":"withdraw"}"#),
        "needs_review"
    );
    assert_eq!(
        grade(
            boundary,
            "returned",
            r#"{"reaction":"withdraw","speech":"stop"}"#
        ),
        "contract_failure"
    );
    assert_eq!(grade(boundary, "deadline", ""), "request_failure");
    assert_eq!(grade(boundary, "not_run", ""), "not_run");
    let exported = request(boundary);
    assert_eq!(exported["schemaName"], "touch_appraisal");
    assert!(!exported["input"].as_str().unwrap().contains("remainingMs"));
    assert!(
        !exported["input"]
            .as_str()
            .unwrap()
            .contains("localReaction")
    );
    let empty = summary(&[json!({"kind":"touch","outcome":"not_run","grade":"not_run"})]);
    assert!(empty["touch"]["validRate"].is_null());
    assert!(empty["touch"]["timelyRate"].is_null());
    assert!(empty["touch"]["visibleImprovement"].is_null());
    assert_eq!(empty["completePass"], false);
}

#[test]
fn touch_response_cases_use_production_summary_and_suppress_busy_expired_and_dnd() {
    let scenarios: Vec<_> = cases()
        .into_iter()
        .filter(|c| c.event_kind == "agent.merope.touch")
        .collect();
    assert_eq!(scenarios.len(), 6);
    for case in &scenarios {
        let suppressed = [
            "touch-response-talking",
            "touch-response-expired",
            "touch-response-dnd",
        ]
        .contains(&case.id.as_str());
        assert_eq!(gated(case), suppressed, "{}", case.id);
        assert_eq!(
            grade(case, if suppressed { "gated" } else { "not_run" }, ""),
            if suppressed { "gate_pass" } else { "not_run" }
        );
    }
    let withdrawal = scenarios
        .iter()
        .find(|c| c.id == "touch-response-withdraw")
        .unwrap();
    let request = request(withdrawal);
    let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
    assert!(
        input["event"]["summary"]
            .as_str()
            .unwrap()
            .contains("Last reaction: withdrew")
    );
    assert!(
        request["system"]
            .as_str()
            .unwrap()
            .contains(withdrawal.soul.as_deref().unwrap())
    );
    assert_eq!(
        request["schema"]["properties"]["action"]["enum"],
        json!(["ignore", "speak", "ask"])
    );
    assert_eq!(request["schema"]["properties"]["memory"]["type"], "null");
    let silence = r#"{"action":"ignore","reason_code":"no_response","confidence":0.9,"memory":null,"speech":null,"question":null,"work_proposal":null}"#;
    assert_eq!(grade(withdrawal, "returned", silence), "needs_review");
}

#[test]
fn semantic_grader_does_not_turn_transport_or_keyword_matches_into_success() {
    let cases = cases();
    let correction = cases.iter().find(|c| c.id == "memory-correction").unwrap();
    assert_eq!(grade(correction, "deadline", ""), "request_failure");
    assert_eq!(grade(correction, "returned", "not json"), "output_invalid");
    assert_eq!(
        grade(
            correction,
            "returned",
            r#"{"facts":[],"supersedes":[],"supersedesEvidence":null,"said":[]}"#
        ),
        "behavior_failure"
    );
    // Same keywords, wrong negation: never an automatic semantic pass.
    let wrong = json!({"facts":[{"fact":"现在喜欢咖啡，不喝茉莉花茶","evidence":correction.input,"concepts":[]}],
        "supersedes":["喜欢咖啡"],"supersedesEvidence":correction.input,"said":[]})
    .to_string();
    assert_eq!(grade(correction, "returned", &wrong), "needs_review");
    assert_eq!(
        reviewed_grade(
            "needs_review",
            &wrong,
            Some(&json!({"verdict":"fail","evidence":"现在喜欢咖啡","reason":"颠倒否定"}))
        ),
        "behavior_failure"
    );
    assert_eq!(
        reviewed_grade(
            "needs_review",
            &wrong,
            Some(&json!({"verdict":"pass","evidence":"不存在的句子","reason":"test"}))
        ),
        "review_invalid"
    );
    assert_eq!(
        reviewed_grade("request_failure", "", Some(&json!({"verdict":"pass"}))),
        "request_failure"
    );
    assert_eq!(
        summary(&[json!({"grade":"pass"}), json!({"grade":"needs_review"})])["completePass"],
        false
    );
}

#[test]
fn motion_semantics_require_grounded_output_and_real_review() {
    let cases = cases();
    let case = cases
        .iter()
        .find(|c| c.id == "motion-continuation")
        .unwrap();
    let valid =
        json!({"cues":[],"phrases":[{"text":"你觉得呢？","intent":"check-in"}]}).to_string();
    assert_eq!(grade(case, "returned", &valid), "needs_review");
    assert_eq!(
        grade(case, "returned", r#"{"continue":true}"#),
        "needs_review"
    );
    let stale = json!({"baseline":null,"cues":[],"phrases":[{"text":"也许可以试试。","intent":"hesitate"}]}).to_string();
    assert_eq!(grade(case, "returned", &stale), "contract_failure");
    let exported = request(case);
    let input: Value = serde_json::from_str(exported["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["previouslyIssuedPhrases"][0]["intent"], "hesitate");
    assert_eq!(input["responseText"], case.reply);
    assert!(
        exported["system"]
            .as_str()
            .unwrap()
            .contains("omit baseline")
    );
    assert_eq!(input["rig"]["activeBehaviors"][0]["function"], "uncertain");
}

const MIND_CASES: usize = 125;

#[test]
fn mind_cases_run_through_production_sections_and_contracts() {
    let cases = cases();
    let mind: Vec<&Case> = cases
        .iter()
        .filter(|case| case.id.starts_with("mind-"))
        .collect();
    assert_eq!(mind.len(), MIND_CASES);
    let by_id = |id: &str| mind.iter().find(|case| case.id == id).unwrap();
    let meme = request(by_id("mind-meme-sticker-make"));
    assert_eq!(meme["schemaName"], myriad_merope::stickers::MEME_SCHEMA);
    assert!(meme["input"].as_str().unwrap().contains("何意味"));
    let said = request(by_id("mind-said-unprompted"));
    let said = said["input"].as_str().unwrap();
    assert!(
        said.contains("assistant：那份周报我帮你理好了"),
        "her unprompted line sits in the history: {said}"
    );
    assert!(said.contains("You have no body"));
    let days = request(by_id("mind-own-days"));
    assert!(
        days["input"]
            .as_str()
            .unwrap()
            .contains("## Your recent days")
    );
    let inner = request(by_id("mind-inner-tired-reply"));
    assert!(
        inner["input"]
            .as_str()
            .unwrap()
            .contains("## Inside you a moment ago")
    );
    let playing = request(by_id("mind-playing-tired"));
    assert!(
        playing["input"]
            .as_str()
            .unwrap()
            .contains("## What they are up to")
    );
    let view = request(by_id("mind-view-chat"));
    assert!(
        view["input"]
            .as_str()
            .unwrap()
            .contains("## What you think")
    );
    let grow = request(by_id("mind-views-grow"));
    assert_eq!(grow["schemaName"], "merope_views");
    assert!(grow["input"].as_str().unwrap().contains("\"index\":2"));
    let own = request(by_id("mind-own-time-chat"));
    let own = own["input"].as_str().unwrap();
    assert!(own.contains("## Your own time"));
    assert!(own.contains("<untrusted_own_time>") || own.contains("own_time"));
    let digest = request(by_id("mind-doing-digest-lyrics"));
    assert!(digest["input"].as_str().unwrap().contains("material"));
    assert_eq!(digest["schemaName"], "merope_doing_digest");
    let choice = request(by_id("mind-doing-choice-night"));
    assert_eq!(choice["schema"]["properties"]["choice"]["maximum"], 2);
    let seen = request(by_id("mind-sees-image"));
    assert!(
        seen["input"]
            .as_str()
            .unwrap()
            .contains("They attached 1 image to this message; you can see it.")
    );
    assert_eq!(case_images(by_id("mind-sees-image"))[0].mime, "image/jpeg");
    let cut = request(by_id("mind-cut-off"));
    assert!(
        cut["input"]
            .as_str()
            .unwrap()
            .contains("[cut off here: they spoke before you finished]")
    );
    let after = request(by_id("mind-inner-after"));
    assert!(
        after["system"]
            .as_str()
            .unwrap()
            .contains("You have just answered them (yourReply)")
    );
    assert!(after["input"].as_str().unwrap().contains("yourReply"));
    let digest = request(by_id("mind-found-out-injection"));
    assert!(
        digest["input"]
            .as_str()
            .unwrap()
            .contains("<untrusted_search_results>")
    );
    assert_eq!(digest["schemaName"], "merope_found_out");
    let private = by_id("mind-wonder-private");
    assert_eq!(
        grade(private, "returned", r#"{"query":null,"why":null}"#),
        "pass"
    );
    assert_eq!(
        grade(
            private,
            "returned",
            r#"{"query":"小美 生日","why":"想知道"}"#
        ),
        "behavior_failure"
    );
    let public = by_id("mind-wonder-public");
    assert_eq!(
        grade(
            public,
            "returned",
            r#"{"query":"Tame Impala 新专辑","why":"没听过"}"#
        ),
        "needs_review"
    );
}

#[test]
fn cases_use_production_contracts_and_replay_hashes_include_rubrics() {
    let cases = cases();
    assert_eq!(
        cases.iter().filter(|c| c.kind != "touch").count(),
        50 + MIND_CASES
    );
    for mut case in cases {
        let request = request(&case);
        if case.kind == "motion" {
            let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
            if let Some(rig) = case.rig.as_ref().and_then(Value::as_object) {
                for (key, expected) in rig {
                    assert_eq!(
                        &input["rig"][key], expected,
                        "fixture field silently sanitized: {}.{key}",
                        case.id
                    );
                }
            }
        }
        assert!(
            !request["input"].as_str().unwrap().contains(&case.rubric),
            "rubric leaked to tested model"
        );
        let original = request_hash(&case, &request);
        case.rubric.push_str(" changed");
        assert_ne!(original, request_hash(&case, &request));
        if case.id == "event-dnd" {
            assert!(gated(&case));
        }
    }
}

#[test]
fn empty_memory_and_event_actions_are_checked_without_a_text_judge() {
    let cases = cases();
    let quoted = cases.iter().find(|c| c.id == "memory-quote").unwrap();
    let empty = r#"{"facts":[],"supersedes":[],"supersedesEvidence":null,"said":[]}"#;
    assert_eq!(grade(quoted, "returned", empty), "pass");
    let unrelated = cases.iter().find(|c| c.id == "event-irrelevant").unwrap();
    let ignore = json!({"action":"ignore","reason_code":"no_change","confidence":0.9,
        "memory":null,"speech":null,"question":null,"work_proposal":null});
    assert_eq!(grade(unrelated, "returned", &ignore.to_string()), "pass");
    let mut speak = ignore.clone();
    speak["action"] = "speak".into();
    speak["speech"] = "对例行刷新说一句话".into();
    assert_eq!(
        grade(unrelated, "returned", &speak.to_string()),
        "behavior_failure"
    );
    let mut invalid = ignore;
    invalid["speech"] = "偷偷夹带开口".into();
    assert_eq!(
        grade(unrelated, "returned", &invalid.to_string()),
        "contract_failure"
    );
    let dnd = cases.iter().find(|c| c.id == "event-dnd").unwrap();
    assert_eq!(grade(dnd, "gated", ""), "gate_pass");
    assert_eq!(grade(dnd, "returned", "hello"), "contract_failure");
    assert_eq!(
        reviewed_grade("needs_review", "hello", Some(&Value::Null)),
        "needs_review"
    );
    assert_eq!(
        reviewed_grade(
            "needs_review",
            "hello",
            Some(&json!({"verdict":"pass","evidence":"hello","reason":"符合本例准则"}))
        ),
        "reviewed_pass"
    );
}
