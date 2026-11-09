//! Weeks with someone, gone over again and again: one person talks with her
//! over six weeks (tests/merope/memory-weeks-zh.json, made up), each
//! session written down the settle way as production does (short notes
//! line by line, the session gone over once it is over, what lasts kept
//! beside what she already knows), and at the end of each week the same
//! questions asked again. What is measured is whether going over talk again
//! and again keeps what she knows true: what was said once in the first
//! week and never again should still be there in the sixth, and what
//! changed should be how it is now.
//!
//! Each probe has its answer as of each week (`versions`); `held` are words
//! one lasting memory should still carry, checked in code beside the
//! graded answers. Questions are answered from what she remembers only
//! (`cued`) and also scrolling back through the chat as production does
//! (`wide`): the difference is what her memory lost that the chat still has.
//! Short notes are left out: what is asked is what lasted.
//!
//! Opt-in and spends on the site's models:
//! `MEROPE_MEMORY_WEEKS_REPORT=<new file> MYRIAD_MEDIA_TEST_DATABASE_URL=…
//! DATABASE_URL=… cargo test -p myriad-backend --bin myriad-backend --
//! --ignored her_memory_over_weeks --nocapture`; `MEROPE_MEMORY_BENCH_TIMES`
//! runs the weeks that many times, each by someone new, for pass^n at each
//! week. Writes go to a fresh schema that is dropped afterwards.
//!
//! `MEROPE_MEMORY_WEEKS=<file>` runs other weeks of the same shape instead:
//! tests/merope/memory-chain-zh.json has a few things said again and again
//! differently, each changed three times or more, its held words the parts
//! that never changed beside how it is now, so what is measured is whether
//! being replaced again and again keeps what still holds.

use super::*;

#[derive(Deserialize)]
struct Weeks {
    sessions: Vec<Day>,
    checkpoints: Vec<Checkpoint>,
    probes: Vec<Probe>,
}

#[derive(Deserialize)]
struct Day {
    date: String,
    turns: Vec<Turn>,
}

/// Where the questions are asked: after session `after`, on `date`.
#[derive(Deserialize)]
struct Checkpoint {
    after: usize,
    date: String,
}

#[derive(Deserialize, Clone)]
struct Probe {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    question: String,
    versions: Vec<Version>,
}

/// The answer from session `from` on, until the next version.
#[derive(Deserialize, Clone)]
struct Version {
    from: usize,
    answer: String,
    #[serde(default)]
    held: Vec<String>,
}

impl Probe {
    /// Its answer as of session `after`; none before it was first told.
    fn as_of(&self, after: usize) -> Option<&Version> {
        self.versions
            .iter()
            .rev()
            .find(|version| version.from <= after)
    }

    /// Told once and never changed: what should simply stay.
    fn stable(&self) -> bool {
        self.versions.len() == 1
    }

    fn question(&self, version: &Version, date: &str) -> Question {
        Question {
            question_id: self.id.clone(),
            question_type: self.kind.clone(),
            question: self.question.clone(),
            answer: json!(version.answer),
            question_date: date.to_string(),
            haystack_dates: Vec::new(),
            haystack_sessions: Vec::new(),
            haystack_venues: Vec::new(),
            question_venue: None,
            question_speaker: None,
            memories: Vec::new(),
            category: None,
            pair: None,
            background_sessions: Vec::new(),
        }
    }
}

/// The six weeks, or `MEROPE_MEMORY_WEEKS` when set.
fn weeks() -> Weeks {
    match std::env::var("MEROPE_MEMORY_WEEKS") {
        Ok(path) => serde_json::from_str(&std::fs::read_to_string(&path).expect("weeks file"))
            .expect("memory weeks"),
        Err(_) => serde_json::from_str(SIX_WEEKS).expect("memory weeks"),
    }
}

const SIX_WEEKS: &str = include_str!("../../../../../../tests/merope/memory-weeks-zh.json");
const CHAIN: &str = include_str!("../../../../../../tests/merope/memory-chain-zh.json");

/// The weeks read as the run needs: sessions in order, each checkpoint
/// after its session, each probe's versions in order and asked by the end,
/// a rule for each, and every held word said by then.
#[test]
fn the_weeks_read_as_the_run_needs() {
    for weeks in [SIX_WEEKS, CHAIN] {
        reads_as_the_run_needs(&serde_json::from_str(weeks).expect("memory weeks"));
    }
}

