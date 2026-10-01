//! The sections of her speaking prompt, in the order the prompt lays them
//! out. Each appends to `sections`; `speaking_prompt_from_db` calls them in turn.

use super::*;

/// Who they are to her: how her lines differ from theirs, what they told her
/// she got wrong, how long they have known each other, whether it reads like them.
pub(super) async fn who_they_are(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    turn: Turn<'_>,
    group: bool,
    sections: &mut Vec<String>,
) {
    if !group && let Some(differs) = how_she_differs_with(db, user_id).await {
        sections.push(differs);
    }
    if !group && let Some(told) = making_sense::told_by_section(db, user_id).await {
        sections.push(told);
    }
    // How long the two of them have known each other is between them.
    if !group && let Some((first, days)) = acquaintance(db, user_id).await {
        sections.push(myriad_merope::speaking::format_acquaintance_section(
            first,
            days,
            super::super::clock::now(),
        ));
    }
    // Whether it reads like them typing.
    if matches!(turn, Turn::Chat(_))
        && let Some(block) = likeness::section(db, user_id).await
    {
        sections.push(block);
    }
}

/// What she remembers of them, recalled against their words, and what she
/// scrolls back to find. Returns whether her own life is in mind this turn,
/// and whether they are only now starting to talk again.
pub(super) async fn recollection(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    turn: Turn<'_>,
    present: &crate::services::agent::memory::unified::Audience,
    myself: &self_state::SelfState,
    sections: &mut Vec<String>,
) -> (bool, bool) {
    let group = present.is_group();
    // Before answering what they said, she thinks what to try to remember.
    // And, as anyone does, keeps in mind what the moment asks for: her own
    // life when the talk is about her or they are only now starting to talk
    // again, not at every line. Speaking up on her own, it is her own
    // things she brings, so all of it is at hand.
    let (cues, her_life, opening) = match turn {
        Turn::Chat(words) => {
            let attention = remembering::cues(user_id, &present.venue(), words).await;
            let her_life = attention.her_life();
            (attention.cues, her_life, attention.opening)
        }
        _ => (None, true, true),
    };
    // Only a chat turn (it has the person's words) carries its train of
    // thought to the next turn; other readers see memory without moving it.
    let remembered = match turn {
        Turn::Chat(words) | Turn::Event(words) => remembering::recall_with(
            db,
            user_id,
            present,
            words,
            cues.as_ref(),
            REMEMBERED_PROMPT_LIMIT,
            remembering::THOROUGH,
            &if group {
                crate::services::agent::memory::unified::Priming::default()
            } else {
                priming::current(user_id)
            },
            myself.recall_breadth(),
        )
        .await
        .map(|(recalled, next)| {
            if matches!(turn, Turn::Chat(_)) && !group {
                priming::keep(user_id, next);
            }
            recalled
        }),
        Turn::Plain => store::recall_remembered(db, user_id, None, REMEMBERED_PROMPT_LIMIT)
            .await
            .map(|named| store::Recalled {
                named,
                brought_to_mind: Vec::new(),
            }),
    };
    if let Ok(recalled) = remembered {
        if let Some(block) = format_remembered_section(&recalled.named) {
            sections.push(block);
        }
        // What their words brought to mind, apart from what they named: the
        // stuff of callbacks and unexpected remarks, hers to use or not.
        if let Some(block) = format_brought_to_mind_section(&recalled.brought_to_mind) {
            sections.push(block);
        }
    }
    // Asked about what was said in detail, or for all of it, she scrolls
    // back through the chat.
    // Asked back to something before, she scrolls back even when she could
    // not think in time what to look for (their words are the query then),
    // and when their words plainly reach back though her cues did not say so.
    let reaching_back = |words: &str| {
        myriad_merope::remembering::reaches_back(words)
            || cues
                .as_ref()
                .is_some_and(|cues| cues.look_back || cues.thorough)
    };
    let query_of = |words: &str| {
        std::iter::once(words)
            .chain(
                cues.iter()
                    .flat_map(|cues| cues.cues.iter().map(String::as_str)),
            )
            .collect::<Vec<_>>()
            .join(" ")
    };
    let thorough = cues.as_ref().is_some_and(|cues| cues.thorough);
    if let (false, Turn::Chat(words)) = (group, turn)
        && reaching_back(words)
    {
        let query = query_of(words);
        let found = remembering::look_back(
            db,
            user_id,
            &query,
            words,
            if thorough {
                remembering::LOOK_BACK_THOROUGH
            } else {
                remembering::LOOK_BACK
            },
        )
        .await;
        if let Some(block) = myriad_merope::remembering::looked_back_section(&found) {
            sections.push(block);
        }
    }
    // In a group, the group's own days and talk: nothing private.
    if let (Some(venue), Turn::Chat(words)) = (present.group_id(), turn)
        && reaching_back(words)
    {
        let query = query_of(words);
        let found =
            chat_days::turn_back(db, chat_days::Place::In(venue), user_id, &query, words).await;
        if let Some(block) = myriad_merope::remembering::looked_back_in_group_section(&found) {
            sections.push(block);
        }
    }
    (her_life, opening)
}

