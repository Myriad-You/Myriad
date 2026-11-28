//! What is asked again about the lines the first answer accounted for
//! nowhere, put together with it.

use std::collections::BTreeSet;
use std::time::Duration;

use myriad_merope::chat_remember::{Asked, Examples, Lasting};
use myriad_merope::settle_raw::{self as raw, Line, RawDropped, RawSettled, Who};

/// Counts of items dropped across the passes of one stretch, by reason
/// (counts only, never user text).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::services::agent::merope) struct DropCounts {
    pub parse: RawDropped,
    pub doubts_taken_back: usize,
    pub again_taken_back: usize,
    pub already_dropped: usize,
    pub not_to_keep_dropped: usize,
    pub kept_lasting: usize,
}

impl DropCounts {
    pub(super) fn any_drop(&self) -> bool {
        self.parse.any()
            || self.doubts_taken_back > 0
            || self.again_taken_back > 0
            || self.already_dropped > 0
            || self.not_to_keep_dropped > 0
    }

    pub(super) fn finish(
        &mut self,
        user_id: i32,
        before_already: usize,
        updates: &super::super::ChatMemoryUpdates,
        asked_again: usize,
        left: usize,
    ) {
        let after = kept_facts(updates);
        self.already_dropped = before_already.saturating_sub(after);
        self.kept_lasting = after;
        log_drops(user_id, self, asked_again, left);
    }
}

pub(super) fn kept_facts(updates: &super::super::ChatMemoryUpdates) -> usize {
    updates
        .updates
        .iter()
        .filter(|one| one.fact.is_some())
        .count()
}

pub(super) fn log_drops(user_id: i32, dropped: &DropCounts, asked_again: usize, left: usize) {
    if dropped.parse.raw_lasting == 0 && !dropped.any_drop() {
        return;
    }
    tracing::info!(
        user_id,
        raw_lasting = dropped.parse.raw_lasting,
        empty_or_too_long = dropped.parse.empty_or_too_long,
        bad_evidence = dropped.parse.bad_evidence,
        changed_language = dropped.parse.changed_language,
        outside_only = dropped.parse.outside_only,
        let_go = dropped.parse.let_go,
        over_limit = dropped.parse.over_limit,
        said_dropped = dropped.parse.said_dropped,
        put_onto_dropped = dropped.parse.put_onto_dropped,
        doubts_taken_back = dropped.doubts_taken_back,
        again_taken_back = dropped.again_taken_back,
        already_dropped = dropped.already_dropped,
        not_to_keep_dropped = dropped.not_to_keep_dropped,
        kept_lasting = dropped.kept_lasting,
        asked_again,
        left,
        "[chat-remember] settle pass drop summary"
    );
}

/// The `asked` list shown when asking again: `asked` with anything `newly`
/// replaces removed and any newly recognized `notToKeep` added.
pub(super) fn asked_for_again(asked: &[String], newly: &[Asked]) -> Vec<String> {
    let mut next: Vec<String> = asked
        .iter()
        .filter(|old| !newly.iter().any(|one| one.replaces.contains(old)))
        .cloned()
        .collect();
    for one in newly {
        if !next.contains(&one.fact) {
            next.push(one.fact.clone());
        }
    }
    next
}

/// Rebuilds `(asked, system)` for the second pass only when pass-1
/// `notToKeep` changed the active `asked` list, keeping the prompt prefix
/// identical otherwise.
pub(super) fn prompt_for_again(
    known: &[String],
    asked: &[String],
    newly: &[Asked],
    examples: Examples,
) -> Option<(Vec<String>, String)> {
    let next = asked_for_again(asked, newly);
    (next != asked).then(|| {
        let system = raw::raw_settle_system_prompt(known, &next, examples);
        (next, system)
    })
}

pub(super) fn prompt_parts<'a>(
    again: &'a Option<(Vec<String>, String)>,
    asked: &'a [String],
    system: &'a str,
) -> (&'a [String], &'a str) {
    again
        .as_ref()
        .map_or((asked, system), |(next, sys)| (next.as_slice(), sys.as_str()))
}

fn retain_not_dropped<T>(items: &mut Vec<T>, dropped: &BTreeSet<usize>) {
    let mut index = 0..;
    items.retain(|_| !dropped.contains(&index.next().unwrap_or(0)));
}

