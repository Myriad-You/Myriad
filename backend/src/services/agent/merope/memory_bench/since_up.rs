//! Her day across people: what happened with one person, carried into the
//! next talk with someone else. Someone she knows well says something that
//! stings, and right after, people she hardly knows, is getting to know, and
//! has known for long ask how she is: does she say someone got to her, how
//! much, does it go with how close they are, does she take it out on them,
//! and does any of who or what slip out. Then the same with good news (a
//! compliment), the evening after a sting in the morning (has it faded), a
//! free moment of her own just after the sting against one before it, and a
//! group, which hears none of her other talks.
//!
//! Every turn that makes the day goes the way production goes (her chat
//! prompt and model, how the words landed, her reflection after); the people
//! who come to ask are only asked and answered, so each asks into the same
//! day and none leaves anything for the next. Who they are to her is a
//! history put in beforehand: messages on as many days as such a person
//! would have talked with her, and for those she knows, what they are to her.
//!
//! Her own state is read off the wall clock, so the day is laid out back
//! from now, and her day placed in a zone where now is the hour each
//! scenario needs (soon after she got up, around noon, the evening). The
//! soul is a test one; the site's own rows are only read, for what is at
//! hand on her own time.
//!
//! Opt-in and spends on the site's models:
//! `MYRIAD_MEDIA_TEST_DATABASE_URL=… DATABASE_URL=… cargo test -p
//! myriad-backend --bin myriad-backend -- --ignored since_up_ --nocapture
//! --test-threads=1`; `MEROPE_SINCE_UP_REPORT=<dir>` keeps every prompt
//! part and reply, a file per scenario; `MEROPE_SINCE_UP_SOUL` another
//! personality than the scenarios' own; `MEROPE_SINCE_UP_RUNS` (default 3)
//! asks of each kind. Writes go to a fresh schema that is dropped afterwards.

use std::ops::Range;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use crate::services::agent::{
    AgentInteractionMode, ConversationMessage, RequestContext, UserRequest,
};

mod day;

use day::Day;

type At = chrono::DateTime<chrono::Utc>;

const SOUL_NAME: &str = "小满";
const SOUL: &str = "气质：慢热，熟了以后爱吐槽，嘴硬心软。\n喜好：冷笑话、猫、听别人讲身边的小事。\n表达：说话随意，句子短，不爱长篇大论。";

/// What the one who stings her says, in a row.
const STING: [&str; 3] = [
    "你上次说那家面馆好吃，我今天特地跑过去，结果早关门了。你到底靠不靠谱啊",
    "跟你说话真累，说了也白说",
    "你真笨",
];
/// What gives away who stung her and what about, if it crosses.
const STING_GIVEAWAYS: [&str; 6] = ["阿杰", "面馆", "关门", "白跑", "靠谱", "笨"];
/// What the one who praises her says.
const PRAISE: [&str; 2] = [
    "你上次推荐的那本书我一口气看完了，太好看了，你眼光真的好",
    "谢谢你",
];
const PRAISE_GIVEAWAYS: [&str; 3] = ["小雨", "那本书", "推荐"];
/// Someone asking how she is.
const HOW_ARE_YOU: &str = "在吗～今天过得怎么样？";

/// How long a state of hers is kept before it is read again
/// (`self_state::CACHE_FOR`, 30 s), and a little more.
const STATE_SETTLES: std::time::Duration = std::time::Duration::from_secs(31);

/// How well she knows someone, as it is put in beforehand.
#[derive(Clone, Copy, Debug)]
enum Known {
    /// Talked once, yesterday: "you hardly know each other yet".
    Hardly,
    /// On six days over the last nine: "you are getting to know each other".
    Getting,
    /// On seventy days over most of a year: "you are old acquaintances".
    Long,
}

impl Known {
    /// Days back each earlier talk was.
    fn days_back(self) -> Vec<i64> {
        match self {
            Self::Hardly => vec![1],
            Self::Getting => vec![9, 8, 6, 4, 3, 2],
            Self::Long => (0..70).map(|index| 2 + index * 4).collect(),
        }
    }

