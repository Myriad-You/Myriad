//! 谁是卧底, as a game she hosts at a group's table (D2c, its first real
//! game with seats): all of it mechanical here, the referee's part. Most
//! players get one word, the undercover(s) a close but different one, and
//! nobody is told which they are. In seat order each describes their word in
//! one line in the group without saying it; then everyone votes, in
//! private, whom to put out. A tie: those tied describe once more and the
//! rest vote among them; tied again, nobody goes out that round. Whoever
//! goes out is said to be undercover or not. The civilians win when every
//! undercover is out; the undercovers win when they are as many as the
//! civilians left, or (six or more at the start) when one of them lasts
//! until three are left. Saying one's own word outright (or by a homophone,
//! or its characters split) puts one out at once.
//!
//! Where only a model will do: the pair of words (made fresh each game, not
//! one used lately; D2c-17 A), whether a description gives its word away
//! beyond the literal check here, her lines as the game's voice, and her
//! seat's description and vote when she plays one (from her seat's view
//! alone). No blank seat: the decided rules do not have one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::group::Rolls;
use crate::hosting::{Fact, Prepare, Rules, Seat, Waiting};

pub const KIND: &str = "undercover";
pub const NAME: &str = "谁是卧底";
pub const FEWEST: usize = 4;
pub const MOST: usize = 10;
/// How long a seat has to describe, and everyone to vote.
pub const SECONDS: f64 = 60.0;
/// Pairs kept as used, so the next game's is not one of them.
pub const RECENT_KEPT: usize = 20;

pub const WORDS_SCHEMA: &str = "merope_undercover_words";
pub const JUDGE_SCHEMA: &str = "merope_undercover_judge";

/// The rules as she tells the table at the start.
const TOLD_RULES: &str = "每人私聊收到一个词，大多数人的一样，卧底的相近但不同，谁也不知道自己是不是卧底。按顺序每人在群里说一句描述自己的词，不能说出词本身（谐音、拆字也不行，说了直接出局）；说完大家私聊投票，票最多的出局，平票的人再各说一句、其他人在他们中间再投。卧底全出局平民赢；卧底和剩下的平民一样多就是卧底赢。";
const TOLD_RULES_SIX: &str = "每人私聊收到一个词，大多数人的一样，卧底的相近但不同，谁也不知道自己是不是卧底。按顺序每人在群里说一句描述自己的词，不能说出词本身（谐音、拆字也不行，说了直接出局）；说完大家私聊投票，票最多的出局，平票的人再各说一句、其他人在他们中间再投。卧底全出局平民赢；卧底撑到只剩三个人，或者和剩下的平民一样多，就是卧底赢。";

/// How many undercovers for `players`.
pub fn undercovers_for(players: usize) -> usize {
    match players {
        0..=6 => 1,
        7..=9 => 2,
        _ => 3,
    }
}

/// The pair of words: what most get, and what the undercovers get.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Words {
    pub civilian: String,
    pub undercover: String,
}

impl Words {
    /// As kept among those used lately.
    pub fn kept(&self) -> String {
        format!("{}/{}", self.civilian, self.undercover)
    }
}

/// Who voted for whom (`[voter, seat]`), in the order they voted: a list,
/// as a state kept as JSON holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Votes(pub Vec<[usize; 2]>);

impl Votes {
    fn contains_key(&self, voter: &usize) -> bool {
        self.0.iter().any(|[by, _]| by == voter)
    }

    fn insert(&mut self, voter: usize, seat: usize) {
        if !self.contains_key(&voter) {
            self.0.push([voter, seat]);
        }
    }

    fn values(&self) -> impl Iterator<Item = &usize> {
        self.0.iter().map(|[_, seat]| seat)
    }
}

