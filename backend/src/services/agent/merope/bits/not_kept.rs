//! What someone asked her not to keep, held to in what she makes of a day
//! with them, or of their group's talk: what it says of that is left out.

use myriad_merope::bits::{Change, Changes};
use myriad_merope::group_reading::Reading;

use super::super::memory_jobs::Failure;

/// How long what a stretch of a group's talk says may be judged for, all who
/// spoke in it together.
const TOLD_WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

/// What a bit says, with the words it was read from.
fn said_of(bit: &Change) -> String {
    let quote = bit.quote.as_deref().unwrap_or_default();
    format!("{}: {} {quote}", bit.handle, bit.how)
}

/// Leave out of `changes`, a private day gone over, what they asked her not
/// to keep. Whether it could be told: not, none of it may be kept.
pub(super) async fn leave_out_of_a_day(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    changes: &mut Changes,
) -> bool {
    let wish = changes
        .wish
        .as_ref()
        .map(|wish| format!("{}: {}", wish.about, wish.then));
    let mut items = vec![
        changes.us.clone(),
        changes.day.clone(),
        changes.lands.clone(),
        wish.unwrap_or_default(),
    ];
    items.extend(changes.bits.iter().map(said_of));
    let Ok(out) = super::super::asked::to_leave_out(db, user_id, &items).await else {
        return false;
    };
    // In the order they were put in.
    for (index, text) in [&mut changes.us, &mut changes.day, &mut changes.lands]
        .into_iter()
        .enumerate()
    {
        if out.contains(&index) {
            text.clear();
        }
    }
    if out.contains(&3) {
        changes.wish = None;
    }
    let mut at = 4..;
    changes
        .bits
        .retain(|_| at.next().is_some_and(|index| !out.contains(&index)));
    true
}

