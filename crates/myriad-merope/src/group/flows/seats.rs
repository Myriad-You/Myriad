//! A game with seats at the group's table (D2c), the core its referee: who
//! joins in the group while sign-up is open, whether each can be whispered
//! to (those who cannot are told in the group how to open it, by platform),
//! one short and all agreeing, her taking a seat herself (D2c-11 A), what a
//! kind needs before it is dealt (the words of 谁是卧底), what each seat is
//! dealt, whispered to it alone; then the game's waits, one after another:
//! a seat speaking in the group in turn, seats choosing in private (with
//! buttons where the platform has them; D2c-14), a line of theirs judged.
//! A wait has its time, once longer when someone asks (D2c-20); who lets it
//! pass is reminded in the group the second time in a row (D2c-15). At the
//! moments worth it (it began, someone went out, it is over) she adds a
//! line of her own. Over, the group remembers it.
//!
//! Only what a seat may know is whispered to it, and only what everyone may
//! know is said in the group (`crate::hosting`'s views). Her seat's moves
//! are asked from her seat's view alone, and she is not the one who makes
//! the words when she holds a seat.
//!
//! The table is kept as it stands after every step that changes it
//! ([`Effect::KeepTable`]) and comes back with what else was kept after a
//! restart ([`Restore::table`]): sign-up and every wait go on for the time
//! they had left, what was being asked is asked again (D2-3, D2-9).

use serde::{Deserialize, Serialize};

use super::*;
use crate::hosting::{Seat, SeatGame, Waiting};

/// Her seat, when she takes one: who she is at the table.
pub const HER_SEAT: &str = "her";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SeatStage {
    SignUp,
    /// One short: whether all agree to her taking a seat.
    Agreeing,
    Checking,
    /// What the kind needs before it is dealt.
    Preparing,
    Playing,
}

/// What the table waits on an answer to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Asking {
    /// A whisper seeing whether one who joined can be reached.
    Reach,
    Soul,
    /// What she is called, for her seat.
    Name,
    Recent,
    Prepare,
    Judge,
    /// Her seat's move.
    Her,
    /// Her line at a moment of the game.
    Voice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seating {
    pub kind: String,
    pub name: String,
    pub stage: SeatStage,
    /// Where the group hears it.
    pub chat: Target,
    /// Who its asks are billed to (the site's owner, who hosts her).
    #[serde(default)]
    pub owner: i32,
    #[serde(default)]
    pub joined: Vec<Seat>,
    /// Which of those who joined is being whispered to.
    #[serde(default)]
    pub checking: usize,
    #[serde(default)]
    pub reachable: Vec<Seat>,
    /// Who agreed to her taking a seat.
    #[serde(default)]
    pub agreed: Vec<String>,
    #[serde(default)]
    pub soul: String,
    /// What the kind was dealt lately.
    #[serde(default)]
    pub recent: Vec<String>,
    #[serde(default)]
    pub tries: u8,
    #[serde(default)]
    pub game: Option<SeatGame>,
    /// The timer of the wait on now, and when it goes off (µs).
    #[serde(default)]
    pub timer: Option<String>,
    #[serde(default)]
    pub until: Option<i64>,
    /// What is asked now, and what it is.
    #[serde(default)]
    pub asked: Option<String>,
    #[serde(default)]
    pub asking: Option<Asking>,
    /// The game's wait that was announced (see `hosting::Rules::phase`).
    #[serde(default)]
    pub waited: Option<u64>,
    /// Whether this wait was given more time already.
    #[serde(default)]
    pub extended: bool,
    /// Waits each seat let pass in a row.
    #[serde(default)]
    pub misses: Vec<u32>,
    /// How many of the game's announcements were said, and of its moments
    /// she spoke at.
    #[serde(default)]
    pub told: usize,
    #[serde(default)]
    pub moments: usize,
    /// For a game whose seats learn things as it goes: how many of the
    /// facts for each seat were whispered to it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub told_seats: Vec<usize>,
    /// Secret waits (a move at night) each seat let pass in a row, apart
    /// from the rest: the group is never told of those.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_misses: Vec<u32>,
}

impl Seating {
    /// `seat` moved at a wait the table may know of (`public`), or at a
    /// secret one: that run of waits it let pass is over, not the other (a
    /// move at night must not spare anyone being named for the day's).
    fn moved_now(&mut self, seat: usize, public: bool) {
        let misses = if public {
            &mut self.misses
        } else {
            &mut self.secret_misses
        };
        if let Some(missed) = misses.get_mut(seat) {
            *missed = 0;
        }
    }
}

