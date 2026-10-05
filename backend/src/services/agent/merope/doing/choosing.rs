//! Choosing what to do next: what is at hand, what keeps getting to her, and her pick.

use super::*;

/// What is at hand for a free moment, and which of it works toward
/// something she wants on her own.
pub(super) struct AtHand {
    pub(super) lately: Vec<unified_row::Model>,
    pub(super) options: Vec<Thing>,
    pub(super) advances: Vec<Option<String>>,
    pub(super) taste: myriad_merope::taste::Taste,
}

impl AtHand {
    /// Something she wants pulls her toward what is at hand.
    pub(super) fn pulls(&self) -> bool {
        self.advances.iter().any(Option::is_some)
    }
}

pub(super) async fn at_hand(db: &DatabaseConnection) -> Option<AtHand> {
    let lately = match unified::own_experiences(db, 300).await {
        Ok(lately) => lately,
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not read what she did lately");
            return None;
        }
    };
    let taste = taste(db).await;
    let options = sources::options(db, &taste).await;
    if options.is_empty() {
        tracing::info!("[Merope] nothing at hand for her own time");
        return None;
    }
    let wants: Vec<String> = super::super::wants::open(db)
        .await
        .into_iter()
        .filter(|want| want.reach == myriad_merope::wants::Reach::OnYourOwn && !want.longing)
        .map(|want| want.want)
        .collect();
    let advances = options
        .iter()
        .map(|thing| {
            wants
                .iter()
                .find(|want| myriad_merope::pace::advances(want, thing.title(), thing.by()))
                .cloned()
        })
        .collect();
    Some(AtHand {
        lately,
        options,
        advances,
        taste,
    })
}

/// Whose things keep getting to her lately, a line each, at most `most`.
pub async fn keeps_getting_to_her(db: &DatabaseConnection, most: usize) -> Vec<String> {
    taste(db).await.liked_by(most)
}

/// Her taste as the site's owner looks into her: whose things keep getting
/// to her and whose keep not being for her.
pub(in crate::services::agent::merope) async fn taste_view(db: &DatabaseConnection) -> Value {
    let taste = taste(db).await;
    json!({ "likedBy": taste.liked(5), "notForHer": taste.not_for_her(3) })
}

/// How far back her reactions make up her taste: faded experiences still
/// count until they are purged.
pub(super) const TASTE_DAYS: i64 = 120;

pub(super) const TASTE_ROWS: u64 = 4000;

/// Her taste, from how what she did landed with her (see
/// `myriad_merope::taste`). Empty when it cannot be read.
pub(super) async fn taste(db: &DatabaseConnection) -> myriad_merope::taste::Taste {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    #[derive(Deserialize)]
    struct Row {
        thing: Thing,
        #[serde(default)]
        reaction: Option<Reaction>,
    }
    let since = Utc::now() - chrono::Duration::days(TASTE_DAYS);
    // Faded is still hers; a row taken away (not faded) is not.
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT created_at, jsonb_build_object('thing', e->'thing', 'reaction', e->'reaction')::text AS taken \
             FROM (SELECT created_at, evidence::jsonb AS e FROM agent_memories \
               WHERE user_id IS NULL AND venue = $1 AND source = $2 AND created_at >= $3 \
                 AND evidence IS JSON AND (invalid_at IS NULL OR invalid_reason = 'faded')) own \
             ORDER BY created_at DESC LIMIT $4",
            [
                unified::OWN_VENUE.into(),
                unified::OWN_EXPERIENCE.into(),
                since.fixed_offset().into(),
                (TASTE_ROWS as i64).into(),
            ],
        ))
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "[Merope] could not read her taste");
            Vec::new()
        });
    let now = Utc::now();
    let read: Vec<(Row, f64)> = rows
        .iter()
        .filter_map(|row| {
            let at: DateTime<chrono::FixedOffset> = row.try_get("", "created_at").ok()?;
            let taken: String = row.try_get("", "taken").ok()?;
            let days = now.signed_duration_since(at).num_minutes() as f64 / 1440.0;
            Some((serde_json::from_str::<Row>(&taken).ok()?, days))
        })
        .collect();
    let taken: Vec<myriad_merope::taste::Taken> = read
        .iter()
        .map(|(row, days_ago)| myriad_merope::taste::Taken {
            thing: &row.thing,
            reaction: row.reaction,
            days_ago: *days_ago,
        })
        .collect();
    myriad_merope::taste::Taste::of(&taken)
}

