//! Replays of her speaking up unasked in private: how many lines go out,
//! how many they answer, how many crowd or collide, before the shared
//! core's admission and ledger and after; and, gate by gate, whether she
//! says too much when a gate is handed to her as facts.
//!
//! Two timelines: the site's own (the owner's private talk and what she
//! said unasked, read-only from `DATABASE_URL`), and a made week (two
//! visits a day, events at any hour, the reach pass every ten minutes, her
//! mind wandering while they are here). Every gate is the site's, at the
//! same place in the same order; her judgment is a fixed draw a line (the
//! same draw before and after), so the counts show what the arrangement
//! does, not what she would say. What she would say is the judged part:
//! for each gate, situations it would hold back and situations it would
//! not, put to the judgment model exactly as an event decision puts them
//! (her soul, the decision prompt, the core's facts), several times each.
//!
//! An answer within two hours is counted, as a number about the timeline,
//! not about how good a line was: most lines nobody answers are not read.
//!
//! Opt-in, read-only on the site's database:
//! `DATABASE_URL=… MEROPE_SPEECH_REPLAY_REPORT=<new file> cargo test -p
//! myriad-backend --bin myriad-backend -- --ignored speech_replay
//! --nocapture`; with `MEROPE_SPEECH_JUDGE=<rounds>` it also asks the
//! judgment model (spends on it).

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, TimeZone, Utc};
use myriad_merope::speech::{Admit, Extent, Said, Source, Wish};
use serde::Serialize;
use serde_json::{Value, json};

const MIN: i64 = 60_000_000;
const COOLDOWN: i64 = 3 * MIN;
const SAME_TOPIC: i64 = 15 * MIN;
const GREETING_AGAIN: i64 = 12 * 60 * MIN;
const QUIET: i64 = 3 * MIN;
const BETWEEN_THOUGHTS: i64 = 10 * MIN;
const ANSWERED_WITHIN: i64 = 120 * MIN;
/// After their last message they count as here (the page open) this long,
/// and as looking at her panel this long.
const HERE_FOR: i64 = 15 * MIN;
const PANEL_FOR: i64 = 10 * MIN;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Path {
    Event,
    Greeting,
    Reach,
    Wander,
    Doing,
}

impl Path {
    fn in_person(self) -> bool {
        matches!(self, Self::Wander | Self::Doing)
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    at: i64,
    path: Path,
    topic: String,
    thing: String,
    /// Her judgment, drawn once: the same before and after.
    yes: bool,
    compose: i64,
}

#[derive(Clone, Debug)]
struct Line {
    path: Path,
    topic: String,
    thing: String,
    written: i64,
    out: i64,
}

/// Which of today's gates are still rules.
#[derive(Clone, Copy, Debug, Serialize)]
struct Rules {
    same_topic: bool,
    cooldown: bool,
    greeting: bool,
    night: bool,
    wander_quiet: bool,
}

impl Rules {
    const TODAY: Rules = Rules {
        same_topic: true,
        cooldown: true,
        greeting: true,
        night: true,
        wander_quiet: true,
    };
}

#[derive(Clone, Copy, Debug)]
enum Arrangement {
    /// Each path with its own gates and records, as before the ledger.
    Before,
    /// One ledger, the core's admission, one line at a time, looked at
    /// again before it goes; `Rules` say which gates are still rules.
    After(Rules),
}

struct Timeline {
    name: &'static str,
    theirs: Vec<i64>,
    candidates: Vec<Candidate>,
}

#[derive(Debug, Default, Serialize)]
struct Tally {
    lines: usize,
    per_path: BTreeMap<String, usize>,
    answered: usize,
    /// Two lines to them written without either seeing the other.
    collisions: usize,
    /// A line out within three minutes of the one before.
    crowded: usize,
    /// Out during her night.
    at_night: usize,
    days: f64,
}

fn asleep(at: i64) -> bool {
    let zone = crate::services::agent::merope::clock::zone();
    myriad_merope::timing::asleep_at(
        DateTime::<Utc>::from_timestamp_micros(at).unwrap_or_default(),
        &zone,
    )
    .is_some()
}

impl Timeline {
    fn last_theirs(&self, at: i64) -> Option<i64> {
        let index = self.theirs.partition_point(|theirs| *theirs <= at);
        index.checked_sub(1).map(|index| self.theirs[index])
    }

