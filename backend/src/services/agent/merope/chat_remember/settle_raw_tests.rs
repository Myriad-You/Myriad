use super::*;

fn at(minute: i64) -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::from_timestamp(1_700_000_000 + minute * 60, 0)
        .unwrap()
        .fixed_offset()
}

fn line(id: i32, theirs: bool, minute: i64) -> Spoken {
    Spoken {
        id,
        theirs,
        text: format!("第{id}句，说了点什么"),
        at: at(minute),
        gone: false,
        in_game: false,
    }
}

/// What came after the stretch's last line of theirs and her answer to it
/// is the next stretch's: gone over with it, not twice.
#[test]
fn a_stretch_ends_with_her_answer_to_its_last_line() {
    let lines = vec![
        line(1, true, 0),
        line(2, false, 0),
        line(3, true, 1),
        line(4, false, 1),
        line(5, true, 2),
        line(6, false, 2),
    ];
    let ids = |lines: Vec<Spoken>| lines.into_iter().map(|line| line.id).collect::<Vec<_>>();
    assert_eq!(ids(up_to(lines.clone(), at(1))), [1, 2, 3, 4]);
    assert_eq!(ids(up_to(lines.clone(), at(2))), [1, 2, 3, 4, 5, 6]);
    assert_eq!(ids(up_to(lines, at(0))), [1, 2]);
}

#[test]
fn a_long_stretch_is_gone_over_twelve_of_their_lines_at_a_time() {
    let lines: Vec<Spoken> = (0..30)
        .map(|index| line(index + 1, index % 2 == 0, index as i64))
        .collect();
    // Their 13th line is the 25th line: the first pass ends before it.
    assert_eq!(pass_end(&lines), 24);
    assert_eq!(pass_end(&lines[..10]), 10);
}

#[test]
fn lines_gone_over_are_shown_as_context_only_and_a_pass_starts_two_before() {
    let mut lines: Vec<Spoken> = (0..6)
        .map(|index| line(index + 1, index % 2 == 0, index as i64))
        .collect();
    for line in &mut lines[..4] {
        line.gone = true;
    }
    // Two lines gone over before the first not gone over, then the rest.
    assert_eq!(this_pass(&lines), 2..6);
    let at = vec![None; lines.len()];
    let shown = shown(&lines, &at);
    assert!(shown[0].before && !shown[4].before);
    // None gone over: from the first.
    for line in &mut lines {
        line.gone = false;
    }
    assert_eq!(this_pass(&lines), 0..6);
}

/// A line said a moment before the last one gone over but written after it
/// was read is looked for, and what was read is not gone over again.
#[test]
fn a_stretch_looks_back_for_lines_written_late() {
    let read = Read::default().with(&[(at(10), 10), (at(11), 11)]);
    assert!(read.has(10) && !read.has(9));
    // A stretch from minute 12 looks from two minutes before the last read.
    assert_eq!(Read::looks_from(Some(&read), at(12)), at(9));
    // With none known of, a little before it: their message is kept a
    // moment before it is noted as said.
    assert_eq!(Read::looks_from(None, at(12)), at(10));
    // A stretch from before that looks from where it begins.
    assert_eq!(Read::looks_from(Some(&read), at(5)), at(5));
    // Only lines within the slack of the last are kept.
    let later = read.with(&[(at(14), 14)]);
    assert!(!later.has(10) && !later.has(11) && later.has(14));
    assert_eq!(later.to(), Some(at(14)));
}

/// What came after the stretch's last line not gone over is the next
/// stretch's; a line gone over among them does not end it.
#[test]
fn a_line_gone_over_does_not_end_a_stretch() {
    let mut lines = vec![line(1, true, 0), line(2, true, 5), line(3, false, 5)];
    lines[1].gone = true;
    assert_eq!(up_to(lines, at(0)).len(), 3);
}

/// One too short to be one of a stretch ('对') begins none: it is of the
/// stretch it is said in, and what came after it is the next.
#[test]
fn a_word_too_short_to_wait_begins_no_stretch() {
    let mut lines = vec![
        line(1, true, 0),
        line(2, false, 0),
        line(3, true, 1),
        line(4, false, 1),
        line(5, true, 2),
    ];
    lines[2].text = "对".into();
    let ids: Vec<i32> = up_to(lines, at(0)).iter().map(|line| line.id).collect();
    assert_eq!(ids, [1, 2, 3, 4]);
}

fn long(id: i32) -> Spoken {
    Spoken {
        text: (0..400).map(|n| format!("第{n}件小事说给你听。")).collect(),
        ..line(id, true, 0)
    }
}

