//! A game she hosts at a table (D2c): one shape for every kind, the rules of
//! each plugged in.
//!
//! The rules are mechanical and deterministic: who sits where, what each seat
//! is dealt (by the rolls the end hands in), what may be done now, how a move
//! changes the game, when it is over. No rule asks a model. A model is asked
//! only where meaning is needed: whether a question fits a story's truth, what
//! she says in her own voice, which of the moves open to her seat she takes
//! when she plays one. Those asks are given views, never the game itself.
//!
//! Every fact a game holds says whom it is for: everyone at the table, one
//! seat, or the host. What anyone is shown is made from the facts for them
//! and nothing else: a [`SeatView`] has the facts for all and for that seat;
//! a [`PublicView`] the facts for all; a [`HostView`] everything, only for a
//! game she hosts herself. Views are made here only, from a game's facts; a
//! prompt for her as a player or as the voice of a game the core referees
//! takes a view, so what she does not know as a seat cannot reach her.
//!
//! ```compile_fail
//! // A view cannot be made by hand, with facts of anyone's choosing.
//! let view = myriad_merope::hosting::SeatView { seat: 0, facts: Vec::new(), choices: Vec::new() };
//! ```

use serde::{Deserialize, Serialize};

use crate::group::Rolls;

/// Whom a fact of a game is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "to", content = "seat")]
pub enum Audience {
    All,
    Seat(usize),
    Host,
}

/// One thing a game holds, and whom it is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub to: Audience,
    pub text: String,
    /// For all, but heard at the table already (what a seat said there):
    /// not announced again.
    #[serde(default)]
    pub heard: bool,
}

impl Fact {
    pub fn all(text: impl Into<String>) -> Self {
        Self {
            to: Audience::All,
            text: text.into(),
            heard: false,
        }
    }

    /// For all, as heard at the table already.
    pub fn heard(text: impl Into<String>) -> Self {
        Self {
            to: Audience::All,
            text: text.into(),
            heard: true,
        }
    }

    pub fn seat(seat: usize, text: impl Into<String>) -> Self {
        Self {
            to: Audience::Seat(seat),
            text: text.into(),
            heard: false,
        }
    }

    pub fn host(text: impl Into<String>) -> Self {
        Self {
            to: Audience::Host,
            text: text.into(),
            heard: false,
        }
    }
}

/// Someone at the table, and whether it is her.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seat {
    /// Who they are on the platform (`telegram:123`).
    pub who: String,
    pub name: String,
    #[serde(default)]
    pub hers: bool,
}

/// The rules of one kind of game.
pub trait Rules {
    type State;
    /// What the game is called where it is kept (`soup`).
    const KIND: &'static str;
    /// Who sits at it; empty when anyone at the table may play (a story the
    /// table guesses together).
    fn seats(state: &Self::State) -> Vec<Seat>;
    /// Everything it holds, each with whom it is for.
    fn facts(state: &Self::State) -> Vec<Fact>;
    /// What `seat` may do now, as plain choices.
    fn choices(state: &Self::State, seat: usize) -> Vec<String>;
    /// `seat` takes choice `index` (one of [`Rules::choices`]); chance from
    /// `rolls`. A choice that is not open does nothing.
    fn take(state: &mut Self::State, seat: usize, index: usize, rolls: &mut Rolls);
    fn over(state: &Self::State) -> bool;

    /// The rules as a player is told them, when she plays one.
    const RULES: &'static str = "";

    /// Seats learn things as it goes (what they found at night), not only
    /// what they were dealt: what is new for a seat is whispered to it.
    const NEWS_IN_PRIVATE: bool = false;

    /// Whether the table may know who the game waits for now: then one who
    /// lets it pass twice is named in the group, else reminded in private
    /// (a move at night would tell their role).
    fn public_wait(_: &Self::State) -> bool {
        true
    }

    /// Before and after the name of the seat whose turn it is to speak
    /// (none: as [`speak_turn_lines`] has it).
    fn speak_turn(_: &Self::State) -> Option<(String, String)> {
        None
    }

    /// What the game waits for now: by default, the seats with choices
    /// open choose.
    fn waiting(state: &Self::State) -> Waiting {
        if Self::over(state) {
            return Waiting::Over;
        }
        let seats = (0..Self::seats(state).len().max(1))
            .filter(|seat| !Self::choices(state, *seat).is_empty())
            .collect();
        Waiting::Choose(seats)
    }

    /// Which wait it is: the same while the game waits for the same thing
    /// (seats choosing one by one in it), another once it moved on.
    fn phase(state: &Self::State) -> u64 {
        Self::facts(state).len() as u64
    }

