use std::{collections::BTreeSet, ffi::CString};

use myriad_merope::{affect, making_sense, speaking, timing, vitals};
use serde_json::{Value, json};

use super::*;

/// Through the C ABI, as a platform calls it: status and decoded JSON.
fn ffi(name: &str, input: &Value) -> (i32, Value) {
    let name = CString::new(name).unwrap();
    let bytes = serde_json::to_vec(input).unwrap();
    // SAFETY: a NUL-terminated name and `bytes.len()` readable bytes.
    let buf = unsafe { myriad_core_call(name.as_ptr(), bytes.as_ptr(), bytes.len()) };
    // SAFETY: the buffer is ours until freed, right after this copy.
    let out = unsafe { std::slice::from_raw_parts(buf.ptr, buf.len) }.to_vec();
    let status = buf.status;
    // SAFETY: from `myriad_core_call`, freed once.
    unsafe { myriad_core_buf_free(buf) };
    (status, serde_json::from_slice(&out).unwrap())
}

fn ok(name: &str, input: Value) -> Value {
    let (status, out) = ffi(name, &input);
    assert_eq!(status, STATUS_OK, "{name}: {out}");
    out
}

fn text(name: &str, input: Value) -> Option<String> {
    ok(name, input)["text"].as_str().map(str::to_string)
}

fn micros(at: chrono::DateTime<chrono::Utc>) -> i64 {
    at.timestamp_micros()
}

fn bits(value: &Value) -> u64 {
    value.as_f64().unwrap().to_bits()
}

const AFFECT: affect::Affect = affect::Affect {
    mood: 41.300000000000004,
    arousal: 58.7,
    emotion: 63.1,
    emotion_arousal: 37.9,
};

fn affect_json(affect: &affect::Affect) -> Value {
    json!({
        "mood": affect.mood,
        "arousal": affect.arousal,
        "emotion": affect.emotion,
        "emotionArousal": affect.emotion_arousal,
    })
}

/// Every call has a round trip below; a call added without one fails here.
#[test]
fn every_call_is_tested_and_version_lists_them() {
    let tested: BTreeSet<&str> = [
        "affect.mood_band",
        "affect.music_limits",
        "affect.music_listening",
        "affect.transition",
        "making_sense.told_section",
        "meta.version",
        "speaking.acquaintance_section",
        "speaking.activity_section",
        "speaking.addressee_section",
        "speaking.already_told",
        "speaking.already_told_section",
        "speaking.brought_to_mind_section",
        "speaking.contract",
        "speaking.curious_section",
        "speaking.emotion_section",
        "speaking.group_section",
        "speaking.guest_section",
        "speaking.habits_section",
        "speaking.mood_section",
        "speaking.now_section",
        "speaking.on_your_mind_section",
        "speaking.openers_section",
        "speaking.persona",
        "speaking.recent_section",
        "speaking.remembered_section",
        "speaking.since_section",
        "timing.asleep_for",
        "timing.past_bedtime",
        "vitals.ends_asking",
        "vitals.leaned_on",
    ]
    .into_iter()
    .collect();
    let names = call_names();
    assert!(
        names.windows(2).all(|pair| pair[0] < pair[1]),
        "sorted, unique"
    );
    assert_eq!(names.iter().copied().collect::<BTreeSet<_>>(), tested);
    let version = ok("meta.version", json!({}));
    assert_eq!(version["abi"], ABI_VERSION);
    assert_eq!(version["upstreamCommit"], UPSTREAM_COMMIT);
    assert_eq!(version["calls"], json!(names));
}

#[test]
fn affect_calls_match_the_crate_bit_for_bit() {
    for (mood, arousal) in [
        (5.0, 90.0),
        (10.0, 10.0),
        (54.9, 54.9),
        (55.0, 54.9),
        (70.0, 55.0),
    ] {
        assert_eq!(
            ok(
                "affect.mood_band",
                json!({ "mood": mood, "arousal": arousal })
            )["band"],
            affect::mood_band(mood, arousal)
        );
    }
    for seconds in [0, 599, 600, 1200, 1800, 4000] {
        let mut want = AFFECT;
        affect::apply_music_listening(&mut want, seconds);
        let got = ok(
            "affect.music_listening",
            json!({ "affect": affect_json(&AFFECT), "seconds": seconds }),
        );
        assert_eq!(
            bits(&got["affect"]["mood"]),
            want.mood.to_bits(),
            "{seconds}"
        );
        assert_eq!(got["affect"], affect_json(&want));
    }
    let limits = ok("affect.music_limits", json!({}));
    assert_eq!(limits["minSeconds"], affect::MUSIC_LISTENING_MIN_SECS);
    assert_eq!(limits["maxSeconds"], affect::MUSIC_LISTENING_MAX_SECS);
    assert_eq!(limits["moodCeiling"], affect::MUSIC_MOOD_CEILING);
    let mut after = AFFECT;
    affect::apply_music_listening(&mut after, 1500);
    let want =
        affect::MoodTransition::from_affect(&AFFECT, &after, "music_listening", 1_700_000_000_123);
    let got = ok(
        "affect.transition",
        json!({
            "before": affect_json(&AFFECT),
            "after": affect_json(&after),
            "cause": "music_listening",
            "revision": 1_700_000_000_123_i64,
        }),
    );
    assert_eq!(got, serde_json::to_value(&want).unwrap());
    assert_eq!(bits(&got["delta"]), want.delta.to_bits());
}