/// What she chooses from, as the model reads it.
pub(super) async fn choice_input(
    db: &DatabaseConnection,
    hand: &AtHand,
    pace: &[String],
) -> String {
    let AtHand {
        lately,
        options,
        advances,
        taste,
    } = hand;
    let myself = super::super::self_state::current(db).await.facts_view();
    let now = Utc::now();
    let lately_view: Vec<Value> = lately
        .iter()
        .take(8)
        .filter_map(|row| {
            let experience = Experience::of(row)?;
            let ago = now.signed_duration_since(row.created_at.with_timezone(&Utc));
            Some(json!(format!(
                "{}, {}",
                experience.line_felt(),
                ago_text(ago)
            )))
        })
        .collect();
    let mut option_views = Vec::with_capacity(options.len() + super::super::pace::LAZING.len());
    for (index, thing) in options.iter().enumerate() {
        let mut view = sources::view(db, index, thing).await;
        if let Some(want) = &advances[index] {
            view["advances"] = json!(want);
        }
        // What she had before, she knows she had, and how it went.
        if let Some((reaction, days_ago)) = taste.last_time(thing) {
            let ago = ago_text(chrono::Duration::minutes((days_ago * 1440.0) as i64));
            let last = match reaction {
                Some(reaction) => format!("{}, {ago}", reaction.felt()),
                None => format!("nothing of it reached you, {ago}"),
            };
            view["hadBefore"] = json!(match taste.times(thing) {
                0 | 1 => format!("once: {last}"),
                times => format!("{times} times; the last time {last}"),
            });
        }
        option_views.push(view);
    }
    for (offset, (_, what)) in super::super::pace::LAZING.iter().enumerate() {
        option_views.push(json!({
            "index": options.len() + offset,
            "kind": "lazing",
            "what": what,
        }));
    }
    let kinds: Vec<(&str, chrono::Duration)> = lately
        .iter()
        .filter_map(|row| {
            let experience = Experience::of(row)?;
            Some((
                experience.thing.kind(),
                now.signed_duration_since(row.created_at.with_timezone(&Utc)),
            ))
        })
        .collect();
    json!({
        "myself": myself,
        "lately": lately_view,
        "yourPace": pace,
        "sameThingLately": myriad_merope::doing::same_run(&kinds),
        "keepsGettingToYou": taste.liked_by(3),
        "yourViews": views_for(db, options).await,
        "yourWants": super::super::wants::lines(&super::super::wants::open(db).await),
        "whoYouHaveBeen": super::super::self_story::current(db).await,
        "options": option_views,
    })
    .to_string()
}

/// Views in mind as she picks: those the things at hand touch (a view of
/// an artist when their song is there), then her latest, five in all.
pub(super) async fn views_for(db: &DatabaseConnection, options: &[Thing]) -> Vec<String> {
    const SHOWN: usize = 5;
    const TOUCHED: usize = 4;
    let words = options
        .iter()
        .map(|thing| format!("{} {}", thing.title(), thing.by().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join(" ");
    let mut views: Vec<String> = super::super::views::touched(db, &words, TOUCHED)
        .await
        .into_iter()
        .map(|(about, view)| format!("{about}: {view}"))
        .collect();
    for view in super::super::views::held(db, SHOWN as u64).await {
        if views.len() >= SHOWN {
            break;
        }
        if !views.contains(&view) {
            views.push(view);
        }
    }
    views
}

/// What she took up at a free moment.
pub(super) enum Picked {
    Doing(Doing),
    /// A way of lazing about (its kind), for this long.
    Lazing(&'static str, chrono::Duration),
}

/// What she picked, or how long she would rather leave it (none when she
/// did not say or could not choose). `pace` is her pace as she knows it.
pub(super) async fn choose(
    db: &DatabaseConnection,
    owner: i32,
    hand: AtHand,
    pace: &[String],
    tone: myriad_merope::pace::Tone,
) -> Result<Picked, Option<chrono::Duration>> {
    let input = choice_input(db, &hand, pace).await;
    let soul = soul().await;
    let AtHand { options, taste, .. } = hand;
    let choice: Option<Choice> = call::Ask::new(Voice::Judge, owner, "doing_choice")
        .within(CALL_TIMEOUT)
        .json(
            &choice_system(&soul),
            &input,
            CHOICE_SCHEMA,
            &choice_schema(options.len() + super::super::pace::LAZING.len()),
        )
        .await
        .ok();
    let Some(choice) = choice else {
        tracing::info!("[Merope] could not decide what to do on her own");
        return Err(Some(UNREACHED_AGAIN));
    };
    // A way of lazing about, picked like anything else.
    if let Some((kind, _)) = choice
        .choice
        .and_then(|index| index.checked_sub(options.len()))
        .and_then(|index| super::super::pace::LAZING.get(index))
    {
        let minutes = choice
            .rest_minutes
            .unwrap_or_else(|| myriad_merope::pace::laze_minutes(tone, rand::random()))
            .clamp(*REST_MINUTES.start(), *REST_MINUTES.end());
        tracing::info!(kind, minutes, "[Merope] chose to laze about");
        return Ok(Picked::Lazing(kind, chrono::Duration::minutes(minutes)));
    }
    let Some(thing) = choice.choice.and_then(|index| options.get(index)).cloned() else {
        let rest = choice.rest_minutes.map(|minutes| {
            chrono::Duration::minutes(minutes.clamp(*REST_MINUTES.start(), *REST_MINUTES.end()))
        });
        tracing::info!(
            rest_minutes = rest.map(|rest| rest.num_minutes()),
            "[Merope] chose to do nothing for a while"
        );
        return Err(rest);
    };
    // A book off the shelf is opened only now.
    let Some(thing) = sources::open(db, thing).await else {
        return Err(None);
    };
    let started = Utc::now();
    let length = sources::length(db, &thing).await;
    tracing::info!(kind = %thing.key(), "[Merope] doing something of her own");
    Ok(Picked::Doing(Doing {
        ends: started + length,
        started,
        why: choice.why.unwrap_or_default().chars().take(80).collect(),
        had_before: taste.times(&thing),
        thing,
    }))
}