    fn here(&self, at: i64) -> bool {
        self.last_theirs(at).is_some_and(|last| at - last < HERE_FOR)
    }

    fn panel(&self, at: i64) -> bool {
        self.last_theirs(at).is_some_and(|last| at - last < PANEL_FOR)
    }

    fn answered(&self, out: i64) -> bool {
        let index = self.theirs.partition_point(|theirs| *theirs <= out);
        self.theirs
            .get(index)
            .is_some_and(|next| *next - out <= ANSWERED_WITHIN)
    }

    fn run(&self, arrangement: Arrangement) -> Tally {
        let mut lines: Vec<Line> = Vec::new();
        let mut asked: HashSet<String> = HashSet::new();
        let mut last_thought: Option<i64> = None;
        let rules = match arrangement {
            Arrangement::Before => Rules::TODAY,
            Arrangement::After(rules) => rules,
        };
        let after = matches!(arrangement, Arrangement::After(_));
        for c in &self.candidates {
            let now = c.at;
            let seen = |lines: &[Line], at: i64| -> Vec<Line> {
                lines.iter().filter(|line| line.out <= at).cloned().collect()
            };
            match c.path {
                Path::Reach => {
                    if (rules.night && asleep(now)) || self.panel(now) || !asked.insert(c.thing.clone()) {
                        continue;
                    }
                    if after && seen(&lines, now).iter().any(|line| line.thing == c.thing) {
                        continue;
                    }
                    if !c.yes {
                        continue;
                    }
                    let mut out = now + c.compose;
                    if after {
                        // One at a time, and looked at again as things stand.
                        out = in_flight_end(&lines, out);
                        if self.panel(out) || seen(&lines, out).iter().any(|line| line.thing == c.thing) {
                            continue;
                        }
                    }
                    lines.push(line_of(c, out));
                }
                path => {
                    if path == Path::Wander {
                        if (rules.night && asleep(now)) || !self.here(now) {
                            continue;
                        }
                        if rules.wander_quiet {
                            let seen_now = seen(&lines, now);
                            let spoke_up = seen_now
                                .iter()
                                .filter(|line| after || line.path != Path::Reach)
                                .map(|line| line.out)
                                .max();
                            let quiet = self.last_theirs(now).is_none_or(|t| now - t >= QUIET)
                                && spoke_up.is_none_or(|t| now - t >= COOLDOWN)
                                && last_thought.is_none_or(|t| now - t >= BETWEEN_THOUGHTS);
                            if !quiet {
                                continue;
                            }
                        }
                        last_thought = Some(now);
                    }
                    if path.in_person() && !self.here(now) {
                        continue;
                    }
                    let held = |lines: &[Line], at: i64| -> bool {
                        let seen = seen(lines, at);
                        if path == Path::Greeting
                            && rules.greeting
                            && seen
                                .iter()
                                .any(|line| line.topic == c.topic && at - line.out < GREETING_AGAIN)
                        {
                            return true;
                        }
                        // Before, the topic gate read the transcript (every
                        // path wrote it) and the cooldown only what the
                        // event path wrote; after, both read the ledger.
                        if rules.same_topic
                            && seen
                                .iter()
                                .any(|line| line.topic == c.topic && at - line.out < SAME_TOPIC)
                        {
                            return true;
                        }
                        rules.cooldown
                            && seen
                                .iter()
                                .filter(|line| after || line.path != Path::Reach)
                                .any(|line| at - line.out < COOLDOWN)
                    };
                    if held(&lines, now) {
                        continue;
                    }
                    if after {
                        if seen(&lines, now).iter().any(|line| line.thing == c.thing) {
                            continue;
                        }
                    } else if lines.iter().any(|line| {
                        // The in-process claim: the same topic or event in
                        // flight, or the same event a moment ago.
                        (line.topic == c.topic && line.written <= now && now < line.out)
                            || (line.thing == c.thing && now - line.out < SAME_TOPIC)
                    }) {
                        continue;
                    }
                    if !c.yes {
                        continue;
                    }
                    let mut out = now + c.compose;
                    if after {
                        out = in_flight_end(&lines, out);
                        if held(&lines, out)
                            || seen(&lines, out).iter().any(|line| line.thing == c.thing)
                            || (path.in_person() && !self.here(out))
                        {
                            continue;
                        }
                    }
                    lines.push(line_of(c, out));
                }
            }
        }
        self.tally(lines)
    }

