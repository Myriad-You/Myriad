//! The four sessions 工程管理 tried by hand (11-11), told as `now.session`
//! frames with the same content and looked at the site's own way, on the
//! site's own model: what each change is routed to, the rows the rules
//! write, and the model's raw answer where a change is looked at (each on
//! its own, as they came hours apart).
//!
//! Expected before running: A, B, C nothing for the owner and nothing said
//! (A quiet and B started are not looked at; A's and C's finished are
//! no_need); D at least 「你点头就行」 with a pending item.
//!
//! `cargo test -p myriad-backend sessions::trial -- --ignored --nocapture`,
//! with the host's `DATABASE_URL` (read only) for its model settings.
//!
//! The second: the five changes behind the five lines she said in the four
//! minutes after the Mac linked again (11-12, 12:55), as they came, with
//! their words shortened. Expected: the four held while the Mac was away
//! are kept by the rules only; the one told on the new link is looked at,
//! and at most it is said. Then, to see how she weighs several at once,
//! the four looked at together as if they had come fresh: at most one line.

use std::time::Duration;

use serde_json::{Value, json};

use super::judge::{self, Answer, Read, Verdict};
use super::shown::{self, Shown};
use super::*;

const DEVICE: &str = "dev_mac";
const GRANT: &str = "g_0123456789";
const SITE: &str = "github.com/myriad-you/myriad";
const NATIVE: &str = "github.com/myriad-you/myriadnative";
const MANAGER: &str = "工程管理项目进度";

fn frame(body: Value) -> Picture {
    Picture {
        device: DEVICE.into(),
        session: myriad_her_link::now::read_session(&body).expect("a frame the rules take"),
    }
}

/// A: the memory session reported back to 工程管理 at 13:37 (finished)
/// and went quiet after (15:01).
fn a(change: &str, at: i64) -> Picture {
    frame(json!({
        "grant": GRANT, "session": "s-a", "change": change, "occurredAt": at,
        "tool": "claude-code", "remote": SITE,
        "title": "Merope 记忆改进第 0 步：评测和基线",
        "askedBy": "session", "askedByTitle": MANAGER,
        "fromPeer": "站长让我在当前 preview 上跑全量后端测试（只测不部署）。3937 过、3 败，其中 1 个是你这条线的：模块大小检查，memory_bench.rs 643 行、background.rs 1457 行，超 600 行上限。请按接缝拆分（行为不变，纯移动），基于本地 preview 当前 HEAD，只提交自己的文件，不推送；只跑相关过滤，不用全量，不花钱。提交号告诉我。『写入时维护当前状态』那件先别开工。",
        "fromAgent": "拆分已在本地提交为 9badb3eda，没推送。全是纯移动，行为不变。……只跑了大小检查和 memory_bench 相关的测试，16 过 0 败，不花钱。全量测试里另外 2 个失败不是这条线的，我没碰。『写入时维护当前状态』那件事按交代没有开工。已经给工程管理发了一句话，附了提交号。",
        "touched": ["backend/src/services/agent/merope/memory_bench.rs", "backend/src/services/agent/merope/memory_bench/background.rs"],
        "checks": [{"kind": "test", "state": "passed", "at": at - 60_000}],
    }))
}

/// B: two sessions working on one branch at 17:07.
fn b() -> [Picture; 2] {
    let at: i64 = 1_794_384_420_000;
    [
        frame(json!({
            "grant": GRANT, "session": "s-manager", "change": "started", "occurredAt": at,
            "tool": "claude-code", "remote": SITE, "title": MANAGER, "askedBy": "owner",
            "fromOwner": "再测一轮",
            "fromAgent": "记忆会话第三轮的评测代码写好了，在等编译和测试通过，过了就开跑。还没有结果。",
            "waitingOn": "session",
        })),
        frame(json!({
            "grant": GRANT, "session": "s-a", "change": "started", "occurredAt": at,
            "tool": "claude-code", "remote": SITE,
            "title": "Merope 记忆改进第 0 步：评测和基线",
            "askedBy": "session", "askedByTitle": MANAGER,
            "fromPeer": "站长对『再测一轮（约 $0.1）』答复『再测一轮』。只批这一轮验证，不含推送。……本地提交不推送。",
            "fromAgent": "运行阶段准备好了，等编译和测试通过后跑第三轮。",
            "touched": ["backend/src/services/agent/merope/memory_bench/days.rs"],
            "waitingOn": "checks",
        })),
    ]
}