/// The replacing weeks are what they are for: things changed three times
/// or more, each version still holding a word that never changed, beside
/// some told once and left alone.
#[test]
fn the_replacing_weeks_change_things_again_and_again() {
    let weeks: Weeks = serde_json::from_str(CHAIN).unwrap();
    let chains: Vec<&Probe> = weeks
        .probes
        .iter()
        .filter(|probe| probe.versions.len() >= 4)
        .collect();
    assert!(chains.len() >= 5);
    assert!(weeks.probes.iter().any(Probe::stable));
    for probe in chains {
        let kept: Vec<&String> = probe.versions[0]
            .held
            .iter()
            .filter(|word| {
                probe
                    .versions
                    .iter()
                    .all(|version| version.held.contains(word))
            })
            .collect();
        assert!(!kept.is_empty(), "{}", probe.id);
        // What never changed is said once, so only memory carries it on.
        for word in kept {
            let saying: usize = weeks
                .sessions
                .iter()
                .filter(|day| {
                    day.turns
                        .iter()
                        .any(|turn| turn.role == "user" && turn.content.contains(word.as_str()))
                })
                .count();
            assert_eq!(saying, 1, "{} {word}", probe.id);
        }
    }
}

fn reads_as_the_run_needs(weeks: &Weeks) {
    let dates: Vec<_> = weeks
        .sessions
        .iter()
        .map(|day| date_of(&day.date))
        .collect();
    assert!(dates.windows(2).all(|pair| pair[0] < pair[1]));
    let mut last = None;
    for checkpoint in &weeks.checkpoints {
        assert!(checkpoint.after < weeks.sessions.len());
        assert!(date_of(&checkpoint.date) > dates[checkpoint.after]);
        assert!(last.is_none_or(|last| last < checkpoint.after));
        last = Some(checkpoint.after);
    }
    let end = weeks.checkpoints.last().unwrap().after;
    for probe in &weeks.probes {
        assert!(!probe.versions.is_empty(), "{}", probe.id);
        assert!(
            probe
                .versions
                .windows(2)
                .all(|pair| pair[0].from < pair[1].from),
            "{}",
            probe.id
        );
        assert!(probe.versions[0].from <= end, "{}", probe.id);
        let question = probe.question(&probe.versions[0], "");
        // Graded by its own rule, or the default for a plain recall.
        assert!(
            prompts()["grade"]["rules"][rule_key(&question)].is_string()
                || probe.kind == "single-session-user",
            "{}",
            probe.id
        );
        for version in &probe.versions {
            let said: String = weeks.sessions[..=version.from]
                .iter()
                .flat_map(|day| &day.turns)
                .filter(|turn| turn.role == "user")
                .map(|turn| turn.content.as_str())
                .collect();
            for word in &version.held {
                assert!(said.contains(word.as_str()), "{} {word}", probe.id);
            }
        }
    }
}

/// One probe asked at one checkpoint of one run.
struct Asked {
    round: usize,
    checkpoint: usize,
    probe: Probe,
    answer: String,
    /// The held words of the version asked.
    held_words: Vec<String>,
    cued: Answered,
    wide: Answered,
    /// Whether one lasting memory still carries its held words; none when
    /// it has none.
    held: Option<bool>,
}

/// What she keeps for good at one checkpoint of one run.
struct Kept {
    round: usize,
    checkpoint: usize,
    memories: Vec<String>,
    /// Retired so far, by why.
    retired: BTreeMap<String, i64>,
    /// Each session since the last checkpoint: its short notes and what of
    /// them lasted, to tell a note never taken from one not kept.
    sessions: Vec<Value>,
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
async fn her_memory_over_weeks() {
    let report_path = std::env::var("MEROPE_MEMORY_WEEKS_REPORT").expect("report path");
    assert!(
        !std::path::Path::new(&report_path).exists(),
        "report must not exist"
    );
    let weeks = weeks();
    let config_db = super::super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
    embedding_as_asked().await;
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("judgment model");
    let lite = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("Lite model")
    .with_light_thinking();
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "memory_weeks").await;
    let db = isolated.db.clone();
    let times = longmemeval::times();
    let runs = futures::future::join_all(
        (0..times).map(|round| run_weeks(&db, &judge, &lite, &weeks, round)),
    )
    .await;
    isolated.drop().await;
    let (asked, kept): (Vec<_>, Vec<_>) = runs.into_iter().unzip();
    let asked: Vec<Asked> = asked.into_iter().flatten().collect();
    let kept: Vec<Kept> = kept.into_iter().flatten().collect();
    let mut report = report(&weeks, times, &asked, &kept);
    report["spent"] = crate::services::ai_cost_ledger::spent::take();
    for line in report["weeks"].as_array().unwrap() {
        println!("{line}");
    }
    std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}