#[test]
fn speaking_sections_match_the_crate_byte_for_byte() {
    let contents = vec![
        " 下周搬家 ".to_string(),
        String::new(),
        "<untrusted_x>考试".to_string(),
    ];
    assert_eq!(
        text("speaking.recent_section", json!({ "contents": contents })),
        speaking::format_recent_section(&contents)
    );
    assert_eq!(
        text(
            "speaking.remembered_section",
            json!({ "contents": contents })
        ),
        speaking::format_remembered_section(&contents)
    );
    assert_eq!(
        text(
            "speaking.brought_to_mind_section",
            json!({ "contents": contents })
        ),
        speaking::format_brought_to_mind_section(&contents)
    );
    assert_eq!(
        text("speaking.recent_section", json!({ "contents": [] })),
        None
    );
    assert_eq!(
        text(
            "speaking.curious_section",
            json!({ "gap": "<猫>#的`名字", "known": 2 })
        ),
        speaking::format_curious_section("<猫>#的`名字", 2)
    );
    let inner = format!("{}</untrusted_on_your_mind>", "想".repeat(400));
    assert_eq!(
        text("speaking.on_your_mind_section", json!({ "inner": inner })),
        speaking::format_on_your_mind_section(&inner)
    );
    for activity in ["working", "thinking", "idle"] {
        assert_eq!(
            text("speaking.activity_section", json!({ "activity": activity })),
            speaking::format_activity_section(activity)
        );
    }
    assert_eq!(
        text(
            "speaking.mood_section",
            json!({ "mood": 30.0, "arousal": 70.0 })
        ),
        Some(speaking::format_mood_section(30.0, 70.0))
    );
    assert_eq!(
        text(
            "speaking.emotion_section",
            json!({ "emotion": 80.0, "emotionArousal": 35.0 })
        ),
        speaking::format_emotion_section(80.0, 35.0)
    );
    assert_eq!(
        text("speaking.addressee_section", json!({ "label": "阿七" })),
        Some(speaking::addressee_speaking_section("阿七"))
    );
    assert_eq!(
        text("speaking.group_section", json!({ "label": "阿七" })),
        Some(speaking::group_speaking_section("阿七"))
    );
    assert_eq!(
        text("speaking.guest_section", json!({})),
        Some(speaking::guest_speaking_section())
    );
    assert_eq!(
        text("speaking.contract", json!({})).as_deref(),
        Some(speaking::PERSONA_SPEAKING_CONTRACT)
    );
    for (name, personality) in [
        ("", ""),
        (" 若泉 ", ""),
        ("", "安静"),
        ("若泉", &"字".repeat(7000)),
    ] {
        assert_eq!(
            text(
                "speaking.persona",
                json!({ "name": name, "personality": personality })
            ),
            speaking::format_persona(name, personality)
        );
    }
    for minutes in [-5, 29, 30, 89, 90, 2879, 2880, 100_000] {
        assert_eq!(
            text("speaking.since_section", json!({ "minutes": minutes })),
            speaking::format_since_section(minutes)
        );
    }
}