    /// How long the game waits, in seconds.
    fn seconds(_: &Self::State) -> f64 {
        MOVE_SECONDS
    }

    /// `seat`, whose turn it is to speak, said `text` at the table.
    fn spoke(_: &mut Self::State, _: usize, _: &str) {}

    /// What a judgment the game waited for came to (none when it could not
    /// be had).
    fn judged(_: &mut Self::State, _: Option<&str>, _: &mut Rolls) {}

    /// Nobody waited for moved in time: by default each passes with its
    /// first choice.
    fn timed_out(state: &mut Self::State, rolls: &mut Rolls) {
        if let Waiting::Choose(seats) = Self::waiting(state) {
            for seat in seats {
                Self::take(state, seat, 0, rolls);
            }
        }
    }

    /// How many moments worth a word of hers it has had (it began, someone
    /// went out, it is over).
    fn moments(_: &Self::State) -> usize {
        0
    }
}

/// What a game waits for.
#[derive(Debug, Clone, PartialEq)]
pub enum Waiting {
    /// These seats each choose among their choices, in private.
    Choose(Vec<usize>),
    /// This seat speaks to the table, in turn.
    Speak(usize),
    /// What a seat said needs judging, which only a model can do: asked
    /// as a judge (not as her), with what it takes.
    Judge {
        system: String,
        input: String,
        schema_name: &'static str,
        schema: serde_json::Value,
    },
    Over,
}

/// What one seat knows, and may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatView {
    seat: usize,
    facts: Vec<String>,
    choices: Vec<String>,
}

impl SeatView {
    pub fn seat(&self) -> usize {
        self.seat
    }

    pub fn facts(&self) -> &[String] {
        &self.facts
    }

    pub fn choices(&self) -> &[String] {
        &self.choices
    }
}

/// What everyone at the table knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicView {
    facts: Vec<String>,
}

impl PublicView {
    pub fn facts(&self) -> &[String] {
        &self.facts
    }
}

/// Everything, for a game she hosts herself (she is no seat in it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostView {
    facts: Vec<String>,
}

impl HostView {
    pub fn facts(&self) -> &[String] {
        &self.facts
    }
}

/// What `seat` knows: the facts for all, and for that seat.
pub fn seat_view<R: Rules>(state: &R::State, seat: usize) -> SeatView {
    SeatView {
        seat,
        facts: R::facts(state)
            .into_iter()
            .filter(|fact| matches!(fact.to, Audience::All) || fact.to == Audience::Seat(seat))
            .map(|fact| fact.text)
            .collect(),
        choices: R::choices(state, seat),
    }
}

/// What everyone at the table knows.
pub fn public_view<R: Rules>(state: &R::State) -> PublicView {
    PublicView {
        facts: R::facts(state)
            .into_iter()
            .filter(|fact| matches!(fact.to, Audience::All))
            .map(|fact| fact.text)
            .collect(),
    }
}

/// Everything, when she hosts it and holds no seat; none when she does.
pub fn host_view<R: Rules>(state: &R::State) -> Option<HostView> {
    if R::seats(state).iter().any(|seat| seat.hers) {
        return None;
    }
    Some(HostView {
        facts: R::facts(state).into_iter().map(|fact| fact.text).collect(),
    })
}

/// What she announces to the table, in order: the facts for all that were
/// not heard there already.
pub fn announced<R: Rules>(state: &R::State) -> Vec<String> {
    R::facts(state)
        .into_iter()
        .filter(|fact| fact.to == Audience::All && !fact.heard)
        .map(|fact| fact.text)
        .collect()
}

/// What `seat` was dealt: the facts for it alone (what is whispered to it
/// when the game is dealt).
pub fn dealt<R: Rules>(state: &R::State, seat: usize) -> Vec<String> {
    R::facts(state)
        .into_iter()
        .filter(|fact| fact.to == Audience::Seat(seat))
        .map(|fact| fact.text)
        .collect()
}

/// What she is asked to say at the table as a player of `game`, her turn
/// to speak: from her seat's view alone. Answered as `{"say": "…"}`.
pub fn speaker_prompt(
    soul: &str,
    game: &str,
    rules: &str,
    how: &str,
    view: &SeatView,
) -> (String, String) {
    let system = format!(
        "{soul}\n\nYou are playing {game} as one of the players. {rules} You know only what is shown to you here; whatever others were dealt or did in secret, you do not know. It is your turn to speak at the table: {how} Answer as JSON {{\"say\": \"…\"}}, what you say in the group, one short line in Chinese."
    );
    let input = serde_json::json!({ "youKnow": view.facts }).to_string();
    (system, input)
}