/// One run of the weeks, by someone new: session after session gone over,
/// and the probes asked at each checkpoint.
async fn run_weeks(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    lite: &crate::services::analyzer::AiAnalyzer,
    weeks: &Weeks,
    round: usize,
) -> (Vec<Asked>, Vec<Kept>) {
    let name = format!("weeks-{round}");
    let user_id = new_user(db, &name).await;
    let present = Audience::private(user_id);
    let mut asked = Vec::new();
    let mut kept = Vec::new();
    let mut next = 0;
    let mut sessions: Vec<Value> = Vec::new();
    for (number, checkpoint) in weeks.checkpoints.iter().enumerate() {
        for (session, day) in weeks.sessions[..=checkpoint.after]
            .iter()
            .enumerate()
            .skip(next)
        {
            let at = date_of(&day.date);
            keep_chat(
                db,
                &format!("{name}-{session}"),
                user_id,
                at,
                &day.turns,
                &present,
            )
            .await;
            let (notes, lasting, passed_over) =
                settle::over_time(db, judge, user_id, &present, at, &day.turns).await;
            println!("#{round} session {session}: {} lasting", lasting.len());
            sessions.push(
                json!({"session": session, "notes": notes, "lasting": lasting,
                "passedOver": passed_over}),
            );
        }
        next = checkpoint.after + 1;
        let memories: Vec<String> = crate::services::agent::memory::unified::active_in(
            db,
            user_id,
            &present,
            &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.source == "chat")
        .map(|row| row.content)
        .collect();
        let retired: BTreeMap<String, i64> = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT COALESCE(invalid_reason, '') AS reason, COUNT(*) AS retired \
                 FROM agent_memories WHERE user_id = $1 AND invalid_at IS NOT NULL GROUP BY 1",
                [user_id.into()],
            ))
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|row| {
                Some((
                    row.try_get::<String>("", "reason").ok()?,
                    row.try_get::<i64>("", "retired").ok()?,
                ))
            })
            .collect();
        let probes: Vec<(Probe, Version)> = weeks
            .probes
            .iter()
            .filter_map(|probe| Some((probe.clone(), probe.as_of(checkpoint.after)?.clone())))
            .collect();
        let memories_ref = &memories;
        let present_ref = &present;
        let answered: Vec<Asked> = futures::stream::iter(probes)
            .map(|(probe, version)| async move {
                let question = probe.question(&version, &checkpoint.date);
                let cues = judge
                    .analyze_json(
                        &myriad_merope::remembering::system(),
                        &myriad_merope::remembering::input(
                            &question.question,
                            None,
                            &question.question_date,
                        ),
                        myriad_merope::remembering::SCHEMA_NAME,
                        Some(&myriad_merope::remembering::schema()),
                    )
                    .await
                    .ok()
                    .and_then(|raw| myriad_merope::remembering::parse(&raw));
                let cued = answer_one(
                    db,
                    judge,
                    lite,
                    &question,
                    user_id,
                    present_ref,
                    cues.as_ref(),
                    THOROUGH,
                    false,
                    &[],
                )
                .await;
                let wide = answer_one(
                    db,
                    judge,
                    lite,
                    &question,
                    user_id,
                    present_ref,
                    cues.as_ref(),
                    THOROUGH,
                    true,
                    &[],
                )
                .await;
                let held = (!version.held.is_empty()).then(|| {
                    memories_ref.iter().any(|memory| {
                        version
                            .held
                            .iter()
                            .all(|word| memory.contains(word.as_str()))
                    })
                });
                Asked {
                    round,
                    checkpoint: number,
                    probe,
                    held_words: version.held,
                    answer: version.answer,
                    cued,
                    wide,
                    held,
                }
            })
            .buffer_unordered(at_once())
            .collect()
            .await;
        println!(
            "#{round} week {number}: {} lasting, cued {}/{}, wide {}/{}",
            memories.len(),
            answered
                .iter()
                .filter(|asked| asked.cued.correct == Some(true))
                .count(),
            answered.len(),
            answered
                .iter()
                .filter(|asked| asked.wide.correct == Some(true))
                .count(),
            answered.len(),
        );
        asked.extend(answered);
        kept.push(Kept {
            round,
            checkpoint: number,
            memories,
            retired,
            sessions: std::mem::take(&mut sessions),
        });
    }
    (asked, kept)
}

