//! Her acting asked of a decisions model with the body itself (direct
//! control): every control this face has is its own question, answered with
//! the contract's tiers or rest; the expression, posture and energy are picked
//! from the contract; each sentence gets how it is said and which there-and-
//! back move lands on it. The answers are the plan: no table in between.
//!
//! A decisions model answers some forty questions in about a second (Jev
//! 0.7–1.1 s, Microsoft-Decision 1–2 s, 10-12); a chat model takes 5–12 s, so
//! it answers the shorter questions in `questions` instead.

use myriad_merope::RigStateSummary;
use serde_json::{Map, Value, json};

use super::MotionContext;
use super::offered::{offered_body_controls, offered_score_moves};
use super::questions::{OWN, SAYING, SURE, agreed_face, asked_for_question, sentences};

const EXPRESSIONS: &[(&str, &str)] = &[
    ("withdrawn", "drawn in, avoiding, not opening up"),
    (
        "subdued",
        "held down but still present; restrained, earnest",
    ),
    ("steady", "her ordinary face"),
    ("warm", "relaxed, close, smiling"),
    (
        "tense",
        "irritable, taut, out of patience; brow down, eyes more open",
    ),
];
const POSTURES: &[(&str, &str)] = &[
    ("closed", "drawn in, not taking space"),
    ("neutral", "ordinary stance"),
    ("open", "open, closer, welcoming"),
];
const ENERGY: &[(&str, &str)] = &[
    ("low", "quiet, small movements"),
    ("usual", "her usual liveliness"),
    ("high", "lively, big movements"),
];

pub(super) fn criteria(pairs: &[(&str, &str)]) -> Value {
    pairs
        .iter()
        .map(|(name, means)| ((*name).to_owned(), json!(means)))
        .collect::<Map<_, _>>()
        .into()
}

/// The tiers a control may take, each in words, and rest.
fn tier_options(control: &str) -> Value {
    let mut options = Map::new();
    options.insert("rest".into(), json!("where it rests; not directed"));
    for tier in myriad_merope::extent_tiers(control) {
        let (sign, strength) = match tier.strip_prefix('-') {
            Some(strength) => ("toward the negative end", strength),
            None => ("toward the positive end", tier.as_str()),
        };
        let how = match strength {
            "slight" => "a little, small yet plainly seen",
            "clear" => "clearly",
            _ => "as far as it goes",
        };
        options.insert(tier.clone(), json!(format!("{how}, {sign}")));
    }
    options.into()
}

/// Head, gaze, torso and arms: how she holds herself, not what she feels.
fn is_bearing(control: &str) -> bool {
    control.starts_with("head")
        || control.starts_with("gaze")
        || control.starts_with("torso")
        || control.contains("Arm")
}

/// Her face, bearing and every control this face has, `when` she is at it
/// (saying it, or what she is doing on her own). How a new face comes over
/// hers is asked only where there are faces to choose.
fn ask_bearing(questions: &mut Map<String, Value>, state: Option<&RigStateSummary>, when: &str) {
    if let Some(question) = face_question(state, when) {
        questions.insert("face".into(), question);
        questions.insert("shift".into(), shift_question(state, when));
    }
    questions.insert(
        "expression".into(),
        json!({"type": "choice",
        "instructions": format!("Her face's standing expression {when}."),
        "criteria": criteria(EXPRESSIONS)}),
    );
    questions.insert(
        "posture".into(),
        json!({"type": "choice",
        "instructions": format!("How she holds herself {when}."),
        "criteria": criteria(POSTURES)}),
    );
    questions.insert(
        "energy".into(),
        json!({"type": "choice",
        "instructions": format!("How much she moves {when}."),
        "criteria": criteria(ENERGY)}),
    );
    for control in offered_body_controls(state) {
        let means = myriad_merope::PERFORMANCE_BODY_CONTROLS
            .iter()
            .find(|c| c.0 == control)
            .map(|c| c.4)
            .unwrap_or_default();
        let hint = if control.contains("Arm") {
            " People use their hands: arms open when explaining or welcoming, draw in when shy or hurt, lift when excited."
        } else if control.starts_with("eye") {
            " The eyes carry most of a face: lids half down when content, drowsy or reading, wide when struck, smiling when amused."
        } else if control == "headNod" {
            " The head goes down over a page or when low, up when daydreaming or proud."
        } else {
            ""
        };
        questions.insert(format!("body.{control}"), json!({"type": "choice",
            "instructions": format!("Her {control} {when} ({means}) Image-left/right are as the viewer sees her.{hint} When the moment calls for it, show it plainly: on a small screen slight barely shows, clear reads."),
            "criteria": tier_options(control)}));
    }
}