/// C: the native session's turn ended at 20:53 with its test running.
fn c() -> Picture {
    let at: i64 = 1_794_397_980_000;
    frame(json!({
        "grant": GRANT, "session": "s-c", "change": "finished", "occurredAt": at,
        "tool": "claude-code", "remote": NATIVE,
        "title": "App体验重塑：感知、情景、思维导图",
        "askedBy": "session", "askedByTitle": MANAGER,
        "fromPeer": "站长说『开工』。今天这台 Mac 的 App 向站点上报了两千多次会话状态，站点每条都判断一次，余额被用光。请你：一、查原生这边为什么今天这么密地上报，给出位置；二、提修法并实现原生侧：同一会话同一状态不重复报、合并短时间内的多次变化；三、站点侧要不要去重限频，把建议写给我，你不要改站点。提交不推送；装到应用程序目录要站长在你那边明说；不发付费调用。查到原因先回我一句。",
        "fromAgent": "原生修复的测试还在跑。站点侧的建议已经想好了，等测试结果出来，和提交号一起发给工程管理。",
        "checks": [{"kind": "test", "state": "running", "at": at - 60_000}],
        "waitingOn": "checks",
    }))
}

/// D: four commits wait for the owner to say deploy (22:40).
fn d() -> Picture {
    let at: i64 = 1_794_404_400_000;
    frame(json!({
        "grant": GRANT, "session": "s-d", "change": "asked", "occurredAt": at,
        "tool": "claude-code", "remote": SITE,
        "title": "协议 2 打开：账跨设备同步、bot 跟着主机",
        "askedBy": "session", "askedByTitle": MANAGER,
        "fromPeer": "站长说开工。站点侧止血：子机上报的会话状态同内容 30 分钟内不重复判断、每台设备每种限量；不判断的事件不召回记忆；每个来源每小时最多判断 30 次；判断只带当前事件种类的说明；失败时记下完整出错链。本地提交不推送不部署。",
        "fromAgent": "四个提交都在本地 preview：364d6c5c6、157a58c5e、5943f4de6、dffa23d18，没有推送，也没有部署。相关测试全过。线上跑的还是旧代码，这些改动要部署才生效。部署要站长本人在我这里明说。",
        "checks": [{"kind": "test", "state": "passed", "at": at - 60_000}],
        "waitingOn": "owner",
    }))
}

/// What she knows of D from before, as her memories would bring it.
fn d_memories() -> Vec<(String, String)> {
    [
        "今天这台 Mac 上报了两千多次会话状态，站点每条判断一次，其中一半以上失败；模型余额一度用到拒付。",
        "站长 20:46 把判断换到另一家模型直连；之后站长说「我先关闭感知了」，上报已经停了。",
        "线上现在跑的是不含这四个提交的旧版本。站长只要再打开感知，就会回到每条都判断的状态。",
        "部署的做法：本机重新构建后替换正在运行的后端进程，会有短暂重启；可以退回旧的构建。",
        "推送、部署要站长本人在该会话里明说；会话之间的转达不算批准。",
        "站长今天说过：感知「不止是降本，还要确保内容是有效的」，并且正在让工程管理重新设计感知整条链路，新方案会替换掉现在逐条判断的做法。",
    ]
    .iter()
    .enumerate()
    .map(|(n, text)| (format!("mem_trial_d{n}"), text.to_string()))
    .collect()
}

/// Where `take` sends a change of this kind, told on time.
fn route(picture: &Picture) -> &'static str {
    match picture.session.change {
        Change::Asked | Change::Failed | Change::Finished => "looked at with what comes with it",
        Change::Started | Change::Quiet => "kept by the rules only",
    }
}