/// Week by week: each way of answering right on average and every time,
/// all probes and those told once and never changed; held words still
/// carried; how much she keeps and has retired.
fn report(weeks: &Weeks, times: usize, asked: &[Asked], kept: &[Kept]) -> Value {
    let arms: [(&str, fn(&Asked) -> &Answered); 2] =
        [("cued", |asked| &asked.cued), ("wide", |asked| &asked.wide)];
    let mut by_week = Vec::new();
    for (number, checkpoint) in weeks.checkpoints.iter().enumerate() {
        let here: Vec<&Asked> = asked
            .iter()
            .filter(|asked| asked.checkpoint == number)
            .collect();
        let ids: std::collections::BTreeSet<&str> =
            here.iter().map(|asked| asked.probe.id.as_str()).collect();
        let mut line = json!({"week": number, "after": checkpoint.after, "date": checkpoint.date});
        for (arm, answered) in arms {
            let tally = |stable_only: bool| {
                let (mut probes, mut right, mut every) = (0, 0, 0);
                for id in &ids {
                    let runs: Vec<&&Asked> = here
                        .iter()
                        .filter(|asked| asked.probe.id == *id)
                        .filter(|asked| !stable_only || asked.probe.stable())
                        .collect();
                    if runs.is_empty() {
                        continue;
                    }
                    let rights = runs
                        .iter()
                        .filter(|asked| answered(asked).correct == Some(true))
                        .count();
                    probes += 1;
                    right += rights;
                    every += usize::from(rights == times);
                }
                json!({"probes": probes, "right": format!("{right}/{}", probes * times),
                    format!("pass^{times}"): every})
            };
            let chars: usize = here
                .iter()
                .map(|asked| {
                    answered(asked)
                        .recalled
                        .iter()
                        .map(|line| line.chars().count())
                        .sum::<usize>()
                })
                .sum();
            line[arm] = json!({"all": tally(false), "stable": tally(true),
                "recalledChars": chars as f64 / here.len().max(1) as f64});
        }
        let held: Vec<bool> = here.iter().filter_map(|asked| asked.held).collect();
        line["held"] = json!(format!(
            "{}/{}",
            held.iter().filter(|held| **held).count(),
            held.len()
        ));
        let kept_here: Vec<&Kept> = kept
            .iter()
            .filter(|kept| kept.checkpoint == number)
            .collect();
        line["lasting"] = json!(
            kept_here
                .iter()
                .map(|kept| kept.memories.len())
                .collect::<Vec<_>>()
        );
        let mut retired: BTreeMap<&str, i64> = BTreeMap::new();
        for kept in &kept_here {
            for (reason, count) in &kept.retired {
                *retired.entry(reason.as_str()).or_default() += count;
            }
        }
        line["retired"] = json!(retired);
        line["offTopic"] = off_topic(&here, &kept_here);
        by_week.push(line);
    }
    let answered = |answered: &Answered| {
        json!({"correct": answered.correct, "recalled": answered.recalled,
            "response": answered.answer})
    };
    let rows: Vec<Value> = asked
        .iter()
        .map(|asked| {
            json!({"round": asked.round, "week": asked.checkpoint, "id": asked.probe.id,
                "question": asked.probe.question, "answer": asked.answer,
                "held": asked.held, "cued": answered(&asked.cued),
                "wide": answered(&asked.wide)})
        })
        .collect();
    let memories: Vec<Value> = kept
        .iter()
        .map(|kept| {
            json!({"round": kept.round, "week": kept.checkpoint,
                "memories": kept.memories, "retired": kept.retired,
                "sessions": kept.sessions})
        })
        .collect();
    json!({"times": times, "wider": wider(), "weeks": by_week, "rows": rows,
        "memories": memories})
}

/// How often what she keeps came up, answering the wide way, for probes it
/// is not about (it carries none of the probe's held words, or the probe
/// has none): the times in all, and the memories it happened to most. A
/// few memories coming up for everything is what reinforcing what is
/// recalled would make worse.
fn off_topic(asked: &[&Asked], kept: &[&Kept]) -> Value {
    let mut times: BTreeMap<&str, usize> = BTreeMap::new();
    for kept in kept {
        for memory in &kept.memories {
            let here = asked
                .iter()
                .filter(|asked| asked.round == kept.round)
                .filter(|asked| {
                    !asked
                        .held_words
                        .iter()
                        .any(|word| memory.contains(word.as_str()))
                })
                .filter(|asked| {
                    asked
                        .wide
                        .recalled
                        .iter()
                        .any(|line| line.contains(memory.as_str()))
                })
                .count();
            if here > 0 {
                *times.entry(memory.as_str()).or_default() += here;
            }
        }
    }
    let mut most: Vec<(&str, usize)> = times.into_iter().collect();
    most.sort_by(|left, right| right.1.cmp(&left.1));
    json!({"times": most.iter().map(|(_, count)| count).sum::<usize>(),
        "memories": kept.iter().map(|kept| kept.memories.len()).sum::<usize>(),
        "most": most.into_iter().take(5).collect::<Vec<_>>()})
}