fn platform_of(who: &str) -> (&str, &str) {
    who.split_once(':').unwrap_or(("", who))
}

/// Say `pieces` in the group, as hers.
fn tell_pieces(cx: &mut Cx, s: &Seating, pieces: Vec<Piece>) {
    let now = cx.now();
    let text: String = pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text { text } => text.clone(),
            Piece::Mention { name, .. } => format!("@{name}"),
        })
        .collect();
    record_hers(cx.group, &text, now, cx.out);
    let to = s.chat.clone();
    cx.effect(|id| Effect::Send {
        id,
        to,
        pieces,
        quote: false,
    });
}

/// Say `text` in the group, as hers.
fn tell(cx: &mut Cx, s: &Seating, text: &str) {
    tell_pieces(
        cx,
        s,
        vec![Piece::Text {
            text: text.to_string(),
        }],
    );
}

/// Say `before`, `seat` named, `after` in the group.
fn tell_naming(cx: &mut Cx, s: &Seating, before: &str, seat: &Seat, after: &str) {
    let mut pieces = Vec::new();
    if !before.is_empty() {
        pieces.push(Piece::Text {
            text: before.to_string(),
        });
    }
    if seat.hers {
        pieces.push(Piece::Text {
            text: seat.name.clone(),
        });
    } else {
        pieces.push(Piece::Mention {
            id: platform_of(&seat.who).1.to_string(),
            name: seat.name.clone(),
        });
    }
    pieces.push(Piece::Text {
        text: after.to_string(),
    });
    tell_pieces(cx, s, pieces);
}

/// Whisper `text` to `seat` (with `choices` as buttons where there are
/// buttons); the id it is answered under.
fn whisper(cx: &mut Cx, s: &Seating, seat: &Seat, text: String, choices: Vec<String>) -> String {
    let (platform, user) = platform_of(&seat.who);
    let (platform, user, chat) = (platform.to_string(), user.to_string(), s.chat.chat.clone());
    let wait = (!choices.is_empty()).then_some(s.waited).flatten();
    cx.effect(|id| Effect::Whisper {
        id,
        platform,
        user,
        chat,
        text,
        choices,
        wait,
    })
}

/// Wait `after` seconds for the table.
fn arm(cx: &mut Cx, s: &mut Seating, after: f64) {
    let key = cx.wake(after);
    s.until = Some(cx.now() + seconds(after));
    s.timer = Some(key.clone());
    park(cx, key);
}

/// Wait for what `id` comes to.
fn asked(cx: &mut Cx, s: &mut Seating, id: String, asking: Asking) {
    s.asked = Some(id.clone());
    s.asking = Some(asking);
    park(cx, id);
}

fn park(cx: &mut Cx, awaiting: String) {
    let errand = cx.id();
    cx.group.errands.push(Errand {
        id: errand,
        awaiting,
        kind: Kind::Seats,
        doing: Doing::Seats,
    });
}

#[allow(clippy::too_many_arguments)]
fn ask(
    cx: &mut Cx,
    s: &mut Seating,
    asking: Asking,
    voice: &str,
    operation: String,
    within: u64,
    (system, input): (String, String),
    schema_name: &str,
    schema: Value,
) {
    let (voice, owner, schema_name) = (voice.to_string(), s.owner, schema_name.to_string());
    let id = cx.effect(|id| Effect::Ask {
        id,
        how: AskKind::Json,
        voice,
        owner,
        operation,
        timeout_ms: within,
        system,
        input,
        schema_name,
        schema,
        turn: None,
    });
    asked(cx, s, id, asking);
}

/// Her words named a seat game of `kind`: sign-up opens at the table `chat`,
/// its asks billed to `owner`. The line she adds to her words, or none when
/// a game is on there already (one game a table) or there is no such game.
pub(crate) fn open_sign_up(cx: &mut Cx, kind: &str, chat: Target, owner: i32) -> Option<String> {
    if cx.group.seating.is_some() {
        return None;
    }
    let known = crate::hosting::seat_kind(kind)?;
    let mut s = Seating {
        kind: known.kind.to_string(),
        name: known.name.to_string(),
        stage: SeatStage::SignUp,
        chat: Target {
            message_id: String::new(),
            ..chat
        },
        owner,
        joined: Vec::new(),
        checking: 0,
        reachable: Vec::new(),
        agreed: Vec::new(),
        soul: String::new(),
        recent: Vec::new(),
        tries: 0,
        game: None,
        timer: None,
        until: None,
        asked: None,
        asking: None,
        waited: None,
        extended: false,
        misses: Vec::new(),
        told: 0,
        moments: 0,
        told_seats: Vec::new(),
        secret_misses: Vec::new(),
    };
    arm(cx, &mut s, crate::hosting::SIGN_UP_SECONDS);
    cx.group.seating = Some(s);
    Some(crate::hosting::sign_up_line(known.name))
}

