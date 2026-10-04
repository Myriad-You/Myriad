//! When the person asks for a motion by name: noticing it, and whether her reply plays along.

use super::*;

pub(super) const USER_CUE_REQUEST_MARKERS: &[&str] = &[
    "做",
    "来个",
    "来一下",
    "给我做",
    "给我来",
    "表演",
    "露出",
    "摆出",
    "make a",
    "do a",
    "show me",
];

pub(super) const USER_CUE_ALIASES: &[(&str, &[&str])] = &[
    ("maniac", &["狂笑", "疯笑", "发狂", "maniac"]),
    ("silly", &["呆呆", "发呆", "犯傻", "傻眼", "silly"]),
    ("cry", &["哭脸", "哭一个", "哭泣", "哭一下", "cry"]),
    ("angry", &["生气", "愤怒", "怒脸", "angry"]),
    ("speechless", &["无语", "无语脸", "speechless"]),
    ("dizzy", &["晕倒", "眩晕", "dizzy"]),
    ("lovestruck", &["心动", "花痴", "lovestruck"]),
    ("think", &["思考脸", "思考的表情", "think"]),
];

pub(super) fn user_requested_cue(text: &str) -> Option<&'static str> {
    let folded = text.to_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    for (intent, aliases) in USER_CUE_ALIASES {
        for alias in *aliases {
            let Some(pos) = alias_request_pos(text, &folded, alias) else {
                continue;
            };
            if best.is_none_or(|(seen, _)| pos < seen) {
                best = Some((pos, *intent));
            }
        }
    }
    best.map(|(_, intent)| intent)
}

pub(super) fn alias_request_pos(text: &str, folded: &str, alias: &str) -> Option<usize> {
    let alias_folded = alias.to_lowercase();
    // A Latin alias must stand on its own here too: `cry` sits inside
    // `cryptic`, and `think` in `thinking about it` is not a request for the
    // think face — the marker below is what turns a mention into an ask.
    let pos = text_mention_pos(text, folded, alias)?;
    if alias.contains("一下") || alias.contains("一个") {
        return Some(pos);
    }
    let asked = USER_CUE_REQUEST_MARKERS
        .iter()
        .any(|marker| text.contains(marker) || folded.contains(&marker.to_lowercase()));
    if asked {
        return Some(pos);
    }
    let compounds = [
        format!("{alias}一下"),
        format!("一下{alias}"),
        format!("{alias}一个"),
        format!("{alias}的表情"),
        format!("做个{alias}"),
        format!("make a {alias_folded}"),
        format!("do a {alias_folded}"),
        format!("show me {alias_folded}"),
    ];
    compounds
        .iter()
        .any(|phrase| text.contains(phrase) || folded.contains(&phrase.to_lowercase()))
        .then_some(pos)
}

pub fn text_mentions_any(text: &str, markers: &[&str]) -> bool {
    let folded = text.to_lowercase();
    markers
        .iter()
        .any(|marker| text_mention_pos(text, &folded, marker).is_some())
}

/// A Latin marker has to stand as its own word.
///
/// `hi` sits inside `this` and `which`, `hey` inside `they`, and `ww` inside
/// `www.` — with a plain substring test the floor greeted on most English
/// sentences and read any link as a joke. Chinese and Japanese have no word
/// boundary to test, so those markers stay substrings.
///
/// A repeated-letter marker may still be extended by more of the same letter,
/// because that is how `hhh` and `ww` are actually written; a following `.`
/// still rules the match out, which is what separates `www` from a hostname.
pub(super) fn text_mention_pos(text: &str, folded: &str, marker: &str) -> Option<usize> {
    if !marker.is_ascii() {
        return text.find(marker).or_else(|| folded.find(marker));
    }
    let lowered = marker.to_lowercase();
    let repeated = lowered
        .chars()
        .next()
        .filter(|first| lowered.len() > 1 && lowered.chars().all(|letter| letter == *first));
    let blocks = |letter: Option<char>| letter.is_some_and(|letter| letter.is_ascii_alphanumeric());
    folded.match_indices(&lowered).find_map(|(index, found)| {
        // Widen to the whole run first: `www.` is a hostname however much of
        // it a two-letter marker happens to land on.
        let (head, tail) = (&folded[..index], &folded[index + found.len()..]);
        let (head, tail) = match repeated {
            Some(letter) => (
                head.trim_end_matches(letter),
                tail.trim_start_matches(letter),
            ),
            None => (head, tail),
        };
        let standalone = !blocks(head.chars().next_back())
            && !blocks(tail.chars().next())
            && !tail.starts_with('.');
        standalone.then_some(index)
    })
}

pub(super) fn requested_cue(intent: &str) -> ChatPerformanceCue {
    let sticker = matches!(
        intent,
        "maniac" | "silly" | "cry" | "dizzy" | "lovestruck" | "angry"
    );
    ChatPerformanceCue {
        intent: intent.to_string(),
        at_ms: 0,
        intensity: 1.15,
        tempo: 1.0,
        fade_in_ms: if sticker { 180 } else { 100 },
        fade_out_ms: if sticker { 420 } else { 220 },
        interrupt: "replace".to_string(),
    }
}

pub(super) fn landing_baseline(_intent: &str) -> ChatPerformanceBaseline {
    ChatPerformanceBaseline {
        expression: "steady".to_string(),
        posture: "neutral".to_string(),
        motion_energy: 1.0,
        attention: 0.65,
        pose: None,
    }
}

pub(super) const REQUEST_REFUSALS: &[&str] = &[
    "才不",
    "才不会",
    "才不要",
    "才不给",
    "不要做",
    "不做",
    "不想做",
    "拒绝",
    "别做",
    "i won't",
    "i will not",
    "no way",
];

pub(super) fn response_refuses_requested_cue(response: &str) -> bool {
    let folded = response.to_lowercase();
    REQUEST_REFUSALS
        .iter()
        .any(|marker| response.contains(marker) || folded.contains(&marker.to_lowercase()))
}

pub(super) fn play_along_requested_cue(
    phase: MotionPhase,
    user_text: &str,
    response_text: Option<&str>,
) -> Option<&'static str> {
    if phase != MotionPhase::Delivery {
        return None;
    }
    let response = response_text
        .map(str::trim)
        .filter(|text| !text.is_empty())?;
    let intent = user_requested_cue(user_text)?;
    if response_refuses_requested_cue(response) {
        return None;
    }
    Some(intent)
}

pub(super) fn apply_user_requested_cue(
    mut plan: ChatPerformancePlan,
    phase: MotionPhase,
    user_text: &str,
    response_text: Option<&str>,
    rig: Option<&RigStateSummary>,
) -> ChatPerformancePlan {
    let Some(intent) = play_along_requested_cue(phase, user_text, response_text) else {
        return plan;
    };
    if let Some(state) = rig {
        if !cue_is_playable(&state.capabilities, intent) {
            return plan;
        }
    }
    plan.cues.retain(|cue| cue.intent != intent);
    plan.cues.insert(0, requested_cue(intent));
    if plan.cues.len() > 3 {
        plan.cues.truncate(3);
    }
    // A requested face must not discard independently directed body targets.
    let pose = plan
        .baseline
        .as_mut()
        .and_then(|baseline| baseline.pose.take());
    let mut baseline = landing_baseline(intent);
    baseline.pose = pose;
    plan.baseline = Some(baseline);
    plan
}