/// What the game is doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "phase")]
pub enum Phase {
    /// Seats describe in `order`; `at` is whose turn it is. `said` is what
    /// they said while it is being judged. `tied` when it is a tie's second
    /// round of descriptions.
    #[serde(rename_all = "camelCase")]
    Describe {
        order: Vec<usize>,
        at: usize,
        #[serde(default)]
        said: Option<String>,
        #[serde(default)]
        tied: bool,
    },
    /// `voters` each vote for one of `among` (not themselves); `votes` who
    /// voted for whom, `done` who voted or let it pass.
    #[serde(rename_all = "camelCase")]
    Vote {
        among: Vec<usize>,
        voters: Vec<usize>,
        #[serde(default)]
        votes: Votes,
        #[serde(default)]
        tied: bool,
    },
    #[serde(rename_all = "camelCase")]
    Over { undercovers_won: bool },
}

/// One thing everyone at the table knows: something she announces, or
/// what a seat said in the group (heard there, not said again).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logged {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub said: bool,
}

/// A game of 谁是卧底.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Undercover {
    pub seats: Vec<Seat>,
    pub words: Words,
    /// Which seats are undercover.
    pub undercover: Vec<bool>,
    pub alive: Vec<bool>,
    /// Who describes first in the first round.
    pub start: usize,
    pub round: u32,
    pub phase: Phase,
    /// What everyone at the table knows, in order.
    pub log: Vec<Logged>,
    pub moments: usize,
    /// Which wait it is (one more each time the game moves on).
    pub phases: u64,
}

impl Undercover {
    /// Dealt to `seats`: who is undercover and who starts, by `rolls`.
    pub fn deal(seats: Vec<Seat>, words: Words, rolls: &mut Rolls) -> Self {
        let players = seats.len();
        let mut undercover = vec![false; players];
        let mut left: Vec<usize> = (0..players).collect();
        for _ in 0..undercovers_for(players).min(players) {
            let pick = rolls.range(0..=(left.len() as u64 - 1)) as usize;
            undercover[left.remove(pick)] = true;
        }
        let start = rolls.range(0..=(players.max(1) as u64 - 1)) as usize;
        let mut game = Self {
            alive: vec![true; players],
            seats,
            words,
            undercover,
            start,
            round: 1,
            phase: Phase::Over {
                undercovers_won: false,
            },
            log: Vec::new(),
            moments: 1,
            phases: 0,
        };
        let names: Vec<&str> = game.seats.iter().map(|seat| seat.name.as_str()).collect();
        game.announce(format!(
            "{NAME}：{} 人（{}），{} 个卧底。{}",
            players,
            names.join("、"),
            undercovers_for(players),
            if players >= 6 {
                TOLD_RULES_SIX
            } else {
                TOLD_RULES
            }
        ));
        game.describe(game.order(), false);
        game
    }

    fn announce(&mut self, text: String) {
        self.log.push(Logged { text, said: false });
    }

    fn said(&mut self, text: String) {
        self.log.push(Logged { text, said: true });
    }

    /// How it ended, as the group remembers it.
    pub fn outcome(&self) -> Option<String> {
        let Phase::Over { undercovers_won } = self.phase else {
            return None;
        };
        let names: Vec<&str> = (0..self.seats.len())
            .filter(|seat| self.undercover[*seat])
            .map(|seat| self.name(seat))
            .collect();
        Some(format!(
            "{}赢了，卧底是{}，词是「{}」和「{}」",
            if undercovers_won { "卧底" } else { "平民" },
            names.join("、"),
            self.words.civilian,
            self.words.undercover
        ))
    }

    /// The pair, as kept among those used.
    pub fn used(&self) -> String {
        self.words.kept()
    }

    fn word(&self, seat: usize) -> &str {
        if self.undercover[seat] {
            &self.words.undercover
        } else {
            &self.words.civilian
        }
    }

    fn name(&self, seat: usize) -> &str {
        &self.seats[seat].name
    }

    /// Those still in, in seat order from this round's first.
    fn order(&self) -> Vec<usize> {
        let players = self.seats.len();
        let first = (self.start + self.round as usize - 1) % players.max(1);
        (0..players)
            .map(|offset| (first + offset) % players)
            .filter(|seat| self.alive[*seat])
            .collect()
    }

    fn describe(&mut self, order: Vec<usize>, tied: bool) {
        self.phases += 1;
        self.phase = Phase::Describe {
            order,
            at: 0,
            said: None,
            tied,
        };
        self.skip_out();
    }

