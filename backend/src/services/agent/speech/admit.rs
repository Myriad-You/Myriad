//! Whether a wish goes on to be said: the shared core's admission
//! (`myriad_merope::speech::admit`) with what the site knows put in, the
//! ledger rows that bear on it, the grants (the runtime layer only), and how
//! things stand with them and with her.

use chrono::{DateTime, Utc};
use myriad_agent_rules::memory::audience::Venue;
use myriad_merope::speech::{Admission, Admit, Refusal, Wish};
use sea_orm::DatabaseConnection;

use crate::services::agent::merope;

/// How things stand with them and with her as a wish comes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scene {
    pub dnd: bool,
    pub they_opened: bool,
    pub present: bool,
    pub last_talk_at: Option<DateTime<Utc>>,
    pub they_wrote_at: Option<DateTime<Utc>>,
    pub doing: Option<String>,
}

/// What she is in the middle of, as she is told it.
fn doing_now() -> Option<String> {
    merope::doing::current().map(|doing| doing.thing.title().to_string())
}

/// How things stand in a private talk with `user_id`.
pub async fn private_scene(db: &DatabaseConnection, user_id: i32) -> Scene {
    let state = merope::get_or_create_state(db, user_id).await.ok();
    let wrote = state
        .as_ref()
        .and_then(|state| state.last_user_message_at)
        .map(|at| at.with_timezone(&Utc));
    let said = super::ledger::bearing(db, &super::private(user_id), None)
        .await
        .into_iter()
        .find(|said| said.reached())
        .and_then(|said| DateTime::<Utc>::from_timestamp_micros(said.at));
    Scene {
        dnd: state.as_ref().is_some_and(merope::effective_do_not_disturb),
        they_opened: false,
        present: crate::services::agent::consciousness::owner_here(user_id).present,
        last_talk_at: wrote.into_iter().chain(said).max(),
        they_wrote_at: wrote,
        doing: doing_now(),
    }
}

/// How things stand in the group `venue`: when anyone last said anything
/// there and when she last did.
pub async fn group_scene(
    db: &DatabaseConnection,
    venue: &str,
    theirs: Option<DateTime<Utc>>,
) -> Scene {
    let said = super::ledger::bearing(db, &super::group(venue), None)
        .await
        .into_iter()
        .find(|said| said.reached())
        .and_then(|said| DateTime::<Utc>::from_timestamp_micros(said.at));
    Scene {
        present: true,
        last_talk_at: theirs.into_iter().chain(said).max(),
        they_wrote_at: theirs,
        doing: doing_now(),
        ..Scene::default()
    }
}

/// The core's admission of `wish` in `scene`. A private wish is checked
/// against what the person it is for is granted now; a group's against
/// none (nothing said in a group does anything).
pub async fn admit_in(db: &DatabaseConnection, wish: &Wish, scene: &Scene) -> Admission {
    let granted = match (&wish.to.venue, super::user_of(&wish.to)) {
        (Venue::Private, Some(user)) if !wish.needs.is_empty() => {
            let mut granted: Vec<String> = crate::services::agent::get_user_permissions(db, user)
                .await
                .into_iter()
                .collect();
            granted.sort_unstable();
            granted
        }
        _ => Vec::new(),
    };
    let micros = |at: Option<DateTime<Utc>>| at.map(super::micros);
    let input = Admit {
        wish: wish.clone(),
        ledger: super::ledger::bearing(db, &wish.to, wish.thing.as_deref()).await,
        granted,
        dnd: scene.dnd,
        they_opened: scene.they_opened,
        present: scene.present,
        now: super::micros(crate::clock::now()),
        last_talk_at: micros(scene.last_talk_at),
        they_wrote_at: micros(scene.they_wrote_at),
        doing: scene.doing.clone(),
        touched_at: None,
    };
    let admission = myriad_merope::speech::admit(&input, &merope::clock::zone());
    match &admission {
        Admission::Refused(Refusal::Fuse { count }) => {
            tracing::error!(
                count,
                source = ?wish.source,
                "[Speech] far more unasked lines than she ever says: a loop, stopped"
            );
        }
        Admission::Refused(refusal) => {
            tracing::info!(?refusal, source = ?wish.source, "[Speech] not said");
        }
        _ => {}
    }
    admission
}

/// [`admit_in`] for a private wish, in the talk as it stands now.
pub async fn admit(db: &DatabaseConnection, wish: &Wish) -> Admission {
    let scene = match super::user_of(&wish.to) {
        Some(user) => private_scene(db, user).await,
        None => Scene::default(),
    };
    admit_in(db, wish, &scene).await
}

/// Whether `admission` lets her go on (to judge, or to say it).
pub fn goes_on(admission: &Admission) -> bool {
    matches!(admission, Admission::Ask { .. } | Admission::Say)
}

/// The facts she judges by, when it is hers to judge.
pub fn facts(admission: &Admission) -> &[String] {
    match admission {
        Admission::Ask { facts } => facts,
        _ => &[],
    }
}
