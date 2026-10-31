//! Scheduling evaluation: whether what comes in reaches the right place,
//! once, in time. Not whether she answers well.
//!
//! Synthetic message streams play on a simulated clock against Work that
//! only takes time. A private chat goes through the channel's own rules:
//! what is read together (`chat::take_in`), where a line goes
//! (`works::route`, `works::act`), her lines as parsed (`delegate::split`),
//! parked questions (`decide_pending_reply`, `works::stamp`), labels
//! (`works::labelled`), and the chat's one lock, which a reply holds while
//! she writes it. A group goes through the group mind itself
//! (`myriad_merope::group::step`), answered by a small end. What the channel
//! keeps across a restart (each piece, its question and outbox and delivery
//! cursor, what came and was not answered, the claimed inbound ids) is kept
//! here the same way; the rest is dropped.
//!
//! Deterministic by default: her lines are scripted from what each message
//! means, so only scheduling is measured; it runs with the other tests.
//! `MYRIAD_SCHEDULING_LIVE=1 cargo test … scheduling_eval::live -- --ignored`
//! asks the configured chat model for her lines instead (read-only, as the
//! semantic evaluation does) and scores which piece she gives each message
//! to. A report is written only where `MYRIAD_SCHEDULING_REPORT` points.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use myriad_agent_rules::channel::{
    PendingDecision, PendingKind, PendingOption, PendingPrompt, decide_pending_reply,
    format_pending_prompt,
};
use serde_json::{Value, json};

use super::works::{self, Act, Piece, Route, Standing};
use crate::services::agent::delegate::{self, HandOff};

/// She sees a message this soon while in talk.
const LOOK: f64 = 2.0;
/// Her reply takes this long; she holds the chat while she writes it.
const TURN: f64 = 6.0;
/// The recovery worker's tick: a piece's delivery is picked up again.
const RECOVER: f64 = 10.0;
/// A refused send is tried again after this.
const RETRY: f64 = 1.0;

fn ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

// ------------------------------------------------------------ private chat