    fn tally(&self, mut lines: Vec<Line>) -> Tally {
        lines.sort_by_key(|line| line.out);
        let mut tally = Tally {
            lines: lines.len(),
            ..Tally::default()
        };
        for (index, line) in lines.iter().enumerate() {
            *tally
                .per_path
                .entry(format!("{:?}", line.path).to_lowercase())
                .or_default() += 1;
            if self.answered(line.out) {
                tally.answered += 1;
            }
            if asleep(line.out) {
                tally.at_night += 1;
            }
            if let Some(before) = index.checked_sub(1).map(|before| &lines[before]) {
                if line.out - before.out < COOLDOWN {
                    tally.crowded += 1;
                }
            }
            tally.collisions += lines[..index]
                .iter()
                .filter(|other| line.written < other.out && other.written < line.out)
                .count();
        }
        let span = match (
            self.candidates.first().map(|c| c.at),
            self.candidates.last().map(|c| c.at),
        ) {
            (Some(first), Some(last)) => (last - first) as f64 / (24.0 * 60.0 * MIN as f64),
            _ => 0.0,
        };
        tally.days = (span * 10.0).round() / 10.0;
        tally
    }
}

/// When a line written to go out at `out` can go: after whatever of hers
/// is still going out then.
fn in_flight_end(lines: &[Line], out: i64) -> i64 {
    lines
        .iter()
        .filter(|line| line.out > out)
        .map(|line| line.out)
        .max()
        .unwrap_or(out)
}

fn line_of(c: &Candidate, out: i64) -> Line {
    Line {
        path: c.path,
        topic: c.topic.clone(),
        thing: c.thing.clone(),
        written: c.at,
        out,
    }
}

/// A small seeded draw, so a replay is the same every time.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

const YES: f64 = 0.35;

fn compose(draw: &mut Draw) -> i64 {
    ((5.0 + 20.0 * draw.next()) * 1_000_000.0) as i64
}

fn candidate(draw: &mut Draw, at: i64, path: Path, topic: &str, thing: String) -> Candidate {
    Candidate {
        at,
        path,
        topic: topic.to_string(),
        thing,
        yes: draw.next() < YES,
        compose: compose(draw),
    }
}

/// A made week: two visits a day (morning, and an evening that sometimes
/// runs past her bedtime), messages every few minutes while there; events
/// at any hour; the reach pass every ten minutes, with a reason after six
/// hours apart; her mind wandering each minute and what she is doing told
/// every twenty while they are there.
fn made_week() -> Timeline {
    let zone = crate::services::agent::merope::clock::zone();
    let start = zone
        .with_ymd_and_hms(2026, 10, 26, 0, 0, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
        .timestamp_micros();
    let mut draw = Draw(0x5eed_2026_1103);
    let mut theirs = Vec::new();
    for day in 0..7 {
        let base = start + day * 24 * 60 * MIN;
        let late = if day % 3 == 2 { 90 } else { 0 };
        for (from, until) in [(8 * 60, 8 * 60 + 40), (21 * 60, 23 * 60 + 30 + late)] {
            let mut at = base + from * MIN;
            while at < base + until * MIN {
                theirs.push(at);
                at += ((1.0 + 4.0 * draw.next()) * MIN as f64) as i64;
            }
        }
    }
    let timeline = Timeline {
        name: "made_week",
        theirs,
        candidates: Vec::new(),
    };
    let end = start + 7 * 24 * 60 * MIN;
    let topics = [
        "phantasi.digest",
        "agent.task_completed",
        "agent.merope.report_ready",
        "agent.merope.platform_activity",
    ];
    let mut candidates = Vec::new();
    let mut at = start;
    while at < end {
        at += (-(1.0 - draw.next()).ln() * 45.0 * MIN as f64) as i64;
        let topic = topics[(draw.next() * topics.len() as f64) as usize % topics.len()];
        let thing = format!("{topic}|{}", at / SAME_TOPIC);
        candidates.push(candidate(&mut draw, at, Path::Event, topic, thing));
    }
    let mut tick = start;
    let mut was_here = false;
    let mut last_visit: Option<i64> = None;
    while tick < end {
        let here = timeline.here(tick);
        if here && !was_here {
            let first_today = last_visit.is_none_or(|last| tick - last > 6 * 60 * MIN);
            if first_today {
                let thing = format!("greeting|{}", tick / SAME_TOPIC);
                candidates.push(candidate(&mut draw, tick, Path::Greeting, "agent.merope.greeting", thing));
            }
        }
        if here {
            last_visit = Some(tick);
            candidates.push(candidate(&mut draw, tick + 7_000_000, Path::Wander, "agent.merope.thought", format!("thought|{tick}")));
            if (tick - start) % (20 * MIN) == 0 {
                candidates.push(candidate(&mut draw, tick + 13_000_000, Path::Doing, "agent.merope.doing", format!("doing|{}", tick / (20 * MIN))));
            }
        }
        was_here = here;
        if (tick - start) % (10 * MIN) == 0
            && timeline
                .last_theirs(tick)
                .is_some_and(|last| tick - last > 6 * 60 * MIN)
        {
            let apart = (tick - timeline.last_theirs(tick).unwrap_or(tick)) / (6 * 60 * MIN);
            candidates.push(candidate(&mut draw, tick + 2_000_000, Path::Reach, "agent.merope.reach_out", format!("reach|{apart}")));
        }
        tick += MIN;
    }
    candidates.sort_by_key(|c| c.at);
    Timeline {
        candidates,
        ..timeline
    }
}

/// The site's own timeline: the owner's private messages, what she said
/// unasked (said then: her judgment was yes) and the events she weighed
/// (drawn), read-only.
async fn site_timeline(db: &sea_orm::DatabaseConnection) -> Option<Timeline> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let owner: i32 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT s.user_id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL AND m.role = 'user' \
             GROUP BY 1 ORDER BY count(*) DESC LIMIT 1",
        ))
        .await
        .ok()??
        .try_get("", "user_id")
        .ok()?;
    let times = |rows: Vec<sea_orm::QueryResult>| -> Vec<(i64, String)> {
        rows.iter()
            .filter_map(|row| {
                let at: DateTime<chrono::FixedOffset> = row.try_get("", "at").ok()?;
                let what: Option<String> = row.try_get("", "what").ok()?;
                Some((at.timestamp_micros(), what.unwrap_or_default()))
            })
            .collect()
    };
    let query = |sql: &str| {
        Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, [owner.into()])
    };
    let theirs: Vec<i64> = times(
        db.query_all_raw(query(
            "SELECT m.created_at AS at, NULL::text AS what FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id WHERE s.user_id = $1 \
             AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL AND m.role = 'user' \
             ORDER BY 1",
        ))
        .await
        .ok()?,
    )
    .into_iter()
    .map(|(at, _)| at)
    .collect();
    let said = times(
        db.query_all_raw(query(
            "SELECT created_at AS at, event_key AS what FROM agent_proactive_messages \
             WHERE user_id = $1 AND role = 'assistant' ORDER BY 1",
        ))
        .await
        .ok()?,
    );
    let events = times(
        db.query_all_raw(query(
            "SELECT created_at AS at, content AS what FROM agent_diary \
             WHERE user_id = $1 AND source = 'event' ORDER BY 1",
        ))
        .await
        .ok()?,
    );
    let mut draw = Draw(0x51de_2026_1103);
    let mut candidates = Vec::new();
    for (at, key) in said {
        let path = match key.as_str() {
            "agent.merope.greeting" => Path::Greeting,
            "agent.merope.reach_out" => Path::Reach,
            "agent.merope.thought" => Path::Wander,
            "agent.merope.doing" => Path::Doing,
            _ => Path::Event,
        };
        let compose = compose(&mut draw);
        candidates.push(Candidate {
            at: at - compose,
            path,
            thing: format!("{key}|{}", at / SAME_TOPIC),
            topic: key,
            yes: true,
            compose,
        });
    }
    for (at, what) in events {
        let (path, topic) = if what.contains("来了") {
            (Path::Greeting, "agent.merope.greeting".to_string())
        } else {
            (Path::Event, what.chars().take(12).collect())
        };
        candidates.push(Candidate {
            at,
            path,
            thing: format!("{topic}|{}", at / SAME_TOPIC),
            topic,
            yes: draw.next() < YES,
            compose: compose(&mut draw),
        });
    }
    candidates.sort_by_key(|c| c.at);
    Some(Timeline {
        name: "site",
        theirs,
        candidates,
    })
}

