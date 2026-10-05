//! Finishing something: what she made of it, and telling whoever is here.

use super::*;

pub(super) async fn finish(db: &DatabaseConnection, owner: i32, done: Doing) {
    let Some(intake) = sources::intake(db, owner, &done.thing).await else {
        return;
    };
    let views = own_views_on(db, &done.thing).await;
    let before = notes_before(db, &done.thing).await;
    // What she has been quietly hoping for is there when something reaches
    // her, not when she picks: it colours how things land.
    let mut alongside = intake.alongside.clone();
    if let Some(longings) = myriad_merope::wants::undercurrent(&super::super::wants::open(db).await)
    {
        alongside.push((
            "Underneath lately, not something you set out for, you have been hoping".to_string(),
            longings,
        ));
    }
    let input = digest_input(
        intake.material.as_deref(),
        intake.limit,
        &views,
        &before,
        &alongside,
    );
    let soul = soul().await;
    let what = format!(
        "{} {}",
        done.thing.verb(),
        intake
            .called
            .clone()
            .unwrap_or_else(|| done.thing.describe())
    );
    let system = digest_system(
        &soul,
        &what,
        picked_for(&done.thing, &done.why),
        &intake.how,
    );
    let schema = digest_schema(&intake.asks);
    let ask = || {
        call::Ask::new(Voice::Hers, owner, "doing_digest")
            .within(CALL_TIMEOUT)
            .json_raw(&system, &input, DIGEST_SCHEMA, &schema)
    };
    // What she spent the while on is not lost to one bad call.
    let raw = match ask().await {
        Err(failure) if failure.retryable() => {
            tokio::time::sleep(DIGEST_AGAIN_AFTER).await;
            ask().await
        }
        raw => raw,
    };
    let Ok(raw) = raw else {
        return;
    };
    let Some((digest, wrote)) = read_digest(&raw, &intake.asks) else {
        tracing::warn!(kind = %done.thing.key(), "[Merope] what stayed with her came back unreadable; not kept");
        return;
    };
    let impression = super::super::ingest::compact_summary(&digest.impression);
    if impression.is_empty() {
        return;
    }
    let reached = intake.reached;
    let kept = sources::after(db, owner, &done.thing, &wrote, intake.carry).await;
    let evidence = Experience {
        key: done.thing.key(),
        thing: done.thing.clone(),
        // Nothing reached her: no taste to keep.
        reaction: reached.then_some(digest.reaction),
        tell: digest.tell,
        kept,
    };
    match unified::remember_own(
        db,
        &impression,
        &serde_json::to_string(&evidence).unwrap_or_default(),
        digest.concepts,
        unified::OWN_EXPERIENCE,
    )
    .await
    {
        Ok(Some(_)) => {}
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(%error, kind = %done.thing.key(), "[Merope] could not keep what she did");
            return;
        }
    }
    // Something she read or found out that got to her may give her an idea
    // for a puzzle of her own.
    let sparks = matches!(
        done.thing,
        Thing::Note { .. } | Thing::Chapter { .. } | Thing::Inquiry { .. }
    ) && reached
        && matches!(
            digest.reaction,
            myriad_merope::doing::Reaction::Moved | myriad_merope::doing::Reaction::Liked
        );
    if sparks {
        let (db, what, took_in, material) = (
            db.clone(),
            what.clone(),
            impression.clone(),
            intake.material.clone(),
        );
        super::super::background::spawn("make_soup", async move {
            super::super::making::maybe_make(&db, owner, &what, &took_in, material.as_deref())
                .await;
        });
    }
    // Fallen asleep over it, she tells no one now.
    if digest.tell && super::super::timing::asleep_now().is_none() {
        tell_whoever_is_here(&done.thing, &impression);
        // The groups she is in may hear it too, if one is where she would
        // say it.
        let what = evidence
            .noted(&impression)
            .trim_start_matches("- ")
            .to_string();
        super::super::background::spawn("share_first", async move {
            crate::services::channel_group::share_first(owner, what).await;
        });
    }
}

/// Why she picked it, as she goes by it when she writes what stayed with
/// her. A song is picked by its name alone, so why she picked it is a
/// guess from the name; given back as she writes, the note reads the name
/// instead of the song (a wordless piece called "Ocean of Memories" comes
/// back as the sea). What she heard is what she goes by.
pub(super) fn picked_for<'a>(thing: &Thing, why: &'a str) -> &'a str {
    match thing {
        Thing::Song { .. } => "",
        _ => why,
    }
}

/// What she takes it in with: the material, what its kind shows beside it,
/// her views that touch it, and what she wrote before.
pub(super) fn digest_input(
    material: Option<&str>,
    limit: usize,
    views: &[String],
    before: &[String],
    alongside: &[(String, String)],
) -> String {
    let mut input = match material {
        Some(text) if !text.trim().is_empty() => myriad_agent_rules::untrusted_block(
            "material",
            &text.chars().take(limit).collect::<String>(),
        ),
        _ => "(no material)".to_string(),
    };
    for (heading, text) in alongside {
        input.push_str(&format!("\n\n{heading}:\n"));
        input.push_str(&myriad_agent_rules::untrusted_block("alongside", text));
    }
    if !views.is_empty() {
        input.push_str("\n\nYour views:\n");
        input.push_str(&myriad_agent_rules::untrusted_block(
            "your_views",
            &views.join("\n"),
        ));
    }
    if !before.is_empty() {
        input.push_str("\n\nWhat you wrote when you had this same one before:\n");
        input.push_str(&myriad_agent_rules::untrusted_block(
            "this_one_before",
            &before.join("\n"),
        ));
    }
    input
}

/// Her views that touch this thing. Only those: a view she brings to
/// everything becomes the words she says about everything.
pub(super) async fn own_views_on(db: &DatabaseConnection, thing: &Thing) -> Vec<String> {
    let words = format!("{} {}", thing.title(), thing.by().unwrap_or_default());
    super::super::views::touched(db, &words, 3)
        .await
        .into_iter()
        .map(|(about, view)| format!("{about}: {view}"))
        .collect()
}

/// What she wrote the times she had this same thing before: her memory of
/// it, most recent first. Not her notes on other things: shown those, she
/// writes them again.
pub(super) async fn notes_before(db: &DatabaseConnection, thing: &Thing) -> Vec<String> {
    const BEFORE: usize = 2;
    let key = thing.key();
    let now = Utc::now();
    unified::own_experiences(db, 300)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| Some((Experience::of(row)?, row)))
        .filter(|(experience, _)| experience.key == key)
        .take(BEFORE)
        .map(|(experience, row)| {
            format!(
                "{} ({})",
                experience.noted(&row.content),
                ago(now, row.created_at.with_timezone(&Utc))
            )
        })
        .collect()
}

/// Someone who can see her may hear about it; the decision is hers, live.
pub(super) fn tell_whoever_is_here(thing: &Thing, impression: &str) {
    let summary = format!(
        "你刚自己{}{}：{impression}",
        thing.done_verb(),
        thing.title()
    );
    let people: Vec<i32> = {
        let Ok(mut life) = LIFE.lock() else {
            return;
        };
        life.told.retain(|_, at| at.elapsed() < TELL_EVERY);
        let people: Vec<i32> = crate::services::agent::consciousness::present_users()
            .into_iter()
            .filter(|user_id| *user_id > 0 && !life.told.contains_key(user_id))
            .collect();
        for user_id in &people {
            life.told.insert(*user_id, Instant::now());
        }
        people
    };
    for user_id in people {
        super::super::spawn_ingest(user_id, DOING_EVENT, summary.clone());
    }
}