/// What a decisions model is asked about this moment, from what this face can do.
pub(super) fn control_questions(context: &MotionContext, state: Option<&RigStateSummary>) -> Value {
    let mut questions = Map::new();
    questions.insert("own".into(), json!({"type": "noul",
        "instructions": "Is she showing her own feeling, rather than quoting or describing someone else's?",
        "criteria": {"true": "her own feeling, in her own words", "false": "quoting or describing someone else, or a story"}}));
    ask_bearing(&mut questions, state, "while she says it");
    let offered = offered_score_moves(state);
    let moves: Vec<(&str, &str)> = myriad_merope::PERFORMANCE_SCORE_MOVES
        .iter()
        .filter(|(name, _, _)| offered.contains(name))
        .map(|(name, _, means)| (*name, *means))
        .chain([("none", "no movement of its own on this sentence")])
        .collect();
    for (index, sentence) in sentences(context.response_text.as_deref())
        .iter()
        .enumerate()
    {
        questions.insert(
            format!("say{index}"),
            json!({"type": "choice",
            "instructions": format!("How she says 「{sentence}」"),
            "criteria": criteria(SAYING)}),
        );
        questions.insert(format!("move{index}"), json!({"type": "choice",
            "instructions": format!("Which movement that goes and comes back by itself lands as she says 「{sentence}」, if any. A person explaining or stressing a point often beats a hand; an unsure one shrugs."),
            "criteria": criteria(&moves)}));
    }
    if let Some(question) = asked_for_question(context, state) {
        questions.insert("askedFor".into(), question);
    }
    Value::Object(questions)
}

/// The looks drawn from her own controls (`facePresets` in the contract)
/// that this face can show, offered as a whole face at once.
pub(super) fn offered_faces(state: Option<&RigStateSummary>) -> Vec<(&'static str, &'static str)> {
    let controls = offered_body_controls(state);
    myriad_merope::PERFORMANCE_FACE_PRESETS
        .iter()
        .filter(|(_, _, targets)| {
            let shown = targets
                .iter()
                .filter(|(control, _)| controls.contains(control))
                .count();
            // A look named for its drawn accent is not that look without it.
            let drawn = targets.iter().all(|(control, _)| {
                controls.contains(control)
                    || !myriad_merope::PERFORMANCE_BODY_CONTROLS.iter().any(|c| {
                        c.0 == *control && myriad_merope::RIG_STATE_DRAWN_ACCENTS.contains(&c.3)
                    })
            });
            shown * 2 >= targets.len() && drawn
        })
        .map(|(name, means, _)| (*name, *means))
        .collect()
}

pub(super) fn face_question(state: Option<&RigStateSummary>, when: &str) -> Option<Value> {
    let mut faces = offered_faces(state);
    if faces.is_empty() {
        return None;
    }
    faces.push((
        "none",
        "none of these: her face as the other answers make it",
    ));
    Some(json!({"type": "choice",
        "instructions": format!("Which of these whole looks her face takes {when}, if any."),
        "criteria": criteria(&faces)}))
}

/// How she gets from the look she holds now to the one she takes `when`:
/// the body carries on from where it is, at that pace and in that order.
pub(super) fn shift_question(state: Option<&RigStateSummary>, when: &str) -> Value {
    json!({"type": "choice",
        "instructions": format!(
            "Her face now: {}. If her look changes {when}, how she gets from this one to the next.",
            face_now(state)
        ),
        "criteria": criteria(myriad_merope::PERFORMANCE_FACE_SHIFTS)})
}

fn face_now(state: Option<&RigStateSummary>) -> &'static str {
    look_means(
        state
            .and_then(|state| state.face.as_deref())
            .unwrap_or(myriad_merope::PLAIN_FACE),
    )
}

/// What a look is, in words: a preset's meaning, else her ordinary face.
pub(super) fn look_means(face: &str) -> &'static str {
    myriad_merope::PERFORMANCE_FACE_PRESETS
        .iter()
        .find(|(name, _, _)| *name == face)
        .map_or("her ordinary face", |(_, means, _)| *means)
}

/// The look she chose, whole: a look is tuned as one, so it holds over a
/// control answered on its own (two eyes answered apart read as a wink);
/// her other answers fill what it leaves. With `own` false only her
/// bearing is taken. The look is named back when it is hers, so the body
/// plays its life over it.
pub(super) fn lay_face(
    targets: &mut Map<String, Value>,
    answers: &Value,
    state: Option<&RigStateSummary>,
    own: bool,
) -> Option<&'static str> {
    let face = sure(answers, "face").filter(|face| *face != "none")?;
    let controls = offered_body_controls(state);
    let (name, _, look) = myriad_merope::PERFORMANCE_FACE_PRESETS
        .iter()
        .find(|(name, _, _)| *name == face)?;
    for (control, tier) in look.iter() {
        if controls.contains(control) && (own || is_bearing(control)) {
            targets.insert((*control).to_owned(), json!(tier));
        }
    }
    own.then_some(*name)
}

/// The standing targets the answers give, control by control; with `own`
/// false only her bearing stays.
fn answered_targets(
    answers: &Value,
    state: Option<&RigStateSummary>,
    own: bool,
) -> Map<String, Value> {
    let mut targets = Map::new();
    for control in offered_body_controls(state) {
        // Someone else's feeling is not hers to wear: only where she looks
        // and how she holds herself stay hers.
        if !own && !is_bearing(control) {
            continue;
        }
        if let Some(tier) = sure(answers, &format!("body.{control}")).filter(|tier| *tier != "rest")
        {
            targets.insert(control.to_owned(), json!(tier));
        }
    }
    targets
}

