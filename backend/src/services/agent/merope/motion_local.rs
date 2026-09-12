//! Deterministic acting floor.
//!
//! Lite refines; it must never be the only producer. `local_performance_plan`
//! uses phase, mood, task_success, response_text, motion_style (no activity).
//! Chat publishes `local_directive` before the reply stream; Lite refine uses
//! `fallback_to_local: false`. `direct_motion` is the no-publisher path.
//! Amplitudes clamp 0.2..=1.4 (performance contract, not ambient).
//! A floor nobody can see is the same as no floor.

use myriad_merope::{ChatPerformanceBaseline, ChatPerformanceCue, ChatPerformancePlan};

use super::motion::text_mentions_any;
use super::state::MoodTransition;
use super::MotionPhase;

/// Mood moves smaller than this are noise, not a change of bearing.
const MOOD_NUDGE: f64 = 8.0;

const EXPRESSION_LADDER: [&str; 4] = ["withdrawn", "subdued", "steady", "warm"];

pub fn local_performance_plan(
    phase: MotionPhase,
    mood: &MoodTransition,
    task_success: Option<bool>,
    response_text: Option<&str>,
    motion_style: &str,
    user_text: Option<&str>,
) -> ChatPerformancePlan {
    // Quoted dialogue/code describes someone else's expression, not necessarily
    // the speaker's own affect. The prompt and displayed reply stay untouched.
    let response = response_text.map(unquoted_text);
    let user = user_text.map(unquoted_text);
    let baseline = local_baseline(phase, mood, motion_style);
    let cues: Vec<ChatPerformanceCue> = local_cue(
        phase,
        mood,
        task_success,
        response.as_deref(),
        user.as_deref(),
        &baseline,
    )
    .into_iter()
    .collect();
    ChatPerformancePlan {
        baseline: Some(baseline),
        cues,
    }
}

fn local_baseline(
    phase: MotionPhase,
    mood: &MoodTransition,
    motion_style: &str,
) -> ChatPerformanceBaseline {
    ChatPerformanceBaseline {
        expression: baseline_expression(&mood.band_after, mood.delta).to_string(),
        posture: baseline_posture(motion_style, &mood.band_after).to_string(),
        motion_energy: baseline_energy(motion_style, &mood.band_after),
        attention: phase_attention(phase),
    }
}

/// Band sets the rung; a decisive swing this round moves it one step.
///
/// `tense` answers before the ladder because it is not on it. `mood_band`
/// splits `sad` from `tense` on arousal. The ladder is a valence axis;
/// only `subdued` / `withdrawn` close the eyes.
fn baseline_expression(band: &str, delta: f64) -> &'static str {
    if band == "tense" {
        return "tense";
    }
    let rung = match band {
        "floor" => 0i32,
        "sad" | "low" => 1,
        "excited" | "high" => 3,
        _ => 2,
    };
    let step = if delta >= MOOD_NUDGE {
        1
    } else if delta <= -MOOD_NUDGE {
        -1
    } else {
        0
    };
    EXPRESSION_LADDER[(rung + step).clamp(0, 3) as usize]
}

fn baseline_posture(motion_style: &str, band: &str) -> &'static str {
    if band == "floor" {
        return "closed";
    }
    match motion_style {
        "restrained" => "closed",
        "open" => "open",
        _ if band == "excited" || band == "high" => "open",
        _ => "neutral",
    }
}

fn baseline_energy(motion_style: &str, band: &str) -> f32 {
    let style: f32 = match motion_style {
        "restrained" => 0.6,
        "open" => 1.2,
        _ => 0.9,
    };
    let band_scale: f32 = match band {
        "floor" => 0.85,
        "sad" | "low" => 0.92,
        "tense" => 1.05,
        "excited" | "high" => 1.12,
        _ => 1.0,
    };
    (style * band_scale).clamp(0.2, 1.4)
}

