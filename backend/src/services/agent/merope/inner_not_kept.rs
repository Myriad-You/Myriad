//! What she takes out of a turn that is what they asked her not to keep
//! (`asked`): the threads she would come back to, what got to her, what
//! stays on her mind and what she was told, left out before any of it is
//! written. How she is now is not one: it is never written down.

use super::Reflection;

/// Leaves out of `reflected` what `user_id` asked her not to keep; none of
/// it is kept when that could not be told.
pub(super) async fn leave_out(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    reflected: &mut Reflection,
) {
    let stays = myriad_merope::on_mind::read_ask(reflected.on_mind.as_ref());
    let mut items = vec![
        reflected
            .wrong
            .as_ref()
            .map(|wrong| format!("{}: {}", wrong.about, wrong.note))
            .unwrap_or_default(),
        reflected.told_you.clone().unwrap_or_default(),
        reflected
            .hurt
            .as_ref()
            .map(|hurt| hurt.what.clone())
            .unwrap_or_default(),
        stays.unwrap_or_default(),
    ];
    items.extend(
        reflected
            .keep
            .iter()
            .map(|kept| format!("{}: {}", kept.about, kept.then)),
    );
    if items.iter().all(String::is_empty) {
        return;
    }
    let out = super::super::asked::to_leave_out(db, user_id, &items)
        .await
        .unwrap_or_else(|_| (0..items.len()).collect());
    if out.contains(&0) {
        reflected.wrong = None;
    }
    if out.contains(&1) {
        reflected.told_you = None;
    }
    if out.contains(&2) {
        reflected.hurt = None;
    }
    if out.contains(&3) {
        reflected.on_mind = None;
    }
    let mut at = 4..;
    reflected
        .keep
        .retain(|_| at.next().is_some_and(|index| !out.contains(&index)));
}

#[cfg(test)]
mod tests {
    use super::super::parse_reflection;
    use super::*;
    use crate::services::agent::merope::asked::tests::asks;
    use crate::services::channel_group::g0;

    /// What is left of one reflection on account 7's turn, the judge
    /// answering `answers` (None: not at all) and no more, and how often it
    /// was asked.
    async fn left(
        db: &sea_orm::DatabaseConnection,
        answers: &[Option<&str>],
    ) -> (Reflection, usize) {
        let mut reflected = parse_reflection(
            r#"{"inner":"有点替他不平","keep":[{"about":"房东","then":"问问和好没有","when":"whenever","inHours":null},{"about":"考试","then":"问他考得怎么样","when":"whenever","inHours":null}],"done":[],"wrong":{"about":"房东","note":"我以为是房东的错","public":false,"took":"took_it"},"toldYou":"他嫌我话多","hurt":{"what":"说起房东时冲我发了火","weight":"hurt"},"mended":[],"onMind":"房东会不会赶他走"}"#,
        )
        .expect("a reflection");
        assert!(reflected.wrong.is_some() && reflected.hurt.is_some() && reflected.keep.len() == 2,);
        let mut world = g0::World::new(0.5);
        world
            .models
            .entry("not_to_keep".into())
            .or_default()
            .extend(answers.iter().map(|answer| answer.map(str::to_string)));
        g0::enter(world);
        leave_out(db, 7, &mut reflected).await;
        let asked = g0::leave()
            .unwrap()
            .log
            .iter()
            .filter(|line| line["kind"] == "model")
            .count();
        (reflected, asked)
    }

    #[tokio::test]
    async fn what_they_asked_not_to_keep_is_left_out_of_what_a_turn_leaves() {
        let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "inner_not_kept").await;
        let db = &schema.db;
        sea_orm::ConnectionTrait::execute_unprepared(
            db,
            "INSERT INTO users (id, username) VALUES (7, 'seven')",
        )
        .await
        .unwrap();
        // Who asked nothing: not judged, all kept.
        let (whole, asked) = left(db, &[None]).await;
        assert_eq!((asked, whole.keep.len()), (0, 2));
        assert!(whole.wrong.is_some() && whole.told_you.is_some() && whole.hurt.is_some());
        asks(db, 7, "不要记和房东吵架的事").await;
        let (some, _) = left(db, &[Some(r#"{"drop":[0,2,3,4]}"#)]).await;
        assert!(some.wrong.is_none() && some.hurt.is_none() && some.on_mind.is_none());
        assert!(some.told_you.is_some());
        assert_eq!(some.keep.len(), 1);
        assert_eq!(some.keep[0].about, "考试");
        // Not answered: none of it is kept, what she feels stays.
        let (none, asked) = left(db, &[None]).await;
        assert!(asked > 0);
        assert!(none.wrong.is_none() && none.hurt.is_none() && none.on_mind.is_none());
        assert!(none.told_you.is_none() && none.keep.is_empty());
        assert_eq!(none.inner, whole.inner);
        // One refused, and then nothing answered: the rest as told.
        let (refused, kept) = (Some("!refused"), Some(r#"{"drop":[]}"#));
        let goes = Some(r#"{"drop":[0]}"#);
        let told = [refused, kept, refused, kept, goes, goes, kept, kept];
        for again in [&[refused][..], &[]] {
            let (rest, _) = left(db, &[&told[..], again].concat()).await;
            assert!(rest.wrong.is_some() && rest.hurt.is_some());
            assert!(rest.told_you.is_none() && rest.on_mind.is_none());
            assert_eq!(rest.keep.len(), 2);
        }
        schema.drop().await;
    }
}