    /// Past those no longer in; all described, on to the vote.
    fn skip_out(&mut self) {
        let Phase::Describe {
            order, at, tied, ..
        } = &self.phase
        else {
            return;
        };
        let (order, tied) = (order.clone(), *tied);
        let mut at = *at;
        while at < order.len() && !self.alive[order[at]] {
            at += 1;
        }
        if at < order.len() {
            if let Phase::Describe { at: now, .. } = &mut self.phase {
                *now = at;
            }
            return;
        }
        self.vote(
            order.into_iter().filter(|seat| self.alive[*seat]).collect(),
            tied,
        );
    }

    fn vote(&mut self, among: Vec<usize>, tied: bool) {
        // Of those tied, one went out saying their word: the round is done.
        if among.len() < 2 {
            self.round += 1;
            self.describe(self.order(), false);
            return;
        }
        let alive: Vec<usize> = (0..self.seats.len())
            .filter(|seat| self.alive[*seat])
            .collect();
        // A tie: the rest vote among those tied (all, when all are tied).
        let mut voters: Vec<usize> = if tied {
            alive
                .iter()
                .copied()
                .filter(|seat| !among.contains(seat))
                .collect()
        } else {
            alive.clone()
        };
        if voters.is_empty() {
            voters = alive;
        }
        voters.retain(|voter| among.iter().any(|seat| seat != voter));
        self.phases += 1;
        self.phase = Phase::Vote {
            among,
            voters,
            votes: Votes::default(),
            tied,
        };
    }

    /// The next of this round's describers.
    fn next_describer(&mut self) {
        if let Phase::Describe { at, said, .. } = &mut self.phase {
            *at += 1;
            *said = None;
        }
        self.phases += 1;
        self.skip_out();
    }

    /// `seat` goes out (`why` says so), said to be undercover or not.
    fn put_out(&mut self, seat: usize, why: String) {
        self.alive[seat] = false;
        self.moments += 1;
        let what = if self.undercover[seat] {
            "是卧底"
        } else {
            "不是卧底"
        };
        self.announce(format!("{why}{what}。"));
    }

    /// Whether it is over; if so, everything is told.
    fn check_over(&mut self) -> bool {
        let players = self.seats.len();
        let alive = self.alive.iter().filter(|alive| **alive).count();
        let undercovers = (0..players)
            .filter(|seat| self.alive[*seat] && self.undercover[*seat])
            .count();
        let civilians = alive - undercovers;
        let undercovers_won = if undercovers == 0 {
            false
        } else if civilians <= undercovers || (players >= 6 && alive <= 3) {
            true
        } else {
            return false;
        };
        let names: Vec<&str> = (0..players)
            .filter(|seat| self.undercover[*seat])
            .map(|seat| self.name(seat))
            .collect();
        self.announce(format!(
            "{}赢了。平民的词是「{}」，卧底的词是「{}」，卧底是{}。",
            if undercovers_won { "卧底" } else { "平民" },
            self.words.civilian,
            self.words.undercover,
            names.join("、")
        ));
        self.moments += 1;
        self.phases += 1;
        self.phase = Phase::Over { undercovers_won };
        true
    }

    /// Votes counted: one with the most goes out; a tie, once more among
    /// them; tied again or nobody voted, nobody goes out this round.
    fn count(&mut self) {
        let Phase::Vote {
            among, votes, tied, ..
        } = &self.phase
        else {
            return;
        };
        let (among, votes, tied) = (among.clone(), votes.clone(), *tied);
        let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
        for target in votes.values() {
            *counts.entry(*target).or_default() += 1;
        }
        let tally: Vec<String> = among
            .iter()
            .map(|seat| {
                format!(
                    "{} {} 票",
                    self.name(*seat),
                    counts.get(seat).copied().unwrap_or(0)
                )
            })
            .collect();
        let most = counts.values().copied().max().unwrap_or(0);
        let top: Vec<usize> = among
            .iter()
            .copied()
            .filter(|seat| most > 0 && counts.get(seat) == Some(&most))
            .collect();
        let heading = format!("第{}轮投票：{}", self.round, tally.join("、"));
        match top.len() {
            1 => {
                let why = format!("{heading}，{}出局，", self.name(top[0]));
                self.put_out(top[0], why);
                if self.check_over() {
                    return;
                }
            }
            0 => self.announce(format!("{heading}，没人投票，这轮没人出局。")),
            _ if !tied => {
                let names: Vec<&str> = top.iter().map(|seat| self.name(*seat)).collect();
                self.announce(format!(
                    "{heading}，{}平票：他们再各说一句，其他人在他们中间再投。",
                    names.join("、")
                ));
                self.describe(top, true);
                return;
            }
            _ => self.announce(format!("{heading}，又平票，这轮没人出局。")),
        }
        self.round += 1;
        self.describe(self.order(), false);
    }
}