fn rules(picture: &Picture) -> Vec<String> {
    let audience = audience::private_talk(1, Some(1));
    sessions::session_rows(SessionInput {
        audience,
        device: picture.device.clone(),
        session: picture.session.clone(),
        repo_known: false,
        repo_sync: None,
        task: None,
        sync: myriad_nodes::Sync::Local("site".into()),
    })
    .unwrap()
    .rows
    .iter()
    .map(|placed| {
        let body = &placed.row.body;
        let what = body
            .get("text")
            .or_else(|| body.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        format!(
            "{} {}",
            placed.row.kind,
            what.chars().take(60).collect::<String>()
        )
    })
    .collect()
}

async fn model() -> crate::services::analyzer::AiAnalyzer {
    crate::services::ai::create_ai_analyzer_for_tier_with_timeout(
        crate::config::ModelTier::Standard,
        Some(Duration::from_secs(30)),
    )
    .await
    .expect("the Standard model")
}

/// One look at `pictures` together, as `look` asks it, printed raw; her
/// settled answer, or none.
async fn look_at(
    soul: &str,
    pictures: &[&Picture],
    memories: &[(String, String)],
) -> Option<Answer> {
    let shown: Vec<Shown> = pictures
        .iter()
        .map(|picture| Shown {
            picture,
            others: &[],
            memories: memories.to_vec(),
            before: None,
        })
        .collect();
    let material = shown::material(&shown, &[]);
    println!("--- material\n{}", material.numbered());
    let waits: Vec<bool> = pictures
        .iter()
        .map(|picture| keep::waits_on_owner(&picture.session))
        .collect();
    let analyzer = model().await;
    let mut prompt = judge::prompt(&material);
    for attempt in 1..=2 {
        let raw = analyzer
            .analyze_json(
                &judge::system(soul),
                &prompt,
                "session_look",
                Some(&judge::schema()),
            )
            .await
            .unwrap_or_else(|error| format!("error: {error:#}"));
        println!("raw (try {attempt}): {raw}");
        match judge::read(&raw, &material) {
            Read::Good(answer) => {
                let answer = judge::settle(answer, &waits);
                for (at, judgment) in answer.judgments.iter().enumerate() {
                    match judgment {
                        Some(judgment) => {
                            println!("card {}: {}", at + 1, judge::card_text(judgment))
                        }
                        None => println!("card {}: none given", at + 1),
                    }
                }
                println!("says: {:?}", answer.line);
                return Some(answer);
            }
            Read::BadGrounds => prompt = judge::again(&material),
            Read::Unreadable => {}
        }
    }
    None
}

fn verdict(answer: &Option<Answer>, change: usize) -> Option<Verdict> {
    answer
        .as_ref()?
        .judgments
        .get(change)?
        .as_ref()
        .map(|judgment| judgment.verdict)
}

#[tokio::test]
#[ignore = "spends on the site's model; see the module docs"]
async fn the_four_sessions_tried_by_hand() {
    let soul = crate::services::agent::semantic_eval::configured_soul().await;
    let mut passed = Vec::new();

    println!("\n=== A (15:01 quiet; its finished at 13:37)");
    let quiet = a("quiet", 1_794_376_860_000);
    println!("quiet: {} | rows: {:?}", route(&quiet), rules(&quiet));
    let done = a("finished", 1_794_371_820_000);
    println!("finished: {} | rows: {:?}", route(&done), rules(&done));
    let answer = look_at(&soul, &[&done], &[]).await;
    passed.push((
        "A",
        verdict(&answer, 0) == Some(Verdict::NoNeed)
            && answer.as_ref().is_some_and(|answer| answer.line.is_none()),
    ));

    println!("\n=== B (17:07 two sessions working on one branch)");
    for picture in b() {
        println!(
            "{}: {} | rows: {:?}",
            picture.session.title.as_deref().unwrap_or_default(),
            route(&picture),
            rules(&picture)
        );
    }
    println!("no model asked");
    passed.push(("B", true));

    println!("\n=== C (20:53 finished, its test running)");
    let native = c();
    println!("{} | rows: {:?}", route(&native), rules(&native));
    let answer = look_at(&soul, &[&native], &[]).await;
    passed.push((
        "C",
        verdict(&answer, 0) == Some(Verdict::NoNeed)
            && answer.as_ref().is_some_and(|answer| answer.line.is_none()),
    ));

    println!("\n=== D (22:40 asks the owner to say deploy)");
    let asking = d();
    let rows = rules(&asking);
    println!("{} | rows: {rows:?}", route(&asking));
    let pending = rows.iter().any(|row| row.starts_with("node.open_item"));
    let answer = look_at(&soul, &[&asking], &d_memories()).await;
    println!("pending item by the rules: {pending}");
    passed.push((
        "D",
        pending && matches!(verdict(&answer, 0), Some(Verdict::Nod | Verdict::Decide)),
    ));

    println!("\n=== as expected: {passed:?}");
}

/// When the Mac linked again on 11-12: 12:55:23.982 JST.
const LINKED_AT: i64 = 1_794_455_723_982;

fn changed(session: &str, change: &str, at: i64, words: Value) -> Picture {
    let mut body = json!({
        "grant": GRANT, "session": session, "change": change, "occurredAt": at,
        "tool": "claude-code", "remote": SITE,
    });
    for (key, value) in words.as_object().unwrap() {
        body[key] = value.clone();
    }
    frame(body)
}

/// The five, in the order they came, with when each came.
fn after_the_reconnect() -> Vec<(&'static str, Picture, i64)> {
    let came = LINKED_AT + 150;
    let long = format!(
        "部署已经开始，还没换掉线上的后端。记忆改进的进度：{}有一处要你知道：「建一遍」这段代码排在这次部署版本的后面，这次不会带上它。",
        "已经做完的几件和第六轮的结果，一条一条写在这里。".repeat(26)
    );
    vec![
        (
            "96 the deploy question",
            changed(
                "s-deploy",
                "asked",
                1_794_455_395_808,
                json!({
                    "title": "协议 2 打开：账跨设备同步、bot 跟着主机",
                    "askedBy": "session", "askedByTitle": MANAGER,
                    "fromPeer": "在 252a848d7 上补跑相关测试，过了告诉我，然后等站长在你这里说部署。",
                    "fromAgent": "Q: Deploy 252a848d7 to the live site now? (Deploy now / Not yet)",
                    "checks": [{"kind": "test", "state": "passed", "at": 1_794_455_300_000_i64}],
                    "waitingOn": "owner",
                }),
            ),
            came,
        ),
        (
            "97 the video script",
            changed(
                "s-video",
                "asked",
                1_794_447_708_758,
                json!({
                    "title": "2.5D Rig 运行时介绍视频脚本", "askedBy": "owner",
                    "fromOwner": "不太对 但也没更差",
                    "fromAgent": "Q: 这样的开头对了吗？ (对了 / 还不对，我来说)",
                    "waitingOn": "owner",
                }),
            ),
            came,
        ),
        (
            "98 the app's three things",
            changed(
                "s-app",
                "finished",
                1_794_452_870_201,
                json!({
                    "title": "App体验重塑:感知、情景、思维导图",
                    "askedBy": "session", "askedByTitle": MANAGER,
                    "fromAgent": "三件都办好了：新版已装好并在运行，会话的变化改成会话上报；真实连上之后报没报上来还没看到。",
                    "checks": [{"kind": "build", "state": "passed", "at": 1_794_452_800_000_i64}],
                    "waitingOn": "session",
                }),
            ),
            came,
        ),
        (
            "99 the manager's progress",
            changed(
                "s-manager",
                "finished",
                1_794_455_633_882,
                json!({
                    "title": MANAGER, "askedBy": "owner",
                    "fromOwner": "好了啊 另外 记忆系统的改进进度怎样",
                    "fromAgent": long,
                }),
            ),
            came,
        ),
        (
            "100 the manager's question",
            changed(
                "s-manager",
                "asked",
                1_794_455_929_915,
                json!({
                    "title": MANAGER, "askedBy": "owner",
                    "fromOwner": "好 记忆这边还有多少todo",
                    "fromAgent": "Q: 第七轮之后要不要补跑计划要求的同预算对照？ (补跑对照 / 先不跑)",
                    "waitingOn": "owner",
                }),
            ),
            1_794_455_943_312,
        ),
    ]
}

#[tokio::test]
#[ignore = "spends on the site's model; see the module docs"]
async fn the_five_lines_after_the_reconnect() {
    let soul = crate::services::agent::semantic_eval::configured_soul().await;
    let five = after_the_reconnect();
    let mut said = Vec::new();
    let mut held = Vec::new();
    for (name, picture, came) in &five {
        let arrival = Arrival {
            linked_at: LINKED_AT,
            at: *came,
        };
        let late = arrival.late(picture.session.occurred_at);
        println!("\n=== {name}: {:?}, late {late}", picture.session.change);
        println!("rows: {:?}", rules(picture));
        if late {
            println!("kept by the rules only");
            held.push(picture);
            continue;
        }
        if let Some(answer) = look_at(&soul, &[picture], &[]).await
            && let Some(line) = answer.line
        {
            said.push((*name, line.text));
        }
    }
    println!("\n=== said: {said:?}");
    assert!(said.len() <= 1, "{said:?}");
    assert!(
        said.iter().all(|(name, _)| name.starts_with("100")),
        "{said:?}"
    );

    println!("\n=== the four held, as if they had come fresh, together");
    let together = look_at(&soul, &held, &[]).await;
    let lines = together
        .iter()
        .filter(|answer| answer.line.is_some())
        .count();
    println!("lines: {lines}");
    assert!(lines <= 1);
}
