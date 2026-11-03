//! A group played through the group mind itself, answered by a small end.

use super::*;
use myriad_merope::group::{
    self as core, Answer, At, Effect, Event, Group, Her, May, Message, Resume, Rolls, ToRead, Who,
};

const BASE: i64 = 1_791_957_600_000_000;

fn us(seconds: f64) -> i64 {
    BASE + (seconds * 1_000_000.0) as i64
}

fn message(id: usize, from: i64, text: &str, addressed: bool) -> Message {
    Message {
        platform: "telegram".into(),
        chat: "-100999".into(),
        message_id: id.to_string(),
        thread: None,
        from: from.to_string(),
        display_name: format!("成员{from}"),
        text: text.into(),
        addressed,
        reply_to: None,
        images: Vec::new(),
        shown_as: None,
    }
}

fn answer(kind: &str) -> Answer {
    match kind {
        "identify" => Answer::Identified {
            who: Who::Member {
                user_id: 101,
                bound: true,
            },
            owner: Some(1),
        },
        "prepare" => Answer::Prepared { ok: true },
        "soul" => Answer::Soul {
            text: "你是测试里的她。".into(),
        },
        "titles" => Answer::Titles { titles: Vec::new() },
        "label" => Answer::Label {
            label: "某人".into(),
        },
        "now" => Answer::Now {
            local: "2026-10-14T14:00:00+08:00".into(),
            doing: None,
            state: Vec::new(),
        },
        "stickers" => Answer::Stickers {
            stickers: Vec::new(),
            can_make: false,
            left: 0,
        },
        "game" => Answer::Game { game: None },
        "recent" => Answer::Surfaces {
            surfaces: Vec::new(),
        },
        "life" => Answer::Life {
            did: Vec::new(),
            would_tell: Vec::new(),
            now: None,
        },
        "cues" => Answer::Cues {
            cues: HashMap::new(),
        },
        "vectors" => Answer::Vectors {
            vectors: Vec::new(),
            known: false,
        },
        "embed" => Answer::Embedded { vector: None },
        "ledger" => Answer::LedgerRead { stored: None },
        "begin" => Answer::TurnBegun {
            turn: Some("turn".into()),
            sections: Vec::new(),
            doing: None,
        },
        "react" => Answer::Reacted { ok: true },
        "may" => Answer::MayLookUp { may: May::Tried },
        "define" => Answer::Defined { entry: None },
        "search" => Answer::Found { found: None },
        "typing" => Answer::Typed,
        "send" => Answer::Sent { ok: true },
        "whisper" => Answer::Whispered { ok: true },
        "member" => Answer::Asked {
            raw: Some("嗯嗯".into()),
            failure: None,
        },
        "ask" => Answer::Asked {
            raw: None,
            failure: Some("unavailable".into()),
        },
        _ => Answer::Rows { rows: Vec::new() },
    }
}

struct End<'a> {
    scene: &'a GroupScene,
    now: i64,
    group: Group,
    seed: u64,
    due: BTreeMap<(i64, u64), Event>,
    order: u64,
    /// Kept across a restart: claimed inbound ids, the group's lines.
    claimed: HashSet<String>,
    kept: Option<Vec<core::Line>>,
    /// Which lines each replying turn read, and when it went out.
    reading: Option<(String, Vec<String>)>,
    replies: Vec<(i64, String, Vec<String>)>,
    again_ignored: usize,
    log: Vec<Value>,
}