/// A game is on at this table, past sign-up: she does not speak up unasked
/// meanwhile, as one hosting it would not.
pub(crate) fn hosting_now(group: &Group) -> bool {
    group
        .seating
        .as_ref()
        .is_some_and(|s| s.stage != SeatStage::SignUp)
}

/// A line of the group's, as the table hears it: one who joins while
/// sign-up is open; whether they agree to her taking a seat; someone asking
/// for more time; the line of the seat whose turn it is to speak.
pub(crate) fn heard_at_table(cx: &mut Cx, message: &Message) {
    let Some(mut s) = cx.group.seating.take() else {
        return;
    };
    let who = format!("{}:{}", message.platform, message.from);
    match s.stage {
        SeatStage::SignUp if crate::hosting::joins(&message.text) => {
            let most = crate::hosting::seat_kind(&s.kind).map_or(0, |kind| kind.most);
            if !s.joined.iter().any(|seat| seat.who == who) && s.joined.len() < most {
                s.joined.push(Seat {
                    who,
                    name: message.display_name.clone(),
                    hers: false,
                });
            }
        }
        SeatStage::Agreeing if s.reachable.iter().any(|seat| seat.who == who) => {
            match crate::hosting::agrees(&message.text) {
                Some(true) if !s.agreed.contains(&who) => {
                    s.agreed.push(who);
                    if s.reachable.iter().all(|seat| s.agreed.contains(&seat.who)) {
                        s.reachable.push(Seat {
                            who: HER_SEAT.into(),
                            name: "我".into(),
                            hers: true,
                        });
                        s.timer = None;
                        s.until = None;
                        return prepare(cx, s);
                    }
                }
                Some(false) => {
                    let line = crate::hosting::not_filled_line(&s.name);
                    tell(cx, &s, &line);
                    return end(cx, s);
                }
                _ => {}
            }
        }
        SeatStage::Playing => return heard_playing(cx, s, &who, message),
        _ => {}
    }
    cx.group.seating = Some(s);
}

fn heard_playing(cx: &mut Cx, mut s: Seating, who: &str, message: &Message) {
    let Some(waiting) = s.game.as_ref().map(SeatGame::waiting) else {
        cx.group.seating = Some(s);
        return;
    };
    let timed = matches!(waiting, Waiting::Speak(_) | Waiting::Choose(_));
    // More time, once a wait.
    if timed && !s.extended && s.timer.is_some() && crate::hosting::waits(&message.text) {
        s.extended = true;
        let left = s
            .until
            .map_or(0.0, |until| (until - cx.now()).max(0) as f64 / 1e6);
        arm(cx, &mut s, left + crate::hosting::MOVE_SECONDS);
        tell(cx, &s, crate::hosting::MORE_TIME_LINE);
        cx.group.seating = Some(s);
        return;
    }
    if let Waiting::Speak(seat) = waiting
        && s.reachable
            .get(seat)
            .is_some_and(|sat| sat.who == who && !sat.hers)
    {
        if let Some(game) = s.game.as_mut() {
            game.spoke(seat, &message.text);
        }
        s.moved_now(seat, true);
        s.timer = None;
        s.until = None;
        return after(cx, s);
    }
    cx.group.seating = Some(s);
}

/// The errand of the table: a timer came, or what was asked came back.
pub(crate) fn seats_step(cx: &mut Cx, errand: Errand, answer: Option<Answer>) {
    let Some(mut s) = cx.group.seating.take() else {
        return;
    };
    let key = errand.awaiting.as_str();
    if answer.is_none() && s.timer.as_deref() == Some(key) {
        s.timer = None;
        s.until = None;
        return timed_out(cx, s);
    }
    if let Some(answer) = answer
        && s.asked.as_deref() == Some(key)
    {
        s.asked = None;
        let asking = s.asking.take();
        return answered(cx, s, asking, answer);
    }
    // A stale timer, or nothing it waits for.
    cx.group.seating = Some(s);
}