pub const SAY_SCHEMA: &str = "merope_game_say";

pub fn say_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": { "say": { "type": "string" } },
        "required": ["say"],
        "additionalProperties": false
    })
}

pub const CHOICE_SCHEMA: &str = "merope_game_choice";

pub fn choice_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": { "n": { "type": "integer" } },
        "required": ["n"],
        "additionalProperties": false
    })
}

/// What she said at her turn, read back.
pub fn read_say(raw: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Said {
        say: String,
    }
    let said: Said = crate::answer::parse(raw)?;
    let said = said.say.trim().to_string();
    (!said.is_empty()).then_some(said)
}

/// What she is asked as a player of `game` (named, with its rules told in
/// `rules`): from her seat's view alone, the choices numbered.
pub fn player_prompt(soul: &str, game: &str, rules: &str, view: &SeatView) -> (String, String) {
    let system = format!(
        "{soul}\n\nYou are playing {game} as one of the players. {rules} You know only what is shown to you here; whatever others were dealt or did in secret, you do not know. Pick one of the choices by its number, as yourself. Answer as JSON {{\"n\": <number>}}."
    );
    let input = serde_json::json!({
        "youKnow": view.facts,
        "choices": view
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| serde_json::json!({ "n": index, "choice": choice }))
            .collect::<Vec<_>>(),
    })
    .to_string();
    (system, input)
}

/// What she says, in her own voice, for a game the core referees: from what
/// everyone knows, and what just happened in it.
pub fn narration_prompt(
    soul: &str,
    game: &str,
    view: &PublicView,
    latest: &[String],
) -> (String, String) {
    let system = format!(
        "{soul}\n\nYou are the voice of a game of {game} at this table: the rules are kept for you and what happened is told already; add a line of your own about what just happened, as yourself, short, in Chinese. Say only what everyone may know. Answer as JSON {{\"say\": \"…\"}}."
    );
    let input = serde_json::json!({ "everyoneKnows": view.facts, "justNow": latest }).to_string();
    (system, input)
}

/// The choice she made as a player, read back: one of her seat's choices,
/// or none.
pub fn read_choice(raw: &str, view: &SeatView) -> Option<usize> {
    #[derive(Deserialize)]
    struct Chosen {
        n: usize,
    }
    let chosen: Chosen = crate::answer::parse(raw)?;
    (chosen.n < view.choices.len()).then_some(chosen.n)
}

/// A game with private seats and nothing else, for tests of how a game she
/// hosts reaches its players.
#[cfg(any(test, feature = "test-games"))]
pub mod marks {
    use super::*;

    /// A game with private seats and nothing else, for tests: each seat is dealt a secret mark;
    /// a turn passes or guesses the next seat's mark; three rounds and it is
    /// over. It holds what a real party game does (secrets per seat, a public
    /// log, choices) and nothing else.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Marks {
        pub seats: Vec<Seat>,
        pub marks: Vec<String>,
        pub log: Vec<String>,
        pub turn: usize,
        pub rounds: usize,
    }

    impl Marks {
        pub fn deal(players: usize, hers: usize, rolls: &mut Rolls) -> Self {
            let seats = (0..players)
                .map(|seat| Seat {
                    who: format!("telegram:{seat}"),
                    name: format!("玩家{seat}"),
                    hers: seat == hers,
                })
                .collect();
            let marks = (0..players)
                .map(|seat| format!("SECRET-{seat}-{}", rolls.range(1000..=9999)))
                .collect();
            Self {
                seats,
                marks,
                log: Vec::new(),
                turn: 0,
                rounds: 0,
            }
        }
    }

    impl Rules for Marks {
        type State = Marks;
        const KIND: &'static str = "marks";

        fn seats(game: &Marks) -> Vec<Seat> {
            game.seats.clone()
        }

        fn facts(game: &Marks) -> Vec<Fact> {
            let mut facts: Vec<Fact> = game
                .marks
                .iter()
                .enumerate()
                .map(|(seat, mark)| Fact::seat(seat, format!("your mark: {mark}")))
                .collect();
            facts.extend(game.log.iter().map(Fact::all));
            facts
        }

        fn choices(game: &Marks, seat: usize) -> Vec<String> {
            if seat != game.turn || Self::over(game) {
                return Vec::new();
            }
            vec!["pass".into(), "guess the next seat's mark".into()]
        }

        fn take(game: &mut Marks, seat: usize, index: usize, rolls: &mut Rolls) {
            if Self::choices(game, seat).get(index).is_none() {
                return;
            }
            let next = (seat + 1) % game.seats.len();
            // A guess is a number drawn; whether it hit is told to all, the
            // mark itself never.
            let line = if index == 0 {
                format!("{} passed", game.seats[seat].name)
            } else {
                let guess = format!("SECRET-{next}-{}", rolls.range(1000..=9999));
                let hit = guess == game.marks[next];
                format!(
                    "{} guessed {}'s mark: {}",
                    game.seats[seat].name,
                    game.seats[next].name,
                    if hit { "right" } else { "wrong" }
                )
            };
            game.log.push(line);
            game.turn = next;
            if next == 0 {
                game.rounds += 1;
            }
        }

        fn over(game: &Marks) -> bool {
            game.rounds >= 3
        }
    }

    impl Marks {
        /// Dealt to those seated, in their order.
        pub fn deal_to(seats: Vec<Seat>, rolls: &mut Rolls) -> Self {
            let marks = (0..seats.len())
                .map(|seat| format!("SECRET-{seat}-{}", rolls.range(1000..=9999)))
                .collect();
            Self {
                seats,
                marks,
                log: Vec::new(),
                turn: 0,
                rounds: 0,
            }
        }
    }
}