/// Filters items returned by the second pass (`again`) only against pass-1's
/// newly recognized `notToKeep` requests (`newly`), without re-reading stored
/// requests. If judging fails or runs out of time, drops the whole
/// second-pass result so pass-1 results stand unchanged.
pub(super) async fn without_newly_asked(
    user_id: i32,
    newly: &[Asked],
    mut again: RawSettled,
    until: Option<std::time::Instant>,
) -> (Option<RawSettled>, usize) {
    if newly.is_empty() {
        return (Some(again), 0);
    }
    enum At {
        Lasting(usize),
        Said(usize),
        PutOnto(usize),
    }
    let mut at = Vec::new();
    let mut items = Vec::new();
    for (index, fact) in again.settled.lasting.iter().enumerate() {
        at.push(At::Lasting(index));
        items.push(fact.fact.clone());
    }
    for (index, said) in again.said.iter().enumerate() {
        at.push(At::Said(index));
        items.push(said.said.clone());
    }
    for (index, thing) in again.put_onto.iter().enumerate() {
        at.push(At::PutOnto(index));
        items.push(thing.clone());
    }
    if items.is_empty() {
        return (Some(again), 0);
    }
    let drop = match crate::services::agent::merope::asked::left_out_of_asked(
        user_id, newly, &items, until,
    )
    .await
    {
        Ok(drop) => drop,
        Err(failure) => {
            tracing::warn!(
                user_id,
                %failure,
                items = items.len(),
                "[chat-remember] second-pass items could not be judged against pass-1 notToKeep; dropping second-pass result"
            );
            return (None, 0);
        }
    };
    let (mut lasting_drop, mut said_drop, mut put_onto_drop) =
        (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for index in drop {
        match at[index] {
            At::Lasting(i) => {
                lasting_drop.insert(i);
            }
            At::Said(i) => {
                said_drop.insert(i);
            }
            At::PutOnto(i) => {
                put_onto_drop.insert(i);
            }
        }
    }
    // Remap `again.doubted` indexes if any lasting fact was dropped.
    let mut remap = Vec::with_capacity(again.settled.lasting.len());
    let (mut next, mut index) = (0usize, 0usize);
    again.settled.lasting.retain(|_| {
        let keep = !lasting_drop.contains(&index);
        remap.push(keep.then(|| {
            let slot = next;
            next += 1;
            slot
        }));
        index += 1;
        keep
    });
    again.doubted.retain_mut(|doubt| {
        let Some(slot) = remap.get(doubt.fact).copied().flatten() else {
            return false;
        };
        doubt.fact = slot;
        true
    });
    retain_not_dropped(&mut again.said, &said_drop);
    retain_not_dropped(&mut again.put_onto, &put_onto_drop);
    (Some(again), lasting_drop.len())
}

fn check_within(until: Option<std::time::Instant>) -> Duration {
    let timeout = super::super::super::memory_nodes::take_back_check::TIMEOUT;
    until.map_or(timeout, |until| {
        until
            .saturating_duration_since(std::time::Instant::now())
            .min(timeout)
    })
}

/// Applies first-pass `doubts` feedback and, when a second-pass result exists,
/// judges it against pass-1 `notToKeep`, checks its own `doubts`, and merges
/// it with the first pass while recomputing remaining time before each check.
pub(super) async fn with_second_pass(
    user_id: i32,
    mut got: RawSettled,
    again: Option<RawSettled>,
    told: &[Option<bool>],
    lines: &[Line<'_>],
    check_until: Option<std::time::Instant>,
) -> (RawSettled, DropCounts) {
    let mut dropped = DropCounts::default();
    let else_said = told_something_else(&got, told);
    let before_doubts = got.settled.lasting.len();
    got.doubts_told(told);
    dropped.doubts_taken_back += before_doubts - got.settled.lasting.len();
    if let Some(again) = again {
        let (again, not_to_keep_dropped) =
            without_newly_asked(user_id, &got.asked, again, check_until).await;
        dropped.not_to_keep_dropped += not_to_keep_dropped;
        if let Some(mut again) = again {
            let told = doubts(user_id, &again, lines, check_within(check_until)).await;
            let before_again = again.settled.lasting.len();
            again.doubts_told(&told);
            dropped.doubts_taken_back += before_again - again.settled.lasting.len();
            let (merged, again_taken_back) = put_together(
                user_id,
                got,
                again,
                &else_said,
                lines,
                check_within(check_until),
            )
            .await;
            got = merged;
            dropped.again_taken_back = again_taken_back;
        }
    }
    dropped.parse = got.dropped.clone();
    (got, dropped)
}

/// `again`, asked about the lines `first` left, added to it: what the first
/// answer replaced or took back stays as it has it, and what it has from
/// lines `again` takes back is not kept (see [`taken_back`], in the time
/// `within`), but for what was told already to be something else
/// (`else_said`, [`told_something_else`]). Also returns how many lasting facts
/// were removed by `again` taking back their lines.
pub(super) async fn put_together(
    user_id: i32,
    mut first: RawSettled,
    again: RawSettled,
    else_said: &[String],
    lines: &[Line<'_>],
    within: Duration,
) -> (RawSettled, usize) {
    first.dropped.merge(&again.dropped);
    let said = first
        .left
        .iter()
        .filter_map(|index| lines.get(*index))
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");
    let kept = &first.settled.lasting;
    let gone = taken_back(
        user_id,
        kept,
        else_said,
        lines,
        &again.let_go,
        &said,
        within,
    )
    .await;
    let again_taken_back = gone.len();
    let mut at = 0;
    first.settled.lasting.retain(|_| {
        at += 1;
        !gone.contains(&(at - 1))
    });
    // A line something of it is kept from is not let go.
    let kept = |index: &usize| (first.settled.lasting.iter()).any(|fact| fact.from.contains(index));
    let let_go: Vec<usize> = again
        .let_go
        .into_iter()
        .filter(|index| !first.let_go.contains(index) && !kept(index))
        .collect();
    first.let_go.extend(let_go);
    // Replaced or taken back once, as the first answer has it.
    let replaced: Vec<String> = first
        .settled
        .lasting
        .iter()
        .flat_map(|fact| fact.replaces.iter().cloned())
        .collect();
    let taken: Vec<String> = first
        .settled
        .took_back
        .iter()
        .map(|one| one.entry.clone())
        .collect();
    for mut fact in again.settled.lasting {
        fact.replaces
            .retain(|old| !replaced.contains(old) && !taken.contains(old));
        first.settled.lasting.push(fact);
    }
    first.settled.took_back.extend(
        again
            .settled
            .took_back
            .into_iter()
            .filter(|one| !replaced.contains(&one.entry) && !taken.contains(&one.entry)),
    );
    // What they ask of it, each once.
    for mut one in again.asked {
        let already = |held: &Asked| held.fact == one.fact;
        if first.asked.iter().any(already) {
            continue;
        }
        one.replaces
            .retain(|old| !first.asked.iter().any(|held| held.replaces.contains(old)));
        first
            .settled
            .took_back
            .retain(|taken| !one.replaces.contains(&taken.entry));
        first.asked.push(one);
    }
    first.left = again.left;
    (first, again_taken_back)
}

/// What `got` keeps that it doubted and was `told` to be something else they
/// said.
pub(super) fn told_something_else(got: &RawSettled, told: &[Option<bool>]) -> Vec<String> {
    (got.doubted.iter().zip(told))
        .filter(|(_, told)| **told == Some(false))
        .filter_map(|(doubt, _)| got.settled.lasting.get(doubt.fact))
        .map(|fact| fact.fact.clone())
        .collect()
}

/// Which of the facts `kept`, taken from the lines `let_go` (by their place)
/// alone, the second look says they took back ([`told`]) in `said`, the
/// lines asked about again. Unanswered, or told already to be something else
/// (`else_said`), they are kept.
async fn taken_back(
    user_id: i32,
    kept: &[Lasting],
    else_said: &[String],
    lines: &[Line<'_>],
    let_go: &[usize],
    said: &str,
    within: Duration,
) -> Vec<usize> {
    let places: Vec<usize> = (0..kept.len())
        .filter(|place| {
            let fact = &kept[*place];
            !else_said.contains(&fact.fact)
                && !fact.from.is_empty()
                && fact.from.iter().all(|index| let_go.contains(index))
        })
        .collect();
    let after = vec![said.to_string(); places.len()];
    let told = told(user_id, kept, lines, &places, &after, within).await;
    places
        .into_iter()
        .zip(told)
        .filter(|(_, told)| *told == Some(true))
        .map(|(place, _)| place)
        .collect()
}

/// Of each fact `got` doubts (`RawSettled::doubted`), whether they took it
/// back in what they said from its lines on, in the time `within`: None
/// when no answer came.
pub(super) async fn doubts(
    user_id: i32,
    got: &RawSettled,
    lines: &[Line<'_>],
    within: Duration,
) -> Vec<Option<bool>> {
    let kept = &got.settled.lasting;
    let places: Vec<usize> = got.doubted.iter().map(|doubt| doubt.fact).collect();
    let after: Vec<String> = places
        .iter()
        .map(|place| {
            let from = &kept[*place].from;
            let first = from.iter().min().copied().unwrap_or(0);
            lines
                .iter()
                .enumerate()
                .skip(first)
                .filter(|(index, line)| {
                    line.who == Who::Them && !line.before && !line.in_game && !from.contains(index)
                })
                .map(|(_, line)| line.text)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    told(user_id, kept, lines, &places, &after, within).await
}

fn shares(one: &Lasting, other: &Lasting) -> bool {
    one.from.iter().any(|index| other.from.contains(index))
}

/// Whether another of the facts `kept` is from a line of the one at `place`.
fn shared(kept: &[Lasting], place: usize) -> bool {
    (0..kept.len()).any(|other| other != place && shares(&kept[place], &kept[other]))
}

/// Whether they took back each of the facts `places`, by the second look
/// (`memory_nodes::take_back_check`): shown its lines whole ("what I said of
/// work, forget it") and what they said `after`; and where something else is
/// from its lines and none of it is, shown what it quotes of them ("forget
/// all that"). None when no answer came.
async fn told(
    user_id: i32,
    kept: &[Lasting],
    lines: &[Line<'_>],
    places: &[usize],
    after: &[String],
    within: Duration,
) -> Vec<Option<bool>> {
    if within.is_zero() {
        return vec![None; places.len()];
    }
    let until = std::time::Instant::now() + within;
    let mut whole = Vec::new();
    for (place, after) in places.iter().zip(after) {
        let theirs: Vec<&str> = (kept[*place].from.iter())
            .filter_map(|index| lines.get(*index))
            .map(|line| line.text)
            .collect();
        whole.push(says(user_id, &theirs.join("\n"), after, &kept[*place], until).await);
    }
    let mut told = whole.clone();
    for (at, place) in places.iter().enumerate() {
        let taken = places
            .iter()
            .zip(&whole)
            .any(|(other, whole)| *whole == Some(true) && shares(&kept[*place], &kept[*other]));
        if taken || !shared(kept, *place) {
            continue;
        }
        let evidence = &kept[*place].evidence;
        match says(user_id, evidence, &after[at], &kept[*place], until).await {
            Some(false) => {}
            said => told[at] = said,
        }
    }
    told
}

/// Whether `theirs` and then `said` takes back `fact`.
async fn says(
    user_id: i32,
    theirs: &str,
    said: &str,
    fact: &Lasting,
    until: std::time::Instant,
) -> Option<bool> {
    let left = until.saturating_duration_since(std::time::Instant::now());
    if left.is_zero() {
        return None;
    }
    super::super::super::memory_nodes::take_back_check::takes_back(
        user_id,
        format!("{theirs}\n{said}").trim(),
        &fact.fact,
        left,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::channel_group::g0;

    fn sample_lasting(fact: &str, from: Vec<usize>) -> Lasting {
        Lasting {
            fact: fact.into(),
            evidence: fact.into(),
            from,
            concepts: serde_json::json!([]),
            when_for: Vec::new(),
            topic: myriad_merope::topics::Under::AboutThem,
            standing: Default::default(),
            replaces: Vec::new(),
        }
    }

    #[tokio::test]
    async fn without_newly_asked_drops_second_pass_when_out_of_time_with_no_model_call() {
        let newly = vec![Asked {
            fact: "不要记自己的体重".into(),
            evidence: "体重的事别记".into(),
            said: Some(0),
            replaces: Vec::new(),
        }];
        let again = RawSettled {
            put_onto: vec!["每日称重记录".into()],
            ..Default::default()
        };
        g0::enter(g0::World::new(0.5));
        let (filtered, dropped) =
            without_newly_asked(7, &newly, again, Some(std::time::Instant::now())).await;
        let log = g0::leave().unwrap().log;
        assert!(filtered.is_none());
        assert_eq!(dropped, 0);
        assert!(
            log.iter().all(|entry| entry["kind"] != "model"),
            "unexpected model calls when deadline has < LEAST remaining: {log:?}"
        );
    }

    #[tokio::test]
    async fn with_second_pass_past_deadline_skips_doubts_and_put_together_model_calls() {
        let lines = [
            Line {
                who: Who::Them,
                text: "以前住在杭州",
                in_game: false,
                before: false,
                at: None,
                message: None,
            },
            Line {
                who: Who::Them,
                text: "现在喜欢喝乌龙茶",
                in_game: false,
                before: false,
                at: None,
                message: None,
            },
        ];
        let mut got = RawSettled::default();
        got.settled.lasting = vec![sample_lasting("以前住在杭州", vec![0])];
        got.left = vec![1];
        let mut again = RawSettled::default();
        again.settled.lasting = vec![sample_lasting("现在喜欢喝乌龙茶", vec![1])];
        again.doubted = vec![raw::Doubted {
            fact: 0,
            replaces: Vec::new(),
        }];
        again.let_go = vec![0];

        g0::enter(g0::World::new(0.5));
        let (merged, _) = with_second_pass(
            7,
            got,
            Some(again),
            &[],
            &lines,
            Some(std::time::Instant::now()),
        )
        .await;
        let log = g0::leave().unwrap().log;
        assert!(
            log.iter().all(|entry| entry["kind"] != "model"),
            "unexpected model calls when check deadline has passed: {log:?}"
        );
        let facts: Vec<&str> = merged
            .settled
            .lasting
            .iter()
            .map(|one| one.fact.as_str())
            .collect();
        assert_eq!(facts, ["以前住在杭州", "现在喜欢喝乌龙茶"]);
    }
}