/// Scales ambient wander (`1 - 0.3 * attention`); not a face budget.
fn phase_attention(phase: MotionPhase) -> f32 {
    match phase {
        MotionPhase::Reaction => 0.9,
        MotionPhase::Delivery => 0.72,
        MotionPhase::Outcome => 0.6,
        MotionPhase::Proactive => 0.55,
        MotionPhase::Mood => 0.4,
    }
}

/// One beat per phase. A mood shift is a change of bearing, not an event, so it
/// moves the baseline and plays nothing.
fn local_cue(
    phase: MotionPhase,
    mood: &MoodTransition,
    task_success: Option<bool>,
    response_text: Option<&str>,
    user_text: Option<&str>,
    baseline: &ChatPerformanceBaseline,
) -> Option<ChatPerformanceCue> {
    let intent = match phase {
        // Text submission is already a completed turn. A small acknowledgement
        // is honest; "listen" falsely claims an ongoing listener state that
        // this path cannot observe.
        MotionPhase::Reaction if mood.cause == "user_praise" || mood.delta >= MOOD_NUDGE => {
            "delight"
        }
        MotionPhase::Reaction if mood.cause == "user_scold" => "speechless",
        MotionPhase::Reaction if text_is_affectionate(user_text) => "lovestruck",
        MotionPhase::Reaction if text_is_greeting(user_text) => "greet",
        MotionPhase::Reaction if reply_is_playful(user_text) => "delight",
        MotionPhase::Reaction if text_needs_thought(user_text) => "think",
        MotionPhase::Reaction => "respond",
        MotionPhase::Delivery => {
            if reply_is_unrestrained_laughter(response_text) {
                "maniac"
            } else if reply_is_self_deprecating(response_text) {
                "silly"
            } else if text_is_affectionate(response_text) {
                "lovestruck"
            } else if reply_is_personally_hurt(response_text) {
                "cry"
            } else if reply_is_angry(response_text) {
                "angry"
            } else if reply_is_speechless(response_text) {
                "speechless"
            } else if reply_is_thinking(response_text) {
                "think"
            } else if reply_asks_back(response_text) {
                "question"
            } else if reply_is_playful(response_text) {
                "delight"
            } else if reply_is_emphatic(response_text) {
                "emphasize"
            } else {
                "respond"
            }
        }
        MotionPhase::Outcome => match task_success {
            Some(true) => "delight",
            _ => "respond",
        },
        MotionPhase::Proactive => "notify",
        MotionPhase::Mood => return None,
    };
    let energy = baseline.motion_energy;
    let semantic_scale: f32 = match intent {
        "maniac" => 1.2,
        "angry" => 1.12,
        "silly" => 1.06,
        "cry" | "lovestruck" => 0.9,
        "think" => 0.82,
        _ => 1.0,
    };
    let tempo_scale: f32 = match intent {
        "maniac" => 1.18,
        "angry" => 1.08,
        "cry" | "lovestruck" | "think" => 0.88,
        _ => 1.0,
    };
    let (fade_in_ms, fade_out_ms) = match intent {
        "question" => (125, 380),
        "delight" => (105, 460),
        "notify" | "emphasize" => (110, 380),
        "speechless" => (115, 500),
        _ if phase == MotionPhase::Reaction => (105, 420),
        _ => (135, 400),
    };
    Some(ChatPerformanceCue {
        intent: intent.to_string(),
        at_ms: 0,
        intensity: ((0.55 + energy * 0.45) * semantic_scale).clamp(0.5, 1.4),
        tempo: ((0.8 + energy * 0.3) * tempo_scale).clamp(0.6, 1.4),
        fade_in_ms,
        fade_out_ms,
        // The floor never stomps acting that is already richer than it.
        interrupt: "if-lower".to_string(),
    })
}

fn unquoted_text(text: &str) -> String {
    let mut out = String::new();
    let mut closing = None;
    for ch in text.chars().take(2_000) {
        if let Some(end) = closing {
            if ch == end {
                closing = None;
                out.push(' ');
            }
            continue;
        }
        closing = match ch {
            '“' => Some('”'),
            '「' => Some('」'),
            '『' => Some('』'),
            '"' | '`' => Some(ch),
            _ => None,
        };
        if closing.is_none() {
            out.push(ch);
        }
    }
    out
}