/// Their own recent things: the chat diary, what they are doing on the site,
/// what the owner is playing. None of it in a group.
pub(super) async fn their_recent(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    group: bool,
    state: &crate::models::entities::agent_addressee_state::Model,
    sections: &mut Vec<String>,
) {
    let diary = if group {
        Ok(Vec::new())
    } else {
        list_diary_from_sources(
            db,
            user_id,
            RECENT_SPEAKING_DIARY_SOURCES,
            RECENT_LEDGER_LIMIT,
        )
        .await
    };
    if let Ok(notes) = diary {
        let contents: Vec<String> = notes
            .into_iter()
            .map(|note| ingest::compact_summary(&note.content))
            .filter(|content| !content.is_empty())
            .collect();
        if let Some(block) = format_recent_section(&contents) {
            sections.push(block);
        }
    }
    // What they are doing on the site is theirs: not for a group to hear.
    if !group && let Some(block) = format_activity_section(current_activity(&state)) {
        sections.push(block);
    }
    // What the site's owner is playing, to the owner alone.
    if !group {
        if let Some(block) = playing::now_for(user_id, super::super::clock::now())
            .and_then(|line| format_playing_section(&line))
        {
            sections.push(block);
        }
    }
}

/// How she is: mood and what her other talks left her with, unless her inner
/// state already says it; the emotion of the moment; the time and her day.
/// Returns her inner state, which goes last, nearest their words.
pub(super) async fn her_state(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    turn: Turn<'_>,
    present: &crate::services::agent::memory::unified::Audience,
    state: &crate::models::entities::agent_addressee_state::Model,
    myself: &self_state::SelfState,
    sections: &mut Vec<String>,
) -> Option<String> {
    // Her state after the last exchange already weighs how she has been and
    // how her day went; the raw facts would say it twice, and can say it
    // differently.
    let compiled = match turn {
        Turn::Chat(_) => inner::current(user_id, present),
        _ => None,
    };
    if compiled.is_none() {
        sections.push(format_mood_section(state.mood, state.arousal));
        if let Some(block) =
            myriad_merope::speaking::format_carried_section(&carried(db, user_id).await)
        {
            sections.push(block);
        }
    }
    // Her inner state goes last, nearest their words, so it is what she
    // answers from; without it, how the words landed stands here instead.
    // It was written before their latest words: when those words clearly
    // landed (praise or a scolding, or her appraisal of them since), how
    // they landed is newer than it and stands beside it.
    let inner_block = compiled
        .as_deref()
        .and_then(format_inner_moment_ago_section);
    let landed_since = match turn {
        Turn::Chat(words) => {
            let (praised, scolded) = myriad_merope::affect::detect_mood_cue(words);
            praised || scolded || inner::landed_since_reflection(user_id, present)
        }
        _ => false,
    };
    if inner_block.is_none() || landed_since {
        if let Some(block) = format_emotion_section(state.emotion, state.emotion_arousal) {
            sections.push(block);
        }
    }
    if !matches!(turn, Turn::Plain) {
        sections.push(speaking_prompts::format_now_section(
            super::super::clock::local_now(),
        ));
        // Woken, or still up talking: by her hours she would be asleep.
        if let Some(block) = super::super::timing::past_bedtime() {
            sections.push(block);
        }
    }
    if !matches!(turn, Turn::Plain) && compiled.is_none() {
        sections.push(self_state::format_day_section(&myself.facts));
    }
    inner_block
}