fn timed_out(cx: &mut Cx, mut s: Seating) {
    match s.stage {
        // Sign-up closes: each who joined is whispered to, one by one.
        SeatStage::SignUp => {
            s.stage = SeatStage::Checking;
            s.checking = 0;
            check_next(cx, s);
        }
        SeatStage::Agreeing => {
            let line = crate::hosting::not_filled_line(&s.name);
            tell(cx, &s, &line);
            end(cx, s);
        }
        SeatStage::Playing => {
            let waited: Vec<usize> = match s.game.as_ref().map(SeatGame::waiting) {
                Some(Waiting::Speak(seat)) => vec![seat],
                Some(Waiting::Choose(seats)) => seats,
                _ => Vec::new(),
            };
            let public = s.game.as_ref().is_none_or(SeatGame::public_wait);
            if !public && s.secret_misses.len() < s.reachable.len() {
                s.secret_misses.resize(s.reachable.len(), 0);
            }
            let mut reminded = Vec::new();
            for seat in waited {
                if s.reachable.get(seat).is_none_or(|sat| sat.hers) {
                    continue;
                }
                let misses = if public {
                    &mut s.misses
                } else {
                    &mut s.secret_misses
                };
                if let Some(missed) = misses.get_mut(seat) {
                    *missed += 1;
                    if *missed == 2 {
                        reminded.push(seat);
                    }
                }
            }
            if let Some(game) = s.game.as_mut() {
                game.timed_out(&mut cx.at.rolls);
            }
            tell_news(cx, &mut s);
            for seat in reminded {
                let named = s.reachable[seat].clone();
                if public {
                    let after = crate::hosting::reminder_after(&named.name);
                    tell_naming(cx, &s, "", &named, &after);
                } else {
                    let line = crate::hosting::REMINDER_IN_PRIVATE.to_string();
                    whisper(cx, &s, &named, line, Vec::new());
                }
            }
            after(cx, s);
        }
        _ => cx.group.seating = Some(s),
    }
}

fn answered(cx: &mut Cx, mut s: Seating, asking: Option<Asking>, answer: Answer) {
    // A model that could not be asked, or whatever else came back: nothing.
    let answer = match (asking, answer) {
        (
            Some(Asking::Prepare | Asking::Judge | Asking::Her | Asking::Voice),
            answer @ Answer::Asked { .. },
        ) => answer,
        (Some(Asking::Prepare | Asking::Judge | Asking::Her | Asking::Voice), _) => Answer::Asked {
            raw: None,
            failure: None,
        },
        (_, answer) => answer,
    };
    match (asking, answer) {
        (Some(Asking::Reach), Answer::Whispered { ok }) => {
            let seat = s.joined[s.checking].clone();
            if ok {
                cx.out.effects.push(Effect::Seat {
                    who: seat.who.clone(),
                });
                s.reachable.push(seat);
            } else {
                let (platform, _) = platform_of(&seat.who);
                let line = crate::hosting::unreachable_line(platform, &seat.name);
                tell(cx, &s, &line);
            }
            s.checking += 1;
            check_next(cx, s);
        }
        (Some(Asking::Soul), Answer::Soul { text }) => {
            s.soul = text;
            if s.reachable.iter().any(|seat| seat.hers) {
                let id = cx.effect(|id| Effect::Read {
                    id,
                    of: Of::Hers,
                    what: ToRead::HerName,
                });
                asked(cx, &mut s, id, Asking::Name);
                cx.group.seating = Some(s);
                return;
            }
            prepare_kind(cx, s);
        }
        (Some(Asking::Name), answer) => {
            if let Answer::Label { label } = answer
                && !label.trim().is_empty()
                && let Some(seat) = s.reachable.iter_mut().find(|seat| seat.hers)
            {
                seat.name = label.trim().to_string();
            }
            prepare_kind(cx, s);
        }
        (Some(Asking::Recent), Answer::Surfaces { surfaces }) => {
            s.recent = surfaces;
            ask_prepare(cx, s);
        }
        (Some(Asking::Prepare), Answer::Asked { raw, failure }) => {
            let unavailable = failure.as_deref() == Some("unavailable");
            deal_or_retry(cx, s, raw, unavailable);
        }
        (Some(Asking::Judge), Answer::Asked { raw, .. }) => {
            if let Some(game) = s.game.as_mut() {
                game.judged(raw.as_deref(), &mut cx.at.rolls);
            }
            after(cx, s);
        }
        (Some(Asking::Her), Answer::Asked { raw, .. }) => her_moved(cx, s, raw),
        (Some(Asking::Voice), Answer::Asked { raw, .. }) => {
            if let Some(line) = raw.as_deref().and_then(crate::hosting::read_say) {
                tell(cx, &s, &line);
            }
            next_move(cx, s);
        }
        // Not read, or not what it waits for: as if it came to nothing.
        (Some(Asking::Soul | Asking::Recent), _) => {
            let line = crate::hosting::not_prepared_line(&s.name);
            tell(cx, &s, &line);
            end(cx, s);
        }
        (Some(Asking::Reach), _) => {
            s.checking += 1;
            check_next(cx, s);
        }
        _ => cx.group.seating = Some(s),
    }
}