fn reply_asks_back(response_text: Option<&str>) -> bool {
    response_text
        .map(str::trim_end)
        .and_then(|text| text.chars().next_back())
        .is_some_and(|last| last == '?' || last == '？')
}

fn reply_is_playful(response_text: Option<&str>) -> bool {
    let Some(text) = response_text.map(str::trim).filter(|text| !text.is_empty()) else {
        return false;
    };
    text_mentions_any(
        text,
        &[
            "哈哈", "嘿嘿", "嘻嘻", "笑死", "hhh", "lol", "ww", "😂", "🤣",
        ],
    )
}

fn reply_is_unrestrained_laughter(text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        text_mentions_any(
            text,
            &["哈哈哈哈", "哈哈哈", "笑疯了", "笑死我了", "🤣", "wwwww"],
        )
    })
}

fn reply_is_self_deprecating(text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        text_mentions_any(
            text,
            &[
                "我犯傻",
                "我好笨",
                "我真笨",
                "我搞砸",
                "我出糗",
                "ドジし",
                "i messed up",
                "i'm silly",
            ],
        )
    })
}

fn text_is_affectionate(text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        if text_mentions_any(
            text,
            &[
                "不爱你",
                "不喜欢你",
                "don't love you",
                "do not love you",
                "don't miss you",
                "大好きじゃない",
            ],
        ) {
            return false;
        }
        text_mentions_any(
            text,
            &[
                "爱你",
                "喜欢你",
                "想你了",
                "love you",
                "miss you",
                "大好き",
                "好きだよ",
            ],
        )
    })
}

fn reply_is_personally_hurt(text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        text_mentions_any(
            text,
            &[
                "我很难过",
                "我也难过",
                "我很伤心",
                "我想哭",
                "i'm sad",
                "i am sad",
                "悲しい",
            ],
        )
    })
}

fn reply_is_angry(text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        text_mentions_any(text, &["我生气了", "我很生气", "i'm angry", "腹が立つ"])
    })
}

fn reply_is_speechless(text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        text_mentions_any(
            text,
            &[
                "我无语",
                "无语了",
                "我愣住了",
                "真的假的",
                "まさか",
                "are you serious",
            ],
        )
    })
}

fn reply_is_thinking(text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        text_mentions_any(
            text,
            &[
                "让我想想",
                "我想一下",
                "我回忆一下",
                "考えさせて",
                "let me think",
            ],
        )
    })
}

fn text_needs_thought(text: Option<&str>) -> bool {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return false;
    };
    text_mentions_any(
        text,
        &[
            "为什么",
            "怎么回事",
            "怎么办",
            "你觉得",
            "你记得",
            "帮我想",
            "能不能帮",
            "how",
            "why",
            "what do you think",
            "どうして",
            "どう思う",
            "覚えてる",
        ],
    )
}

fn reply_is_emphatic(response_text: Option<&str>) -> bool {
    response_text
        .map(str::trim_end)
        .and_then(|text| text.chars().next_back())
        .is_some_and(|last| last == '!' || last == '！')
}