impl End<'_> {
    fn at(&mut self, at: i64, event: Event) {
        self.order += 1;
        self.due.insert((at, self.order), event);
    }

    fn step(&mut self, mut event: Event) {
        if !self.group.restored
            && let Event::Heard { restore, .. } = &mut event
        {
            restore.get_or_insert_with(|| core::Restore {
                lines: self.kept.clone(),
                ..Default::default()
            });
        }
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut at = At {
            now: self.now,
            her: Her {
                away: false,
                asleep_for: None,
                busy_for: None,
                today: "2026-10-14".into(),
            },
            rolls: Rolls::new(self.seed, Vec::new()),
        };
        let out = core::step(&mut self.group, &mut at, event);
        for effect in out.effects {
            self.does(effect);
        }
    }

    fn done(&mut self, id: String, after: f64, answer: Answer) {
        let at = self.now + (after * 1_000_000.0) as i64;
        self.at(at, Event::Done { id, answer });
    }

    fn does(&mut self, effect: Effect) {
        match effect {
            Effect::Wake { key, after } => {
                let at = self.now + (after * 1_000_000.0) as i64;
                self.at(at, Event::Woke { key });
            }
            Effect::KeepLines { lines } => self.kept = Some(lines),
            Effect::Claim { id, inbound_id } => {
                let ok = self.claimed.insert(inbound_id);
                if !ok {
                    self.again_ignored += 1;
                }
                self.done(id, 0.0, Answer::Claimed { ok });
            }
            Effect::Identify { id, .. } => self.done(id, 0.0, answer("identify")),
            Effect::Prepare { id, .. } => self.done(id, 0.0, answer("prepare")),
            Effect::Read { id, what, .. } => {
                let kind = match what {
                    ToRead::Soul => "soul",
                    ToRead::Titles { .. } => "titles",
                    ToRead::Label { .. } | ToRead::HerName => "label",
                    ToRead::Now => "now",
                    ToRead::Stickers { .. } => "stickers",
                    ToRead::Game { .. } => "game",
                    ToRead::RecentSurfaces | ToRead::RecentDealt { .. } => "recent",
                    ToRead::Life { .. } => "life",
                    ToRead::Cues { .. } => "cues",
                    ToRead::Vectors { .. } => "vectors",
                    _ => "rows",
                };
                self.done(id, 0.0, answer(kind));
            }
            Effect::Embed { id, .. } => self.done(id, 0.0, answer("embed")),
            Effect::ReadLedger { id } => self.done(id, 0.0, answer("ledger")),
            Effect::ReadKept { id } => self.done(
                id,
                0.0,
                Answer::Kept {
                    restore: Some(core::Restore {
                        lines: self.kept.clone(),
                        ..Default::default()
                    }),
                },
            ),
            Effect::TurnBegin {
                id,
                input,
                transcript,
                ..
            } => {
                let mut read: Vec<String> =
                    transcript.into_iter().map(|line| line.content).collect();
                read.push(input);
                self.reading = Some(("turn".into(), read));
                self.done(id, 0.0, answer("begin"));
            }
            Effect::Ask { id, operation, .. } => {
                if operation == "group_member" || operation == "group_stranger" {
                    self.done(id, self.scene.turn_secs, answer("member"));
                } else {
                    self.done(id, 0.0, answer("ask"));
                }
            }
            Effect::TurnEnd { id, said, .. } => self.done(
                id,
                0.0,
                Answer::TurnEnded {
                    reply: said,
                    bound: true,
                },
            ),
            Effect::Send { id, to, .. } => {
                let read = self
                    .reading
                    .take()
                    .map(|(_, read)| read)
                    .unwrap_or_default();
                self.log.push(json!({
                    "at": (self.now - BASE) as f64 / 1e6,
                    "replyTo": to.message_id,
                    "read": read,
                }));
                self.replies.push((self.now, to.message_id, read));
                self.done(id, 0.0, answer("send"));
            }
            Effect::React { id, .. } => self.done(id, 0.0, answer("react")),
            Effect::MayLookUp { id, .. } => self.done(id, 0.0, answer("may")),
            Effect::Define { id, .. } => self.done(id, 0.0, answer("define")),
            Effect::Search { id, .. } => self.done(id, 0.0, answer("search")),
            Effect::Typing { id, .. } => self.done(id, 0.0, answer("typing")),
            Effect::Whisper { id, .. } => self.done(id, 0.0, answer("whisper")),
            Effect::Look { id, .. } => self.done(id, 0.0, Answer::Looked { seen: Vec::new() }),
            Effect::Sticker { .. } => {}
            _ => {}
        }
    }

    fn hear(&mut self, at: i64, index: usize, from: i64, text: &str, addressed: bool) {
        let message = message(index, from, text, addressed);
        self.at(
            at,
            Event::Heard {
                message: message.clone(),
                restore: None,
            },
        );
        let event = if addressed {
            Event::Called {
                message,
                waiter: Some(format!("w{index}")),
            }
        } else {
            Event::Noticed { message }
        };
        self.at(at, event);
    }

    fn restart(&mut self) {
        self.log
            .push(json!({ "at": (self.now - BASE) as f64 / 1e6, "restart": true }));
        // Her mind of the group is gone; what was kept is not.
        self.group = Group::default();
        self.reading = None;
        self.due.clear();
        // The end reads back the group and hands in what it missed;
        // the platform hands in what it handed in before.
        let past = core::kept_as_heard(
            "telegram",
            "-100999",
            None,
            self.kept.clone().unwrap_or_default(),
        );
        self.at(
            self.now + 1_000_000,
            Event::Resumed {
                why: Resume::Restart,
                past,
                up_since: Some(self.now),
            },
        );
        for (index, (at, from, text, addressed)) in self.scene.lines.iter().enumerate() {
            if us(*at) <= self.now {
                let at = self.now + 2_000_000;
                self.hear(at, index, *from, text, *addressed);
            }
        }
    }
}

pub(super) fn play(scene: &GroupScene) -> (GroupTally, Vec<Value>) {
    let mut end = End {
        scene,
        now: BASE,
        group: Group::default(),
        seed: 1,
        due: BTreeMap::new(),
        order: 0,
        claimed: HashSet::new(),
        kept: None,
        reading: None,
        replies: Vec::new(),
        again_ignored: 0,
        log: Vec::new(),
    };
    for (index, (at, from, text, addressed)) in scene.lines.iter().enumerate() {
        end.hear(us(*at), index, *from, text, *addressed);
    }
    let mut restarted = false;
    loop {
        let next = end.due.keys().next().copied();
        if let Some(at) = scene.restart_at.map(us)
            && !restarted
            && next.is_none_or(|(when, _)| when > at)
        {
            restarted = true;
            end.now = at;
            end.restart();
            continue;
        }
        let Some((at, order)) = next else {
            break;
        };
        if at > us(scene.until) {
            break;
        }
        let event = end.due.remove(&(at, order)).unwrap();
        end.now = end.now.max(at);
        end.step(event);
    }
    let mut tally = GroupTally {
        again_ignored: end.again_ignored,
        ..Default::default()
    };
    let mut answered: HashMap<String, usize> = HashMap::new();
    for (_, to, _) in &end.replies {
        *answered.entry(to.clone()).or_default() += 1;
    }
    for (index, (at, _, text, addressed)) in scene.lines.iter().enumerate() {
        if !addressed {
            continue;
        }
        tally.called += 1;
        // Answered: a reply went out after it came whose turn read it.
        let reply = end.replies.iter().find(|(when, _, read)| {
            *when >= us(*at)
                && read
                    .iter()
                    .any(|line| line.contains(text.trim_start_matches("@bot ")))
        });
        match reply {
            Some((when, ..)) => tally.latencies.push((*when - us(*at)) as f64 / 1e6),
            None => tally.missed += 1,
        }
        if answered.get(&index.to_string()).copied().unwrap_or(0) > 1 {
            tally.duplicated += 1;
        }
    }
    (tally, end.log)
}