/// A long line is shown part by part, each accounted for on its own; of one
/// a pass ended within, the parts gone over are only what came before.
#[test]
fn a_long_line_is_gone_over_part_by_part() {
    let lines = vec![long(1), line(2, false, 0)];
    let none = vec![None; 2];
    let ids = messages_of(&lines);
    let (parts, of) = in_parts(&lines, &none, &ids);
    let count = raw::line_parts(&lines[0].text).len();
    assert!(count > 2);
    assert_eq!(parts.len(), count + 1);
    assert_eq!(of.iter().filter(|index| **index == 0).count(), count);
    let said: String = parts[..count].iter().map(|part| part.text).collect();
    assert_eq!(said, lines[0].text);

    let spoken = by_part(lines.clone(), Some((1, 2)));
    assert_eq!(spoken.len(), count + 1);
    let gone: Vec<bool> = spoken.iter().map(|line| line.gone).collect();
    assert!(gone[0] && gone[1] && !gone[2] && !gone[count]);
    // Each a line of its own, placed as the message it is, and once.
    let given = messages(&spoken, &spoken, None);
    assert_eq!(given.len(), 2);
    assert_eq!(given[0].1, lines[0].text);
    assert_eq!(messages(&spoken, &spoken, Some(1)).len(), 1);

    // One only a part of which they took back is not let go; all of it, it is.
    let spoken = by_part(lines, None);
    let none = vec![None; spoken.len()];
    let ids = messages_of(&spoken);
    let (parts, of) = in_parts(&spoken, &none, &ids);
    assert!(whole(&spoken, &parts, &of, &[1]).is_empty());
    let all: Vec<usize> = (0..count).collect();
    assert_eq!(whole(&spoken, &parts, &of, &all), [1]);
}

/// However long, their words are gone over.
#[test]
fn a_long_line_is_held_as_any() {
    let text = "我".repeat(2_001);
    assert_eq!(words_of(&text), Some(text.clone()));
    assert_eq!(words_of("对"), None);
}

/// Of the lines a fact is from, it is told of those that say what it quotes.
#[test]
fn a_few_words_quoted_are_of_a_line_that_says_only_them() {
    let said = |id: i32, text: &str| Spoken {
        text: text.into(),
        ..line(id, true, i64::from(id))
    };
    let lines = [
        said(1, "我周五考试，数学那门"),
        said(2, "不是"),
        said(3, "那件事不是这样的"),
        said(4, "不是。"),
    ];
    let lines: Vec<&Spoken> = lines.iter().collect();
    assert_eq!(saying("我周五考试，数学 / 不是", &lines), [1, 2, 4]);
    assert_eq!(saying("不是", &lines), [2, 4]);
    // Of no line as all it says: of the one it is in.
    assert_eq!(saying("我周五", &lines), [1]);
    assert!(saying("下周五", &lines).is_empty());
    // Each part on its own, one of them only a part of its line.
    let lines = [
        said(1, "我家养了只小狗"),
        said(2, "它今年两岁了"),
        said(3, "嗯嗯"),
    ];
    let lines: Vec<&Spoken> = lines.iter().collect();
    assert_eq!(saying("我家养了只小狗 / 两岁了", &lines), [1, 2]);
}

/// What she is told quotes `evidence` from the lines `from`.
fn quoting(evidence: &str, from: &[usize]) -> String {
    serde_json::json!({
        "lasting": [{
            "fact": "他们说的一件事",
            "evidence": evidence,
            "from": from,
            "concepts": [],
            "whenFor": [],
            "topic": {"on": "", "name": ""},
            "happened": {"from": "", "to": "", "about": false},
            "state": "",
            "open": null,
            "replaces": [],
        }],
        "tookBack": [], "letGo": [], "moment": [], "known": [],
        "said": [], "own": [], "putOnto": [],
    })
    .to_string()
}

/// How many facts quoting `evidence` across the parts `from` are kept, as
/// a pass shows `lines` to the core.
fn kept_quoting(lines: &[Spoken], evidence: &str, from: &[usize]) -> usize {
    let at: Vec<Option<String>> = lines.iter().map(|line| Some(said_at(line.at))).collect();
    let ids = messages_of(lines);
    let (parts, _) = in_parts(lines, &at, &ids);
    raw::parse_raw_settle(&quoting(evidence, from), &parts, &[], &[], 0, None)
        .unwrap()
        .settled
        .lasting
        .len()
}

/// A quote runs on from one part of a long message into the next; two
/// messages said in the same minute (the time she is shown) are two, and
/// a quote stitched from the end of one and the start of the other is not
/// taken.
#[test]
fn a_quote_runs_on_within_one_message_never_across_two_said_in_one_minute() {
    let spoken = by_part(vec![long(1)], None);
    assert!(spoken.len() > 1 && spoken.iter().all(|part| part.id == 1));
    let end: String = {
        let first: Vec<char> = spoken[0].text.chars().collect();
        first[first.len() - 6..].iter().collect()
    };
    let start: String = spoken[1].text.chars().take(6).collect();
    assert_eq!(kept_quoting(&spoken, &format!("{end}{start}"), &[0, 1]), 1);

    let said = |id: i32, text: &str| Spoken {
        text: text.into(),
        ..line(id, true, 3)
    };
    let two = [said(1, "我周六要去医院"), said(2, "复查膝盖的事就算了")];
    assert_eq!(said_at(two[0].at), said_at(two[1].at));
    assert_eq!(kept_quoting(&two, "去医院复查膝盖", &[0, 1]), 0);
    // Each said in its own words is kept.
    assert_eq!(kept_quoting(&two, "我周六要去医院", &[0]), 1);
}
