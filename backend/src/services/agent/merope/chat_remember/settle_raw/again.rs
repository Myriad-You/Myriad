//! What is asked again about the lines the first answer accounted for
//! nowhere, put together with it.

use std::time::Duration;

use myriad_merope::chat_remember::{Asked, Lasting};
use myriad_merope::settle_raw::{Line, RawSettled, Who};

/// `again`, asked about the lines `first` left, added to it: what the first
/// answer replaced or took back stays as it has it, and what it has from
/// lines `again` takes back is not kept (see [`taken_back`], in the time
/// `within`), but for what was told already to be something else
/// (`else_said`, [`told_something_else`]).
pub(super) async fn put_together(
    user_id: i32,
    mut first: RawSettled,
    again: RawSettled,
    else_said: &[String],
    lines: &[Line<'_>],
    within: Duration,
) -> RawSettled {
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
    first
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
    super::super::super::memory_nodes::take_back_check::takes_back(
        user_id,
        format!("{theirs}\n{said}").trim(),
        &fact.fact,
        until.saturating_duration_since(std::time::Instant::now()),
    )
    .await
}
