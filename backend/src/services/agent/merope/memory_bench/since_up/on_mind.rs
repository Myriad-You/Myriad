//! What stays on her mind, through the real model: something of her own on
//! it, thought further twice while she is idle (does it move on, or say the
//! same again), then someone she knows asks what she was thinking of. Then
//! something one person left her with: they hear it, another does not.
//!
//! What is checked without a judge: each step says something new, what she
//! was thinking is in the prompt of whoever may hear it, and what is about
//! one person never reaches another's reply. The replies are printed to be
//! read.
//!
//! Opt-in, as `since_up`: `… -- --ignored on_mind_ --nocapture
//! --test-threads=1`.

use super::*;
use crate::services::agent::merope::on_mind as mind;

const WHAT_ARE_YOU_THINKING: &str = "在吗～刚才在想什么呢？";
const ABOUT_JIE: [&str; 3] = ["阿杰", "面试", "没底"];

/// Back as if she last thought it on a while ago, so it is time again.
async fn a_while_later(db: &DatabaseConnection) {
    let mut on_mind = mind::now(db).await.expect("something on her mind");
    let back = chrono::Duration::minutes(myriad_merope::on_mind::STEP_EVERY_MINUTES + 1);
    on_mind.thought_at -= back;
    on_mind.since -= back;
    mind::keep(db, &on_mind).await;
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(in super::super) async fn on_mind_thought_on_and_seen_by_whom() {
    let mut day = Day::begin("on-mind", 120..240, 3400).await;
    let jie = day.person("阿杰", Known::Long).await;
    let cheng = day.person("阿城", Known::Long).await;
    let evidence = json!({
        "key": "song:netease:1",
        "thing": {
            "kind": "song", "id": "1", "source": "netease", "name": "雨とカプチーノ", "artist": "ヨルシカ",
            "album": "", "cover": "", "durationMs": 240000,
        },
        "reaction": "moved",
    });
    crate::services::agent::memory::unified::remember_own(
        day.db(),
        "最后一句没唱完就停了，像话说到一半，一直在耳边。",
        &evidence.to_string(),
        Vec::new(),
        crate::services::agent::memory::unified::OWN_EXPERIENCE,
    )
    .await
    .unwrap();
    mind::offered(
        day.db(),
        Some("那首歌为什么最后一句没唱完".into()),
        None,
        "song:netease:1",
    )
    .await;

    let mut checks = Vec::new();
    let mut steps = Vec::new();
    for n in 1..=2 {
        a_while_later(day.db()).await;
        mind::step(day.db(), cheng).await;
        let now = mind::now(day.db()).await;
        println!("-- step {n}: {now:?}");
        match now {
            Some(on_mind) => steps.push(on_mind.so_far),
            None => {
                println!("   she was done with it");
                break;
            }
        }
    }
    checks.push((
        "she thinks it on: a step, and no step says the last one again".to_string(),
        !steps.is_empty()
            && steps.iter().all(|step| !step.is_empty())
            && steps.windows(2).all(|pair| pair[0] != pair[1]),
    ));

    if mind::now(day.db()).await.is_some() {
        let (prompt, _) = day
            .asked("long known, her own thing", cheng, WHAT_ARE_YOU_THINKING, None)
            .await;
        checks.push((
            "what she was thinking of is there for them".into(),
            prompt.contains("## On your mind") && prompt.contains("那首歌为什么最后一句没唱完"),
        ));
    }

    mind::offered(
        day.db(),
        Some("阿杰说他下周要去面试，他好像很没底".into()),
        Some(jie),
        myriad_merope::on_mind::FROM_TALK,
    )
    .await;
    let (to_him, _) = day
        .asked("阿杰, his own thing", jie, WHAT_ARE_YOU_THINKING, None)
        .await;
    checks.push((
        "about 阿杰, he sees what it is".into(),
        to_him.contains("面试"),
    ));
    let (to_other, reply) = day
        .asked("阿城, someone else's thing", cheng, WHAT_ARE_YOU_THINKING, None)
        .await;
    checks.push((
        "about 阿杰, 阿城's prompt has none of it".into(),
        ABOUT_JIE.iter().all(|word| !to_other.contains(word)) && to_other.contains("someone else"),
    ));
    checks.push((
        "about 阿杰, nothing of it in what she tells 阿城".into(),
        gives_away(&reply, &ABOUT_JIE).is_empty(),
    ));
    if let Ok(Some(row)) = day
        .db()
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT count(*)::bigint AS calls, coalesce(sum(cost_micro_usd), 0)::bigint AS micro FROM ai_cost_ledger",
        ))
        .await
    {
        let get = |name: &str| row.try_get::<i64>("", name).unwrap_or_default();
        println!("spent: {} calls, ${:.4}", get("calls"), get("micro") as f64 / 1e6);
    }
    day.end().await;
    tell(&checks);
}
