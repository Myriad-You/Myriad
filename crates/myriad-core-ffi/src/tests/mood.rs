//! How what happens moves her mood, through the C entry point and straight
//! from the crate: the same bits.

use myriad_merope::affect;
use serde_json::json;

use super::{AFFECT, affect_json, bits, ok};

pub(super) const TESTED: &[&str] = &[
    "affect.apply_appraisal",
    "affect.lite_appraisal",
    "affect.mood_cue",
    "affect.persona_baseline",
    "affect.pull_push",
    "affect.settle",
    "affect.user_utterance",
];

fn same_affect(got: &serde_json::Value, want: &affect::Affect) {
    assert_eq!(*got, affect_json(want));
    for (field, value) in [
        ("mood", want.mood),
        ("arousal", want.arousal),
        ("emotion", want.emotion),
        ("emotionArousal", want.emotion_arousal),
    ] {
        assert_eq!(bits(&got[field]), value.to_bits(), "{field}");
    }
}

#[test]
fn appraisals_move_her_as_the_crate_does() {
    let start = affect::Affect {
        emotion: 91.7,
        emotion_arousal: 12.3,
        ..AFFECT
    };
    let mut want = start;
    affect::pull_push(&mut want);
    let got = ok("affect.pull_push", json!({ "affect": affect_json(&start) }));
    same_affect(&got["affect"], &want);

    for (emotion, arousal, scale) in [
        (82.0, 58.0, 1.0),
        (8.0, 70.0, 0.8),
        (30.0, 64.0, f64::NAN),
        (99.9, 0.1, 3.0),
    ] {
        let mut want = AFFECT;
        affect::apply_appraisal(&mut want, affect::Appraisal { emotion, arousal }, scale);
        let scale = if scale.is_nan() {
            json!(null)
        } else {
            json!(scale)
        };
        let input = json!({
            "affect": affect_json(&AFFECT),
            "appraisal": { "emotion": emotion, "arousal": arousal },
            "scale": scale,
        });
        if scale.is_null() {
            // NaN does not cross JSON; a missing number is refused.
            let (status, _) = super::ffi("affect.apply_appraisal", &input);
            assert_eq!(status, crate::STATUS_BAD_INPUT);
            continue;
        }
        same_affect(&ok("affect.apply_appraisal", input)["affect"], &want);
    }

    for (valence, arousal) in [(-3, 2), (1, -1), (2, 9)] {
        let want = affect::lite_appraisal(valence, arousal);
        let got = ok(
            "affect.lite_appraisal",
            json!({ "valence": valence, "arousal": arousal }),
        );
        assert_eq!(bits(&got["emotion"]), want.emotion.to_bits());
        assert_eq!(bits(&got["arousal"]), want.arousal.to_bits());
    }

    for (index, praised, scolded) in [
        (0, true, false),
        (3, false, true),
        (7, true, true),
        (1, false, false),
    ] {
        let mut want = AFFECT;
        affect::apply_user_utterance(&mut want, index, praised, scolded);
        let got = ok(
            "affect.user_utterance",
            json!({ "affect": affect_json(&AFFECT), "index": index, "praised": praised, "scolded": scolded }),
        );
        same_affect(&got["affect"], &want);
    }
}

#[test]
fn settling_and_her_baseline_match_the_crate() {
    let base = affect::AffectBaseline {
        mood: 66.6,
        arousal: 42.0,
    };
    for (mood_hours, emotion_hours) in [(0.0, 0.0), (3.5, 0.25), (200.0, 9.0), (-1.0, 1.0)] {
        let want = affect::settle(AFFECT, base, mood_hours, emotion_hours);
        let got = ok(
            "affect.settle",
            json!({
                "affect": affect_json(&AFFECT),
                "base": { "mood": base.mood, "arousal": base.arousal },
                "moodHours": mood_hours,
                "emotionHours": emotion_hours,
            }),
        );
        same_affect(&got["affect"], &want);
    }

    for text in [
        "谢谢！！",
        " thank you~ ",
        "滚。",
        "谢谢你的礼物",
        "Shut Up!",
        "",
    ] {
        let (praised, scolded) = affect::detect_mood_cue(text);
        assert_eq!(
            ok("affect.mood_cue", json!({ "text": text })),
            json!({ "praised": praised, "scolded": scolded }),
            "{text}"
        );
    }

    for (persona, personality) in [
        (None, ""),
        (
            Some(json!({ "temperament": ["克制", "温柔"], "socialStyle": "" })),
            "",
        ),
        (
            Some(json!({ "temperament": [], "socialStyle": "Cheerful" })),
            "",
        ),
        (Some(json!({ "temperament": 3 })), "很活泼"),
    ] {
        let want = affect::persona_affect_baseline(persona.as_ref(), personality);
        let got = ok(
            "affect.persona_baseline",
            json!({ "persona": persona, "personality": personality }),
        );
        assert_eq!(got, json!({ "mood": want.mood, "arousal": want.arousal }));
    }
}