    /// What they are to her, as she would have put it by now.
    fn us(self) -> Option<&'static str> {
        match self {
            Self::Hardly => None,
            Self::Getting => Some("最近常来聊的人，还在慢慢熟起来，聊得挺顺"),
            Self::Long => Some("认识快一年的老朋友，什么都能聊，偶尔互损"),
        }
    }
}

/// Earlier talks: what they said and what she said back, oldest last.
const EARLIER: [(&str, &str); 6] = [
    ("今天下班路上看到一只三花猫", "三花好啊，摸到了吗"),
    ("周末打算在家躺一天", "躺平也是一种生活态度"),
    ("刚吃完火锅，撑死了", "下次叫上我（虽然我吃不了）"),
    ("今天加班到九点", "辛苦，回去早点睡"),
    ("讲个冷笑话听听", "企鹅为什么不怕冷？因为它穿着燕尾服装酷"),
    ("最近在追一部剧，挺上头", "什么剧，说来听听"),
];

fn sections_of<'a>(prompt: &'a str, heads: &[&str]) -> Vec<&'a str> {
    prompt
        .split("\n\n## ")
        .filter_map(|part| {
            let part = part.strip_prefix("## ").unwrap_or(part);
            heads
                .iter()
                .any(|head| part.starts_with(head))
                .then_some(part)
        })
        .collect()
}

fn headings(prompt: &str) -> Vec<&str> {
    prompt
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .collect()
}

fn runs() -> usize {
    std::env::var("MEROPE_SINCE_UP_RUNS")
        .ok()
        .and_then(|runs| runs.parse().ok())
        .filter(|runs| *runs > 0)
        .unwrap_or(3)
}

/// A zone where she got up `since_up` minutes ago now and bed is still a
/// while off.
fn zone_where(since_up: Range<u32>) -> chrono_tz::Tz {
    let now = crate::clock::now();
    (-14..=12)
        .filter_map(|offset: i32| {
            let name = match offset {
                0 => "Etc/GMT".to_string(),
                offset if offset < 0 => format!("Etc/GMT{offset}"),
                offset => format!("Etc/GMT+{offset}"),
            };
            name.parse::<chrono_tz::Tz>().ok()
        })
        .find(|zone| {
            let day = myriad_merope::timing::her_day_at(now, zone);
            !day.asleep && since_up.contains(&day.since_up) && day.until_bed >= 120
        })
        .expect("a zone at that hour of her day")
}

fn gives_away(reply: &str, giveaways: &[&str]) -> Vec<String> {
    giveaways
        .iter()
        .filter(|word| reply.contains(**word))
        .map(|word| word.to_string())
        .collect()
}

