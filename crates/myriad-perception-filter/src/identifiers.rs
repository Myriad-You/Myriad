//! Card numbers and national identity numbers: wiped before any model reads
//! the text, and a hit means the window shows the owner's own identity.
//!
//! Only numbers whose check digit is right are taken; a well-formed number
//! with a wrong check digit stays. Order and tracking numbers that happen to
//! pass Luhn are wiped too: one number fewer seen.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::secrets::Wiped;

/// What a wiped number is replaced with.
pub const MARK: &str = "[号码已抹去]";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierKind {
    /// 13–19 digits starting 3–6, Luhn.
    Card,
    /// Singapore NRIC / FIN, S T F G series (the 2022 M series is not taken yet).
    SgNric,
    /// Japan's individual number, 12 digits.
    JpMyNumber,
}

/// Digit groups joined by single spaces or hyphens.
static DIGIT_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]+(?:[ \-][0-9]+)*").unwrap());
static NRIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[STFG][0-9]{7}[A-Z]\b").unwrap());
/// Within 20 characters of a My Number, unless it is written 4-4-4.
static MY_NUMBER_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)マイナンバー|個人番号|my\s?number").unwrap());

pub fn scan(text: &str) -> Wiped<IdentifierKind> {
    let mut found: Vec<(usize, usize, IdentifierKind)> = Vec::new();
    for run in DIGIT_RUN.find_iter(text) {
        // A digit glued to a letter or another digit is part of something else.
        let before = text[..run.start()].chars().next_back();
        let after = text[run.end()..].chars().next();
        if before.is_some_and(|c| c.is_ascii_alphanumeric())
            || after.is_some_and(|c| c.is_ascii_alphanumeric())
        {
            continue;
        }
        found.extend(numbers_in(text, run.start(), run.as_str()));
    }
    for nric in NRIC.find_iter(text) {
        if nric_valid(nric.as_str()) {
            found.push((nric.start(), nric.end(), IdentifierKind::SgNric));
        }
    }
    found.sort_by_key(|(start, _, _)| *start);
    let mut out = text.to_string();
    for (start, end, _) in found.iter().rev() {
        out.replace_range(*start..*end, MARK);
    }
    Wiped {
        text: out,
        wiped: found.into_iter().map(|(_, _, kind)| kind).collect(),
    }
}

/// Cards and My Numbers among one run's groups: the longest valid window of
/// whole groups wins, then the search goes on after it.
fn numbers_in(text: &str, offset: usize, run: &str) -> Vec<(usize, usize, IdentifierKind)> {
    // (start, end) of each group of digits, in bytes within `run`.
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (at, c) in run.char_indices() {
        match (c.is_ascii_digit(), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                groups.push((from, at));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        groups.push((from, run.len()));
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < groups.len() {
        let mut best: Option<(usize, IdentifierKind)> = None;
        for j in (i..groups.len()).rev() {
            let window = &run[groups[i].0..groups[j].1];
            let digits: Vec<u32> = window.chars().filter_map(|c| c.to_digit(10)).collect();
            let sizes: Vec<usize> = (i..=j).map(|k| groups[k].1 - groups[k].0).collect();
            if card(&digits) {
                best = Some((j, IdentifierKind::Card));
                break;
            }
            let grouped = sizes == [4, 4, 4];
            let labelled =
                sizes == [12] && labelled_near(text, offset + groups[i].0, offset + groups[j].1);
            if (grouped || labelled) && my_number(&digits) {
                best = Some((j, IdentifierKind::JpMyNumber));
                break;
            }
        }
        match best {
            Some((j, kind)) => {
                out.push((offset + groups[i].0, offset + groups[j].1, kind));
                i = j + 1;
            }
            None => i += 1,
        }
    }
    out
}

fn card(digits: &[u32]) -> bool {
    (13..=19).contains(&digits.len()) && (3..=6).contains(&digits[0]) && luhn(digits)
}

pub fn luhn(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(index, &digit)| {
            let value = if index % 2 == 1 { digit * 2 } else { digit };
            if value > 9 { value - 9 } else { value }
        })
        .sum();
    sum.is_multiple_of(10)
}

pub fn nric_valid(raw: &str) -> bool {
    let chars: Vec<char> = raw.to_ascii_uppercase().chars().collect();
    if chars.len() != 9 {
        return false;
    }
    let body: Vec<u32> = chars[1..8].iter().filter_map(|c| c.to_digit(10)).collect();
    if body.len() != 7 {
        return false;
    }
    let mut sum: u32 = body
        .iter()
        .zip([2, 7, 6, 5, 4, 3, 2])
        .map(|(d, w)| d * w)
        .sum();
    if matches!(chars[0], 'T' | 'G') {
        sum += 4;
    }
    let letters: &[u8] = match chars[0] {
        'S' | 'T' => b"JZIHGFEDCBA",
        'F' | 'G' => b"XWUTRQPNMLK",
        _ => return false,
    };
    letters[(sum % 11) as usize] as char == chars[8]
}

pub fn my_number(digits: &[u32]) -> bool {
    if digits.len() != 12 {
        return false;
    }
    // From the right, the n-th digit before the check digit weighs n+1 (n ≤ 6) or n−5.
    let sum: u32 = (1..=11)
        .map(|n| digits[11 - n] * if n <= 6 { n as u32 + 1 } else { n as u32 - 5 })
        .sum();
    let check = if sum % 11 <= 1 { 0 } else { 11 - sum % 11 };
    digits[11] == check
}

/// A My Number label within 20 characters before or after `start..end`.
fn labelled_near(text: &str, start: usize, end: usize) -> bool {
    let from = text[..start]
        .char_indices()
        .rev()
        .nth(19)
        .map_or(0, |(at, _)| at);
    let to = text[end..]
        .char_indices()
        .nth(20)
        .map_or(text.len(), |(at, _)| end + at);
    MY_NUMBER_LABEL.is_match(&text[from..to])
}