/// Leave out of `reading` what it says of each of `people` (account, the name
/// they go by there) of what they asked her not to keep. Only those who spoke
/// in the stretch are known here: what others say of someone not there is not
/// held to what that someone asked. An `Err`: not told, and none of it may
/// be kept till it is.
pub(super) async fn leave_out(
    db: &sea_orm::DatabaseConnection,
    people: &[(i32, String)],
    reading: &mut Reading,
) -> Result<(), Failure> {
    let mut items = vec![reading.day.clone(), reading.lands.clone()];
    items.extend(reading.bits.iter().map(said_of));
    items.extend(reading.heard.iter().map(|heard| heard.thing.clone()));
    items.extend(
        reading
            .explained
            .iter()
            .map(|told| format!("{}: {} {}", told.term, told.meaning, told.quote)),
    );
    let until = std::time::Instant::now() + TOLD_WITHIN;
    let mut out = std::collections::BTreeSet::new();
    for (user_id, name) in people {
        out.extend(
            super::super::asked::to_leave_out_of(db, *user_id, Some(name), &items, until).await?,
        );
    }
    // In the order they were put in.
    if out.contains(&0) {
        reading.day.clear();
    }
    if out.contains(&1) {
        reading.lands.clear();
    }
    let mut at = 2..;
    let mut kept = || at.next().is_some_and(|index| !out.contains(&index));
    reading.bits.retain(|_| kept());
    reading.heard.retain(|_| kept());
    reading.explained.retain(|_| kept());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::merope::asked::tests::asks;
    use crate::services::channel_group::g0;
    use myriad_merope::bits::{ChangeKind, Wish};
    use myriad_merope::group_reading::{Explained, Heard};
    use sea_orm::ConnectionTrait;

    fn bit(handle: &str, quote: Option<&str>) -> Change {
        Change {
            handle: handle.into(),
            how: format!("{handle}的梗"),
            change: ChangeKind::New,
            quote: quote.map(str::to_string),
        }
    }

    fn answering(answers: &[Option<&str>]) {
        let mut world = g0::World::new(0.5);
        world
            .models
            .entry("not_to_keep".into())
            .or_default()
            .extend(answers.iter().map(|answer| answer.map(str::to_string)));
        g0::enter(world);
    }

    /// What the judge was shown, ask by ask.
    fn shown() -> Vec<serde_json::Value> {
        g0::leave()
            .unwrap()
            .log
            .iter()
            .filter(|entry| entry["kind"] == "model")
            .map(|entry| serde_json::from_str(entry["what"]["input"].as_str().unwrap()).unwrap())
            .collect()
    }

    async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
        let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
        let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
        schema
            .db
            .execute_unprepared(
                "INSERT INTO users (id, username) VALUES (7, 'seven'), (8, 'eight')",
            )
            .await
            .unwrap();
        asks(&schema.db, 7, "不要记和房东吵架的事").await;
        Some(schema)
    }

    #[tokio::test]
    async fn a_groups_reading_leaves_out_what_one_of_them_asked_not_to_keep() {
        let Some(schema) = schema("bits_group_asked").await else {
            return;
        };
        let reading = || Reading {
            day: "今天聊了阿北和房东".into(),
            lands: "大家喜欢她接话".into(),
            bits: vec![bit("房东", Some("房东又来了")), bit("鸽王", None)],
            heard: vec![Heard {
                thing: "一本讲登山的书".into(),
                hangs_on: None,
                because: String::new(),
            }],
            explained: vec![Explained {
                term: "老登".into(),
                meaning: "阿北的房东".into(),
                who: "阿北".into(),
                quote: "老登就是我房东".into(),
            }],
            ..Reading::default()
        };
        let people = [(7, "阿北".to_string()), (8, "小满".to_string())];
        // The one who asked is asked about, by their name there; the other,
        // who asked nothing, is not.
        answering(&[Some(r#"{"drop":[0,2,5]}"#)]);
        let mut left = reading();
        assert!(leave_out(&schema.db, &people, &mut left).await.is_ok());
        let asks = shown();
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0]["who"], "阿北");
        assert_eq!(asks[0]["items"].as_array().unwrap().len(), 6);
        assert!(left.day.is_empty());
        assert_eq!(left.lands, "大家喜欢她接话");
        assert_eq!(left.bits.len(), 1);
        assert_eq!(left.bits[0].handle, "鸽王");
        assert_eq!(left.heard.len(), 1);
        assert!(left.explained.is_empty());
        // Not told: none of it.
        answering(&[None]);
        assert!(
            leave_out(&schema.db, &people, &mut reading())
                .await
                .is_err()
        );
        g0::leave();
        // No one there who asked: not asked at all.
        answering(&[]);
        assert!(
            leave_out(&schema.db, &people[1..], &mut reading())
                .await
                .is_ok()
        );
        assert!(shown().is_empty());
        schema.drop().await;
    }

    /// A stretch read and not told of keeps nothing, and is kept as it was
    /// read, without being read again, once it is told; but for what she has
    /// found anew since.
    #[tokio::test]
    async fn a_stretch_not_told_of_is_kept_as_it_was_read_once_it_is() {
        use super::super::{Again, DAY_SOURCE, LANDS_SOURCE, go_over_stretch};
        use crate::services::agent::merope::{memory_jobs, store::memory_jobs as queue};
        let Some(schema) = schema("bits_group_later").await else {
            return;
        };
        let db = &schema.db;
        let now = crate::clock::now().fixed_offset();
        let said: Vec<_> = ["房东又来了", "好烦", "周末爬山吗", "去"]
            .iter()
            .enumerate()
            .map(|(n, text)| {
                let at = now - chrono::Duration::minutes(10 - n as i64);
                (at, "阿北".to_string(), text.to_string(), n == 3)
            })
            .collect();
        let people = [(7, "阿北".to_string())];
        let raw = serde_json::json!({
            "moments": [], "bits": [], "day": "阿北和房东吵了一架",
            "lands": "大家喜欢她接话", "letGo": [], "heard": [], "explained": [],
            "corrected": [],
        })
        .to_string();
        let kept = async || {
            let mut kept = Vec::new();
            for source in [DAY_SOURCE, LANDS_SOURCE] {
                let rows = crate::services::agent::memory::unified::venue_source_rows(
                    db,
                    None,
                    "group:telegram:42",
                    source,
                    10,
                )
                .await
                .unwrap();
                kept.extend(rows.into_iter().map(|row| row.content));
            }
            kept
        };
        answering(&[Some("!rateLimited"); 3]);
        g0::with(|world| {
            world
                .models
                .insert("merope_group_reading".into(), [Some(raw)].into())
        });
        let heard = go_over_stretch(db, 8, "telegram:42", said.clone(), Some(&people), None);
        let Err(not_told) = heard.await else {
            panic!("not told");
        };
        g0::leave();
        assert!(kept().await.is_empty());
        let read = not_told.read.clone();
        memory_jobs::heard_later(db, 8, "telegram:42", said.clone(), Vec::new(), read)
            .await
            .unwrap();
        let claimed = queue::claim(db).await.unwrap().expect("waits");
        let again_as = |read| Again {
            read,
            own: queue::Own::of(&claimed),
        };
        // Who they are could not be read: nor then.
        answering(&[]);
        let read = Some(again_as(&not_told.read));
        let again = go_over_stretch(db, 8, "telegram:42", said.clone(), None, read);
        assert!(again.await.is_err());
        assert!(shown().is_empty());
        assert!(kept().await.is_empty());
        answering(&[Some(r#"{"drop":[0]}"#)]);
        let again = go_over_stretch(db, 8, "telegram:42", said.clone(), Some(&people), read);
        assert!(again.await.is_ok());
        assert_eq!(shown().len(), 1, "not read again");
        assert_eq!(kept().await, ["大家喜欢她接话"]);
        // Found anew since: left as it is.
        db.execute_unprepared(
            "UPDATE agent_memories SET content = '后来的', created_at = NOW() + INTERVAL '1 minute' \
             WHERE venue = 'group:telegram:42'",
        )
        .await
        .unwrap();
        answering(&[Some(r#"{"drop":[0]}"#)]);
        let again = go_over_stretch(db, 8, "telegram:42", said.clone(), Some(&people), read);
        assert!(again.await.is_ok());
        g0::leave();
        assert_eq!(kept().await, ["后来的"]);
        // Never told, or too long ago: let go, counted, nothing of it kept.
        let let_go = || {
            super::super::super::asked::counts()["groupLetGo"]
                .as_u64()
                .unwrap()
        };
        let before = let_go();
        let mut old = not_told.read.clone();
        old.at -= chrono::Duration::hours(7);
        for (read, gives_up) in [(&not_told.read, true), (&old, false)] {
            let again = crate::services::channel_group::heard_again(
                db,
                8,
                "telegram:42",
                &said,
                &[],
                again_as(read),
                gives_up,
            );
            assert!(again.await.is_ok());
        }
        assert!(let_go() >= before + 2);
        assert_eq!(kept().await, ["后来的"]);
        schema.drop().await;
    }

    #[tokio::test]
    async fn a_day_with_them_leaves_out_what_they_asked_not_to_keep() {
        let Some(schema) = schema("bits_day_asked").await else {
            return;
        };
        let mut changes = Changes {
            bits: vec![bit("房东", None), bit("鸽王", None)],
            us: "常一起吐槽房东".into(),
            day: "聊了爬山".into(),
            lands: "喜欢被逗".into(),
            let_go: Vec::new(),
            wish: Some(Wish {
                about: "房东".into(),
                then: "问问他和房东和好没有".into(),
            }),
        };
        answering(&[Some(r#"{"drop":[0,3,4]}"#)]);
        assert!(leave_out_of_a_day(&schema.db, 7, &mut changes).await);
        g0::leave();
        assert!(changes.us.is_empty() && changes.wish.is_none());
        assert_eq!(
            (changes.day.as_str(), changes.lands.as_str()),
            ("聊了爬山", "喜欢被逗")
        );
        assert_eq!(changes.bits.len(), 1);
        answering(&[None]);
        assert!(!leave_out_of_a_day(&schema.db, 7, &mut changes).await);
        g0::leave();
        schema.drop().await;
    }
}