/// An answer's choice when the model is sure enough of it.
pub(super) fn sure<'a>(answers: &'a Value, key: &str) -> Option<&'a str> {
    let answer = &answers[key];
    (answer["confidence"].as_f64().unwrap_or(0.0) >= SURE)
        .then(|| answer["choice"].as_str())
        .flatten()
}

/// The answers as the plan, control by control.
pub(super) fn control_plan(
    answers: &Value,
    context: &MotionContext,
    state: Option<&RigStateSummary>,
) -> Value {
    let sure = |key: &str| sure(answers, key);
    let own = answers["own"]["noul"].as_f64().unwrap_or(1.0) >= OWN;
    let mut targets = answered_targets(answers, state, own);
    let face = lay_face(&mut targets, answers, state, own);
    let energy = sure("energy").unwrap_or("usual");
    let amount = match energy {
        "low" => "slight",
        "high" => "strong",
        _ => "clear",
    };
    let mut score = Vec::new();
    let mut phrases = Vec::new();
    for (index, sentence) in sentences(context.response_text.as_deref())
        .into_iter()
        .enumerate()
    {
        if let Some(intent) = sure(&format!("say{index}")) {
            // A sentence said plainly is "none" too, so the speech layer does
            // not act its question mark or laughter instead.
            let intent = if own || intent != "laugh" {
                intent
            } else {
                "none"
            };
            phrases.push(json!({"text": sentence, "intent": intent}));
        }
        if let Some(kind) = sure(&format!("move{index}")).filter(|kind| *kind != "none") {
            // Delight or a sigh for someone else's feeling is not hers to act.
            if own || !matches!(kind, "bounce" | "sigh") {
                let anchor: String = sentence.chars().take(24).collect();
                score.push(json!({"text": anchor, "move": {"kind": kind, "amount": amount}}));
            }
        }
    }
    let asked = sure("askedFor").filter(|intent| *intent != "none");
    let expression = match (asked, own) {
        (None, true) => sure("expression").unwrap_or("steady"),
        _ => "steady",
    };
    json!({
        "phrases": phrases,
        "baseline": {"expression": expression, "posture": sure("posture").unwrap_or("neutral"),
            "motionEnergy": energy, "attention": "usual",
            "pose": {"targets": targets, "transitionMs": 350, "holdMs": 0}, "face": face,
            "shift": sure("shift")},
        "cues": agreed_face(asked),
        "score": score,
        "askedFor": asked,
    })
}

/// Her standing acting plays again each this many milliseconds.
pub(super) const STANDING_LOOP_MS: u64 = 20_000;
/// Moments in each loop she is asked about: where her eyes go, what she does.
const STANDING_MOMENTS: u64 = 6;

/// When in the loop each moment falls: uneven, so the repeat does not show,
/// and the same for the same activity, so everyone watching sees one.
pub(super) fn standing_moments(activity: &str) -> Vec<u64> {
    let mut seed = activity_seed(activity);
    let step = STANDING_LOOP_MS / STANDING_MOMENTS;
    (0..STANDING_MOMENTS)
        .map(|index| index * step + u64::from(next_seed(&mut seed)) % (step * 2 / 3))
        .collect()
}

