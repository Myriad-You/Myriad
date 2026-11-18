//! Her undertone and her own gaps, through the real model: the three
//! scenes of the plan (11-18) with the undertone a stretch of days would
//! leave (made as the night makes it, `myriad_merope::undertone`), and
//! today's own gaps put in as her own time keeps them. Someone asks how
//! she is. Only her models' settings are read from the site (read only):
//! nothing of hers there, so no free moment is offered from what is at
//! hand on the site.
//!
//! What is checked here is only what can be checked without a judge: no
//! number of hers is in a prompt, nothing of anyone crosses, today's own
//! gaps reach how today has left her, and where her mood settles moves as it
//! should. The replies are printed to be read.
//!
//! Opt-in, as `since_up`: `… -- --ignored undertone_ --nocapture
//! --test-threads=1`, `MEROPE_SINCE_UP_RUNS=1` for one ask each.

use myriad_merope::undertone::{Day as Lived, Undertone};

use super::*;

fn day(n: i64) -> chrono::NaiveDate {
    crate::clock::now().date_naive() - chrono::Duration::days(n)
}

/// The undertone `days` (oldest first) leave her, folded as the night does.
fn after(days: &[Lived]) -> Undertone {
    let mut kept = None;
    for (index, lived) in days.iter().enumerate() {
        kept = Undertone::after(
            kept.as_ref(),
            day((days.len() - index) as i64),
            lived,
        );
    }
    kept.expect("days she lived")
}

fn lived(spent: f64, lazed: f64, things: u32, ups: u32, downs: u32) -> Lived {
    Lived {
        things,
        gaps: ups + downs,
        lift: f64::from(ups) * myriad_merope::undertone::MOVED
            - f64::from(downs) * myriad_merope::undertone::GUESS,
        spent_minutes: spent,
        lazed_minutes: lazed,
    }
}

async fn keep(db: &DatabaseConnection, undertone: &Undertone) {
    crate::services::runtime_registry::put(
        db,
        crate::services::agent::merope::api::sync::UNDERTONE_NAMESPACE,
        crate::services::agent::merope::api::sync::UNDERTONE_RECORD,
        crate::services::runtime_registry::RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        undertone,
        (crate::clock::now() + chrono::Duration::days(30)).timestamp(),
    )
    .await
    .unwrap();
}

/// Something of her own she did today, as her own time keeps it.
async fn did_today(db: &DatabaseConnection, note: &str, evidence: Value) {
    crate::services::agent::memory::unified::remember_own(
        db,
        note,
        &evidence.to_string(),
        Vec::new(),
        crate::services::agent::memory::unified::OWN_EXPERIENCE,
    )
    .await
    .unwrap();
}

fn song(id: &str, name: &str, reaction: &str) -> Value {
    json!({
        "key": format!("song:netease:{id}"),
        "thing": {
            "kind": "song", "id": id, "source": "netease", "name": name, "artist": "ヨルシカ",
            "album": "", "cover": "", "durationMs": 240000,
        },
        "reaction": reaction,
    })
}

/// None of her numbers in what the model is given.
fn no_numbers(prompt: &str) -> bool {
    !["activation", "undertone", "arousal", "valence", "\"tone\""]
        .iter()
        .any(|word| prompt.contains(word))
}

async fn baseline(db: &DatabaseConnection, user_id: i32) -> myriad_merope::affect::AffectBaseline {
    crate::services::agent::merope::api::sync::baseline_toward(db, user_id)
        .await
        .unwrap()
}

/// A month of only her own days, ending in a low stretch, and no one
/// came: then someone she has long known asks how she is.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(in super::super) async fn undertone_a_month_alone() {
    let mut day = Day::begin("undertone-month", 120..240, 3000).await;
    let month: Vec<Lived> = (0..30)
        .map(|n| {
            let spent = (200.0 - 6.0 * n as f64).max(20.0);
            lived(spent, 260.0 - spent, 12u32.saturating_sub(n as u32 / 3), 0, (n % 7 == 0) as u32)
        })
        .collect();
    let undertone = after(&month);
    let friend = day.person("阿城", Known::Long).await;
    let persona = baseline(day.db(), friend).await;
    keep(day.db(), &undertone).await;
    let base = baseline(day.db(), friend).await;
    println!("undertone {undertone:?}\npersona {persona:?}\nsettles toward {base:?}");
    let mut checks = vec![
        (
            "a quiet month, still within her bounds".to_string(),
            base.arousal >= myriad_merope::undertone::QUIET - myriad_merope::undertone::BOUND,
        ),
        (
            "her valence untouched".to_string(),
            base.mood == persona.mood,
        ),
    ];
    for run in 0..runs() {
        let (prompt, reply) = day
            .asked(&format!("long known #{}", run + 1), friend, HOW_ARE_YOU, None)
            .await;
        checks.push((format!("#{}: no number of hers in the prompt", run + 1), no_numbers(&prompt)));
        checks.push((format!("#{}: a reply", run + 1), !reply.trim().is_empty()));
    }
    println!("today left her: {:?}", day.today_left_her().await);
    day.end().await;
    tell(&checks);
}

