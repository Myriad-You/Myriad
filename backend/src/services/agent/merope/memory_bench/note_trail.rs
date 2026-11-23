//! Where each short note went, the settle way, for one question
//! (`MEROPE_MEMORY_BENCH_NOTE_TRAIL=<jsonl>`, bench only): which of their
//! lines that answer the question were noted at all, and for each note what
//! going over made of it (kept for good, left to the moment, already
//! known, passed over and kept as it was, or nothing: the call failed), whether
//! what it was kept as was written, and whether that is still there when
//! they ask. Off, nothing is kept and nothing is asked of the database.

use super::*;

/// One question's trail, kept as it is written.
pub(super) struct Trail {
    on: bool,
    rows: Vec<Value>,
    /// The notes of the session now being gone over: where each went and
    /// what it was kept as.
    session: Vec<Value>,
    number: usize,
}

impl Trail {
    pub(super) fn new() -> Self {
        Self {
            on: std::env::var_os("MEROPE_MEMORY_BENCH_NOTE_TRAIL").is_some(),
            rows: Vec::new(),
            session: Vec::new(),
            number: 0,
        }
    }

    /// Kept for nothing (what goes over sessions outside a question).
    pub(super) fn off() -> Self {
        Self {
            on: false,
            ..Self::new()
        }
    }

    /// One of their lines that answers the question: how many notes it
    /// left, or that it was too long to be noted at all.
    pub(super) fn answer_line(&mut self, text: &str, notes: usize, too_long: bool) {
        if self.on {
            self.rows.push(json!({
                "kind": "answerLine", "session": self.number, "line": text,
                "notes": notes, "tooLong": too_long,
            }));
        }
    }

    /// A session's notes, about to be gone over, each with whether its line
    /// answers the question.
    pub(super) fn notes(&mut self, notes: &[(String, bool)]) {
        if self.on {
            self.session = notes
                .iter()
                .map(|(note, answer)| {
                    json!({
                        "kind": "note", "session": self.number, "note": note,
                        "answerLine": answer, "went": "not gone over",
                    })
                })
                .collect();
        }
    }

    /// Where going over put note `index`, and what it was kept as.
    pub(super) fn went(&mut self, index: usize, went: &str, kept_as: Option<&str>) {
        if let Some(row) = self.session.get_mut(index) {
            row["went"] = json!(went);
            if let Some(fact) = kept_as {
                row["keptAs"] = json!(fact);
            }
        }
    }

    /// Note `index` still fresh when they ask, so there as a note.
    pub(super) fn fresh(&mut self, index: usize) {
        if let Some(row) = self.session.get_mut(index) {
            row["freshWhenAsked"] = json!(true);
        }
    }

    /// Every note of the session at once (going over failed or was not
    /// read).
    pub(super) fn all_went(&mut self, went: &str) {
        for index in 0..self.session.len() {
            self.went(index, went, None);
        }
    }

    /// The session gone over: whether what each note was kept as was
    /// written (it may have been taken as one she already had).
    pub(super) async fn close(
        &mut self,
        db: &sea_orm::DatabaseConnection,
        user_id: i32,
        present: &Audience,
    ) {
        if !self.on {
            return;
        }
        let held = held(db, user_id, present).await;
        for mut row in std::mem::take(&mut self.session) {
            if let Some(fact) = row["keptAs"].as_str() {
                row["written"] = json!(held.contains(fact));
            }
            self.rows.push(row);
        }
        self.number += 1;
    }

    /// All written: whether what each note was kept as is still there when
    /// they ask, then the trail added to the file, under `question`.
    pub(super) async fn finish(
        mut self,
        db: &sea_orm::DatabaseConnection,
        user_id: i32,
        question: &str,
    ) {
        let Some(path) = std::env::var_os("MEROPE_MEMORY_BENCH_NOTE_TRAIL") else {
            return;
        };
        if !self.on {
            return;
        }
        let held = held(db, user_id, &Audience::private(user_id)).await;
        let mut out = String::new();
        for row in &mut self.rows {
            if let Some(fact) = row["keptAs"].as_str() {
                row["stillThere"] = json!(held.contains(fact));
            }
            row["question"] = json!(question);
            out.push_str(&row.to_string());
            out.push('\n');
        }
        use std::io::Write;
        if let Err(error) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(out.as_bytes()))
        {
            eprintln!("note trail not kept: {error}");
        }
    }
}