/// A seed from what she is doing: the same activity, the same unevenness, so
/// one rhythm serves everyone watching.
pub(super) fn activity_seed(activity: &str) -> u32 {
    activity.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

/// The next of a seed's run of uneven numbers (xorshift).
pub(super) fn next_seed(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

/// What a decisions model is asked about her standing acting: her bearing
/// and every control while she is at it, and at each moment of the loop
/// where her eyes go and which there-and-back move she makes.
pub(super) fn standing_questions(
    context: &MotionContext,
    state: Option<&RigStateSummary>,
) -> Value {
    let mut questions = Map::new();
    ask_bearing(&mut questions, state, "while she is at it");
    let controls = offered_body_controls(state);
    let offered = offered_score_moves(state);
    let moves: Vec<(&str, &str)> = myriad_merope::PERFORMANCE_SCORE_MOVES
        .iter()
        .filter(|(name, _, _)| offered.contains(name))
        .map(|(name, _, means)| (*name, *means))
        .chain([("none", "nothing of its own at this moment")])
        .collect();
    let seconds = STANDING_LOOP_MS / 1000;
    for (index, at) in standing_moments(&context.activity).into_iter().enumerate() {
        let when = format!(
            "about {:.1} s into each {seconds} s stretch of it",
            at as f64 / 1000.0
        );
        for control in ["gazeHorizontal", "gazeVertical"] {
            if controls.contains(&control) {
                // Her eyes are always somewhere: no option leaves the look undirected.
                let mut options = tier_options(control);
                if let Some(options) = options.as_object_mut() {
                    options.remove("rest");
                    options.insert("ahead".into(), json!("straight ahead, at the screen"));
                }
                questions.insert(format!("{control}{index}"), json!({"type": "choice",
                    "instructions": format!("Her {control} {when}: where her eyes go as what she is doing calls for it (reading moves them along lines, a song softens and drops them, a daydream drifts them, looking about turns them)."),
                    "criteria": options}));
            }
        }
        questions.insert(format!("move{index}"), json!({"type": "choice",
            "instructions": format!("Which movement that goes and comes back by itself she makes {when}, if any. Someone busy on their own still moves now and then: a nod with a song, a sigh, a glance up from a page, a blink."),
            "criteria": criteria(&moves)}));
    }
    Value::Object(questions)
}

/// Her standing acting asked as a combination (for a decisions model, which
/// answers each question on its own and so cannot lay out a sequence moment
/// by moment): her bearing, and three choices about the whole stretch —
/// where her eyes mostly are, how they move, and her movements of her own —
/// that together become the sequence.
pub(super) fn standing_combined_questions(state: Option<&RigStateSummary>) -> Value {
    let mut questions = Map::new();
    ask_bearing(&mut questions, state, "while she is at it");
    let controls = offered_body_controls(state);
    if controls.contains(&"gazeHorizontal") && controls.contains(&"gazeVertical") {
        questions.insert("lookLevel".into(), json!({"type": "choice",
            "instructions": "Where her eyes mostly are over the whole stretch of what she is doing.",
            "criteria": {"low": "lowered, on something in front of her", "ahead": "straight ahead, at the screen",
                "high": "raised, up and away", "aside": "off to one side"}}));
        questions.insert("lookPattern".into(), json!({"type": "choice",
            "instructions": "How her eyes move over the whole stretch.",
            "criteria": {"sweep": "back and forth along a line, again and again, as when reading lines",
                "drift": "slowly wandering off and settling, unhurried",
                "roam": "looking about in different directions",
                "hold": "staying put, hardly moving"}}));
    }
    if controls.contains(&"headNod") {
        questions.insert("headPose".into(), json!({"type": "choice",
            "instructions": "How she holds her head over the whole stretch.",
            "criteria": {"down": "bowed over what she is doing", "level": "level", "up": "raised, chin up",
                "tilted": "resting tilted to one side"}}));
    }
    let offered = offered_score_moves(state);
    let mut rhythm: Vec<(&str, &str)> = [
        (
            "nods",
            "a small nod every few seconds, as with a song",
            "nod",
        ),
        ("blinks", "a slow blink now and then", "blink"),
        ("glances", "a glance away and back now and then", "glance"),
        ("sigh", "a sigh once in a while", "sigh"),
    ]
    .into_iter()
    .filter(|(_, _, kind)| offered.contains(kind))
    .map(|(name, means, _)| (name, means))
    .collect();
    rhythm.push(("none", "no movements of her own"));
    questions.insert("rhythm".into(), json!({"type": "choice",
        "instructions": "Her movements of her own over the whole stretch. Someone busy on their own still moves now and then.",
        "criteria": criteria(&rhythm)}));
    Value::Object(questions)
}

/// The sequence a combination of answers makes: at each moment, where her
/// eyes are and the move she makes.
fn combined_moments(
    answers: &Value,
    activity: &str,
    standing_vertical: Option<f64>,
) -> Vec<(Map<String, Value>, Option<Value>)> {
    let pick = |key: &str| answers[key]["choice"].as_str().unwrap_or_default();
    let side: f64 = if activity.len() % 2 == 0 { 1.0 } else { -1.0 };
    // How high her eyes stand, if she was asked for it control by control,
    // is hers; the stretch's level only fills in when it was not.
    let level = standing_vertical.unwrap_or(match pick("lookLevel") {
        "low" => -0.65,
        "high" => 0.35,
        _ => 0.0,
    });
    let aside = if pick("lookLevel") == "aside" {
        0.65 * side
    } else {
        0.0
    };
    // How far her eyes travel grows with how much she moves.
    let reach = match pick("energy") {
        "low" => 0.35,
        "high" => 0.8,
        _ => 0.55,
    };
    let pattern = pick("lookPattern");
    let looking = !pattern.is_empty() || !pick("lookLevel").is_empty();
    let roam = [
        (-1.0, 0.0),
        (1.0, 0.6),
        (0.0, -0.6),
        (1.0, 0.0),
        (-1.0, 0.6),
        (0.0, 0.0),
    ];
    let drift = [1.0, 1.0, 0.0, -1.0, -1.0, 0.0];
    let rhythm = pick("rhythm");
    (0..STANDING_MOMENTS as usize)
        .map(|index| {
            let (h, v): (f64, f64) = match pattern {
                "sweep" => (if index % 2 == 0 { -reach } else { reach } * side, level),
                "drift" => (drift[index] * reach * side, level),
                "roam" => (roam[index].0 * reach * side, level + roam[index].1 * reach),
                _ => (if index == 3 { 0.2 * side } else { 0.0 }, level),
            };
            let mut look = Map::new();
            if looking {
                look.insert("gazeHorizontal".into(), json!((h + aside).clamp(-1.0, 1.0)));
                look.insert("gazeVertical".into(), json!(v.clamp(-1.0, 1.0)));
            }
            let directions = ["left", "up", "right"];
            let mv = match rhythm {
                "nods" if index % 2 == 0 => Some(json!({"kind": "nod"})),
                "blinks" if index == 1 || index == 4 => Some(json!({"kind": "blink"})),
                "glances" if index == 2 || index == 5 => {
                    Some(json!({"kind": "glance", "direction": directions[index % 3]}))
                }
                "sigh" if index == 3 => Some(json!({"kind": "sigh"})),
                _ => None,
            };
            (look, mv)
        })
        .collect()
}

/// The moment the standing questions are about.
pub(super) fn standing_state(context: &MotionContext, persona: Option<&str>) -> Value {
    json!({
        "she": persona.unwrap_or("a lively, warm companion"),
        "nobody is talking with her; she is": context.activity,
        "her mood": context.mood.band_after,
        "how she moves": super::questions::way_of_moving(&context.motion_style),
    })
}

/// The answers as her standing acting: a standing pose and, at each moment
/// of the loop, where her eyes go and the move she makes.
pub(super) fn standing_plan(
    answers: &Value,
    context: &MotionContext,
    state: Option<&RigStateSummary>,
) -> Value {
    let sure = |key: &str| sure(answers, key);
    let mut targets = answered_targets(answers, state, true);
    let face = lay_face(&mut targets, answers, state, true);
    // How she holds her head over the stretch, unless a control said already.
    let controls = offered_body_controls(state);
    let head = match answers["headPose"]["choice"].as_str() {
        Some("down") => Some(("headNod", "-clear")),
        Some("up") => Some(("headNod", "clear")),
        Some("tilted") => Some((
            "headTilt",
            if context.activity.len() % 2 == 0 {
                "clear"
            } else {
                "-clear"
            },
        )),
        _ => None,
    };
    if let Some((control, tier)) = head.filter(|(control, _)| controls.contains(control)) {
        targets
            .entry(control.to_owned())
            .or_insert_with(|| json!(tier));
    }
    let energy = sure("energy").unwrap_or("usual");
    let amount = if energy == "high" { "clear" } else { "slight" };
    let moments = standing_moments(&context.activity);
    let mut score = Vec::new();
    if answers.get("rhythm").is_some() {
        let transition = match answers["lookPattern"]["choice"].as_str() {
            Some("drift") => 1500,
            Some("sweep") => 900,
            _ => 600,
        };
        let standing_vertical = targets
            .get("gazeVertical")
            .and_then(Value::as_str)
            .and_then(|tier| myriad_merope::tier_extent("gazeVertical", tier))
            .map(f64::from);
        for (index, (look, mv)) in combined_moments(answers, &context.activity, standing_vertical)
            .into_iter()
            .enumerate()
        {
            let at = moments[index];
            let next = moments.get(index + 1).copied().unwrap_or(STANDING_LOOP_MS);
            if !look.is_empty() {
                score.push(
                    json!({"atMs": at, "pose": {"targets": look, "transitionMs": transition,
                    "holdMs": next.saturating_sub(at).saturating_sub(400).max(300)}}),
                );
            }
            if let Some(mut mv) = mv {
                mv["amount"] = json!(amount);
                mv["tempo"] = json!("slow");
                score.push(json!({"atMs": at + 200, "move": mv}));
            }
        }
    }
    for (index, at) in moments.iter().enumerate() {
        if answers.get("rhythm").is_some() {
            break;
        }
        let next = moments.get(index + 1).copied().unwrap_or(STANDING_LOOP_MS);
        // Any look is a fine look: the choice stands however sure it is.
        let look: Map<String, Value> = ["gazeHorizontal", "gazeVertical"]
            .into_iter()
            .filter_map(|control| {
                answers[format!("{control}{index}")]["choice"]
                    .as_str()
                    .map(|tier| match tier {
                        "ahead" => (control.to_owned(), json!(0.0)),
                        tier => (control.to_owned(), json!(tier)),
                    })
            })
            .collect();
        if !look.is_empty() {
            score.push(
                json!({"atMs": at, "pose": {"targets": look, "transitionMs": 600,
                "holdMs": next.saturating_sub(*at).saturating_sub(400).max(300)}}),
            );
        }
        if let Some(kind) = sure(&format!("move{index}")).filter(|kind| *kind != "none") {
            score.push(json!({"atMs": at + 200, "move": {"kind": kind, "amount": amount, "tempo": "slow"}}));
        }
    }
    json!({
        "baseline": {"expression": sure("expression").unwrap_or("steady"),
            "posture": sure("posture").unwrap_or("neutral"), "motionEnergy": energy, "attention": "loose",
            "pose": {"targets": targets, "transitionMs": 800, "holdMs": 0}, "face": face,
            "shift": sure("shift")},
        "cues": [],
        "score": score,
        "loopMs": STANDING_LOOP_MS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::merope::motion::{
        MotionDecision, MotionPhase, parse_motion_decision, semantic_valid,
    };

    fn context(user: &str, reply: &str) -> MotionContext {
        MotionContext {
            user_id: 1,
            phase: MotionPhase::Delivery,
            mood: serde_json::from_value(
                json!({"before": 60.0, "after": 60.0, "bandBefore": "calm",
                "bandAfter": "calm", "delta": 0.0, "cause": "test", "revision": 1}),
            )
            .unwrap(),
            activity: "talking".into(),
            user_text: user.into(),
            response_text: Some(reply.into()),
            previous_phrases: vec![],
            task_success: None,
            rig_state: None,
            motion_style: "even".into(),
        }
    }

    fn bust() -> RigStateSummary {
        myriad_merope::sanitize_rig_state(&json!({"speaking": true, "capabilities":
            ["head-body", "torso-volume", "independent-eyes", "mouth-shapes", "left-arm", "right-arm"]}))
        .unwrap()
    }

    #[test]
    fn every_control_this_face_has_is_asked_with_its_tiers_and_rest() {
        let state = bust();
        let asked = control_questions(&context("你好", "嗯，好。"), Some(&state));
        for control in offered_body_controls(Some(&state)) {
            let options = &asked[format!("body.{control}")]["criteria"];
            assert!(options["rest"].is_string(), "{control}");
            for tier in myriad_merope::extent_tiers(control) {
                assert!(options[&tier].is_string(), "{control} {tier}");
            }
        }
        assert!(
            asked["body.cry"].is_null(),
            "a face without tears is not asked"
        );
        assert_eq!(asked["body.eyeOpenLeft"]["criteria"]["slight"], Value::Null);
        assert!(
            asked["body.leftArmRaise"]["instructions"]
                .as_str()
                .unwrap()
                .contains("use their hands")
        );
        assert!(asked["say0"].is_object() && asked["move0"].is_object());
    }

    #[test]
    fn the_answers_are_the_plan_control_by_control() {
        let reply = "还有点难受。不过我愿意慢慢说。";
        let state = bust();
        let answers = json!({
            "own": {"noul": 0.95},
            "expression": {"choice": "subdued", "confidence": 0.8},
            "posture": {"choice": "closed", "confidence": 0.7},
            "energy": {"choice": "low", "confidence": 0.7},
            "body.headNod": {"choice": "-slight", "confidence": 0.8},
            "body.leftArmRaise": {"choice": "-slight", "confidence": 0.6},
            "body.browLift": {"choice": "-clear", "confidence": 0.2},
            "body.headTurn": {"choice": "rest", "confidence": 0.9},
            "say0": {"choice": "none", "confidence": 0.8},
            "move0": {"choice": "sigh", "confidence": 0.7},
        });
        let value = control_plan(&answers, &context("怎么了？", reply), Some(&state));
        let raw = value.to_string();
        assert!(semantic_valid(&raw, reply), "{raw}");
        let Some(MotionDecision::Perform(plan)) = parse_motion_decision(&raw) else {
            panic!("{raw}")
        };
        let targets = plan.baseline.unwrap().pose.unwrap().targets;
        assert_eq!(targets["headNod"], -0.35);
        assert!(targets["leftArmRaise"] < 0.0, "hands drawn in");
        assert!(
            !targets.contains_key("browLift"),
            "an unsure answer is not acted"
        );
        assert!(!targets.contains_key("headTurn"), "rest is not directed");
        assert_eq!(
            value["score"][0]["move"],
            json!({"kind": "sigh", "amount": "slight"})
        );
        assert_eq!(
            value["phrases"][0],
            json!({"text": "还有点难受。", "intent": "none"})
        );
    }

    #[test]
    fn someone_elses_laughter_keeps_her_bearing_but_not_her_face() {
        let reply = "她说：“哈哈，我太开心了！”这是小说角色的台词。";
        let answers = json!({
            "own": {"noul": 0.05},
            "expression": {"choice": "warm", "confidence": 0.9},
            "body.mouthSmile": {"choice": "strong", "confidence": 0.9},
            "body.eyeSmile": {"choice": "clear", "confidence": 0.9},
            "body.headTilt": {"choice": "slight", "confidence": 0.8},
            "body.leftArmRaise": {"choice": "slight", "confidence": 0.7},
            "say0": {"choice": "laugh", "confidence": 0.8},
            "move0": {"choice": "bounce", "confidence": 0.8},
        });
        let value = control_plan(&answers, &context("这句什么意思", reply), Some(&bust()));
        let targets = &value["baseline"]["pose"]["targets"];
        assert_eq!(value["baseline"]["expression"], "steady");
        assert!(targets["mouthSmile"].is_null() && targets["eyeSmile"].is_null());
        assert_eq!(targets["headTilt"], "slight");
        assert_eq!(targets["leftArmRaise"], "slight");
        assert_eq!(value["phrases"][0]["intent"], "none");
        assert_eq!(value["score"], json!([]));
    }

    #[test]
    fn her_standing_acting_is_her_body_and_a_look_and_move_at_each_moment() {
        let mut moment = context("", "");
        moment.phase = MotionPhase::Presence;
        moment.response_text = None;
        moment.activity = "reading 小王子".into();
        let state = bust();
        let asked = standing_questions(&moment, Some(&state));
        assert!(asked["body.headNod"].is_object() && asked["own"].is_null());
        let at = standing_moments(&moment.activity);
        assert_eq!(at.len(), STANDING_MOMENTS as usize);
        assert!(at.windows(2).all(|w| w[0] < w[1]) && at[5] < STANDING_LOOP_MS);
        assert_eq!(
            at,
            standing_moments(&moment.activity),
            "one for everyone watching"
        );
        assert!(asked["gazeHorizontal0"].is_object() && asked["move5"].is_object());
        let answers = json!({
            "expression": {"choice": "steady", "confidence": 0.8},
            "body.gazeVertical": {"choice": "-clear", "confidence": 0.8},
            "gazeHorizontal0": {"choice": "-slight", "confidence": 0.7},
            "gazeHorizontal1": {"choice": "slight", "confidence": 0.7},
            "gazeVertical1": {"choice": "ahead", "confidence": 0.2},
            "move3": {"choice": "nod", "confidence": 0.6},
        });
        let value = standing_plan(&answers, &moment, Some(&state));
        assert_eq!(value["loopMs"], STANDING_LOOP_MS);
        assert_eq!(
            value["baseline"]["pose"]["targets"]["gazeVertical"],
            "-clear"
        );
        let beats = value["score"].as_array().unwrap();
        assert_eq!(beats.len(), 3);
        assert_eq!(
            beats[1]["pose"]["targets"],
            json!({"gazeHorizontal": "slight", "gazeVertical": 0.0}),
            "a look stands however unsure; ahead is straight ahead"
        );
        assert!(asked["gazeVertical0"]["criteria"]["rest"].is_null());
        assert_eq!(beats[2]["move"]["kind"], "nod");
        assert!(matches!(
            parse_motion_decision(&value.to_string()),
            Some(MotionDecision::Perform(_))
        ));
    }

    #[test]
    fn a_combination_of_choices_becomes_the_whole_sequence() {
        let mut moment = context("", "");
        moment.phase = MotionPhase::Presence;
        moment.response_text = None;
        moment.activity = "reading 小王子".into();
        let state = bust();
        let asked = standing_combined_questions(Some(&state));
        assert!(asked["lookPattern"]["criteria"]["sweep"].is_string());
        assert!(
            asked["gazeHorizontal0"].is_null(),
            "no moment is asked on its own"
        );
        let answers = json!({
            "lookLevel": {"choice": "low", "confidence": 0.6},
            "lookPattern": {"choice": "sweep", "confidence": 0.5},
            "rhythm": {"choice": "nods", "confidence": 0.4},
            "headPose": {"choice": "down", "confidence": 0.3},
            "energy": {"choice": "high", "confidence": 0.6},
        });
        let value = standing_plan(&answers, &moment, Some(&state));
        assert_eq!(
            value["baseline"]["pose"]["targets"]["headNod"], "-clear",
            "bowed over the page"
        );
        let beats = value["score"].as_array().unwrap();
        let looks: Vec<f64> = beats
            .iter()
            .filter(|beat| beat["pose"].is_object())
            .map(|beat| beat["pose"]["targets"]["gazeHorizontal"].as_f64().unwrap())
            .collect();
        assert_eq!(looks.len(), STANDING_MOMENTS as usize);
        assert!(
            looks.windows(2).all(|w| w[0] == -w[1]) && looks[0].abs() == 0.8,
            "back and forth: {looks:?}"
        );
        assert!(
            beats
                .iter()
                .filter(|beat| beat["pose"].is_object())
                .all(|beat| beat["pose"]["targets"]["gazeVertical"] == -0.65)
        );
        let nods = beats
            .iter()
            .filter(|beat| beat["move"]["kind"] == "nod")
            .count();
        assert_eq!(nods, 3);
        assert!(matches!(
            parse_motion_decision(&value.to_string()),
            Some(MotionDecision::Perform(_))
        ));
    }

    #[test]
    fn a_whole_look_is_laid_under_her_own_answers() {
        let state = bust();
        let asked = control_questions(&context("你好", "嗯，好。"), Some(&state));
        assert!(asked["face"]["criteria"]["content"].is_string());
        assert!(asked["face"]["criteria"]["none"].is_string());
        let answers = json!({
            "own": {"noul": 0.9},
            "face": {"choice": "content", "confidence": 0.7},
            "body.mouthSmile": {"choice": "strong", "confidence": 0.8},
            "body.eyeOpenRight": {"choice": "-slight", "confidence": 0.8},
            "body.headTurn": {"choice": "slight", "confidence": 0.8},
        });
        let value = control_plan(&answers, &context("好听吗", "好好听。"), Some(&state));
        let targets = &value["baseline"]["pose"]["targets"];
        assert_eq!(targets["eyeOpenLeft"], "-strong", "eyes closed in a smile");
        assert_eq!(targets["eyeOpenRight"], "-strong", "both of them");
        assert_eq!(
            targets["headTurn"], "slight",
            "what the look leaves is hers"
        );
        assert_eq!(
            targets["eyeSmile"], "clear",
            "a content smile arches midway (core 7d3da64)"
        );
        assert_eq!(targets["mouthSmile"], "clear", "the look holds whole");
        assert!(semantic_valid(&value.to_string(), "好好听。"));

        let quoted = json!({"own": {"noul": 0.05}, "face": {"choice": "shy", "confidence": 0.9}});
        let value = control_plan(
            &quoted,
            &context("她怎么了", "她说她很害羞。"),
            Some(&state),
        );
        let targets = &value["baseline"]["pose"]["targets"];
        assert_eq!(
            targets["gazeVertical"], "-clear",
            "where she looks stays hers"
        );
        assert!(targets["mouthSmile"].is_null() && targets["eyeOpenLeft"].is_null());
    }

    #[test]
    fn every_look_in_the_contract_is_one_her_controls_can_make() {
        for (name, _, targets) in myriad_merope::PERFORMANCE_FACE_PRESETS {
            assert!(!targets.is_empty(), "{name}");
            for (control, tier) in *targets {
                assert!(
                    myriad_merope::extent_tiers(control)
                        .iter()
                        .any(|t| t == tier),
                    "{name}: {control} {tier}"
                );
            }
        }
        let mouth_only =
            myriad_merope::sanitize_rig_state(&json!({"capabilities": ["mouth-shapes"]})).unwrap();
        assert!(offered_faces(Some(&mouth_only)).len() < offered_faces(Some(&bust())).len());
    }

    #[test]
    fn a_look_with_a_drawn_accent_needs_the_accent() {
        let names = |state: &RigStateSummary| -> Vec<&str> {
            offered_faces(Some(state))
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        };
        let older = names(&bust());
        assert!(older.contains(&"content") && !older.contains(&"humming"));
        let accented = myriad_merope::sanitize_rig_state(&json!({"capabilities":
            ["head-body", "independent-eyes", "mouth-shapes", "music-notes"]}))
        .unwrap();
        let newer = names(&accented);
        assert!(newer.contains(&"humming") && !newer.contains(&"sleepy"));
    }

    #[test]
    fn a_face_she_agrees_to_make_is_played_over_a_steady_face() {
        let state = myriad_merope::sanitize_rig_state(&json!({"speaking": true, "capabilities":
            ["head-body", "mouth-shapes", "maniac-mouth"]}))
        .unwrap();
        let moment = context("做一下狂笑", "好啊，看我的！");
        assert!(
            control_questions(&moment, Some(&state))["askedFor"]["criteria"]["maniac"].is_string()
        );
        let answers = json!({
            "expression": {"choice": "warm", "confidence": 0.9},
            "askedFor": {"choice": "maniac", "confidence": 0.9},
        });
        let value = control_plan(&answers, &moment, Some(&state));
        assert_eq!(value["cues"][0]["intent"], "maniac");
        assert_eq!(value["baseline"]["expression"], "steady");
        assert_eq!(value["askedFor"], "maniac");
    }

    #[test]
    fn a_look_she_takes_is_named_for_its_life_and_survives_the_plan() {
        let moment = context("你在干嘛", "我在哼歌呀。");
        let answers = |own: f64| json!({"own": {"noul": own}, "face": {"choice": "humming", "confidence": 0.9}});
        let value = control_plan(&answers(0.9), &moment, Some(&bust()));
        assert_eq!(value["baseline"]["face"], "humming");
        let plan = myriad_merope::parse_performance_plan(&value.to_string()).unwrap();
        assert_eq!(plan.baseline.unwrap().face.as_deref(), Some("humming"));
        // Someone else's look keeps only her bearing, and no life of its own.
        let quoted = control_plan(&answers(0.05), &moment, Some(&bust()));
        assert!(quoted["baseline"]["face"].is_null());
        // A name the contract does not know is dropped.
        let mut unknown = value.clone();
        unknown["baseline"]["face"] = json!("cackling");
        let plan = myriad_merope::parse_performance_plan(&unknown.to_string()).unwrap();
        assert_eq!(plan.baseline.unwrap().face, None);
    }

    #[test]
    fn how_she_gets_from_the_face_she_holds_to_the_next_is_asked_and_kept() {
        let drowsy = myriad_merope::sanitize_rig_state(&json!({"face": "drowsy", "capabilities":
            ["head-body", "independent-eyes", "mouth-shapes"]}))
        .unwrap();
        let moment = context("中奖了！", "真的吗！太好了！");
        let asked = control_questions(&moment, Some(&drowsy));
        let instructions = asked["shift"]["instructions"].as_str().unwrap();
        assert!(instructions.contains("lids heavy"), "{instructions}");
        assert!(asked["shift"]["criteria"]["snap"].is_string());
        let answers = json!({"face": {"choice": "excited", "confidence": 0.9},
            "shift": {"choice": "snap", "confidence": 0.8}});
        let value = control_plan(&answers, &moment, Some(&drowsy));
        assert_eq!(value["baseline"]["shift"], "snap");
        let plan = myriad_merope::parse_performance_plan(&value.to_string()).unwrap();
        assert_eq!(plan.baseline.unwrap().shift.as_deref(), Some("snap"));
        // No face of her own yet: she still comes from her ordinary one; a made-up shift is dropped.
        assert!(
            control_questions(&moment, Some(&bust()))["shift"]["instructions"]
                .as_str()
                .unwrap()
                .contains("her ordinary face")
        );
        // Without faces to choose there is no change of face to ask about.
        let bare =
            myriad_merope::sanitize_rig_state(&json!({"capabilities": ["head-body"]})).unwrap();
        let plain = control_questions(&moment, Some(&bare));
        assert_eq!(plain.get("face").is_some(), plain.get("shift").is_some());
        assert!(
            asked.get("feeling").is_none(),
            "an answer no plan reads is not asked"
        );
        let mut unknown = value.clone();
        unknown["baseline"]["shift"] = json!("teleport");
        let plan = myriad_merope::parse_performance_plan(&unknown.to_string()).unwrap();
        assert_eq!(plan.baseline.unwrap().shift, None);
    }
}