/// The kinds of game with seats she can host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SeatGame {
    Undercover(crate::undercover::Undercover),
    Werewolf(crate::werewolf::Werewolf),
    Avalon(crate::avalon::Avalon),
    #[cfg(any(test, feature = "test-games"))]
    Marks(marks::Marks),
}

/// What a kind of seat game is called, and how many may play it.
pub struct SeatKind {
    pub kind: &'static str,
    pub name: &'static str,
    pub fewest: usize,
    pub most: usize,
    /// Something only a model can give is needed before it is dealt (the
    /// words of a game of 谁是卧底).
    pub prepares: bool,
    /// She may take a seat when one player is short and all agree
    /// (D2c-11 A).
    pub she_may_fill: bool,
}

/// The kinds of seat game there are.
pub fn seat_kinds() -> Vec<SeatKind> {
    vec![
        SeatKind {
            kind: crate::undercover::KIND,
            name: crate::undercover::NAME,
            fewest: crate::undercover::FEWEST,
            most: crate::undercover::MOST,
            prepares: true,
            she_may_fill: true,
        },
        SeatKind {
            kind: crate::werewolf::KIND,
            name: crate::werewolf::NAME,
            fewest: crate::werewolf::FEWEST,
            most: crate::werewolf::MOST,
            prepares: false,
            she_may_fill: true,
        },
        SeatKind {
            kind: crate::avalon::KIND,
            name: crate::avalon::NAME,
            fewest: crate::avalon::FEWEST,
            most: crate::avalon::MOST,
            prepares: false,
            she_may_fill: true,
        },
        #[cfg(any(test, feature = "test-games"))]
        SeatKind {
            kind: "marks",
            name: "记号",
            fewest: 2,
            most: 8,
            prepares: false,
            she_may_fill: true,
        },
    ]
}

pub fn seat_kind(kind: &str) -> Option<SeatKind> {
    seat_kinds().into_iter().find(|known| known.kind == kind)
}

/// What a kind asks before it is dealt: as `voice`, the system and input,
/// and the shape of the answer.
pub struct Prepare {
    pub voice: &'static str,
    pub operation: &'static str,
    pub system: String,
    pub input: String,
    pub schema_name: &'static str,
    pub schema: serde_json::Value,
}

/// Run `$body` with `$game` bound to the game whatever its kind, `$R` its
/// rules.
macro_rules! each {
    ($self:expr, $game:ident, $R:ident => $body:expr) => {
        match $self {
            SeatGame::Undercover($game) => {
                type $R = crate::undercover::Undercover;
                $body
            }
            SeatGame::Werewolf($game) => {
                type $R = crate::werewolf::Werewolf;
                $body
            }
            SeatGame::Avalon($game) => {
                type $R = crate::avalon::Avalon;
                $body
            }
            #[cfg(any(test, feature = "test-games"))]
            SeatGame::Marks($game) => {
                type $R = marks::Marks;
                $body
            }
        }
    };
}

impl SeatGame {
    /// What `kind` asks before it is dealt, if anything: `soul` hers when
    /// she only hosts it (none when she holds a seat: then she must not know
    /// what is dealt), `recent` what it was given lately.
    pub fn prepare(kind: &str, soul: Option<&str>, recent: &[String]) -> Option<Prepare> {
        match kind {
            crate::undercover::KIND => Some(crate::undercover::words_prompt(soul, recent)),
            _ => None,
        }
    }