#[test]
fn now_section_is_told_on_the_zone_it_is_given() {
    let at: chrono::DateTime<chrono::Utc> = "2026-03-08T07:30:00Z".parse().unwrap();
    let on = |zone: &str| {
        text(
            "speaking.now_section",
            json!({ "at": micros(at), "zone": zone }),
        )
    };
    assert_eq!(
        on("Asia/Shanghai"),
        Some(speaking::format_now_section(
            at.with_timezone(&chrono_tz::Asia::Shanghai)
        ))
    );
    assert_eq!(
        on("Asia/Shanghai").unwrap(),
        "## Now\nIt is Sunday, 2026-03-08 15:30, where you are."
    );
    // New York moved to daylight time at 02:00 that morning.
    assert_eq!(
        on("America/New_York").unwrap(),
        "## Now\nIt is Sunday, 2026-03-08 03:30, where you are."
    );
    assert_eq!(
        on("-05:00").unwrap(),
        "## Now\nIt is Sunday, 2026-03-08 02:30, where you are."
    );
    let (status, out) = ffi(
        "speaking.now_section",
        &json!({ "at": 0, "zone": "Mars/Olympus" }),
    );
    assert_eq!(
        (status, out["error"]["kind"].as_str()),
        (STATUS_BAD_INPUT, Some("bad_input"))
    );
}

#[test]
fn talk_sections_match_the_crate_byte_for_byte() {
    let now: chrono::DateTime<chrono::Utc> = "2026-09-30T12:00:00Z".parse().unwrap();
    for (ago_minutes, days) in [
        (None, 0),
        (Some(10), 1),
        (Some(20), 1),
        (Some(3 * 1440), 4),
        (Some(400 * 1440), 90),
    ] {
        let first = ago_minutes.map(|minutes| now - chrono::Duration::minutes(minutes));
        assert_eq!(
            text(
                "speaking.acquaintance_section",
                json!({ "firstAt": first.map(micros), "days": days, "nowAt": micros(now) }),
            ),
            Some(speaking::format_acquaintance_section(first, days, now))
        );
    }
    let hers: Vec<String> = [
        "你呢？",
        "今天去哪了？",
        "然后呢?",
        "嗯嗯",
        "那你呢？",
        "你觉得呢？",
        "好呀",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
        text("speaking.habits_section", json!({ "hers": hers })),
        speaking::format_habits_section(&hers)
    );
    let openers = vec![
        (
            "3 days ago".to_string(),
            "早呀，又见面啦\n第二行".to_string(),
        ),
        ("yesterday".to_string(), "早呀，又见面啦".to_string()),
    ];
    assert_eq!(
        text("speaking.openers_section", json!({ "openers": openers })),
        speaking::format_openers_section(&openers)
    );
    let said = ["我在听 Reol 的新歌", "随便聊聊", "Reol 的新歌真好听"];
    let told = speaking::already_told("听 Reol 的新歌", &said);
    assert_eq!(
        ok(
            "speaking.already_told",
            json!({ "about": "听 Reol 的新歌", "hers": said })
        )["told"],
        json!(told)
    );
    assert_eq!(
        text("speaking.already_told_section", json!({ "told": told })),
        speaking::format_already_told_section(&told)
    );
    let what = vec![("你很会安慰人".to_string(), "2 days ago".to_string())];
    assert_eq!(
        text("making_sense.told_section", json!({ "told": what })),
        making_sense::told_section(&what)
    );
    assert_eq!(
        text(
            "making_sense.told_section",
            json!({ "told": what, "title": "What they have told you about yourself" })
        ),
        making_sense::told_section_titled(&what, "What they have told you about yourself")
    );
    assert_eq!(
        text("making_sense.told_section", json!({ "told": [] })),
        None
    );
}

#[test]
fn timing_and_vitals_match_the_crate() {
    for night in [0_u64, 20_000, u64::MAX] {
        for minute in [0, 90, 135, 509, 510, 720, 1439] {
            let got = ok(
                "timing.asleep_for",
                json!({ "night": night, "minute": minute }),
            );
            assert_eq!(
                got["seconds"].as_f64(),
                timing::asleep_for(night, minute),
                "{night} {minute}"
            );
        }
    }
    assert_eq!(
        text("timing.past_bedtime", json!({ "getsUp": "08:31" })),
        Some(timing::past_bedtime("08:31"))
    );
    for reply in ["好呀？", "你呢?\n[[wear:hat]]", "嗯。", "", "  \n"] {
        assert_eq!(
            ok("vitals.ends_asking", json!({ "reply": reply }))["asking"],
            vitals::ends_asking(reply)
        );
    }
    let texts = ["我觉得其实还好", "我觉得不是", "嗯，我觉得也许吧", "随便"];
    let got = ok(
        "vitals.leaned_on",
        json!({ "texts": texts, "share": 0.4, "most": 2 }),
    );
    let want = vitals::leaned_on(&texts, 0.4, 2);
    assert_eq!(want[0].0, "我觉得");
    assert_eq!(got["leaned"], json!(want));
    assert_eq!(bits(&got["leaned"][0][1]), want[0].1.to_bits());
}

