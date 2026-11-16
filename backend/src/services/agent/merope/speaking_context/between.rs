//! The sections of her speaking prompt about what is between them and a
//! chat turn's own, after those of `sections` in the prompt's order.

use super::*;

use super::super::memory_nodes::context::NodeTurn;

/// What is between them, each heard only where it grew: their bits, the
/// group's days, how she comes across, what they are to her, what still
/// stings, what she meant to come back to, and her views their words touch.
pub(super) async fn between_them(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    turn: Turn<'_>,
    present: &crate::services::agent::memory::unified::Audience,
    opening: bool,
    nodes: Option<&NodeTurn>,
    sections: &mut Vec<String>,
) {
    let group = present.is_group();
    let words = match turn {
        Turn::Chat(words) | Turn::Event(words) => Some(words),
        Turn::Plain => None,
    };
    // What only she and this person share, or she and this group: each
    // heard only where it grew. When her nodes are read, theirs are on
    // 「关于你」, and so is the rest of what is between them below.
    let shared = match nodes {
        Some(nodes) => nodes.bits_shown(db, BITS_LIMIT as usize, words).await,
        None => {
            let shared = match present.group_id() {
                Some(venue) => bits::in_group(db, venue, BITS_LIMIT, words).await,
                None => bits::between(db, user_id, BITS_LIMIT).await,
            };
            format_bits_section(&shared, group)
        }
    };
    if let Some(block) = shared {
        sections.push(block);
    }
    // What the last days in this group were like: the group's own.
    if let Some(venue) = present.group_id() {
        let days = match nodes {
            Some(nodes) => nodes.group_days(db, GROUP_DAYS as usize, words).await,
            None => bits::days_in(db, venue, GROUP_DAYS, words).await,
        };
        if let Some(block) = format_group_days_section(&days) {
            sections.push(block);
        }
    }
    // What the talk brings back of her other groups' own bits and days.
    if let (Some(venue), Some(words)) = (present.group_id(), words)
        && let Some(block) = other_groups::theirs_section(db, venue, words).await
    {
        sections.push(block);
    }
    // How she comes across: with them, or in this group, each heard only
    // where it was found.
    if let Some(nodes) = nodes {
        sections.extend(nodes.lands_section(super::super::clock::now()));
        sections.extend(nodes.us_section(super::super::clock::now()));
        sections.extend(nodes.about_head_section());
    }
    let lands = match (nodes, present.group_id()) {
        (Some(_), _) => None,
        (None, Some(venue)) => bits::lands_in(db, venue).await,
        (None, None) => bits::lands_with(db, user_id).await,
    };
    if let Some((lands, since)) = lands
        && let Some(block) = myriad_merope::speaking::format_lands_section(
            &lands,
            since.with_timezone(&chrono::Utc),
            super::super::clock::now(),
            group,
        )
    {
        sections.push(block);
    }
    // What they are to her: private, never in a group.
    if !group && nodes.is_none() {
        if let Some(us) = bits::us(db, user_id).await {
            if let Some(block) = format_us_section(
                &us.now,
                us.since.with_timezone(&chrono::Utc),
                us.before.as_deref(),
                us.first
                    .as_ref()
                    .map(|(first, at)| (first.as_str(), at.with_timezone(&chrono::Utc))),
                super::super::clock::now(),
            ) {
                sections.push(block);
            }
        }
    }
    // What still stings with them: in private all of it; in a group,
    // what they did there, and what they did in private only by how
    // much it weighs, never what it was.
    let now = super::super::clock::now();
    match present.group_id() {
        // From her nodes: on 「关于你」, and theirs in each group.
        None if nodes.is_some() => {
            sections.extend(nodes.and_then(|nodes| sore::section(&nodes.sores, now)));
        }
        None => {
            if let Some(block) = sore::section(&sore::open_all(db, user_id).await, now) {
                sections.push(block);
            }
        }
        Some(venue) => {
            // From her nodes: on their own node in this group.
            let here = match nodes {
                Some(nodes) => nodes.sores_here(now),
                None => sore::section(&sore::open_in_group(db, venue, Some(user_id)).await, now),
            };
            if let Some(block) = here {
                sections.push(block);
            }
            if let Some(block) = sore::carried_section(&sore::open_all(db, user_id).await, now) {
                sections.push(block);
            }
        }
    }
    // What she meant to come back to: with them in private, in mind as they
    // start talking again; in a group, the group's own, once one is due.
    // Either, when their words touch it. Never one place's in the other.
    {
        let now = super::super::clock::now();
        let open = match present.group_id() {
            // From her nodes, those that are her intents come as intents,
            // below.
            None if nodes.is_some() => {
                super::super::memory_nodes::home_intents::not_intents(
                    db,
                    user_id,
                    threads::open(db, user_id).await,
                )
                .await
            }
            None => threads::open(db, user_id).await,
            Some(venue) => threads::open_in_group(db, venue).await,
        };
        let touched = words.is_some_and(|words| {
            open.iter().any(|thread| {
                myriad_merope::remembering::overlap(
                    words,
                    &format!("{} {}", thread.about, thread.then),
                ) >= 2
                    || words.contains(thread.about.as_str())
            })
        });
        let due = open.iter().any(|thread| thread.is_due(now));
        let block = if group {
            (due || touched)
                .then(|| {
                    myriad_merope::threads::group_section(&open, now, &super::super::clock::zone())
                })
                .flatten()
        } else {
            (opening || touched)
                .then(|| threads::section(&open, now, &super::super::clock::zone()))
                .flatten()
        };
        sections.extend(block);
    }
    sections.extend(nodes.and_then(NodeTurn::intents_section));
    // What she thinks of what their words touch: hers, the same whoever asks.
    if let Some(words) = words {
        let views = views::touched(db, words, VIEWS_LIMIT).await;
        if let Some(block) = format_views_section(&views) {
            sections.push(block);
        }
    }
}

/// A chat turn's own: what they named that she knows little about, and what
/// was on her mind.
pub(super) async fn chat_extras(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    turn: Turn<'_>,
    present: &crate::services::agent::memory::unified::Audience,
    sections: &mut Vec<String>,
) {
    let group = present.is_group();
    // One mouth: what was on her mind belongs to the conversation she is
    // now answering in. What she said on her own is in its history
    // (see `with_said_unprompted`).
    if let Turn::Chat(words) = turn {
        // Something they just named that she knows only a little about.
        // Whether she wants to know more is hers to judge.
        let gap =
            crate::services::agent::memory::unified::curiosity_gap(db, user_id, present, words)
                .await;
        if let Some(block) = gap
            .ok()
            .flatten()
            .and_then(|(gap, known)| format_curious_section(&gap, known))
        {
            sections.push(block);
        }
    }
    let on_mind = (!group)
        .then(|| crate::services::agent::consciousness::last_attention(user_id))
        .flatten();
    if let Some(segment) = on_mind {
        if let Some(block) = format_on_your_mind_section(&segment.inner) {
            sections.push(block);
        }
    }
}