    /// A game of `kind` dealt to `seats`, by `rolls`, with what was asked
    /// before (when it asks): none when it cannot be dealt with that.
    pub fn deal(
        kind: &str,
        seats: Vec<Seat>,
        prepared: Option<&str>,
        recent: &[String],
        rolls: &mut Rolls,
    ) -> Option<Self> {
        let _ = (&seats, &prepared, &recent, &rolls);
        match kind {
            crate::undercover::KIND => {
                let pair = crate::undercover::read_words(prepared?, recent)?;
                Some(Self::Undercover(crate::undercover::Undercover::deal(
                    seats, pair, rolls,
                )))
            }
            crate::werewolf::KIND => Some(Self::Werewolf(crate::werewolf::Werewolf::deal(
                seats, rolls,
            ))),
            crate::avalon::KIND => Some(Self::Avalon(crate::avalon::Avalon::deal(seats, rolls))),
            #[cfg(any(test, feature = "test-games"))]
            "marks" => Some(Self::Marks(marks::Marks::deal_to(seats, rolls))),
            _ => None,
        }
    }

    /// What was used dealing it, kept so the next is not the same.
    pub fn used(&self) -> Option<String> {
        match self {
            Self::Undercover(game) => Some(game.used()),
            Self::Werewolf(_) | Self::Avalon(_) => None,
            #[cfg(any(test, feature = "test-games"))]
            Self::Marks(_) => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        each!(self, _game, R => R::KIND)
    }

    pub fn rules(&self) -> &'static str {
        each!(self, _game, R => R::RULES)
    }

    pub fn seats(&self) -> Vec<Seat> {
        each!(self, game, R => R::seats(game))
    }

    pub fn seat_view(&self, seat: usize) -> SeatView {
        each!(self, game, R => seat_view::<R>(game, seat))
    }

    pub fn dealt(&self, seat: usize) -> Vec<String> {
        each!(self, game, R => dealt::<R>(game, seat))
    }

    pub fn public_view(&self) -> PublicView {
        each!(self, game, R => public_view::<R>(game))
    }

    pub fn announced(&self) -> Vec<String> {
        each!(self, game, R => announced::<R>(game))
    }

    /// How it ended, as the group remembers it.
    pub fn outcome(&self) -> Option<String> {
        match self {
            Self::Undercover(game) => game.outcome(),
            Self::Werewolf(game) => game.outcome(),
            Self::Avalon(game) => game.outcome(),
            #[cfg(any(test, feature = "test-games"))]
            Self::Marks(_) => None,
        }
    }

    pub fn waiting(&self) -> Waiting {
        each!(self, game, R => R::waiting(game))
    }

    pub fn phase(&self) -> u64 {
        each!(self, game, R => R::phase(game))
    }

    pub fn seconds(&self) -> f64 {
        each!(self, game, R => R::seconds(game))
    }

    pub fn moments(&self) -> usize {
        each!(self, game, R => R::moments(game))
    }

    pub fn take(&mut self, seat: usize, index: usize, rolls: &mut Rolls) {
        each!(self, game, R => R::take(game, seat, index, rolls))
    }

    pub fn spoke(&mut self, seat: usize, text: &str) {
        each!(self, game, R => R::spoke(game, seat, text))
    }

    pub fn judged(&mut self, raw: Option<&str>, rolls: &mut Rolls) {
        each!(self, game, R => R::judged(game, raw, rolls))
    }

    pub fn timed_out(&mut self, rolls: &mut Rolls) {
        each!(self, game, R => R::timed_out(game, rolls))
    }

    pub fn over(&self) -> bool {
        each!(self, game, R => R::over(game))
    }

    pub fn news_in_private(&self) -> bool {
        each!(self, _game, R => R::NEWS_IN_PRIVATE)
    }

    pub fn public_wait(&self) -> bool {
        each!(self, game, R => R::public_wait(game))
    }

    /// Before and after the name of the seat whose turn it is to speak.
    pub fn speak_turn(&self) -> (String, String) {
        each!(self, game, R => R::speak_turn(game)).unwrap_or_else(|| speak_turn_lines(self.kind()))
    }

    /// Which round or day it is, as a test plays it.
    pub fn round(&self) -> u32 {
        match self {
            Self::Undercover(game) => game.round,
            Self::Werewolf(game) => game.night,
            Self::Avalon(game) => game.results.len() as u32 + 1,
            #[cfg(any(test, feature = "test-games"))]
            Self::Marks(_) => 0,
        }
    }
}