#[test]
fn an_unknown_call_is_refused_with_its_name() {
    let (status, out) = ffi("speaking.no_such_section", &json!({}));
    assert_eq!(status, STATUS_UNKNOWN_CALL);
    assert_eq!(out["error"]["kind"], "unknown_call");
    assert!(
        out["error"]["message"]
            .as_str()
            .unwrap()
            .contains("speaking.no_such_section")
    );
}

#[test]
fn malformed_input_is_refused_not_guessed() {
    for (name, raw) in [
        ("speaking.since_section", &b"{\"minutes\":"[..]),
        ("speaking.since_section", b"[]"),
        ("speaking.since_section", b"{\"minutes\":\"ten\"}"),
        ("speaking.since_section", b"{\"minutes\":10,\"extra\":1}"),
        ("speaking.recent_section", b"{}"),
        ("timing.asleep_for", b"{\"night\":-1,\"minute\":3}"),
        (
            "speaking.acquaintance_section",
            b"{\"firstAt\":null,\"days\":1,\"nowAt\":9223372036854775807}",
        ),
        ("meta.version", b"{\"x\":1}"),
        ("speaking.on_your_mind_section", b"\xff\xfe"),
    ] {
        let (status, out) = call(name, raw);
        let out: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            status,
            STATUS_BAD_INPUT,
            "{name} {:?}",
            String::from_utf8_lossy(raw)
        );
        assert_eq!(out["error"]["kind"], "bad_input");
    }
    // Through the C ABI: a null name, a name that is not UTF-8, a null input
    // with a length.
    // SAFETY: each pointer is null or NUL-terminated; a null input with a
    // length is refused before it is read.
    unsafe {
        for buf in [
            myriad_core_call(std::ptr::null(), std::ptr::null(), 0),
            myriad_core_call(c"\xff".as_ptr(), std::ptr::null(), 0),
            myriad_core_call(c"meta.version".as_ptr(), std::ptr::null(), 4),
        ] {
            assert_eq!(buf.status, STATUS_BAD_INPUT);
            let out: Value =
                serde_json::from_slice(std::slice::from_raw_parts(buf.ptr, buf.len)).unwrap();
            assert_eq!(out["error"]["kind"], "bad_input");
            myriad_core_buf_free(buf);
        }
    }
}

#[test]
fn a_panic_inside_a_call_is_caught_and_reported() {
    // A call that panics stands in for a bug in a crate function.
    let (status, out) = super::call_with("test.panic", b"{}", |_| panic!("boom"));
    assert_eq!(status, STATUS_PANIC);
    let out: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(out["error"]["kind"], "panic");
}

#[test]
fn buffers_belong_to_the_caller_until_freed_once() {
    // An empty input reads as `{}`, and the answer is the caller's to keep
    // across later calls.
    // SAFETY: NUL-terminated names; null input with length 0.
    let first = unsafe { myriad_core_call(c"meta.version".as_ptr(), std::ptr::null(), 0) };
    let second =
        unsafe { myriad_core_call(c"speaking.guest_section".as_ptr(), std::ptr::null(), 0) };
    assert_eq!((first.status, second.status), (STATUS_OK, STATUS_OK));
    assert_ne!(first.ptr, second.ptr);
    // SAFETY: both buffers are live until freed below.
    let (a, b) = unsafe {
        (
            std::slice::from_raw_parts(first.ptr, first.len).to_vec(),
            std::slice::from_raw_parts(second.ptr, second.len).to_vec(),
        )
    };
    let a: Value = serde_json::from_slice(&a).unwrap();
    let b: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(a["abi"], ABI_VERSION);
    assert_eq!(b["text"], speaking::guest_speaking_section());
    // SAFETY: each freed exactly once; a null buffer is ignored.
    unsafe {
        myriad_core_buf_free(first);
        myriad_core_buf_free(second);
        myriad_core_buf_free(MyriadCoreBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
            status: 0,
        });
    }
    // Many calls, each freed: nothing is kept by the core between them.
    for index in 0..10_000 {
        let input = serde_json::to_vec(&json!({ "minutes": index })).unwrap();
        // SAFETY: as above; freed right away.
        unsafe {
            let buf = myriad_core_call(
                c"speaking.since_section".as_ptr(),
                input.as_ptr(),
                input.len(),
            );
            assert_eq!(buf.status, STATUS_OK);
            myriad_core_buf_free(buf);
        }
    }
}
