//! A private chat played on the simulated clock under the channel's rules.

use super::*;

#[derive(Debug, Clone)]
pub(super) struct SimPiece {
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
    /// The messages that reached it after it began.
    heard: Vec<usize>,
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
    /// How it stands, read by the product's own rule.
    fn standing(&self) -> Standing {
        works::stands(&Facts {
            stopped: self.stopped,
            question: self.waiting.as_ref().map(format_pending_prompt),
            under_way: !self.finished,
            ..Facts::default()
        })
    }

    fn piece(&self) -> Piece {
        Piece {
            number: self.number,
            asked: self.asked.clone(),
            started_at: Some(0),
            ended_at: None,
            standing: self.standing(),
        }
    }
}

#[derive(Debug)]
pub(super) enum Ev {
    In(usize),
    Look,
    Free,
    Work(u32, u64),
    Flush(u32),
    Restart,
    Recover,
}

pub(super) enum Hold {
    Inbound(usize),
    Turn(Vec<usize>),
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
        if let Ok(mut unseen) = super::super::chat::UNSEEN.lock() {
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
                match said.meant {
                    Meant::OnPiece(number, text) => {
                        self.handled(index);
                        self.answer_piece(number, text, &[index]);
                        return;
                    }
                    Meant::StopPiece(number) => {
                        self.handled(index);
                        self.stop(number);
                        return;
                    }
                    _ => {}
                }
                self.arriving.push(index);
                self.holds.push_back(Hold::Inbound(index));
                self.take_holds(her);
            }
            Ev::Look => {
                // She looks: what came is read together; she answers once
                // she has the chat.
                let texts = super::super::chat::UNSEEN
                    .lock()
                    .unwrap()
                    .remove(&self.key)
                    .map(|unseen| unseen.texts)
                    .unwrap_or_default();
                let read = std::mem::take(&mut self.unseen);
                assert_eq!(
                    texts.len(),
                    read.len(),
                    "the chat's reading and the scene agree"
                );
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
        super::super::chat::UNSEEN.lock().unwrap().remove(&self.key);
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
        if !self.scene.panel
            && let Route::Answer { work: number } = works::route(&plain)
        {
            self.handled(index);
            let text = self.scene.said[index].text;
            self.answer_piece(number, text, &[index]);
            return;
        }
        self.take_in(index);
    }

    fn take_in(&mut self, index: usize) {
        let text = self.scene.said[index].text;
        if super::super::chat::take_in(&self.key, text, &[]) {
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
                Act::Start { text: asked } => {
                    let from = read
                        .iter()
                        .copied()
                        .find(|index| matches!(self.scene.said[*index].meant, Meant::New(text) if asked == text))
                        .unwrap_or(read[0]);
                    self.start(asked, from);
                }
                Act::Answer { work: number, text } => {
                    let text = match self
                        .piece_mut(number)
                        .and_then(|piece| piece.waiting.clone())
                    {
                        Some(prompt) => works::her_choice(&prompt, &text),
                        None => text,
                    };
                    self.answer_piece(number, &text, &read);
                }
                Act::Continue { work: number, text } => {
                    if let Some(piece) = self.piece_mut(number) {
                        piece.inputs.push(text);
                        piece.heard.extend(&read);
                        piece.finished = false;
                        piece.continued += 1;
                        piece.job.secs += 20.0;
                    }
                    self.go_on(number);
                }
                Act::Steer { work: number, text } => {
                    if let Some(piece) = self.piece_mut(number) {
                        piece.inputs.push(text);
                        piece.heard.extend(&read);
                    }
                }
                Act::Stop { work: number } => self.stop(number),
                // Nothing here is cut off by a restart.
                Act::PickUp { .. } | Act::Nothing => {
                    self.note("nothing", json!(format!("{hand_off:?}")))
                }
            }
        }
    }

    fn stop(&mut self, number: u32) {
        if let Some(piece) = self.piece_mut(number) {
            piece.stopped = true;
            piece.waiting = None;
            piece.round += 1;
        }
        self.note("stopped", json!(number));
    }

    /// Whether her lines placed each message where it was meant.
    fn score_placing(&mut self, read: &[usize], hand_offs: &[HandOff]) {
        let mut want: Vec<String> = Vec::new();
        for index in read {
            match &self.scene.said[*index].meant {
                Meant::Chat | Meant::OnPiece(..) | Meant::StopPiece(_) => {}
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
            let texts: Vec<&str> = read
                .iter()
                .map(|index| self.scene.said[*index].text)
                .collect();
            self.tally
                .misplaced
                .push(format!("{texts:?}: want {want:?}, got {got:?}"));
        }
    }

    fn piece_mut(&mut self, number: u32) -> Option<&mut SimPiece> {
        self.pieces.iter_mut().find(|piece| piece.number == number)
    }

    fn start(&mut self, asked: String, from: usize) {
        let number = works::next_number(self.pieces.iter().map(|piece| piece.number)).unwrap();
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
            heard: Vec::new(),
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
                piece
                    .produced
                    .push((format!("{} 办好了", piece.asked), true));
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
                other.number != number
                    && !other.stopped
                    && (other.waiting.is_some() || !other.finished)
            });
            let refuse =
                self.scene.refuse_first_of == Some(number) && !self.refused.contains(&number);
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
            if others && !sent.starts_with(&myriad_merope::handoff::label(number, &asked)) {
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
    fn answer_piece(&mut self, number: u32, text: &str, from: &[usize]) {
        let Some(prompt) = self
            .piece_mut(number)
            .and_then(|piece| piece.waiting.clone())
        else {
            return;
        };
        match decide_pending_reply(&prompt, text, 0) {
            PendingDecision::Resume { answer, .. } => {
                if let Some(piece) = self.piece_mut(number) {
                    piece.waiting = None;
                    piece.heard.extend(from);
                    piece.inputs.push(text.to_string());
                    piece.inputs.push(format!("answer:{answer}"));
                }
                self.go_on(number);
            }
            PendingDecision::Reask { .. } => {
                // Not an answer: kept for the piece, and asked again.
                if let Some(piece) = self.piece_mut(number) {
                    piece.heard.extend(from);
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
        for (index, said) in self.scene.said.iter().enumerate() {
            match &said.meant {
                Meant::To(work, _) | Meant::OnPiece(work, _) => {
                    let into = |piece: &SimPiece| piece.heard.contains(&index);
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
                Meant::Stop(work) | Meant::StopPiece(work) => {
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