/// Whisper the next who joined; all whispered, the game is prepared, she
/// offers to take a seat, or it is called off.
fn check_next(cx: &mut Cx, mut s: Seating) {
    if let Some(seat) = s.joined.get(s.checking).cloned() {
        let id = whisper(
            cx,
            &s,
            &seat,
            crate::hosting::seated_line(&s.name),
            Vec::new(),
        );
        asked(cx, &mut s, id, Asking::Reach);
        cx.group.seating = Some(s);
        return;
    }
    let Some(kind) = crate::hosting::seat_kind(&s.kind) else {
        return end(cx, s);
    };
    let reached = s.reachable.len();
    if reached >= kind.fewest {
        return prepare(cx, s);
    }
    if kind.she_may_fill && reached >= 1 && reached + 1 == kind.fewest {
        s.stage = SeatStage::Agreeing;
        let line = crate::hosting::fill_line(&s.name);
        tell(cx, &s, &line);
        arm(cx, &mut s, crate::hosting::SIGN_UP_SECONDS);
        cx.group.seating = Some(s);
        return;
    }
    let line = crate::hosting::too_few_line(&s.name, kind.fewest);
    tell(cx, &s, &line);
    end(cx, s);
}

/// What the kind needs before it is dealt: what it was dealt lately, then
/// the ask.
fn prepare_kind(cx: &mut Cx, mut s: Seating) {
    let prepares = crate::hosting::seat_kind(&s.kind).is_some_and(|kind| kind.prepares);
    if !prepares {
        return deal(cx, s, None);
    }
    let game = s.kind.clone();
    let id = cx.effect(|id| Effect::Read {
        id,
        of: Of::Hers,
        what: ToRead::RecentDealt { game },
    });
    asked(cx, &mut s, id, Asking::Recent);
    cx.group.seating = Some(s);
}

/// Before it is dealt: her soul (for her lines; and when she plays, what
/// she is called), then what the kind needs.
fn prepare(cx: &mut Cx, mut s: Seating) {
    s.stage = SeatStage::Preparing;
    let id = cx.effect(|id| Effect::Read {
        id,
        of: Of::Hers,
        what: ToRead::Soul,
    });
    asked(cx, &mut s, id, Asking::Soul);
    cx.group.seating = Some(s);
}

fn ask_prepare(cx: &mut Cx, mut s: Seating) {
    // Holding a seat, she must not know what is dealt: asked as a judge.
    let plays = s.reachable.iter().any(|seat| seat.hers);
    let soul = (!plays).then_some(s.soul.as_str());
    let Some(prepare) = SeatGame::prepare(&s.kind, soul, &s.recent) else {
        return deal(cx, s, None);
    };
    s.tries += 1;
    ask(
        cx,
        &mut s,
        Asking::Prepare,
        prepare.voice,
        prepare.operation.to_string(),
        30_000,
        (prepare.system, prepare.input),
        prepare.schema_name,
        prepare.schema,
    );
    cx.group.seating = Some(s);
}

fn deal_or_retry(cx: &mut Cx, s: Seating, raw: Option<String>, unavailable: bool) {
    let dealt = raw.as_deref().and_then(|raw| {
        SeatGame::deal(
            &s.kind,
            s.reachable.clone(),
            Some(raw),
            &s.recent,
            &mut cx.at.rolls,
        )
    });
    match dealt {
        Some(game) => start(cx, s, game),
        None if s.tries < 2 && !unavailable => ask_prepare(cx, s),
        None => {
            let line = crate::hosting::not_prepared_line(&s.name);
            tell(cx, &s, &line);
            end(cx, s);
        }
    }
}

/// Dealt without anything asked first.
fn deal(cx: &mut Cx, s: Seating, raw: Option<&str>) {
    match SeatGame::deal(
        &s.kind,
        s.reachable.clone(),
        raw,
        &s.recent,
        &mut cx.at.rolls,
    ) {
        Some(game) => start(cx, s, game),
        None => end(cx, s),
    }
}