// --- the judged part: does she say too much without a gate? ---------------

/// One situation put to her: an event, what she said to them lately (how
/// long ago, her words, whether they answered), when they last talked.
struct Situation {
    kind: &'static str,
    summary: &'static str,
    lately: Vec<(i64, &'static str, bool)>,
    talked_ago: Option<i64>,
    present: bool,
    /// Minutes past midnight, her clock.
    clock: u32,
}

/// The gates handed to her one by one, in the order tried: for each, the
/// situations it holds back, and the same with what it holds back on
/// taken away.
fn gates() -> Vec<(&'static str, Vec<Situation>, Vec<Situation>)> {
    let digest = "你订阅的「糖宝小窝」更新了三篇文章";
    let digest2 = "你订阅的「慢慢读」更新了一篇长文";
    let line = "糖宝小窝又更新了，有一篇讲猫咪绝育的。";
    let greeting = "这个人今天第一次来了";
    let hello = "早呀，今天来得挺早。";
    let thought = "你忽然想起关于对方的一件事：上周说要去考驾照科目二";
    let s = |kind, summary, lately: Vec<(i64, &'static str, bool)>, talked: Option<i64>, present, clock| Situation {
        kind,
        summary,
        lately,
        talked_ago: talked,
        present,
        clock,
    };
    vec![
        (
            "same_topic_15m",
            vec![
                s("phantasi.digest", digest2, vec![(6, line, false)], Some(40), false, 15 * 60),
                s("phantasi.digest", digest2, vec![(12, line, false)], Some(90), true, 20 * 60),
            ],
            vec![
                s("phantasi.digest", digest2, vec![(200, line, false)], Some(40), false, 15 * 60),
                s("phantasi.digest", digest2, vec![(300, line, true)], Some(90), true, 20 * 60),
            ],
        ),
        (
            "cooldown_180s",
            vec![
                s("phantasi.digest", digest, vec![(1, hello, false)], Some(5), true, 9 * 60),
                s("agent.merope.thought", thought, vec![(2, line, false)], Some(6), true, 21 * 60),
            ],
            vec![
                s("phantasi.digest", digest, vec![(60, hello, true)], Some(5), true, 9 * 60),
                s("agent.merope.thought", thought, vec![(90, line, true)], Some(6), true, 21 * 60),
            ],
        ),
        (
            "greeting_12h",
            vec![
                s("agent.merope.greeting", greeting, vec![(180, hello, true)], Some(170), true, 12 * 60),
                s("agent.merope.greeting", greeting, vec![(400, hello, false)], Some(380), true, 19 * 60),
            ],
            vec![
                s("agent.merope.greeting", greeting, vec![(900, hello, true)], Some(890), true, 8 * 60 + 30),
                s("agent.merope.greeting", greeting, vec![], Some(1500), true, 9 * 60),
            ],
        ),
        (
            "night",
            vec![
                s("agent.merope.reach_out", "你们已经两天没说话了；他上周说周五面试", vec![], Some(2880), false, 3 * 60 + 20),
                s("agent.merope.thought", thought, vec![], Some(8), true, 2 * 60 + 10),
            ],
            vec![
                s("agent.merope.reach_out", "你们已经两天没说话了；他上周说周五面试", vec![], Some(2880), false, 19 * 60),
                s("agent.merope.thought", thought, vec![], Some(8), true, 22 * 60),
            ],
        ),
        (
            "wander_quiet",
            vec![
                s("agent.merope.thought", thought, vec![], Some(1), true, 21 * 60),
                s("agent.merope.thought", thought, vec![(4, line, false)], Some(5), true, 21 * 60),
            ],
            vec![
                s("agent.merope.thought", thought, vec![], Some(12), true, 21 * 60),
                s("agent.merope.thought", thought, vec![(45, line, true)], Some(30), true, 21 * 60),
            ],
        ),
    ]
}

fn admit_for(situation: &Situation, at: i64) -> Admit {
    let to = super::private(701);
    let source = super::source_of_event(situation.kind);
    let ledger = situation
        .lately
        .iter()
        .map(|(ago, text, _)| Said {
            at: at - ago * MIN,
            to: to.clone(),
            text: text.to_string(),
            place: "web".into(),
            source: Some(Source::Event),
            called: false,
            thing: None,
            exit: "notice".into(),
            extent: Extent::Full,
            heard: None,
            interrupted: false,
            line: None,
            group_name: None,
        })
        .collect();
    let answered = situation
        .lately
        .iter()
        .filter(|(_, _, answered)| *answered)
        .map(|(ago, _, _)| at - ago * MIN + MIN)
        .max();
    Admit {
        wish: Wish {
            to: to.clone(),
            source,
            seed: situation.summary.to_string(),
            seed_from: to,
            thing: None,
            in_person: false,
            keeps_until: None,
            called: false,
            needs: Vec::new(),
            at,
            text: None,
        },
        ledger,
        granted: Vec::new(),
        dnd: false,
        they_opened: false,
        present: situation.present,
        now: at,
        last_talk_at: situation.talked_ago.map(|ago| at - ago * MIN),
        they_wrote_at: answered.or(situation.talked_ago.map(|ago| at - ago * MIN)),
        doing: None,
        touched_at: None,
    }
}

/// Whether she speaks up in `situation`, asked as an event decision asks.
async fn speaks(
    judge: &crate::services::analyzer::AiAnalyzer,
    soul: &str,
    situation: &Situation,
) -> Option<bool> {
    use crate::services::agent::consciousness as event;
    let zone = crate::services::agent::merope::clock::zone();
    let day = zone.with_ymd_and_hms(2026, 10, 28, 0, 0, 0).single()?;
    let at = (day + chrono::Duration::minutes(situation.clock as i64)).with_timezone(&Utc);
    let micros = at.timestamp_micros();
    let facts = myriad_merope::speech::facts(&admit_for(situation, micros), &zone);
    let event = event::ConsciousnessEvent {
        id: format!("replay_{}", situation.kind),
        source: "merope".into(),
        kind: situation.kind.into(),
        headline: situation.summary.into(),
        summary: situation.summary.into(),
        addressee_user_id: 701,
        urgency: event::EventUrgency::Normal,
        occurred_at: at,
        parent_event_id: None,
        safe_facts: Default::default(),
        keeps_memory: false,
    };
    let snapshot = event::SelfSnapshot {
        persona_name: "她".into(),
        addressee_user_id: 701,
        interaction_mode: crate::services::agent::AgentInteractionMode::Chat,
        mood: 65.0,
        activity: "idle".into(),
        do_not_disturb: false,
        has_active_work: false,
        granted_permissions: vec![],
        recent_intents: vec![],
        remembered: vec![],
        captured_at: at,
        live: event::SelfLivePresence {
            page_visible: situation.present,
            face_visible: situation.present,
            captured_at: Some(at),
            ..Default::default()
        },
        attention: None,
        myself: None,
        addressee_name: Some("他".into()),
        minutes_since_you_spoke_unasked: situation.lately.first().map(|(ago, _, _)| *ago as f64),
        said_unasked_lately: vec![],
        speaking: facts,
        inner: None,
        macs_for_work: vec![],
    };
    let (system, schema) = event::semantic_probe_contract(soul, situation.kind);
    let input = json!({ "event": event, "self": snapshot }).to_string();
    let raw = judge
        .analyze_json(&system, &input, "agent_consciousness_decision", Some(&schema))
        .await
        .ok()?;
    let decision: Value = serde_json::from_str(&raw).ok()?;
    Some(matches!(
        decision["action"].as_str(),
        Some("speak") | Some("ask")
    ))
}

/// How often she speaks in `situations`, over `rounds`.
async fn rate(
    judge: &crate::services::analyzer::AiAnalyzer,
    soul: &str,
    situations: &[Situation],
    rounds: usize,
) -> (usize, usize) {
    let mut yes = 0;
    let mut asked = 0;
    for situation in situations {
        for _ in 0..rounds {
            if let Some(spoke) = speaks(judge, soul, situation).await {
                asked += 1;
                yes += usize::from(spoke);
            }
        }
    }
    (yes, asked)
}

#[test]
fn one_ledger_and_one_turn_leave_no_two_lines_written_blind() {
    let week = made_week();
    let before = week.run(Arrangement::Before);
    let after = week.run(Arrangement::After(Rules::TODAY));
    assert!(before.collisions > 0, "{before:?}");
    assert_eq!(after.collisions, 0, "{after:?}");
    assert!(after.crowded <= before.crowded, "{before:?} {after:?}");
}

#[tokio::test]
#[ignore = "reads the site's database; with MEROPE_SPEECH_JUDGE spends on its judgment model"]
async fn speech_replay() {
    let report_path = std::env::var("MEROPE_SPEECH_REPLAY_REPORT").expect("report path");
    assert!(!std::path::Path::new(&report_path).exists(), "report must not exist");
    let db = crate::services::agent::semantic_eval::load_configured_lite().await;
    let mut timelines = vec![made_week()];
    timelines.extend(site_timeline(&db).await);
    let mut report = json!({ "timelines": {} });
    for timeline in &timelines {
        let before = timeline.run(Arrangement::Before);
        let after = timeline.run(Arrangement::After(Rules::TODAY));
        let mut without = serde_json::Map::new();
        let mut rules = Rules::TODAY;
        let gates: [(&str, fn(&mut Rules)); 5] = [
            ("same_topic_15m", |rules| rules.same_topic = false),
            ("cooldown_180s", |rules| rules.cooldown = false),
            ("greeting_12h", |rules| rules.greeting = false),
            ("night", |rules| rules.night = false),
            ("wander_quiet", |rules| rules.wander_quiet = false),
        ];
        for (gate, off) in gates {
            off(&mut rules);
            without.insert(
                gate.into(),
                serde_json::to_value(timeline.run(Arrangement::After(rules))).unwrap(),
            );
        }
        println!(
            "[{}] candidates {} | before {before:?}\n  after {after:?}",
            timeline.name,
            timeline.candidates.len()
        );
        report["timelines"][timeline.name] = json!({
            "candidates": timeline.candidates.len(),
            "theirMessages": timeline.theirs.len(),
            "before": before,
            "after": after,
            "afterWithoutGatesCumulative": without,
        });
    }
    let rounds: usize = std::env::var("MEROPE_SPEECH_JUDGE")
        .ok()
        .and_then(|rounds| rounds.parse().ok())
        .unwrap_or(0);
    if rounds > 0 {
        let soul = crate::services::agent::semantic_eval::configured_soul().await;
        let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(
            std::time::Duration::from_secs(30),
        ))
        .await
        .expect("judgment model");
        let mut judged = serde_json::Map::new();
        for (gate, held, open) in gates() {
            let (held_yes, held_asked) = rate(&judge, &soul, &held, rounds).await;
            let (open_yes, open_asked) = rate(&judge, &soul, &open, rounds).await;
            println!("[judged] {gate}: held back {held_yes}/{held_asked}, not held {open_yes}/{open_asked}");
            judged.insert(
                gate.into(),
                json!({
                    "heldBack": { "spoke": held_yes, "asked": held_asked },
                    "notHeld": { "spoke": open_yes, "asked": open_asked },
                }),
            );
        }
        report["judged"] = Value::Object(judged);
    }
    std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}