/// What a message means, as the script knows it.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Meant {
    Chat,
    New(&'static str),
    /// More for piece N, or the answer to its question.
    To(u32, &'static str),
    Stop(u32),
}

#[derive(Debug, Clone)]
pub(super) struct Said {
    pub at: f64,
    pub text: &'static str,
    pub meant: Meant,
    /// The platform hands in an earlier message again (after a restart).
    pub again_of: Option<usize>,
}

fn said(at: f64, text: &'static str, meant: Meant) -> Said {
    Said {
        at,
        text,
        meant,
        again_of: None,
    }
}

/// Work that only takes time, perhaps asking one question on the way.
#[derive(Debug, Clone)]
pub(super) struct Job {
    pub secs: f64,
    pub asks: Option<(f64, &'static str, &'static [&'static str])>,
}

fn job(secs: f64) -> Job {
    Job { secs, asks: None }
}

fn asking(secs: f64, after: f64, question: &'static str, options: &'static [&'static str]) -> Job {
    Job {
        secs,
        asks: Some((after, question, options)),
    }
}

pub(super) struct Scene {
    pub name: &'static str,
    pub said: Vec<Said>,
    /// What each piece does, in the order pieces are handed off.
    pub jobs: Vec<Job>,
    pub restart_at: Option<f64>,
    /// The first send of piece N's result is refused once.
    pub refuse_first_of: Option<u32>,
    pub until: f64,
}

#[derive(Debug, Clone)]
struct SimPiece {
    number: u32,
    asked: String,
    from: usize,
    job: Job,
    worked: f64,
    since: Option<f64>,
    round: u64,
    asked_once: bool,
    waiting: Option<PendingPrompt>,
    stopped: bool,
    finished: bool,
    /// What reached it after it began: more to go on, and answers.
    inputs: Vec<String>,
    /// What it produced (questions, its result), by sequence.
    produced: Vec<(String, bool)>,
    /// How far delivery got (sent or refused for good).
    cursor: usize,
    results_sent: usize,
    result_at: Option<f64>,
    /// Picked up again after it was done: one more result each time.
    continued: usize,
}

impl SimPiece {
    fn standing(&self) -> Standing {
        if self.stopped {
            Standing::Stopped
        } else if let Some(prompt) = &self.waiting {
            Standing::Waiting {
                question: format_pending_prompt(prompt),
            }
        } else if self.finished {
            Standing::Done { result: None }
        } else {
            Standing::Doing { progress: None }
        }
    }

    fn piece(&self) -> Piece {
        Piece {
            number: self.number,
            asked: self.asked.clone(),
            started_at: Some(0),
            standing: self.standing(),
        }
    }
}

#[derive(Debug)]
enum Ev {
    In(usize),
    Look,
    Free,
    Work(u32, u64),
    Flush(u32),
    Restart,
    Recover,
}

enum Hold {
    Inbound(usize),
    Turn(Vec<usize>),
}

/// Her lines for a turn: scripted from what was meant, or asked of a model.
pub(super) trait Her {
    fn lines(&mut self, read: &[&Said], pieces: &[Piece], waiting: usize) -> String;
}

/// She means what the script says.
pub(super) struct Scripted;

impl Her for Scripted {
    fn lines(&mut self, read: &[&Said], _: &[Piece], _: usize) -> String {
        let mut reply = vec!["好的".to_string()];
        for said in read {
            match &said.meant {
                Meant::Chat => {}
                Meant::New(text) => reply.push(format!("[[work: {text}]]")),
                Meant::To(work, text) => reply.push(format!("[[work #{work}: {text}]]")),
                Meant::Stop(work) => reply.push(format!("[[stop #{work}]]")),
            }
        }
        reply.join("\n")
    }
}

/// What a scene came to.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Tally {
    pub messages: usize,
    pub missed: usize,
    pub duplicated: usize,
    pub heard_again: usize,
    pub results: usize,
    pub results_missed: usize,
    pub results_duplicated: usize,
    pub more_right: usize,
    pub more_wrong: usize,
    pub stops_right: usize,
    pub stops_wrong: usize,
    pub unlabelled_while_others: usize,
    /// Request to result, simulated seconds.
    pub latencies: Vec<f64>,
    /// Her lines that differed from what was meant (live only).
    pub placed: usize,
    pub misplaced: Vec<String>,
}

pub(super) struct Run<'a> {
    scene: &'a Scene,
    key: String,
    now: f64,
    due: BTreeMap<(i64, u64), Ev>,
    order: u64,
    claimed: HashSet<usize>,
    /// Message indices taken in and not yet read, beside `chat::UNSEEN`.
    unseen: Vec<usize>,
    /// What came and is not answered yet, kept across a restart.
    kept_waiting: Vec<usize>,
    /// What came and waits on the chat, kept across a restart.
    arriving: Vec<usize>,
    holds: VecDeque<Hold>,
    holding: Option<Vec<usize>>,
    pieces: Vec<SimPiece>,
    jobs: VecDeque<Job>,
    attached: bool,
    refused: HashSet<u32>,
    handled: HashMap<usize, usize>,
    tally: Tally,
    log: Vec<Value>,
}

impl<'a> Run<'a> {
    pub(super) fn new(scene: &'a Scene, run: usize) -> Self {
        let mut this = Self {
            scene,
            key: format!("sched:{}:{run}", scene.name),
            now: 0.0,
            due: BTreeMap::new(),
            order: 0,
            claimed: HashSet::new(),
            unseen: Vec::new(),
            kept_waiting: Vec::new(),
            arriving: Vec::new(),
            holds: VecDeque::new(),
            holding: None,
            pieces: Vec::new(),
            jobs: scene.jobs.iter().cloned().collect(),
            attached: true,
            refused: HashSet::new(),
            handled: HashMap::new(),
            tally: Tally::default(),
            log: Vec::new(),
        };
        for (index, said) in scene.said.iter().enumerate() {
            this.at(said.at, Ev::In(index));
        }
        if let Some(at) = scene.restart_at {
            this.at(at, Ev::Restart);
        }
        this
    }

    fn at(&mut self, at: f64, event: Ev) {
        self.order += 1;
        self.due.insert((ms(at), self.order), event);
    }

    fn note(&mut self, what: &str, value: Value) {
        self.log
            .push(json!({ "at": self.now, "what": what, "value": value }));
    }

    fn plain(&self) -> Vec<Piece> {
        self.pieces.iter().map(SimPiece::piece).collect()
    }

    pub(super) fn play(mut self, her: &mut dyn Her) -> (Tally, Vec<Value>) {
        while let Some((&(at, order), _)) = self.due.iter().next() {
            if at > ms(self.scene.until) {
                break;
            }
            let event = self.due.remove(&(at, order)).unwrap();
            self.now = at as f64 / 1000.0;
            self.step(event, her);
        }
        self.finish();
        // Whatever this scene left of the chat's reading is let go.
        if let Ok(mut unseen) = super::chat::UNSEEN.lock() {
            unseen.remove(&self.key);
        }
        (self.tally, self.log)
    }

    fn step(&mut self, event: Ev, her: &mut dyn Her) {
        match event {
            Ev::In(index) => {
                let said = &self.scene.said[index];
                let id = said.again_of.unwrap_or(index);
                // Claimed once, restart or not: a message handed in again
                // is not heard again.
                if !self.claimed.insert(id) {
                    self.note("ignoredAgain", json!(index));
                    return;
                }
                self.arriving.push(index);
                self.holds.push_back(Hold::Inbound(index));
                self.take_holds(her);
            }
            Ev::Look => {
                // She looks: what came is read together; she answers once
                // she has the chat.
                let texts = super::chat::UNSEEN
                    .lock()
                    .unwrap()
                    .remove(&self.key)
                    .map(|unseen| unseen.texts)
                    .unwrap_or_default();
                let read = std::mem::take(&mut self.unseen);
                assert_eq!(texts.len(), read.len(), "the chat's reading and the scene agree");
                if !read.is_empty() {
                    self.holds.push_back(Hold::Turn(read));
                    self.take_holds(her);
                }
            }
            Ev::Free => {
                if let Some(read) = self.holding.take() {
                    self.answer(read, her);
                }
                self.take_holds(her);
            }
            Ev::Work(number, round) => self.work(number, round),
            Ev::Flush(number) => self.flush(number),
            Ev::Restart => self.restart(),
            Ev::Recover => {
                self.attached = true;
                // What came unanswered is waited on again, then what waited
                // on the chat comes in again, in order.
                let waited = std::mem::take(&mut self.kept_waiting);
                for index in waited {
                    self.take_in(index);
                }
                let arriving = std::mem::take(&mut self.arriving);
                for index in arriving {
                    self.arriving.push(index);
                    self.holds.push_back(Hold::Inbound(index));
                }
                self.take_holds(her);
                let numbers: Vec<u32> = self.pieces.iter().map(|piece| piece.number).collect();
                for number in numbers {
                    self.flush(number);
                }
            }
        }
    }

    fn restart(&mut self) {
        self.note("restart", Value::Null);
        // In memory: gone. Her reply being written does not go out; what
        // she had in hand is still kept as waiting.
        super::chat::UNSEEN.lock().unwrap().remove(&self.key);
        self.unseen.clear();
        if let Some(read) = self.holding.take() {
            for index in read {
                if !self.kept_waiting.contains(&index) {
                    self.kept_waiting.push(index);
                }
            }
        }
        for hold in std::mem::take(&mut self.holds) {
            if let Hold::Turn(read) = hold {
                for index in read {
                    if !self.kept_waiting.contains(&index) {
                        self.kept_waiting.push(index);
                    }
                }
            }
        }
        // Timers and her looking are gone; Work goes on in its own runs,
        // its delivery picked up again by the recovery worker.
        self.due.retain(|_, event| {
            matches!(event, Ev::In(_) | Ev::Work(..) | Ev::Restart | Ev::Recover)
        });
        self.attached = false;
        self.at(self.now + RECOVER, Ev::Recover);
    }

    fn take_holds(&mut self, her: &mut dyn Her) {
        while self.holding.is_none() {
            let Some(hold) = self.holds.pop_front() else {
                return;
            };
            match hold {
                Hold::Inbound(index) => self.inbound(index),
                Hold::Turn(read) => {
                    // Holding the chat while she writes.
                    self.holding = Some(read);
                    self.at(self.now + TURN, Ev::Free);
                }
            }
        }
        let _ = her;
    }

    fn handled(&mut self, index: usize) {
        *self.handled.entry(index).or_default() += 1;
    }

    /// `continue_text`: a parked question first, else she reads it.
    fn inbound(&mut self, index: usize) {
        self.arriving.retain(|other| *other != index);
        let plain = self.plain();
        if let Route::Answer(number) = works::route(&plain) {
            self.handled(index);
            let text = self.scene.said[index].text;
            self.answer_piece(number, text);
            return;
        }
        self.take_in(index);
    }

    fn take_in(&mut self, index: usize) {
        let text = self.scene.said[index].text;
        if super::chat::take_in(&self.key, text, &[]) {
            self.at(self.now + LOOK, Ev::Look);
        }
        self.unseen.push(index);
        if !self.kept_waiting.contains(&index) {
            self.kept_waiting.push(index);
        }
    }

    /// Her turn ends: her reply goes out, and her lines are carried out.
    fn answer(&mut self, read: Vec<usize>, her: &mut dyn Her) {
        for index in &read {
            self.handled(*index);
        }
        self.kept_waiting.retain(|index| !read.contains(index));
        let plain = self.plain();
        let waiting = plain
            .iter()
            .filter(|piece| matches!(piece.standing, Standing::Waiting { .. }))
            .count();
        let saids: Vec<&Said> = read.iter().map(|index| &self.scene.said[*index]).collect();
        let raw = her.lines(&saids, &plain, waiting);
        let (spoken, hand_offs) = delegate::split(&raw);
        self.note(
            "turn",
            json!({ "read": read, "said": spoken, "lines": hand_offs }),
        );
        self.score_placing(&read, &hand_offs);
        for hand_off in hand_offs {
            let plain = self.plain();
            match works::act(&hand_off, &plain) {
                Act::Start(asked) => {
                    let from = read
                        .iter()
                        .copied()
                        .find(|index| matches!(self.scene.said[*index].meant, Meant::New(text) if asked == text))
                        .unwrap_or(read[0]);
                    self.start(asked, from);
                }
                Act::Answer(number, text) => self.answer_piece(number, &text),
                Act::Continue(number, text) => {
                    if let Some(piece) = self.piece_mut(number) {
                        piece.inputs.push(text);
                        piece.finished = false;
                        piece.continued += 1;
                        piece.job.secs += 20.0;
                    }
                    self.go_on(number);
                }
                Act::Steer(number, text) => {
                    if let Some(piece) = self.piece_mut(number) {
                        piece.inputs.push(text);
                    }
                }
                Act::Stop(number) => {
                    if let Some(piece) = self.piece_mut(number) {
                        piece.stopped = true;
                        piece.waiting = None;
                        piece.round += 1;
                    }
                    self.note("stopped", json!(number));
                }
                Act::Nothing => self.note("nothing", json!(format!("{hand_off:?}"))),
            }
        }
    }

    /// Whether her lines placed each message where it was meant.
    fn score_placing(&mut self, read: &[usize], hand_offs: &[HandOff]) {
        let mut want: Vec<String> = Vec::new();
        for index in read {
            match &self.scene.said[*index].meant {
                Meant::Chat => {}
                Meant::New(_) => want.push("new".into()),
                Meant::To(work, _) => want.push(format!("to#{work}")),
                Meant::Stop(work) => want.push(format!("stop#{work}")),
            }
        }
        let mut got: Vec<String> = hand_offs
            .iter()
            .map(|hand_off| match hand_off {
                HandOff::New { .. } => "new".into(),
                HandOff::To { work, .. } => format!("to#{work}"),
                HandOff::Stop { work } => format!("stop#{work}"),
            })
            .collect();
        want.sort();
        got.sort();
        if want == got {
            self.tally.placed += 1;
        } else {
            let texts: Vec<&str> = read.iter().map(|index| self.scene.said[*index].text).collect();
            self.tally
                .misplaced
                .push(format!("{texts:?}: want {want:?}, got {got:?}"));
        }
    }

    fn piece_mut(&mut self, number: u32) -> Option<&mut SimPiece> {
        self.pieces.iter_mut().find(|piece| piece.number == number)
    }

    fn start(&mut self, asked: String, from: usize) {
        let number = works::next_number(self.pieces.iter().map(|piece| piece.number));
        let job = self.jobs.pop_front().unwrap_or_else(|| job(30.0));
        self.pieces.push(SimPiece {
            number,
            asked,
            from,
            job,
            worked: 0.0,
            since: None,
            round: 0,
            asked_once: false,
            waiting: None,
            stopped: false,
            finished: false,
            inputs: Vec::new(),
            produced: Vec::new(),
            cursor: 0,
            results_sent: 0,
            result_at: None,
            continued: 0,
        });
        self.go_on(number);
    }

    /// Runs on until its question or its end.
    fn go_on(&mut self, number: u32) {
        let now = self.now;
        let Some(piece) = self.piece_mut(number) else {
            return;
        };
        piece.since = Some(now);
        let next = match piece.job.asks {
            Some((after, ..)) if !piece.asked_once => after,
            _ => piece.job.secs,
        };
        let left = (next - piece.worked).max(0.0);
        let round = piece.round;
        self.at(now + left, Ev::Work(number, round));
    }

    fn work(&mut self, number: u32, round: u64) {
        let now = self.now;
        let Some(piece) = self.piece_mut(number) else {
            return;
        };
        if piece.round != round || piece.stopped {
            return;
        }
        if let Some(since) = piece.since.take() {
            piece.worked += now - since;
        }
        match piece.job.asks {
            Some((_, question, options)) if !piece.asked_once => {
                piece.asked_once = true;
                let prompt = works::stamp(
                    number,
                    PendingPrompt {
                        id: String::new(),
                        kind: PendingKind::Answer {
                            task_id: format!("task-{number}"),
                            question_id: "q".into(),
                            question_type: if options.is_empty() {
                                "free_text".into()
                            } else {
                                "single_choice".into()
                            },
                        },
                        question: question.into(),
                        options: options
                            .iter()
                            .map(|option| PendingOption {
                                value: option.to_string(),
                                label: option.to_string(),
                            })
                            .collect(),
                        expires_at_unix: None,
                    },
                );
                let text = format_pending_prompt(&prompt);
                piece.waiting = Some(prompt);
                piece.produced.push((text, false));
            }
            _ => {
                piece.finished = true;
                piece.produced.push((format!("{} 办好了", piece.asked), true));
            }
        }
        self.flush(number);
    }

    /// Deliver what the piece produced past its cursor, one item at a time.
    fn flush(&mut self, number: u32) {
        if !self.attached {
            return;
        }
        loop {
            let others = self.pieces.iter().any(|other| {
                other.number != number && !other.stopped && (other.waiting.is_some() || !other.finished)
            });
            let refuse = self.scene.refuse_first_of == Some(number) && !self.refused.contains(&number);
            let now = self.now;
            let Some(piece) = self.piece_mut(number) else {
                return;
            };
            if piece.stopped {
                return;
            }
            let Some((content, is_result)) = piece.produced.get(piece.cursor).cloned() else {
                return;
            };
            if is_result && refuse {
                self.refused.insert(number);
                self.note("refused", json!(number));
                self.at(now + RETRY, Ev::Flush(number));
                return;
            }
            let (asked, from) = (piece.asked.clone(), piece.from);
            piece.cursor += 1;
            if is_result {
                piece.results_sent += 1;
                piece.result_at.get_or_insert(now);
            }
            let sent = works::labelled(number, &asked, others, &content);
            if others && !sent.starts_with(&works::label(number, &asked)) {
                self.tally.unlabelled_while_others += 1;
            }
            let started = self.scene.said[from].at;
            if is_result {
                self.tally.latencies.push(now - started);
            }
            self.note("sent", json!({ "piece": number, "text": sent }));
        }
    }

    /// The answer, or more, for a piece waiting on its question.
    fn answer_piece(&mut self, number: u32, text: &str) {
        let Some(prompt) = self.piece_mut(number).and_then(|piece| piece.waiting.clone()) else {
            return;
        };
        match decide_pending_reply(&prompt, text, 0) {
            PendingDecision::Resume { answer, .. } => {
                if let Some(piece) = self.piece_mut(number) {
                    piece.waiting = None;
                    piece.inputs.push(text.to_string());
                    piece.inputs.push(format!("answer:{answer}"));
                }
                self.go_on(number);
            }
            PendingDecision::Reask { .. } => {
                // Not an answer: kept for the piece, and asked again.
                if let Some(piece) = self.piece_mut(number) {
                    piece.inputs.push(text.to_string());
                }
                self.note("askedAgain", json!(number));
            }
            PendingDecision::Expired { .. } => {}
        }
    }

    fn finish(&mut self) {
        let tally = &mut self.tally;
        for (index, said) in self.scene.said.iter().enumerate() {
            if said.again_of.is_some() {
                if self.handled.contains_key(&index) {
                    tally.heard_again += 1;
                }
                continue;
            }
            tally.messages += 1;
            match self.handled.get(&index).copied().unwrap_or(0) {
                0 => tally.missed += 1,
                1 => {}
                _ => tally.duplicated += 1,
            }
        }
        for piece in &self.pieces {
            if piece.stopped {
                continue;
            }
            tally.results += 1;
            let want = 1 + piece.continued;
            match piece.results_sent {
                0 => tally.results_missed += 1,
                sent if sent < want => tally.results_missed += 1,
                sent if sent == want => {}
                _ => tally.results_duplicated += 1,
            }
        }
        for said in &self.scene.said {
            match &said.meant {
                Meant::To(work, text) => {
                    let into = |piece: &SimPiece| piece.inputs.iter().any(|input| input.contains(text));
                    let right = self
                        .pieces
                        .iter()
                        .all(|piece| into(piece) == (piece.number == *work));
                    if right {
                        tally.more_right += 1;
                    } else {
                        tally.more_wrong += 1;
                    }
                }
                Meant::Stop(work) => {
                    let right = self
                        .pieces
                        .iter()
                        .all(|piece| piece.stopped == (piece.number == *work));
                    if right {
                        tally.stops_right += 1;
                    } else {
                        tally.stops_wrong += 1;
                    }
                }
                _ => {}
            }
        }
    }
}

pub(super) fn private_scenes() -> Vec<Scene> {
    use Meant::*;
    vec![
        Scene {
            name: "several_lines_in_a_row",
            said: vec![
                said(0.0, "在吗", Chat),
                said(0.6, "问你个事", Chat),
                said(1.2, "帮我查一下明天东京天气", New("查明天东京天气")),
            ],
            jobs: vec![job(40.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 600.0,
        },
        Scene {
            name: "called_while_she_answers",
            said: vec![
                said(0.0, "今天好累", Chat),
                said(4.0, "对了帮我把周报整理一下", New("整理周报")),
                said(5.0, "要按项目分", Chat),
            ],
            jobs: vec![job(30.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 600.0,
        },
        Scene {
            name: "three_at_once",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(20.0, "再帮我订周五的餐厅", New("订周五的餐厅")),
                said(40.0, "还有把周报整理一下", New("整理周报")),
            ],
            jobs: vec![job(60.0), job(90.0), job(30.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
        },
        Scene {
            name: "more_and_a_change_of_mind",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(20.0, "订周五的餐厅", New("订周五的餐厅")),
                said(40.0, "餐厅改成周六吧", To(2, "改成周六")),
                said(55.0, "天气顺便看下大阪", To(1, "顺便看大阪")),
            ],
            jobs: vec![job(90.0), job(120.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
        },
        Scene {
            name: "stop_one_of_three",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(20.0, "订周五的餐厅", New("订周五的餐厅")),
                said(40.0, "整理周报", New("整理周报")),
                said(60.0, "餐厅那个不用了", Stop(2)),
            ],
            jobs: vec![job(120.0), job(120.0), job(120.0)],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
        },
        Scene {
            name: "two_questions_waiting",
            said: vec![
                said(0.0, "订周五的餐厅", New("订周五的餐厅")),
                said(20.0, "订周六的电影票", New("订周六的电影票")),
                // Both ask by 60s; she places the answer.
                said(80.0, "餐厅 19 点", To(1, "19:00")),
                // Only the cinema waits now: it takes the answer as it is.
                said(120.0, "2", Chat),
            ],
            jobs: vec![
                asking(90.0, 30.0, "要几点？", &["18:00", "19:00"]),
                asking(90.0, 40.0, "哪一场？", &["14:00", "20:00"]),
            ],
            restart_at: None,
            refuse_first_of: None,
            until: 900.0,
        },
        Scene {
            name: "a_send_refused_is_sent_again_alone",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(10.0, "订周五的餐厅", New("订周五的餐厅")),
            ],
            jobs: vec![job(60.0), job(30.0)],
            restart_at: None,
            refuse_first_of: Some(2),
            until: 600.0,
        },
        Scene {
            name: "a_restart_in_the_middle",
            said: vec![
                said(0.0, "查明天东京天气", New("查明天东京天气")),
                said(20.0, "订周五的餐厅", New("订周五的餐厅")),
                // She is writing her answer to this when it restarts...
                said(50.0, "餐厅改成周六吧", To(2, "改成周六")),
                // ...and this waits on the chat meanwhile.
                said(53.0, "今天好冷", Chat),
                // The platform hands in what it handed in before.
                Said {
                    again_of: Some(1),
                    ..said(70.0, "订周五的餐厅", Chat)
                },
            ],
            jobs: vec![job(65.0), job(120.0)],
            restart_at: Some(55.0),
            refuse_first_of: None,
            until: 900.0,
        },
    ]
}

// ------------------------------------------------------------ groups

/// A group scene: who calls her when, and how long her turns take.
pub(super) struct GroupScene {
    pub name: &'static str,
    /// (at, from, text, addressed)
    pub lines: Vec<(f64, i64, &'static str, bool)>,
    pub turn_secs: f64,
    /// Restart at, and lines handed in again after it.
    pub restart_at: Option<f64>,
    pub until: f64,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GroupTally {
    pub called: usize,
    pub missed: usize,
    pub duplicated: usize,
    /// Lines the platform handed in again after a restart, not heard again.
    pub again_ignored: usize,
    pub latencies: Vec<f64>,
}

pub(super) fn group_scenes() -> Vec<GroupScene> {
    vec![
        GroupScene {
            name: "three_call_her_at_once",
            lines: vec![
                (0.0, 11, "@bot 周五去哪", true),
                (0.3, 12, "@bot 你吃火锅吗", true),
                (0.6, 13, "@bot 晚上打游戏吗", true),
            ],
            turn_secs: 8.0,
            restart_at: None,
            until: 900.0,
        },
        GroupScene {
            name: "called_while_she_replies",
            lines: vec![
                (0.0, 11, "@bot 周五去哪", true),
                (5.0, 12, "@bot 你吃火锅吗", true),
                (6.0, 12, "要辣的", false),
                (9.0, 13, "@bot 在吗", true),
            ],
            turn_secs: 12.0,
            restart_at: None,
            until: 900.0,
        },
        GroupScene {
            name: "a_restart_while_called",
            lines: vec![
                (0.0, 11, "@bot 周五去哪", true),
                (30.0, 12, "@bot 你吃火锅吗", true),
            ],
            turn_secs: 8.0,
            restart_at: Some(32.0),
            until: 900.0,
        },
    ]
}

mod group_end {
    use super::*;
    use myriad_merope::group::{
        self as core, Answer, At, Effect, Event, Group, Her, May, Message, Resume, Rolls, ToRead,
        Who,
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
            self.log.push(json!({ "at": (self.now - BASE) as f64 / 1e6, "restart": true }));
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

    pub(in super::super) fn play(scene: &GroupScene) -> (GroupTally, Vec<Value>) {
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
            let reply = end
                .replies
                .iter()
                .find(|(when, _, read)| *when >= us(*at) && read.iter().any(|line| line.contains(text.trim_start_matches("@bot "))));
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
}

// ------------------------------------------------------------ the report

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank]
}

/// Every scene played with `her`, and the numbers.
pub(super) fn evaluate(her: &mut dyn Her) -> Value {
    let mut scenes = Vec::new();
    let mut latencies = Vec::new();
    let mut total = Tally::default();
    for (run, scene) in private_scenes().iter().enumerate() {
        let (tally, log) = Run::new(scene, run).play(her);
        latencies.extend(tally.latencies.iter().copied());
        total.messages += tally.messages;
        total.missed += tally.missed;
        total.duplicated += tally.duplicated;
        total.heard_again += tally.heard_again;
        total.results += tally.results;
        total.results_missed += tally.results_missed;
        total.results_duplicated += tally.results_duplicated;
        total.more_right += tally.more_right;
        total.more_wrong += tally.more_wrong;
        total.stops_right += tally.stops_right;
        total.stops_wrong += tally.stops_wrong;
        total.unlabelled_while_others += tally.unlabelled_while_others;
        total.placed += tally.placed;
        total.misplaced.extend(tally.misplaced.iter().cloned());
        scenes.push(json!({ "name": scene.name, "tally": tally, "log": log }));
    }
    let mut group_latencies = Vec::new();
    let mut groups = Vec::new();
    let mut group_total = GroupTally::default();
    for scene in group_scenes() {
        let (tally, log) = group_end::play(&scene);
        group_latencies.extend(tally.latencies.iter().copied());
        group_total.called += tally.called;
        group_total.missed += tally.missed;
        group_total.duplicated += tally.duplicated;
        group_total.again_ignored += tally.again_ignored;
        groups.push(json!({ "name": scene.name, "tally": tally, "log": log }));
    }
    json!({
        "private": {
            "messages": total.messages,
            "missed": total.missed,
            "duplicated": total.duplicated,
            "heardAgain": total.heard_again,
            "results": total.results,
            "resultsMissed": total.results_missed,
            "resultsDuplicated": total.results_duplicated,
            "moreRight": total.more_right,
            "moreWrong": total.more_wrong,
            "stopsRight": total.stops_right,
            "stopsWrong": total.stops_wrong,
            "unlabelledWhileOthers": total.unlabelled_while_others,
            "turnsPlaced": total.placed,
            "misplaced": total.misplaced,
            "resultP50": percentile(&latencies, 50.0),
            "resultP95": percentile(&latencies, 95.0),
        },
        "group": {
            "called": group_total.called,
            "missed": group_total.missed,
            "duplicated": group_total.duplicated,
            "againIgnored": group_total.again_ignored,
            "replyP50": percentile(&group_latencies, 50.0),
            "replyP95": percentile(&group_latencies, 95.0),
        },
        "scenes": scenes,
        "groups": groups,
    })
}

fn write_report(report: &Value) {
    if let Ok(path) = std::env::var("MYRIAD_SCHEDULING_REPORT") {
        std::fs::write(&path, serde_json::to_string_pretty(report).unwrap()).unwrap();
        eprintln!("scheduling report written to {path}");
    }
}

#[test]
fn scheduling_holds_with_scripted_judgment() {
    let report = evaluate(&mut Scripted);
    write_report(&report);
    let private = &report["private"];
    let group = &report["group"];
    eprintln!(
        "private {}\ngroup {}",
        serde_json::to_string(private).unwrap(),
        serde_json::to_string(group).unwrap()
    );
    assert_eq!(private["missed"], 0, "{report:#}");
    assert_eq!(private["duplicated"], 0, "{report:#}");
    assert_eq!(private["heardAgain"], 0, "{report:#}");
    assert_eq!(private["resultsMissed"], 0, "{report:#}");
    assert_eq!(private["resultsDuplicated"], 0, "{report:#}");
    assert_eq!(private["moreWrong"], 0, "{report:#}");
    assert_eq!(private["stopsWrong"], 0, "{report:#}");
    assert_eq!(private["unlabelledWhileOthers"], 0, "{report:#}");
    assert!(private["moreRight"].as_u64().unwrap() >= 4);
    assert_eq!(private["stopsRight"], 1);
    assert_eq!(group["missed"], 0, "{report:#}");
    assert_eq!(group["duplicated"], 0, "{report:#}");
}

// ------------------------------------------------------------ live

/// Her lines from the configured chat model, given what she sees of the
/// pieces as the channel would show it.
struct Live {
    analyzer: std::sync::Arc<crate::services::analyzer::AiAnalyzer>,
    soul: String,
    runtime: tokio::runtime::Handle,
    history: Vec<String>,
}

impl Her for Live {
    fn lines(&mut self, read: &[&Said], pieces: &[Piece], waiting: usize) -> String {
        let section = delegate::section(&crate::services::agent::types::ChannelChat {
            works: works::shown(pieces.to_vec(), 0)
                .iter()
                .map(|piece| works::line(piece, 0))
                .collect(),
            waiting,
            came_while_away: None,
        });
        let system = format!(
            "{}\n\nYou are talking with someone in a private chat app conversation. Reply in their language, briefly.\n\n{section}",
            self.soul
        );
        let input = read.iter().map(|said| said.text).collect::<Vec<_>>().join("\n");
        let mut context = self.history.join("\n");
        if !context.is_empty() {
            context = format!("Earlier in this chat:\n{context}\n\nThey now say:\n");
        }
        let analyzer = self.analyzer.clone();
        let prompt = format!("{context}{input}");
        let raw = tokio::task::block_in_place(|| {
            self.runtime
                .block_on(async { analyzer.analyze_with_system(&system, &prompt).await })
        })
        .unwrap_or_default();
        self.history.push(format!("them: {input}"));
        self.history.push(format!("you: {}", delegate::split(&raw).0));
        raw
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "asks the configured chat model; MYRIAD_SCHEDULING_LIVE=1"]
async fn live() {
    if std::env::var("MYRIAD_SCHEDULING_LIVE").as_deref() != Ok("1") {
        eprintln!("live: set MYRIAD_SCHEDULING_LIVE=1");
        return;
    }
    let mut soul = crate::services::agent::semantic_eval::configured_soul().await;
    if soul.is_empty() {
        soul = "你是她。".into();
    }
    let analyzer = std::sync::Arc::new(
        crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
            std::time::Duration::from_secs(60),
        ))
        .await
        .expect("configured Lite unavailable")
        .with_light_thinking(),
    );
    let runtime = tokio::runtime::Handle::current();
    let mut placed = 0;
    let mut turns = 0;
    let mut misplaced = Vec::new();
    let mut reports = Vec::new();
    let repeats: usize = std::env::var("MYRIAD_SCHEDULING_REPEAT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    for round in 0..repeats {
        for (run, scene) in private_scenes().iter().enumerate() {
            let mut her = Live {
                analyzer: analyzer.clone(),
                soul: soul.clone(),
                runtime: runtime.clone(),
                history: Vec::new(),
            };
            let (tally, log) = Run::new(scene, 1000 + round * 100 + run).play(&mut her);
            turns += tally.placed + tally.misplaced.len();
            placed += tally.placed;
            misplaced.extend(tally.misplaced.iter().map(|line| format!("{}: {line}", scene.name)));
            reports.push(json!({ "name": scene.name, "tally": tally, "log": log }));
        }
    }
    let report = json!({
        "turns": turns,
        "placed": placed,
        "misplaced": misplaced,
        "scenes": reports,
    });
    write_report(&report);
    eprintln!(
        "live: {placed}/{turns} turns placed as meant\n{}",
        misplaced.join("\n")
    );
}