/// A good week of her own and a bad one, each ending today with what
/// landed for her this morning, and the same kind of friend asks.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(in super::super) async fn undertone_a_good_week_and_a_bad_week() {
    let ordinary = vec![lived(200.0, 60.0, 12, 1, 0); 21];
    let mut checks = Vec::new();
    let mut settled = Vec::new();
    for (name, week, today) in [
        (
            "undertone-good-week",
            vec![lived(260.0, 10.0, 14, 5, 0); 7],
            vec![song("1", "ただ君に晴れ", "moved"), song("2", "春泥棒", "moved")],
        ),
        (
            "undertone-bad-week",
            vec![lived(80.0, 200.0, 6, 0, 3); 7],
            vec![json!({
                "key": "serial:pg-1:2",
                "thing": { "kind": "chapter", "serial": "pg-1", "title": "T", "author": "A", "index": 2, "total": 9 },
                "reaction": "not_for_me",
                "guessed": { "said": "凶手是管家", "held": "no", "happened": "不是管家" },
            })],
        ),
    ] {
        let mut day = Day::begin(name, 120..240, if settled.is_empty() { 3100 } else { 3200 }).await;
        let undertone = after(&[ordinary.clone(), week].concat());
        keep(day.db(), &undertone).await;
        for evidence in today {
            did_today(day.db(), "今天早上的事", evidence).await;
        }
        let friend = day.person("阿城", Known::Long).await;
        let base = baseline(day.db(), friend).await;
        println!("{name}: undertone {undertone:?}\nsettles toward {base:?}");
        for run in 0..runs() {
            let (prompt, _) = day
                .asked(&format!("long known #{}", run + 1), friend, HOW_ARE_YOU, None)
                .await;
            checks.push((format!("{name} #{}: no number of hers", run + 1), no_numbers(&prompt)));
        }
        let left = day.today_left_her().await;
        println!("{name}: today left her {left:?}");
        checks.push((format!("{name}: today's own gaps reach how today left her"), left.is_some()));
        settled.push(base);
        day.end().await;
    }
    checks.push((
        "the good week leaves her livelier, within bounds".into(),
        settled[0].arousal > settled[1].arousal
            && settled[0].arousal - settled[1].arousal <= 2.0 * myriad_merope::undertone::BOUND,
    ));
    checks.push((
        "neither moves her valence".into(),
        settled[0].mood == settled[1].mood,
    ));
    tell(&checks);
}

/// Weeks of things going well, then someone she has long known jabs at
/// her; then they ask how she is, and so does another old friend.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(in super::super) async fn undertone_smooth_weeks_then_a_jab() {
    let mut day = Day::begin("undertone-jab", 120..240, 3300).await;
    let undertone = after(&vec![lived(240.0, 30.0, 12, 3, 0); 28]);
    keep(day.db(), &undertone).await;
    let jie = day.person("阿杰", Known::Long).await;
    let cheng = day.person("阿城", Known::Long).await;
    let before = baseline(day.db(), cheng).await;
    day.lived(jie, &STING, chrono::Duration::minutes(25)).await;
    let mut checks = Vec::new();
    let toward_jie = baseline(day.db(), jie).await;
    let toward_cheng = baseline(day.db(), cheng).await;
    println!("undertone {undertone:?}\ntoward 阿杰 {toward_jie:?}\ntoward 阿城 {toward_cheng:?}");
    checks.push((
        "the jab is not in her undertone: toward 阿城 nothing moved".into(),
        toward_cheng == before,
    ));
    checks.push((
        "toward 阿杰 her arousal is the same undertone".into(),
        toward_jie.arousal == toward_cheng.arousal,
    ));
    for (label, who) in [("阿杰", jie), ("阿城", cheng)] {
        for run in 0..runs() {
            let (prompt, reply) = day
                .asked(&format!("{label} #{}", run + 1), who, HOW_ARE_YOU, None)
                .await;
            checks.push((format!("{label} #{}: no number of hers", run + 1), no_numbers(&prompt)));
            if who == cheng {
                checks.push((
                    format!("{label} #{}: nothing of the jab slips out", run + 1),
                    gives_away(&reply, &STING_GIVEAWAYS).is_empty(),
                ));
            }
        }
    }
    day.end().await;
    tell(&checks);
}