/// Her line starting a seat game, as she writes it: the kind named after
/// `[[game:` (turtle soup has its own, see `crate::soup::START_MARKER`). Her
/// words without it, and the kind, when it is one there is.
pub fn split_seat_game(raw: &str) -> (String, Option<&'static str>) {
    for kind in seat_kinds() {
        let marker = format!("[[game:{}]]", kind.kind);
        if raw.contains(&marker) {
            return (
                raw.replace(&marker, "").trim_end().to_string(),
                Some(kind.kind),
            );
        }
    }
    (raw.to_string(), None)
}

/// How long sign-up stays open, and how long a seat has for its move.
pub const SIGN_UP_SECONDS: f64 = 60.0;
pub const MOVE_SECONDS: f64 = 60.0;

/// The words that join a game in the group, as people type them (with her
/// @name or without).
pub fn joins(text: &str) -> bool {
    let words: String = text
        .split_whitespace()
        .filter(|word| !word.starts_with('@'))
        .collect();
    matches!(
        words.as_str(),
        "加入" | "我加入" | "+1" | "我要玩" | "算我一个"
    )
}

/// What she says opening sign-up (it closes a minute after she answered,
/// so a little less after this goes out: no exact seconds promised).
pub fn sign_up_line(name: &str) -> String {
    format!(
        "要玩{name}的，一分钟内在群里说「加入」。私聊会发给每个人自己的东西，收不到的我会说怎么打开。"
    )
}

/// What she whispers each who joined, to see that it reaches them.
pub fn seated_line(name: &str) -> String {
    format!("你在这局{name}里。这里私聊发给你的，只有你自己知道。")
}

/// What she tells the group of someone she cannot whisper to, by platform.
pub fn unreachable_line(platform: &str, name: &str) -> String {
    match platform {
        "telegram" => {
            format!("{name}，我私聊不了你：先私聊我一下、点「开始」，下一局就能玩了。")
        }
        "discord" => format!(
            "{name}，我私信不了你：在这个服务器的隐私设置里打开「允许服务器成员私信」，下一局就能玩了。"
        ),
        _ => format!("{name}，我私聊不了你：先加我好友，下一局就能玩了。"),
    }
}

/// Too few could be reached.
pub fn too_few_line(name: &str, fewest: usize) -> String {
    format!("能私聊到的人不够 {fewest} 个，{name}这局先不开了。")
}

/// It starts, with who plays.
pub fn started_line(name: &str, seats: &[Seat]) -> String {
    let names: Vec<&str> = seats.iter().map(|seat| seat.name.as_str()).collect();
    format!("{name}开始了：{}。", names.join("、"))
}

/// Whispered to the seat whose move it is.
pub fn your_move_line(view: &SeatView, platform: &str) -> String {
    format!("轮到你选了：\n{}", choices_and_how(view, platform))
}

/// A seat whose move it is whispered back what is none of its choices (a
/// number out of range, a word that is no choice): she asks once more,
/// with the choices again, and never picks for them (D2c-14).
pub fn again_line(text: &str, view: &SeatView, platform: &str) -> String {
    let why = if text.trim().parse::<usize>().is_ok() {
        "没有这个号码"
    } else {
        "没看懂这句"
    };
    format!("{why}，再选一次：\n{}", choices_and_how(view, platform))
}

fn choices_and_how(view: &SeatView, platform: &str) -> String {
    let choices: Vec<String> = view
        .choices()
        .iter()
        .enumerate()
        .map(|(index, choice)| format!("{index}. {choice}"))
        .collect();
    let how = if has_buttons(platform) {
        "点下面的按钮，或者回号码、名字。"
    } else {
        "回号码或者名字就行。"
    };
    format!("{}\n{how}", choices.join("\n"))
}

/// Whether her whispers on `platform` can carry buttons (D2c-14 A: Telegram
/// and Discord; QQ types a number or a name).
pub fn has_buttons(platform: &str) -> bool {
    matches!(platform, "telegram" | "discord")
}

/// How her seat speaks when it is its turn, by kind.
pub fn how_to_speak(kind: &str) -> &'static str {
    match kind {
        crate::undercover::KIND => crate::undercover::HOW_TO_DESCRIBE,
        crate::werewolf::KIND => crate::werewolf::HOW_TO_SPEAK,
        _ => "say one short line, as yourself.",
    }
}

/// It is over.
pub fn over_line(name: &str) -> String {
    format!("{name}结束了。")
}

/// What she remembers of a seat game the group played.
pub fn remembered_in_group(name: &str, seats: &[Seat], outcome: Option<&str>) -> String {
    let names: Vec<&str> = seats
        .iter()
        .map(|seat| if seat.hers { "我" } else { seat.name.as_str() })
        .collect();
    match outcome {
        Some(outcome) => format!("群里玩过一局{name}（{}），{outcome}", names.join("、")),
        None => format!("群里玩过一局{name}（{}）", names.join("、")),
    }
}