/// How hard she has been going at her own things, and what she looked up
/// that their words touch.
pub(super) async fn what_she_found(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    turn: Turn<'_>,
    present: &crate::services::agent::memory::unified::Audience,
    sections: &mut Vec<String>,
) {
    // How hard she has been going at her own things, when it says something.
    if !matches!(turn, Turn::Plain)
        && let Some(block) = pace::conversation_section(db).await
    {
        sections.push(block);
    }
    if let Turn::Chat(words) | Turn::Event(words) = turn {
        let found = crate::services::agent::memory::unified::recall(
            db,
            user_id,
            present,
            Some(words),
            &[crate::services::agent::memory::unified::MemoryKind::Knowledge],
            FOUND_OUT_LIMIT,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|note| note.content)
        .collect::<Vec<_>>();
        if let Some(block) = format_found_out_section(&found) {
            sections.push(block);
        }
    }
}

/// Her own life: her days, her story, what she wants, what keeps getting to
/// her, when the moment asks for it; and what she is doing, and did lately.
pub(super) async fn her_own(
    db: &sea_orm::DatabaseConnection,
    turn: Turn<'_>,
    her_life: bool,
    sections: &mut Vec<String>,
) {
    // Her own life: hers, heard wherever she is, in mind when the moment
    // asks for it.
    if her_life {
        if let Some(block) = format_own_days_section(&life::recent_days(db, OWN_DAYS_LIMIT).await) {
            sections.push(block);
        }
        if let Some(block) = format_self_story_section(&self_story::current(db).await) {
            sections.push(block);
        }
        if let Some(block) = wants::section(&wants::open(db).await, super::super::clock::now()) {
            sections.push(block);
        }
        if let Some(block) =
            format_taste_section(&doing::keeps_getting_to_her(db, TASTE_SHOWN).await)
        {
            sections.push(block);
        }
    }
    // Her own time is about public things, so any audience may hear it.
    // What she is doing right now is part of any moment; what she did
    // lately, of one about her.
    let words = match turn {
        Turn::Chat(words) | Turn::Event(words) => Some(words),
        Turn::Plain => None,
    };
    let lately = if her_life {
        doing::recalled(db, words, DOING_RECENT, DOING_RELATED).await
    } else {
        Vec::new()
    };
    let now = doing::now_text(super::super::clock::now());
    if let Some(block) = format_doing_section(now.as_deref(), &lately) {
        sections.push(block);
    }
}

/// What is between them, each heard only where it grew: their bits, the
/// group's days, how she comes across, what they are to her, what still
/// stings, what she meant to come back to, and her views their words touch.
pub(super) async fn between_them(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    turn: Turn<'_>,
    present: &crate::services::agent::memory::unified::Audience,
    opening: bool,
    sections: &mut Vec<String>,
) {
    let group = present.is_group();
    let words = match turn {
        Turn::Chat(words) | Turn::Event(words) => Some(words),
        Turn::Plain => None,
    };
    // What only she and this person share, or she and this group: each
    // heard only where it grew.
    let shared = match present.group_id() {
        Some(venue) => bits::in_group(db, venue, BITS_LIMIT).await,
        None => bits::between(db, user_id, BITS_LIMIT).await,
    };
    if let Some(block) = format_bits_section(&shared, group) {
        sections.push(block);
    }
    // What the last days in this group were like: the group's own.
    if let Some(venue) = present.group_id() {
        let days = bits::days_in(db, venue, GROUP_DAYS).await;
        if let Some(block) = format_group_days_section(&days) {
            sections.push(block);
        }
    }
    // How she comes across: with them, or in this group, each heard only
    // where it was found.
    let lands = match present.group_id() {
        Some(venue) => bits::lands_in(db, venue).await,
        None => bits::lands_with(db, user_id).await,
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
    if !group {
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
        None => {
            if let Some(block) = sore::section(&sore::open_all(db, user_id).await, now) {
                sections.push(block);
            }
        }
        Some(venue) => {
            if let Some(block) =
                sore::section(&sore::open_in_group(db, venue, Some(user_id)).await, now)
            {
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
                .then(|| myriad_merope::threads::group_section(&open, now))
                .flatten()
        } else {
            (opening || touched)
                .then(|| threads::section(&open, now))
                .flatten()
        };
        sections.extend(block);
    }
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