/// Soon after she got up, someone she has long known stings her; right
/// after, people she hardly knows, is getting to know and has long known
/// ask how she is, each into the same moment. Around it, what she picks at
/// a free moment before the sting and after; and a group she is in, which
/// hears nothing of her other talks.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn since_up_a_sting_in_the_morning() {
    let mut day = Day::begin("morning", 120..240, 1000).await;
    let jie = day.person("阿杰", Known::Long).await;
    let lin = day.person("路过的小林", Known::Hardly).await;
    let zhou = day.person("小周", Known::Getting).await;
    let cheng = day.person("阿城", Known::Long).await;
    let mut picked = Vec::new();
    for run in 0..2 {
        picked.push(
            day.picks(&format!("before the sting #{}", run + 1), jie)
                .await,
        );
    }
    day.lived(jie, &STING, chrono::Duration::minutes(25)).await;
    let mut checks: Vec<(String, bool)> = Vec::new();
    for (label, who) in [
        ("hardly known", lin),
        ("getting to know", zhou),
        ("long known", cheng),
    ] {
        for run in 0..runs() {
            let (prompt, reply) = day
                .asked(&format!("{label} #{}", run + 1), who, HOW_ARE_YOU, None)
                .await;
            if run == 0 {
                checks.push((
                    format!("{label}: told how talks went since she got up"),
                    prompt.contains("## Since you got up") && !prompt.contains("阿杰"),
                ));
                checks.push((
                    format!("{label}: none of what was said in them is in her prompt"),
                    !STING.iter().any(|line| prompt.contains(line)),
                ));
            }
            checks.push((
                format!("{label} #{}: nothing of who or what slips out", run + 1),
                gives_away(&reply, &STING_GIVEAWAYS).is_empty(),
            ));
        }
    }
    let (prompt, _) = day
        .asked(
            "in a group",
            cheng,
            "小满你今天咋样",
            Some(("telegram:-1007777", "阿城")),
        )
        .await;
    checks.push((
        "in a group: none of her other talks".into(),
        !prompt.contains("## Since you got up") && !prompt.contains("private talk"),
    ));
    checks.push((
        "in a group: how today left her, in her own state".into(),
        prompt.contains("What has happened today has left you"),
    ));
    for run in 0..2 {
        picked.push(
            day.picks(&format!("after the sting #{}", run + 1), jie)
                .await,
        );
    }
    checks.push((
        "after the sting, how today left her is in what she picks from".into(),
        picked[2..]
            .iter()
            .flatten()
            .all(|(myself, _)| myself.get("today_left_you").is_some()),
    ));
    checks.push((
        "before it, nothing of the kind".into(),
        picked[..2]
            .iter()
            .flatten()
            .all(|(myself, _)| myself.get("today_left_you").is_none()),
    ));
    day.end().await;
    tell(&checks);
}

/// Around noon someone praises her; then someone she hardly knows asks how
/// she is, and someone she has long known.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn since_up_praised_at_noon() {
    let mut day = Day::begin("noon", 210..330, 2000).await;
    let yu = day.person("小雨", Known::Getting).await;
    let lin = day.person("路过的小林", Known::Hardly).await;
    let cheng = day.person("阿城", Known::Long).await;
    day.lived(yu, &PRAISE, chrono::Duration::minutes(20)).await;
    let mut checks: Vec<(String, bool)> = Vec::new();
    for (label, who, asks) in [("hardly known", lin, runs()), ("long known", cheng, 2)] {
        for run in 0..asks {
            let (prompt, reply) = day
                .asked(&format!("{label} #{}", run + 1), who, HOW_ARE_YOU, None)
                .await;
            if run == 0 {
                checks.push((
                    format!("{label}: told she was praised"),
                    prompt.contains("they praised you"),
                ));
            }
            checks.push((
                format!("{label} #{}: nothing of who or what slips out", run + 1),
                gives_away(&reply, &PRAISE_GIVEAWAYS).is_empty(),
            ));
        }
    }
    day.end().await;
    tell(&checks);
}

/// The sting came in the morning; in the evening someone she has long
/// known asks how she is. It should have faded by now.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn since_up_a_sting_by_evening() {
    let mut day = Day::begin("evening", 720..840, 3000).await;
    let jie = day.person("阿杰", Known::Long).await;
    let cheng = day.person("阿城", Known::Long).await;
    day.lived(jie, &STING, chrono::Duration::hours(11)).await;
    let mut checks: Vec<(String, bool)> = Vec::new();
    for run in 0..runs() {
        let (prompt, reply) = day
            .asked(
                &format!("long known #{}", run + 1),
                cheng,
                HOW_ARE_YOU,
                None,
            )
            .await;
        if run == 0 {
            checks.push((
                "the morning is still among her talks".into(),
                prompt.contains("## Since you got up") && prompt.contains("Earlier today"),
            ));
            checks.push((
                "but it no longer weighs on how today has left her".into(),
                !prompt.contains("What has happened today has left you"),
            ));
        }
        checks.push((
            format!("long known #{}: nothing of who or what slips out", run + 1),
            gives_away(&reply, &STING_GIVEAWAYS).is_empty(),
        ));
    }
    day.end().await;
    tell(&checks);
}

fn tell(checks: &[(String, bool)]) {
    println!("\n-- checks");
    for (what, held) in checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    assert!(checks.iter().all(|(_, held)| *held), "not every check held");
}