/// The games she can host, as her answer in a group is told them: with,
/// when a game with seats is on there (`on`, its name), that it is.
pub fn group_offer(on: Option<&str>) -> String {
    match on {
        Some(name) => format!(
            "{}\nA game of {name} is going on in the group now, you hosting it: another waits until it is over; what anyone was dealt in it is not yours to tell.",
            crate::soup::GROUP_OFFER
        ),
        None => crate::soup::GROUP_OFFER.to_string(),
    }
}

/// She offers to take a seat: one short, all must agree.
pub fn fill_line(name: &str) -> String {
    format!("{name}还差一个人。要我也来一个吗？在座的都说「好」我就上，有人不想就算了。")
}

/// Not all agreed to her taking a seat.
pub fn not_filled_line(name: &str) -> String {
    format!("没等到大家都同意，{name}这局先不开了。")
}

/// Whether a line agrees to her taking a seat, or does not; neither, none.
pub fn agrees(text: &str) -> Option<bool> {
    let words: String = text
        .split_whitespace()
        .filter(|word| !word.starts_with('@'))
        .collect();
    let words = words.trim_end_matches(['。', '！', '!', '~', '～']);
    match words {
        "好" | "好的" | "好啊" | "好呀" | "可以" | "行" | "来" | "来吧" | "同意" | "ok" | "OK" => {
            Some(true)
        }
        "不" | "不要" | "不用" | "算了" | "不行" | "不好" => Some(false),
        _ => None,
    }
}

/// Wait a little longer, once (D2c-20).
pub fn waits(text: &str) -> bool {
    let words: String = text
        .split_whitespace()
        .filter(|word| !word.starts_with('@'))
        .collect();
    matches!(
        words.trim_end_matches(['。', '！', '!', '~', '～']),
        "等等" | "等一下" | "等下" | "稍等"
    )
}

/// What she says giving more time.
pub const MORE_TIME_LINE: &str = "好，多等一分钟。";

/// The words could not be had: not this time.
pub fn not_prepared_line(name: &str) -> String {
    format!("这次没想好{name}的词，先不开了，过会儿再叫我。")
}

/// A seat that missed twice in a row, named in the group (D2c-15).
pub fn reminder_after(name: &str) -> String {
    let _ = name;
    " 你已经连着两次没动了，还在吗？".to_string()
}

/// The same, whispered to them, when the table may not know what they
/// were waited for.
pub const REMINDER_IN_PRIVATE: &str = "你已经连着两次没动了，还在吗？";

/// It is a seat's turn to speak at the table: before and after its name.
pub fn speak_turn_lines(game: &str) -> (String, String) {
    let _ = game;
    (
        "轮到 ".to_string(),
        " 了：说一句描述你的词（一分钟；说「等等」可以多等一分钟）。".to_string(),
    )
}

/// Told to one who pressed a button of a choice gone by.
pub const STALE_BUTTON_LINE: &str = "这个按钮是之前那次的，已经不算了。";

/// Told to a seat whose move it is not.
pub const NOT_YOURS_LINE: &str = "现在还没轮到你选。";

/// Told to one seated at a game in a group, not paired with the site, who
/// whispers her something that is not the game's (D2c-13: they play
/// unpaired, the private chat takes only the game for now): never asked to
/// pair in the middle of it.
pub const SEATED_UNPAIRED_LINE: &str =
    "这局还没结束，私聊这里现在只收游戏里的选择；轮到你的时候我会把选项发过来。";

/// What she whispers back when a move is taken.
pub fn taken_line(choice: &str) -> String {
    format!("收到：{choice}。")
}

/// A move as a seat types it (or a button it pressed sends): the number of
/// one of its choices, or the choice itself as written (a name).
pub fn read_move(text: &str, view: &SeatView) -> Option<usize> {
    let text = text
        .trim()
        .trim_start_matches(['@', '＠'])
        .trim_end_matches(['.', '。', '!', '！']);
    if let Ok(number) = text.parse::<usize>() {
        return (number < view.choices().len()).then_some(number);
    }
    let named: Vec<usize> = (0..view.choices().len())
        .filter(|index| view.choices()[*index] == text)
        .collect();
    (named.len() == 1).then(|| named[0])
}

/// Turtle soup, as a game at a table: anyone there asks, she alone knows
/// the truth.
pub struct Soup;

impl Rules for Soup {
    type State = crate::soup::Game;
    const KIND: &'static str = "soup";

    fn seats(_: &Self::State) -> Vec<Seat> {
        Vec::new()
    }