/// It starts: what was dealt kept as dealt lately, each seat whispered its
/// own part, then the game's first wait.
fn start(cx: &mut Cx, mut s: Seating, game: SeatGame) {
    if let Some(dealt) = game.used() {
        cx.out.effects.push(Effect::Write {
            of: Of::Hers,
            what: ToWrite::Dealt {
                game: s.kind.clone(),
                dealt,
            },
        });
    }
    s.stage = SeatStage::Playing;
    s.misses = vec![0; s.reachable.len()];
    if game.announced().is_empty() {
        let line = crate::hosting::started_line(&s.name, &s.reachable);
        tell(cx, &s, &line);
    }
    for (index, seat) in s.reachable.clone().iter().enumerate() {
        if seat.hers {
            continue;
        }
        let dealt = game.dealt(index).join("\n");
        if !dealt.is_empty() {
            whisper(cx, &s, seat, dealt, Vec::new());
        }
    }
    if game.news_in_private() {
        s.told_seats = (0..s.reachable.len())
            .map(|seat| game.dealt(seat).len())
            .collect();
    }
    s.game = Some(game);
    after(cx, s);
}

/// Say what everyone may know that was not said yet; whisper each seat
/// what is new for it alone (a game whose seats learn things as it goes).
fn tell_news(cx: &mut Cx, s: &mut Seating) {
    let announced = s.game.as_ref().map(SeatGame::announced).unwrap_or_default();
    let news = announced[s.told.min(announced.len())..].join("\n");
    s.told = announced.len();
    if !news.is_empty() {
        tell(cx, s, &news);
    }
    for seat in 0..s.told_seats.len() {
        let Some(game) = s.game.as_ref() else {
            return;
        };
        let dealt = game.dealt(seat);
        let new = dealt[s.told_seats[seat].min(dealt.len())..].join("\n");
        s.told_seats[seat] = dealt.len();
        let sat = s.reachable[seat].clone();
        if !new.is_empty() && !sat.hers {
            whisper(cx, s, &sat, new, Vec::new());
        }
    }
}

/// The game moved: what everyone may know of it is said, at a moment she
/// adds a line of her own, and it goes on to its next wait.
fn after(cx: &mut Cx, mut s: Seating) {
    tell_news(cx, &mut s);
    let moments = s.game.as_ref().map_or(0, SeatGame::moments);
    if moments > s.moments {
        s.moments = moments;
        if let Some(game) = s.game.as_ref() {
            let view = game.public_view();
            let announced = game.announced();
            let latest = announced.last().cloned().into_iter().collect::<Vec<_>>();
            let prompt = crate::hosting::narration_prompt(&s.soul, &s.name, &view, &latest);
            let operation = format!("{}_voice", s.kind);
            ask(
                cx,
                &mut s,
                Asking::Voice,
                "hers",
                operation,
                20_000,
                prompt,
                crate::hosting::SAY_SCHEMA,
                crate::hosting::say_schema(),
            );
            cx.group.seating = Some(s);
            return;
        }
    }
    next_move(cx, s);
}

/// On to what the game waits for now.
fn next_move(cx: &mut Cx, mut s: Seating) {
    let Some(game) = s.game.as_ref() else {
        return end(cx, s);
    };
    let (waiting, phase, time) = (game.waiting(), game.phase(), game.seconds());
    match waiting {
        Waiting::Over => over(cx, s),
        Waiting::Judge {
            system,
            input,
            schema_name,
            schema,
        } => {
            let operation = format!("{}_judge", s.kind);
            ask(
                cx,
                &mut s,
                Asking::Judge,
                "judge",
                operation,
                20_000,
                (system, input),
                schema_name,
                schema,
            );
            cx.group.seating = Some(s);
        }
        Waiting::Speak(seat) => {
            if s.waited == Some(phase) {
                cx.group.seating = Some(s);
                return;
            }
            s.waited = Some(phase);
            s.extended = false;
            let sat = s.reachable[seat].clone();
            if sat.hers {
                ask_her(cx, &mut s, seat, false);
            } else {
                let (before, after) = s
                    .game
                    .as_ref()
                    .map(SeatGame::speak_turn)
                    .unwrap_or_else(|| crate::hosting::speak_turn_lines(&s.name));
                tell_naming(cx, &s, &before, &sat, &after);
                arm(cx, &mut s, time);
            }
            cx.group.seating = Some(s);
        }
        Waiting::Choose(seats) => {
            if s.waited == Some(phase) {
                cx.group.seating = Some(s);
                return;
            }
            s.waited = Some(phase);
            s.extended = false;
            for seat in seats {
                let sat = s.reachable[seat].clone();
                if sat.hers {
                    ask_her(cx, &mut s, seat, true);
                    continue;
                }
                let Some(view) = s.game.as_ref().map(|game| game.seat_view(seat)) else {
                    continue;
                };
                let (platform, _) = platform_of(&sat.who);
                let line = crate::hosting::your_move_line(&view, platform);
                let choices = if crate::hosting::has_buttons(platform) {
                    view.choices().to_vec()
                } else {
                    Vec::new()
                };
                whisper(cx, &s, &sat, line, choices);
            }
            arm(cx, &mut s, time);
            cx.group.seating = Some(s);
        }
    }
}

