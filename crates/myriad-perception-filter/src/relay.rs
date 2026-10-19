//! The host's check of the on-device model's relay of other people's words
//! ("who is talking about what"): a relay that carries a number from the
//! message, copies a stretch of its wording, or leaves out who or what, is
//! dropped.
//!
//! It does not know names or places, so a detail the model slips into the
//! topic in its own words passes; the check bounds the model, it is not a
//! guarantee by itself.

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

/// Copying this many units of the message in a row drops the relay.
pub const COPIED_RUN: usize = 5;
/// A run of this many digits from the message, found in the relay, drops it.
pub const DIGIT_RUN: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayCheck {
    Ok,
    Empty,
    Digits,
    Copied,
}

pub fn check_relay(message: &str, who: &str, topic: &str) -> RelayCheck {
    if who.trim().is_empty() || topic.trim().is_empty() {
        return RelayCheck::Empty;
    }
    let carried = folded_alphanumerics(&format!("{who} {topic}"));
    if digit_runs(message)
        .iter()
        .any(|run| carried.contains(run.as_str()))
    {
        return RelayCheck::Digits;
    }
    if longest_common_run(&units(topic), &units(message)) >= COPIED_RUN {
        return RelayCheck::Copied;
    }
    RelayCheck::Ok
}

fn fold(text: &str) -> String {
    text.nfkc().collect::<String>().to_lowercase()
}

fn folded_alphanumerics(text: &str) -> String {
    fold(text).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Runs of [`DIGIT_RUN`] or more digits; `,` `.` `-` and spaces inside a
/// number do not break it.
pub fn digit_runs(text: &str) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = String::new();
    for c in text.nfkc() {
        if c.is_ascii_digit() {
            current.push(c);
        } else if matches!(c, ',' | '.' | '-' | ' ') {
            continue;
        } else {
            if current.len() >= DIGIT_RUN {
                runs.push(current.clone());
            }
            current.clear();
        }
    }
    if current.len() >= DIGIT_RUN {
        runs.push(current);
    }
    runs
}

/// One unit is a Latin word (letters and digits) or one other character
/// (a CJK character, a kana); punctuation and spaces separate.
pub fn units(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in fold(text).chars() {
        if c.is_ascii() {
            if c.is_ascii_alphanumeric() {
                word.push(c);
            } else if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        } else {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            if c.is_alphanumeric() {
                out.push(c.to_string());
            }
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

pub fn longest_common_run(a: &[String], b: &[String]) -> usize {
    let mut previous = vec![0; b.len() + 1];
    let mut best = 0;
    for x in a {
        let mut current = vec![0; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            if x == y {
                current[j + 1] = previous[j] + 1;
                best = best.max(current[j + 1]);
            }
        }
        previous = current;
    }
    best
}