/// A word and a line, as compared: no spaces or punctuation, lower case.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `text` says `word` outright.
pub fn says_outright(text: &str, word: &str) -> bool {
    let word = plain(word);
    !word.is_empty() && plain(text).contains(&word)
}

pub const JUDGE_SYSTEM: &str = "You judge one line in a game of 谁是卧底. The player was given a secret word and had to describe it without giving the word itself away. The literal word is already checked; say whether the line gives the word away some other way: a homophone or near-homophone of it (谐音), its characters split apart or spelled out (拆字), pinyin or initials of it, or the same word in another language. A description that only hints at what the word means is fine. Answer as JSON {\"givesAway\": true|false}.";

pub fn judge_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": { "givesAway": { "type": "boolean" } },
        "required": ["givesAway"],
        "additionalProperties": false
    })
}

pub fn words_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "civilian": { "type": "string" },
            "undercover": { "type": "string" }
        },
        "required": ["civilian", "undercover"],
        "additionalProperties": false
    })
}

/// What she is asked for the words: as herself when she only hosts, as a
/// plain judge with nothing of her when she holds a seat (she must not
/// know them).
pub fn words_prompt(soul: Option<&str>, recent: &[String]) -> Prepare {
    let task = "Think of the pair of words for a game of 谁是卧底 (who is the undercover): two everyday Chinese words, each of two to four characters, close enough that describing one could pass for the other (like 饺子 and 包子, 眼镜 and 墨镜), not one inside the other, nothing obscure. The first is what most players get, the second what the undercover gets. Answer as JSON {\"civilian\": \"…\", \"undercover\": \"…\"}.";
    let (voice, system) = match soul {
        Some(soul) => ("hers", format!("{soul}\n\n{task}")),
        None => ("judge", task.to_string()),
    };
    Prepare {
        voice,
        operation: "undercover_words",
        system,
        input: serde_json::json!({ "notTheseUsedLately": recent }).to_string(),
        schema_name: WORDS_SCHEMA,
        schema: words_schema(),
    }
}

/// The words, read back: two different words, neither in the other, not a
/// pair used lately.
pub fn read_words(raw: &str, recent: &[String]) -> Option<Words> {
    let words: Words = crate::answer::parse(raw)?;
    let words = Words {
        civilian: words.civilian.trim().to_string(),
        undercover: words.undercover.trim().to_string(),
    };
    let (a, b) = (plain(&words.civilian), plain(&words.undercover));
    let short = |word: &str| (1..=8).contains(&word.chars().count());
    if a.is_empty() || b.is_empty() || a.contains(&b) || b.contains(&a) || !short(&a) || !short(&b)
    {
        return None;
    }
    let again = recent.iter().any(|kept| {
        let mut pair: Vec<String> = kept.split('/').map(plain).collect();
        pair.sort();
        let mut this = vec![a.clone(), b.clone()];
        this.sort();
        pair == this
    });
    (!again).then_some(words)
}

/// How she describes, when her seat speaks.
pub const HOW_TO_DESCRIBE: &str = "describe your word in one short line, true of it, without saying the word, a homophone of it or its characters; you do not know whether you are the undercover, so keep it vague enough not to give your word away to the others if theirs differs.";

impl Rules for Undercover {
    type State = Undercover;
    const KIND: &'static str = KIND;
    const RULES: &'static str = "In 谁是卧底 most players got one word and the undercover(s) a close but different one; nobody is told which they are. In turn each describes their word in one line without saying it, then everyone votes out whom they think is an undercover. The civilians win when every undercover is out; the undercovers win when they are as many as the civilians left, or (with six or more players) when one lasts until three are left.";