/// Each answer going over a stretch (`MEROPE_MEMORY_BENCH_UNREAD_SETTLE=<jsonl>`,
/// bench only): how many notes it went over, why it stopped and how many
/// tokens it put out (thinking included) as its source said, how long it is
/// and how long the call took; one that could not be read kept as it came,
/// with why: not JSON, or JSON without `lasting` or `tookBack` as lists.
pub(super) fn keep_settle_call(
    raw: &str,
    read: bool,
    notes: usize,
    reported: Option<&crate::services::ai_cost_ledger::Reported>,
    took: std::time::Duration,
) {
    let Some(path) = std::env::var_os("MEROPE_MEMORY_BENCH_UNREAD_SETTLE") else {
        return;
    };
    let answer = serde_json::from_str::<Value>(myriad_merope::chat_remember::strip_json_fence(raw));
    let list = |answer: &Value, field: &str| answer.get(field).is_some_and(Value::is_array);
    let why = match &answer {
        _ if read => "read",
        Err(_) => "not json",
        Ok(answer) if !list(answer, "lasting") => "no lasting",
        Ok(answer) if !list(answer, "tookBack") => "no tookBack",
        Ok(_) => "other",
    };
    let lasting = answer
        .as_ref()
        .ok()
        .and_then(|answer| answer.get("lasting")?.as_array().map(Vec::len));
    let line = json!({
        "why": why,
        "notes": notes,
        "lasting": lasting,
        "stops": reported.map(|reported| reported.stops.clone()),
        "outputTokens": reported.map(|reported| reported.usage.output),
        "chars": raw.chars().count(),
        "tookMs": took.as_millis() as u64,
        "raw": (!read).then_some(raw),
    })
    .to_string()
        + "\n";
    use std::io::Write;
    if let Err(error) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
    {
        eprintln!("unread answer not kept: {error}");
    }
}

/// Each private session's pass gone over from what was said, on the same
/// file as [`keep_settle_call`]: how long it took, and, read, how many of
/// their lines were asked about again and how many still accounted for
/// nowhere (`None`: out of tries).
pub(super) fn keep_raw_settle_call(took: std::time::Duration, lines: Option<(usize, usize)>) {
    let Some(path) = std::env::var_os("MEROPE_MEMORY_BENCH_UNREAD_SETTLE") else {
        return;
    };
    let line = json!({
        "why": if lines.is_some() { "read" } else { "failed" },
        "fromLines": true,
        "askedAgain": lines.map(|(again, _)| again),
        "left": lines.map(|(_, left)| left),
        "tookMs": took.as_millis() as u64,
    })
    .to_string()
        + "\n";
    use std::io::Write;
    if let Err(error) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
    {
        eprintln!("settle call not kept: {error}");
    }
}

/// What she keeps of them for good here now, as going over writes it.
async fn held(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
) -> std::collections::HashSet<String> {
    crate::services::agent::memory::unified::active_in(
        db,
        user_id,
        present,
        &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
    )
    .await
    .unwrap_or_default()
    .into_iter()
    .filter(|row| row.source == "chat")
    .map(|row| super::super::ingest::compact_summary(&row.content))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each note is kept with where it went and what it was kept as; off,
    /// nothing is kept.
    #[test]
    fn each_note_is_kept_with_where_it_went() {
        let mut trail = Trail {
            on: true,
            rows: Vec::new(),
            session: Vec::new(),
            number: 0,
        };
        trail.notes(&[("turned 32".into(), true), ("likes tea".into(), false)]);
        trail.went(0, "lasting", Some("Turned 32 on February 12"));
        trail.went(1, "moment", None);
        assert_eq!(trail.session[0]["keptAs"], "Turned 32 on February 12");
        assert_eq!(trail.session[0]["answerLine"], true);
        assert_eq!(trail.session[1]["went"], "moment");
        trail.all_went("settle failed");
        assert_eq!(trail.session[1]["went"], "settle failed");
        let mut off = Trail::off();
        off.notes(&[("x".into(), false)]);
        off.answer_line("x", 0, false);
        assert!(off.session.is_empty() && off.rows.is_empty());
    }
}