fn text_is_greeting(text: Option<&str>) -> bool {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return false;
    };
    text_mentions_any(
        text,
        &[
            "你好",
            "早上好",
            "下午好",
            "晚上好",
            "hello",
            "hi",
            "hey",
            "おはよう",
            "こんにちは",
            "こんばんは",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_merope::{
        plan_is_empty, PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_CUE_INTENTS,
        PERFORMANCE_INTERRUPT_MODES, PERFORMANCE_POSTURES,
    };

    fn mood(band: &str, delta: f64) -> MoodTransition {
        MoodTransition {
            before: 50.0,
            after: 50.0 + delta,
            arousal_before: 48.0,
            arousal_after: 48.0,
            band_before: "calm".to_string(),
            band_after: band.to_string(),
            delta,
            cause: "test".to_string(),
            revision: 1,
        }
    }

    #[test]
    fn delivered_lines_have_distinct_readable_reactions() {
        for (line, expected) in [
            ("哈哈哈哈，这也太好笑了！", "maniac"),
            ("我搞砸了，刚才把鞋穿反了。", "silly"),
            ("我也喜欢你。", "lovestruck"),
            ("我很难过，先让我缓一会儿。", "cry"),
            ("我生气了，这样不行。", "angry"),
            ("真的假的，我愣住了。", "speechless"),
            ("让我想想，那应该是上个星期。", "think"),
            ("Which one would you like?", "question"),
            ("I messed up the timing.", "silly"),
            ("Let me think about that.", "think"),
            ("大好きだよ。", "lovestruck"),
        ] {
            let plan = local_performance_plan(
                MotionPhase::Delivery,
                &mood("calm", 0.0),
                None,
                Some(line),
                "even",
                None,
            );
            assert_eq!(plan.cues[0].intent, expected, "{line}");
        }
    }

    #[test]
    fn thoughtful_input_does_not_always_get_the_same_nod() {
        for line in [
            "为什么会这样",
            "你记得我上次说的事吗",
            "Why is that happening?",
            "どう思う？",
        ] {
            let plan = local_performance_plan(
                MotionPhase::Reaction,
                &mood("calm", 0.0),
                None,
                None,
                "even",
                Some(line),
            );
            assert_eq!(plan.cues[0].intent, "think", "{line}");
        }
    }

    #[test]
    fn quoted_emotions_and_negated_affection_do_not_become_our_expression() {
        for line in [
            "我不喜欢你。",
            "I don't love you.",
            "大好きじゃない。",
            "他刚刚说“我生气了”。",
            "例子是 `哈哈哈哈`。",
            "别难过，我在这里。",
        ] {
            let plan = local_performance_plan(
                MotionPhase::Delivery,
                &mood("calm", 0.0),
                None,
                Some(line),
                "even",
                None,
            );
            assert_eq!(plan.cues[0].intent, "respond", "{line}");
        }
    }

    #[test]
    fn transient_expression_preserves_relationship_mood_and_restrained_posture() {
        for band in ["floor", "sad", "tense"] {
            let plan = local_performance_plan(
                MotionPhase::Delivery,
                &mood(band, 0.0),
                None,
                Some("我也喜欢你。"),
                "restrained",
                None,
            );
            assert_eq!(
                plan.baseline.as_ref().unwrap().expression,
                baseline_expression(band, 0.0)
            );
            assert_eq!(plan.baseline.unwrap().posture, "closed");
        }
        let angry = local_performance_plan(
            MotionPhase::Delivery,
            &mood("calm", 0.0),
            None,
            Some("我生气了。"),
            "even",
            None,
        );
        assert_eq!(angry.cues[0].intent, "angry");
        assert_eq!(angry.baseline.unwrap().expression, "steady");
    }

    /// The floor now carries every round, so a marker that fires on ordinary
    /// prose is not a rough edge — it is the character greeting most English
    /// sentences and laughing at every link.
    #[test]
    fn latin_markers_do_not_fire_inside_ordinary_words() {
        for text in [
            "Can you check this?",
            "They said it works",
            "which one do you want",
            "the machine is fine",
            "a short history of it",
        ] {
            let plan = local_performance_plan(
                MotionPhase::Reaction,
                &mood("normal", 0.0),
                None,
                None,
                "even",
                Some(text),
            );
            assert_eq!(plan.cues[0].intent, "respond", "{text:?}");
        }

        for text in ["see www.foo.com", "the lollipop broke", "a swww"] {
            let plan = local_performance_plan(
                MotionPhase::Delivery,
                &mood("normal", 0.0),
                None,
                Some(text),
                "even",
                None,
            );
            assert_eq!(plan.cues[0].intent, "respond", "{text:?}");
        }
    }

    #[test]
    fn latin_markers_still_fire_when_they_stand_alone() {
        for text in ["hi", "Hi there", "hey!", "hello, are you up?", "hi 你在吗"] {
            let plan = local_performance_plan(
                MotionPhase::Reaction,
                &mood("normal", 0.0),
                None,
                None,
                "even",
                Some(text),
            );
            assert_eq!(plan.cues[0].intent, "greet", "{text:?}");
        }

        for text in ["that is great www", "lol", "hhh", "そうですねwwww"] {
            let plan = local_performance_plan(
                MotionPhase::Delivery,
                &mood("normal", 0.0),
                None,
                Some(text),
                "even",
                None,
            );
            assert_eq!(plan.cues[0].intent, "delight", "{text:?}");
        }
    }

    #[test]
    fn every_phase_but_mood_renders_a_playable_plan() {
        for phase in [
            MotionPhase::Reaction,
            MotionPhase::Delivery,
            MotionPhase::Outcome,
            MotionPhase::Proactive,
        ] {
            for style in ["restrained", "even", "open"] {
                for band in ["floor", "sad", "tense", "calm", "excited"] {
                    let plan = local_performance_plan(
                        phase,
                        &mood(band, 0.0),
                        Some(true),
                        None,
                        style,
                        None,
                    );
                    assert!(!plan_is_empty(&plan), "{phase:?}/{style}/{band} was empty");
                    assert_eq!(plan.cues.len(), 1);
                }
            }
        }
    }

    /// `mood_band` splits low mood on arousal; the floor has to keep the split.
    #[test]
    fn irritation_does_not_wear_the_same_face_as_flatness() {
        let flat = local_performance_plan(
            MotionPhase::Reaction,
            &mood("sad", 0.0),
            None,
            None,
            "even",
            None,
        );
        let irritated = local_performance_plan(
            MotionPhase::Reaction,
            &mood("tense", 0.0),
            None,
            None,
            "even",
            None,
        );
        let flat = flat.baseline.expect("baseline");
        let irritated = irritated.baseline.expect("baseline");
        assert_eq!(flat.expression, "subdued");
        assert_eq!(irritated.expression, "tense");
        // The body already carried the arousal; the face used not to.
        assert!(irritated.motion_energy > flat.motion_energy);

        // Irritation is a corner of the circumplex, not a rung: a swing inside
        // the band does not walk it up or down the valence ladder.
        for delta in [-20.0, 0.0, 20.0] {
            let plan = local_performance_plan(
                MotionPhase::Reaction,
                &mood("tense", delta),
                None,
                None,
                "even",
                None,
            );
            assert_eq!(plan.baseline.unwrap().expression, "tense", "{delta}");
        }
    }

    #[test]
    fn a_mood_shift_moves_the_baseline_without_playing_a_beat() {
        let plan = local_performance_plan(
            MotionPhase::Mood,
            &mood("excited", 12.0),
            None,
            None,
            "even",
            None,
        );
        assert!(plan.cues.is_empty());
        assert_eq!(plan.baseline.unwrap().expression, "warm");
    }

    /// A local plan that the contract would reject is worse than no floor.
    #[test]
    fn the_floor_only_emits_contract_vocabulary() {
        for phase in [
            MotionPhase::Reaction,
            MotionPhase::Delivery,
            MotionPhase::Outcome,
            MotionPhase::Proactive,
            MotionPhase::Mood,
        ] {
            for style in ["restrained", "even", "open", "nonsense"] {
                for band in [
                    "floor", "sad", "tense", "calm", "excited", "low", "high", "nonsense",
                ] {
                    for delta in [-20.0, 0.0, 20.0] {
                        let plan = local_performance_plan(
                            phase,
                            &mood(band, delta),
                            Some(false),
                            Some("好的？"),
                            style,
                            None,
                        );
                        let baseline = plan.baseline.expect("floor always sets a baseline");
                        assert!(PERFORMANCE_BASELINE_EXPRESSIONS
                            .contains(&baseline.expression.as_str()));
                        assert!(PERFORMANCE_POSTURES.contains(&baseline.posture.as_str()));
                        assert!((0.2..=1.4).contains(&baseline.motion_energy));
                        assert!((0.0..=1.0).contains(&baseline.attention));
                        for cue in &plan.cues {
                            assert!(PERFORMANCE_CUE_INTENTS.contains(&cue.intent.as_str()));
                            assert!(PERFORMANCE_INTERRUPT_MODES.contains(&cue.interrupt.as_str()));
                            assert!(cue.intensity.is_finite() && cue.tempo.is_finite());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_reply_that_asks_back_answers_with_a_question_beat() {
        let asking = local_performance_plan(
            MotionPhase::Delivery,
            &mood("normal", 0.0),
            None,
            Some("你想先看哪一份？"),
            "even",
            None,
        );
        assert_eq!(asking.cues[0].intent, "question");
        let telling = local_performance_plan(
            MotionPhase::Delivery,
            &mood("normal", 0.0),
            None,
            Some("已经改好了。"),
            "even",
            None,
        );
        assert_eq!(telling.cues[0].intent, "respond");
    }

    #[test]
    fn submitted_text_gets_an_acknowledgement_not_a_fake_listening_state() {
        let plan = local_performance_plan(
            MotionPhase::Reaction,
            &mood("normal", 0.0),
            None,
            None,
            "even",
            None,
        );
        assert_eq!(plan.cues[0].intent, "respond");

        let greeting = local_performance_plan(
            MotionPhase::Reaction,
            &mood("normal", 0.0),
            None,
            None,
            "even",
            Some("你好呀"),
        );
        assert_eq!(greeting.cues[0].intent, "greet");
    }

    #[test]
    fn immediate_floor_uses_known_affect_without_waiting_for_lite() {
        let mut praised = mood("excited", 10.0);
        praised.cause = "user_praise".to_string();
        let delighted =
            local_performance_plan(MotionPhase::Reaction, &praised, None, None, "open", None);
        assert_eq!(delighted.cues[0].intent, "delight");

        let mut scolded = mood("tense", -10.0);
        scolded.cause = "user_scold".to_string();
        let speechless =
            local_performance_plan(MotionPhase::Reaction, &scolded, None, None, "even", None);
        assert_eq!(speechless.cues[0].intent, "speechless");
    }

    #[test]
    fn delivery_floor_reads_the_finished_line() {
        let playful = local_performance_plan(
            MotionPhase::Delivery,
            &mood("excited", 0.0),
            None,
            Some("哈哈，这也太可爱了。"),
            "open",
            None,
        );
        assert_eq!(playful.cues[0].intent, "delight");
        let emphatic = local_performance_plan(
            MotionPhase::Delivery,
            &mood("calm", 0.0),
            None,
            Some("交给我吧！"),
            "even",
            None,
        );
        assert_eq!(emphatic.cues[0].intent, "emphasize");
    }

    #[test]
    fn a_failed_task_does_not_celebrate() {
        let failed = local_performance_plan(
            MotionPhase::Outcome,
            &mood("sad", -12.0),
            Some(false),
            None,
            "even",
            None,
        );
        assert_eq!(failed.cues[0].intent, "respond");
        assert_eq!(failed.baseline.unwrap().expression, "withdrawn");
        let done = local_performance_plan(
            MotionPhase::Outcome,
            &mood("excited", 12.0),
            Some(true),
            None,
            "even",
            None,
        );
        assert_eq!(done.cues[0].intent, "delight");
    }

    #[test]
    fn a_restrained_persona_stays_restrained_when_the_mood_is_high() {
        let plan = local_performance_plan(
            MotionPhase::Delivery,
            &mood("excited", 0.0),
            None,
            None,
            "restrained",
            None,
        );
        let baseline = plan.baseline.unwrap();
        assert_eq!(baseline.posture, "closed");
        assert!(baseline.motion_energy < 0.8);
    }
}