/// Ask her seat's move, from her seat's view alone: what she says when it
/// is her turn to speak, or which choice she takes.
fn ask_her(cx: &mut Cx, s: &mut Seating, seat: usize, choose: bool) {
    let Some(game) = s.game.as_ref() else {
        return;
    };
    let view = game.seat_view(seat);
    let rules = game.rules();
    let operation = format!("{}_her_move", s.kind);
    if choose {
        let prompt = crate::hosting::player_prompt(&s.soul, &s.name, rules, &view);
        ask(
            cx,
            s,
            Asking::Her,
            "hers",
            operation,
            30_000,
            prompt,
            crate::hosting::CHOICE_SCHEMA,
            crate::hosting::choice_schema(),
        );
    } else {
        let how = crate::hosting::how_to_speak(&s.kind);
        let prompt = crate::hosting::speaker_prompt(&s.soul, &s.name, rules, how, &view);
        ask(
            cx,
            s,
            Asking::Her,
            "hers",
            operation,
            30_000,
            prompt,
            crate::hosting::SAY_SCHEMA,
            crate::hosting::say_schema(),
        );
    }
}

/// Her seat's move came back: what she said is said in the group as hers;
/// a choice is taken when it is one of hers. Nothing usable: she lets it
/// pass (a turn to speak), or the wait's time does.
fn her_moved(cx: &mut Cx, mut s: Seating, raw: Option<String>) {
    let Some(game) = s.game.as_mut() else {
        return end(cx, s);
    };
    let hers = s.reachable.iter().position(|seat| seat.hers);
    match (game.waiting(), hers) {
        (Waiting::Speak(seat), Some(mine)) if seat == mine => {
            match raw.as_deref().and_then(crate::hosting::read_say) {
                Some(said) => {
                    game.spoke(seat, &said);
                    tell(cx, &s, &said);
                }
                None => game.timed_out(&mut cx.at.rolls),
            }
            after(cx, s)
        }
        (Waiting::Choose(seats), Some(mine)) if seats.contains(&mine) => {
            let view = game.seat_view(mine);
            if let Some(index) = raw
                .as_deref()
                .and_then(|raw| crate::hosting::read_choice(raw, &view))
            {
                game.take(mine, index, &mut cx.at.rolls);
                return moved(cx, s);
            }
            cx.group.seating = Some(s);
        }
        _ => cx.group.seating = Some(s),
    }
}

/// A choice was taken: when the wait is done, the timer of it goes.
fn moved(cx: &mut Cx, mut s: Seating) {
    let phase = s.game.as_ref().map(SeatGame::phase);
    if phase != s.waited {
        s.timer = None;
        s.until = None;
    }
    after(cx, s)
}

/// Over: her line was said; the group remembers it, and nobody sits at it
/// any more.
fn over(cx: &mut Cx, s: Seating) {
    let outcome = s.game.as_ref().and_then(SeatGame::outcome);
    if outcome.is_none() {
        let line = crate::hosting::over_line(&s.name);
        tell(cx, &s, &line);
    }
    cx.out.effects.push(Effect::Write {
        of: Of::Others,
        what: ToWrite::RememberInVenue {
            venue: super::sense::stored(&format!("{}:{}", s.chat.platform, s.chat.chat)),
            content: crate::hosting::remembered_in_group(&s.name, &s.reachable, outcome.as_deref()),
            evidence: serde_json::json!({ "game": s.kind }).to_string(),
            source: "game".into(),
        },
    });
    end(cx, s);
}