    fn seats(game: &Self) -> Vec<Seat> {
        game.seats.clone()
    }

    fn facts(game: &Self) -> Vec<Fact> {
        let mut facts: Vec<Fact> = (0..game.seats.len())
            .map(|seat| Fact::seat(seat, format!("你的词：{}", game.word(seat))))
            .collect();
        facts.extend(game.log.iter().map(|logged| {
            if logged.said {
                Fact::heard(logged.text.clone())
            } else {
                Fact::all(logged.text.clone())
            }
        }));
        facts
    }

    fn choices(game: &Self, seat: usize) -> Vec<String> {
        match &game.phase {
            Phase::Vote {
                among,
                voters,
                votes,
                ..
            } if voters.contains(&seat) && !votes.contains_key(&seat) => among
                .iter()
                .filter(|other| **other != seat)
                .map(|other| game.name(*other).to_string())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn take(game: &mut Self, seat: usize, index: usize, _: &mut Rolls) {
        let Phase::Vote {
            among,
            voters,
            votes,
            ..
        } = &mut game.phase
        else {
            return;
        };
        if !voters.contains(&seat) || votes.contains_key(&seat) {
            return;
        }
        let open: Vec<usize> = among
            .iter()
            .copied()
            .filter(|other| *other != seat)
            .collect();
        let Some(target) = open.get(index) else {
            return;
        };
        votes.insert(seat, *target);
        if voters.iter().all(|voter| votes.contains_key(voter)) {
            game.count();
        }
    }

    fn over(game: &Self) -> bool {
        matches!(game.phase, Phase::Over { .. })
    }

    fn waiting(game: &Self) -> Waiting {
        match &game.phase {
            Phase::Describe {
                order,
                at,
                said: None,
                ..
            } => Waiting::Speak(order[*at]),
            Phase::Describe {
                order,
                at,
                said: Some(said),
                ..
            } => Waiting::Judge {
                system: JUDGE_SYSTEM.to_string(),
                input: serde_json::json!({ "word": game.word(order[*at]), "line": said })
                    .to_string(),
                schema_name: JUDGE_SCHEMA,
                schema: judge_schema(),
            },
            Phase::Vote { voters, votes, .. } => Waiting::Choose(
                voters
                    .iter()
                    .copied()
                    .filter(|voter| !votes.contains_key(voter))
                    .collect(),
            ),
            Phase::Over { .. } => Waiting::Over,
        }
    }

    fn phase(game: &Self) -> u64 {
        game.phases
    }

    fn seconds(_: &Self) -> f64 {
        SECONDS
    }

    fn spoke(game: &mut Self, seat: usize, text: &str) {
        let Phase::Describe {
            order, at, said, ..
        } = &game.phase
        else {
            return;
        };
        if order.get(*at) != Some(&seat) || said.is_some() {
            return;
        }
        let text = text.trim().to_string();
        if says_outright(&text, game.word(seat)) {
            let line = format!("{}：{text}", game.name(seat));
            game.said(line);
            let why = format!("{}说出了自己的词，出局，", game.name(seat));
            game.put_out(seat, why);
            if !game.check_over() {
                game.next_describer();
            }
            return;
        }
        if let Phase::Describe { said, .. } = &mut game.phase {
            *said = Some(text);
        }
        game.phases += 1;
    }

    fn judged(game: &mut Self, raw: Option<&str>, _: &mut Rolls) {
        let Phase::Describe {
            order,
            at,
            said: Some(said),
            ..
        } = &game.phase
        else {
            return;
        };
        let (seat, said) = (order[*at], said.clone());
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Judged {
            gives_away: bool,
        }
        // Not judged: what they said stands.
        let gives_away = raw
            .and_then(crate::answer::parse::<Judged>)
            .is_some_and(|judged| judged.gives_away);
        let line = format!("{}：{said}", game.name(seat));
        game.said(line);
        if gives_away {
            let why = format!("{}这句等于说出了自己的词，出局，", game.name(seat));
            game.put_out(seat, why);
            if game.check_over() {
                return;
            }
        }
        game.next_describer();
    }

    fn timed_out(game: &mut Self, _: &mut Rolls) {
        match &game.phase {
            Phase::Describe {
                order,
                at,
                said: None,
                ..
            } => {
                let seat = order[*at];
                let line = format!("{}没有描述。", game.name(seat));
                game.announce(line);
                game.next_describer();
            }
            Phase::Vote { voters, votes, .. } => {
                let missing: Vec<String> = voters
                    .iter()
                    .filter(|voter| !votes.contains_key(voter))
                    .map(|voter| game.seats[*voter].name.clone())
                    .collect();
                if !missing.is_empty() {
                    let line = format!("{}没有投票，算弃票。", missing.join("、"));
                    game.announce(line);
                }
                game.count();
            }
            _ => {}
        }
    }

    fn moments(game: &Self) -> usize {
        game.moments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::{narration_prompt, player_prompt, public_view, seat_view, speaker_prompt};

    fn seats(n: usize, hers: Option<usize>) -> Vec<Seat> {
        (0..n)
            .map(|seat| Seat {
                who: format!("telegram:{seat}"),
                name: format!("玩家{seat}"),
                hers: hers == Some(seat),
            })
            .collect()
    }

    fn words() -> Words {
        Words {
            civilian: "饺子".into(),
            undercover: "包子".into(),
        }
    }

    #[test]
    fn undercovers_by_players() {
        assert_eq!(
            (4..=10).map(undercovers_for).collect::<Vec<_>>(),
            [1, 1, 1, 2, 2, 2, 3]
        );
    }

    #[test]
    fn words_must_differ_and_be_new() {
        let recent = vec!["包子/饺子".to_string()];
        assert!(read_words(r#"{"civilian":"饺子","undercover":"包子"}"#, &recent).is_none());
        assert!(read_words(r#"{"civilian":"苹果","undercover":"苹果汁"}"#, &[]).is_none());
        assert!(read_words(r#"{"civilian":"眼镜","undercover":"眼镜"}"#, &[]).is_none());
        assert_eq!(
            read_words(r#"{"civilian":"眼镜","undercover":"墨镜"}"#, &recent).map(|w| w.kept()),
            Some("眼镜/墨镜".into())
        );
    }

    #[test]
    fn saying_the_word_puts_one_out_at_once() {
        let mut rolls = Rolls::new(1, Vec::new());
        let mut game = Undercover::deal(seats(5, None), words(), &mut rolls);
        let Waiting::Speak(first) = Undercover::waiting(&game) else {
            panic!()
        };
        let word = game.word(first).to_string();
        Undercover::spoke(&mut game, first, &format!("我觉得是 {word}！"));
        assert!(!game.alive[first]);
        assert!(
            game.log
                .iter()
                .any(|line| line.text.contains("说出了自己的词"))
        );
    }

    #[test]
    fn a_tie_is_voted_again_among_the_tied_then_nobody_goes_out() {
        let mut rolls = Rolls::new(2, Vec::new());
        let mut game = Undercover::deal(seats(4, None), words(), &mut rolls);
        // Everyone describes.
        while let Waiting::Speak(seat) = Undercover::waiting(&game) {
            Undercover::spoke(&mut game, seat, "很常见");
            Undercover::judged(&mut game, Some(r#"{"givesAway":false}"#), &mut rolls);
        }
        // 0→1, 1→0, 2→1? make a 2–2 tie between seats 0 and 1.
        let vote_for = |game: &Undercover, voter: usize, target: usize| {
            Undercover::choices(game, voter)
                .iter()
                .position(|name| *name == game.seats[target].name)
                .unwrap()
        };
        for (voter, target) in [(0, 1), (1, 0), (2, 0), (3, 1)] {
            let index = vote_for(&game, voter, target);
            Undercover::take(&mut game, voter, index, &mut rolls);
        }
        assert!(
            matches!(game.phase, Phase::Describe { tied: true, ref order, .. } if order == &vec![0, 1])
        );
        while let Waiting::Speak(seat) = Undercover::waiting(&game) {
            Undercover::spoke(&mut game, seat, "还是很常见");
            Undercover::judged(&mut game, None, &mut rolls);
        }
        let Waiting::Choose(voters) = Undercover::waiting(&game) else {
            panic!()
        };
        assert_eq!(voters, vec![2, 3]);
        for (voter, target) in [(2, 0), (3, 1)] {
            let index = vote_for(&game, voter, target);
            Undercover::take(&mut game, voter, index, &mut rolls);
        }
        assert!(game.alive.iter().all(|alive| *alive));
        assert_eq!(game.round, 2);
        assert!(game.log.iter().any(|line| line.text.contains("又平票")));
    }

    /// Whatever the seed, the number of players and which seat is hers:
    /// nothing she is asked as her seat (to describe, to vote) or as the
    /// voice of the game holds the word that is not hers, and before the
    /// end the voice holds neither (unless a seat said it aloud in the
    /// group, which everyone heard).
    #[test]
    fn the_other_word_never_reaches_her_seat() {
        for seed in 0..400u64 {
            let mut rolls = Rolls::new(seed, Vec::new());
            let players = FEWEST + rolls.range(0..=(MOST - FEWEST) as u64) as usize;
            let hers = rolls.range(0..=(players as u64 - 1)) as usize;
            let mut game = Undercover::deal(seats(players, Some(hers)), words(), &mut rolls);
            let mine = game.word(hers).to_string();
            let other = if mine == game.words.civilian {
                game.words.undercover.clone()
            } else {
                game.words.civilian.clone()
            };
            let mut aloud: Vec<String> = Vec::new();
            let mut steps = 0;
            while !Undercover::over(&game) && steps < 400 {
                steps += 1;
                let view = seat_view::<Undercover>(&game, hers);
                let (s1, i1) =
                    speaker_prompt("她", NAME, Undercover::RULES, HOW_TO_DESCRIBE, &view);
                let (s2, i2) = player_prompt("她", NAME, Undercover::RULES, &view);
                let (s3, i3) = narration_prompt("她", NAME, &public_view::<Undercover>(&game), &[]);
                let told = [&s1, &i1, &s2, &i2].map(|s| s.as_str()).join("\n");
                assert!(
                    aloud.contains(&other) || !told.contains(&other),
                    "seed {seed}: the other word reached her seat"
                );
                if game.alive[hers] {
                    assert!(view.facts().iter().any(|fact| fact.contains(&mine)));
                }
                let voice = [s3, i3].join("\n");
                assert!(
                    [&mine, &other]
                        .iter()
                        .all(|word| aloud.contains(word) || !voice.contains(*word)),
                    "seed {seed}: a word reached the voice before the end"
                );
                match Undercover::waiting(&game) {
                    Waiting::Speak(seat) => match rolls.range(0..=9) {
                        0 => Undercover::timed_out(&mut game, &mut rolls),
                        1 => {
                            let word = game.word(seat).to_string();
                            aloud.push(word.clone());
                            Undercover::spoke(&mut game, seat, &word);
                        }
                        _ => Undercover::spoke(&mut game, seat, "一种东西"),
                    },
                    Waiting::Judge { input, .. } => {
                        assert!(input.contains("一种东西"));
                        let gives = rolls.range(0..=9) == 0;
                        let raw = format!(r#"{{"givesAway":{gives}}}"#);
                        Undercover::judged(&mut game, Some(&raw), &mut rolls);
                    }
                    Waiting::Choose(voters) => {
                        if rolls.range(0..=9) == 0 {
                            Undercover::timed_out(&mut game, &mut rolls);
                            continue;
                        }
                        let voter = voters[0];
                        let open = Undercover::choices(&game, voter).len();
                        let index = rolls.range(0..=(open as u64 - 1)) as usize;
                        Undercover::take(&mut game, voter, index, &mut rolls);
                    }
                    Waiting::Ask(_) => unreachable!("nothing asked as it goes"),
                    Waiting::Over => break,
                }
            }
            assert!(Undercover::over(&game), "seed {seed}: never over");
            let end = public_view::<Undercover>(&game).facts().join("\n");
            assert!(end.contains(&mine) && end.contains(&other));
        }
    }
}
