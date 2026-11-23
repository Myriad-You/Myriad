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
    assert_eq!(Read::looks_from(None, at(12)), at(12));
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