/// Nobody sits at it any more.
fn end(cx: &mut Cx, s: Seating) {
    for seat in &s.reachable {
        if !seat.hers {
            cx.out.effects.push(Effect::Unseat {
                who: seat.who.clone(),
            });
        }
    }
    cx.group.seating = None;
}

/// A seat whispered back: a move when it is one of theirs now (taken);
/// while it is theirs, anything that is none of their choices is asked
/// again (taken, never picked for them); a move at the wrong time (taken,
/// and they are told); else not the game's.
pub(crate) fn whispered(cx: &mut Cx, who: &str, text: &str, wait: Option<u64>) -> bool {
    let Some(mut s) = cx.group.seating.take() else {
        return false;
    };
    let seat = s
        .reachable
        .iter()
        .position(|seat| seat.who == who && !seat.hers);
    let (Some(seat), SeatStage::Playing) = (seat, s.stage) else {
        cx.group.seating = Some(s);
        return false;
    };
    let Some(game) = s.game.as_mut() else {
        cx.group.seating = Some(s);
        return false;
    };
    let view = game.seat_view(seat);
    // A button of a wait gone by is not this one's: they are told so.
    if wait.is_some_and(|wait| Some(wait) != s.waited) {
        let sat = s.reachable[seat].clone();
        whisper(
            cx,
            &s,
            &sat,
            crate::hosting::STALE_BUTTON_LINE.to_string(),
            Vec::new(),
        );
        cx.group.seating = Some(s);
        return true;
    }
    let open = matches!(game.waiting(), Waiting::Choose(ref seats) if seats.contains(&seat));
    let public = game.public_wait();
    match crate::hosting::read_move(text, &view).filter(|_| open) {
        Some(index) => {
            let choice = view.choices()[index].clone();
            game.take(seat, index, &mut cx.at.rolls);
            s.moved_now(seat, public);
            let sat = s.reachable[seat].clone();
            whisper(
                cx,
                &s,
                &sat,
                crate::hosting::taken_line(&choice),
                Vec::new(),
            );
            moved(cx, s);
            true
        }
        None if open => {
            let sat = s.reachable[seat].clone();
            let (platform, _) = platform_of(&sat.who);
            let line = crate::hosting::again_line(text, &view, platform);
            let choices = if crate::hosting::has_buttons(platform) {
                view.choices().to_vec()
            } else {
                Vec::new()
            };
            whisper(cx, &s, &sat, line, choices);
            cx.group.seating = Some(s);
            true
        }
        None => {
            // A number, or someone's name, at the wrong time.
            let looks = text.trim().parse::<usize>().is_ok()
                || s.reachable.iter().any(|other| other.name == text.trim());
            if looks {
                let sat = s.reachable[seat].clone();
                whisper(
                    cx,
                    &s,
                    &sat,
                    crate::hosting::NOT_YOURS_LINE.to_string(),
                    Vec::new(),
                );
            }
            cx.group.seating = Some(s);
            looks
        }
    }
}

/// The table as kept, back after a restart: who sits at it is seated again;
/// sign-up and the wait on go on for the time they had left; what was being
/// asked is asked again (a line of hers at a moment is let go).
pub(crate) fn resume_table(cx: &mut Cx, mut s: Seating) {
    for seat in &s.reachable {
        if !seat.hers {
            cx.out.effects.push(Effect::Seat {
                who: seat.who.clone(),
            });
        }
    }
    let left = s.until.map(|until| (until - cx.now()).max(0) as f64 / 1e6);
    s.timer = None;
    s.until = None;
    s.asked = None;
    let asking = s.asking.take();
    match s.stage {
        SeatStage::SignUp | SeatStage::Agreeing => {
            arm(cx, &mut s, left.unwrap_or(0.0));
            cx.group.seating = Some(s);
        }
        SeatStage::Checking => check_next(cx, s),
        SeatStage::Preparing => prepare(cx, s),
        SeatStage::Playing => {
            if let Some(left) = left {
                arm(cx, &mut s, left);
            }
            match asking {
                Some(Asking::Her) => {
                    let seat = s.reachable.iter().position(|seat| seat.hers);
                    let choose = matches!(
                        s.game.as_ref().map(SeatGame::waiting),
                        Some(Waiting::Choose(_))
                    );
                    if let Some(seat) = seat {
                        ask_her(cx, &mut s, seat, choose);
                    }
                    cx.group.seating = Some(s);
                }
                Some(Asking::Judge | Asking::Voice) => next_move(cx, s),
                _ if left.is_none() => next_move(cx, s),
                _ => cx.group.seating = Some(s),
            }
        }
    }
}