    fn facts(game: &Self::State) -> Vec<Fact> {
        let mut facts = vec![
            Fact::all(format!("surface: {}", game.surface)),
            Fact::host(format!("truth: {}", game.truth)),
        ];
        facts.extend(
            game.keys
                .iter()
                .map(|key| Fact::host(format!("key: {key}"))),
        );
        facts.extend(game.asked.iter().map(|asked| {
            Fact::all(format!(
                "asked: {} — {}",
                asked.question,
                asked.verdict.says()
            ))
        }));
        facts
    }

    fn choices(_: &Self::State, _: usize) -> Vec<String> {
        Vec::new()
    }

    fn take(_: &mut Self::State, _: usize, _: usize, _: &mut Rolls) {}

    fn over(game: &Self::State) -> bool {
        game.ending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::marks::Marks;
    use super::*;

    /// Whatever the seed, the number of players and which seat is hers, at
    /// every step: what she is asked as her seat, and as the voice of the
    /// game, holds no other seat's mark; her own is in what her seat knows.
    #[test]
    fn what_a_seat_does_not_know_never_reaches_her() {
        for seed in 0..300u64 {
            let mut rolls = Rolls::new(seed, Vec::new());
            let players = 3 + rolls.range(0..=5) as usize;
            let hers = rolls.range(0..=(players as u64 - 1)) as usize;
            let mut game = Marks::deal(players, hers, &mut rolls);
            let mut steps = 0;
            while !Marks::over(&game) && steps < 100 {
                let seat = game.turn;
                let mine = seat_view::<Marks>(&game, hers);
                let (system, input) = player_prompt("她", "marks", "", &mine);
                let (voice_system, voice_input) =
                    narration_prompt("她", "marks", &public_view::<Marks>(&game), &game.log);
                let told = [system, input, voice_system, voice_input].join("\n");
                for (other, mark) in game.marks.iter().enumerate() {
                    if other == hers {
                        continue;
                    }
                    assert!(
                        !told.contains(mark),
                        "seed {seed}: seat {other}'s mark reached her (seat {hers})"
                    );
                }
                assert!(
                    mine.facts()
                        .iter()
                        .any(|fact| fact.contains(&game.marks[hers]))
                );
                // The voice of the game knows no seat's mark, hers included.
                let voice = public_view::<Marks>(&game).facts().join("\n");
                assert!(game.marks.iter().all(|mark| !voice.contains(mark)));
                let open = Marks::choices(&game, seat).len();
                let index = rolls.range(0..=(open as u64 - 1)) as usize;
                Marks::take(&mut game, seat, index, &mut rolls);
                steps += 1;
            }
            assert!(Marks::over(&game), "seed {seed}");
        }
    }

    /// She holds a seat: the game has no host's view of it for her.
    #[test]
    fn she_who_plays_does_not_host() {
        let mut rolls = Rolls::new(7, Vec::new());
        let game = Marks::deal(4, 2, &mut rolls);
        assert!(host_view::<Marks>(&game).is_none());
        let mut theirs = game.clone();
        for seat in &mut theirs.seats {
            seat.hers = false;
        }
        let host = host_view::<Marks>(&theirs).unwrap();
        assert!(
            theirs
                .marks
                .iter()
                .all(|mark| host.facts().iter().any(|fact| fact.contains(mark)))
        );
    }

    /// What she picks as a player is one of her seat's choices, or nothing.
    #[test]
    fn her_choice_is_one_of_her_seats() {
        let mut rolls = Rolls::new(3, Vec::new());
        let game = Marks::deal(3, 0, &mut rolls);
        let mine = seat_view::<Marks>(&game, 0);
        assert_eq!(read_choice(r#"{"n":1}"#, &mine), Some(1));
        assert_eq!(read_choice(r#"{"n":5}"#, &mine), None);
        let theirs = seat_view::<Marks>(&game, 1);
        assert_eq!(read_choice(r#"{"n":0}"#, &theirs), None);
    }

    /// In a soup she hosts, the truth is hers and the table's view lacks it.
    #[test]
    fn a_soups_truth_is_the_hosts() {
        let game = crate::soup::Game {
            surface: "他喝了一口海龟汤".into(),
            truth: "他曾吃过人肉".into(),
            keys: vec!["吃过人肉".into()],
            asked: Vec::new(),
            found: Vec::new(),
            ending: None,
            solver: None,
            started: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            last: None,
            made: None,
        };
        let table = public_view::<Soup>(&game).facts().join("\n");
        assert!(table.contains("他喝了一口海龟汤") && !table.contains("人肉"));
        let host = host_view::<Soup>(&game).unwrap().facts().join("\n");
        assert!(host.contains("他曾吃过人肉"));
    }
}
