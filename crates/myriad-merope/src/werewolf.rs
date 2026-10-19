//! 狼人杀, as a game she hosts at a group's table (D2c-16): six to twelve
//! players, the standard roles (werewolves, villagers, the seer, the witch,
//! the hunter, and from nine players the guard), all of it mechanical here,
//! the referee's part.
//!
//! Each seat is whispered its role, the werewolves each other. At night the
//! guard, the werewolves and the seer move at once, in private: the guard
//! guards one (not the same two nights running), the werewolves each pick
//! whom to knife (the most picked is knifed; a tie, by chance; nobody
//! picked, nobody), the seer learns whether one is a werewolf. Then the
//! witch, told whom the knife fell on while she has her antidote, may save
//! them (not herself) or poison one, one potion a night. Guarded and saved
//! both, one dies all the same. At dawn the dead are told, never their
//! roles; who died the first night leaves last words. In seat order each
//! still in speaks once, then everyone votes in private; the most voted is
//! out and leaves last words. A tie: those tied speak once more and the rest
//! vote among them; tied again, nobody is out. The hunter, knifed or voted
//! out (not poisoned), may shoot one: on a board with a hunter every death,
//! whoever it is, is followed by the same private wait of the same length,
//! and a shot is told only when it is over, so neither the table's flow nor
//! its pauses tell who the hunter is or that the witch poisoned them. The
//! good side wins when no werewolf is left; the werewolves when every god
//! (seer, witch, hunter, guard) or every villager is out. At the end every role is told.
//!
//! No model is asked for any rule: only for her lines as the game's voice,
//! and her seat's speech and moves when she plays one (from her seat's view
//! alone).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::group::Rolls;
use crate::hosting::{Fact, Rules, Seat, Waiting};

pub const KIND: &str = "werewolf";
pub const NAME: &str = "狼人杀";
pub const FEWEST: usize = 6;
pub const MOST: usize = 12;
/// How long a seat has to speak, to move at night, and everyone to vote.
pub const SECONDS: f64 = 60.0;
/// How long the witch's turn lasts when she cannot move (out, or no potion
/// left): the night goes on a while all the same, so how long it takes
/// does not tell the table.
pub const IDLE_SECONDS: f64 = 15.0;
/// How long the wait after each death lasts on a board with a hunter, the
/// hunter's or not, shot taken early or not: always all of it.
pub const SHOT_SECONDS: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Werewolf,
    Villager,
    Seer,
    Witch,
    Hunter,
    Guard,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Werewolf => "狼人",
            Role::Villager => "村民",
            Role::Seer => "预言家",
            Role::Witch => "女巫",
            Role::Hunter => "猎人",
            Role::Guard => "守卫",
        }
    }

    /// One of the gods, whose loss to the last one wins it for the
    /// werewolves.
    pub fn god(self) -> bool {
        matches!(self, Role::Seer | Role::Witch | Role::Hunter | Role::Guard)
    }
}

/// The roles for `players` (six to twelve).
pub fn board(players: usize) -> Vec<Role> {
    use Role::*;
    let (wolves, villagers, hunter, guard) = match players {
        0..=6 => (2, 2, false, false),
        7 => (2, 2, true, false),
        8 => (3, 2, true, false),
        9 => (3, 2, true, true),
        10 => (3, 3, true, true),
        11 => (4, 3, true, true),
        _ => (4, 4, true, true),
    };
    let mut roles = vec![Werewolf; wolves];
    roles.extend([Seer, Witch]);
    if hunter {
        roles.push(Hunter);
    }
    if guard {
        roles.push(Guard);
    }
    roles.extend(vec![Villager; villagers]);
    roles
}

/// The board as told: `2 狼人、预言家、女巫、2 村民`.
fn board_line(roles: &[Role]) -> String {
    let mut counts: Vec<(Role, usize)> = Vec::new();
    for role in board(roles.len()) {
        match counts.iter_mut().find(|(known, _)| *known == role) {
            Some((_, count)) => *count += 1,
            None => counts.push((role, 1)),
        }
    }
    counts
        .iter()
        .map(|(role, count)| match count {
            1 => role.name().to_string(),
            _ => format!("{count} {}", role.name()),
        })
        .collect::<Vec<_>>()
        .join("、")
}

/// The rules as she tells the table at the start, for the roles in play.
fn told_rules(roles: &[Role]) -> String {
    let has = |role: Role| roles.contains(&role);
    let mut night = vec![
        "每人私聊收到自己的身份，狼人还会知道队友是谁。夜里狼人各自私聊选一个人刀，票多的那个被刀（一样多随机）".to_string(),
        "预言家验一个人是不是狼人".to_string(),
    ];
    if has(Role::Guard) {
        night.push("守卫守一个人，不能连着两晚守同一个人".to_string());
    }
    night.push("然后女巫知道谁被刀，有一瓶解药一瓶毒药，不能救自己，一晚只能用一瓶".to_string());
    let mut rules = format!("{}。", night.join("；"));
    if has(Role::Guard) {
        rules.push_str("被守又被救的人照样会死。");
    }
    if has(Role::Hunter) {
        rules.push_str("猎人被刀或被投出局可以开枪带走一个人，被毒不能开枪。");
    }
    let gods: Vec<&str> = [Role::Seer, Role::Witch, Role::Hunter, Role::Guard]
        .into_iter()
        .filter(|role| has(*role))
        .map(Role::name)
        .collect();
    rules.push_str(&format!(
        "天亮公布昨晚谁死了（不说身份），第一晚死的人留遗言；然后按顺序每人发言一次，再私聊投票，票最多的出局并留遗言；平票的人再各说一句，其他人在他们中间再投，又平就没人出局。狼人全出局好人赢；神职（{}）全出局或村民全出局，狼人赢。",
        gods.join("、")
    ));
    rules
}

/// Who voted for whom (`[voter, seat]`), in the order they voted; a seat of
/// none is a vote let go.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Votes(pub Vec<(usize, Option<usize>)>);

impl Votes {
    fn has(&self, voter: usize) -> bool {
        self.0.iter().any(|(by, _)| *by == voter)
    }

    fn cast(&mut self, voter: usize, seat: Option<usize>) {
        if !self.has(voter) {
            self.0.push((voter, seat));
        }
    }

    /// The seats with the most votes (none voted for, none).
    fn most(&self) -> (Vec<usize>, BTreeMap<usize, usize>) {
        let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
        for seat in self.0.iter().filter_map(|(_, seat)| *seat) {
            *counts.entry(seat).or_default() += 1;
        }
        let most = counts.values().copied().max().unwrap_or(0);
        let top = counts
            .iter()
            .filter(|(_, count)| most > 0 && **count == most)
            .map(|(seat, _)| *seat)
            .collect();
        (top, counts)
    }
}

/// What the night has come to so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tonight {
    /// Who moved in the first part of the night.
    #[serde(default)]
    pub acted: Vec<usize>,
    #[serde(default)]
    pub guarded: Option<usize>,
    #[serde(default)]
    pub knives: Votes,
    /// Whom the knife fell on.
    #[serde(default)]
    pub knifed: Option<usize>,
    #[serde(default)]
    pub saved: bool,
    #[serde(default)]
    pub poisoned: Option<usize>,
}

/// What comes after the dead of a moment: their last words, and on a board
/// with a hunter the wait in which a hunter may shoot (every dead seat's,
/// the same).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "do", content = "seat")]
pub enum Then {
    Words(usize),
    Shoot(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Next {
    Day,
    Night,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    /// The guard, the werewolves and the seer.
    Act,
    Witch,
}

/// What the game is doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "phase")]
pub enum Phase {
    Night {
        step: Step,
    },
    /// Last words and shots, one by one, then on to `next`.
    #[serde(rename_all = "camelCase")]
    After {
        queue: Vec<Then>,
        next: Next,
        /// The hunter has chosen in this wait: whom (none, no shot), told
        /// when the wait is over.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        aimed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        aim: Option<usize>,
    },
    /// Seats speak in `order`; `at` is whose turn it is. `tied` when it is a
    /// tie's second round.
    #[serde(rename_all = "camelCase")]
    Speak {
        order: Vec<usize>,
        at: usize,
        #[serde(default)]
        tied: bool,
    },
    /// `voters` each vote for one of `among` (not themselves) or let it go.
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
    Over {
        wolves_won: bool,
    },
}

/// A game of 狼人杀.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Werewolf {
    pub seats: Vec<Seat>,
    pub roles: Vec<Role>,
    pub alive: Vec<bool>,
    /// Who speaks first on the first day.
    pub start: usize,
    /// Which night it is, and the day after it.
    pub night: u32,
    pub phase: Phase,
    /// Everything the game holds, each with whom it is for, in order.
    pub log: Vec<Fact>,
    pub antidote: bool,
    pub poison: bool,
    /// Whom the guard guarded the night before.
    #[serde(default)]
    pub guarded_last: Option<usize>,
    #[serde(default)]
    pub tonight: Tonight,
    pub moments: usize,
    /// Which wait it is (one more each time the game moves on).
    pub phases: u64,
}

/// How she speaks, when her seat speaks.
pub const HOW_TO_SPEAK: &str = "say in one or two short lines what you make of the game so far and whom you trust or suspect, as a player of your role would; if you are a werewolf, do not give yourself or your teammates away; if these are your last words, say what you want the others to know.";

impl Werewolf {
    /// Dealt to `seats`: the roles and who speaks first, by `rolls`.
    pub fn deal(seats: Vec<Seat>, rolls: &mut Rolls) -> Self {
        let players = seats.len();
        let mut left = board(players);
        let mut roles = Vec::with_capacity(players);
        while !left.is_empty() {
            let pick = rolls.range(0..=(left.len() as u64 - 1)) as usize;
            roles.push(left.remove(pick));
        }
        let start = rolls.range(0..=(players.max(1) as u64 - 1)) as usize;
        let mut game = Self {
            alive: vec![true; players],
            seats,
            roles,
            start,
            night: 1,
            phase: Phase::Over { wolves_won: false },
            log: Vec::new(),
            antidote: true,
            poison: true,
            guarded_last: None,
            tonight: Tonight::default(),
            moments: 1,
            phases: 0,
        };
        let names: Vec<&str> = game.seats.iter().map(|seat| seat.name.as_str()).collect();
        let line = format!(
            "{NAME}：{} 人（{}），{}。{}",
            players,
            names.join("、"),
            board_line(&game.roles),
            told_rules(&game.roles)
        );
        game.announce(line);
        for seat in 0..players {
            let role = game.roles[seat];
            let mut dealt = format!("你的身份：{}。", role.name());
            if role == Role::Werewolf {
                let mates: Vec<&str> = game
                    .wolves()
                    .filter(|mate| *mate != seat)
                    .map(|mate| game.name(mate))
                    .collect();
                dealt.push_str(&format!("你的狼队友：{}。", mates.join("、")));
            }
            game.log.push(Fact::seat(seat, dealt));
        }
        game.nightfall();
        game
    }

    fn announce(&mut self, text: String) {
        self.log.push(Fact::all(text));
    }

    fn tell(&mut self, seat: usize, text: String) {
        self.log.push(Fact::seat(seat, text));
    }

    fn name(&self, seat: usize) -> &str {
        &self.seats[seat].name
    }

    fn wolves(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.seats.len()).filter(|seat| self.roles[*seat] == Role::Werewolf)
    }

    fn living(&self) -> Vec<usize> {
        (0..self.seats.len())
            .filter(|seat| self.alive[*seat])
            .collect()
    }

    /// The living seat with `role`, if any.
    fn living_role(&self, role: Role) -> Option<usize> {
        (0..self.seats.len()).find(|seat| self.alive[*seat] && self.roles[*seat] == role)
    }

    fn names(&self, seats: &[usize]) -> String {
        seats
            .iter()
            .map(|seat| self.name(*seat))
            .collect::<Vec<_>>()
            .join("、")
    }

    /// How it ended, as the group remembers it.
    pub fn outcome(&self) -> Option<String> {
        let Phase::Over { wolves_won } = self.phase else {
            return None;
        };
        let wolves: Vec<usize> = self.wolves().collect();
        Some(format!(
            "{}赢了，狼人是{}",
            if wolves_won { "狼人" } else { "好人" },
            self.names(&wolves)
        ))
    }

    /// Who moves in the first part of the night, still to.
    fn acting(&self) -> Vec<usize> {
        (0..self.seats.len())
            .filter(|seat| {
                self.alive[*seat]
                    && matches!(self.roles[*seat], Role::Werewolf | Role::Seer | Role::Guard)
                    && !self.tonight.acted.contains(seat)
            })
            .collect()
    }

    /// The witch can move tonight: in, and with a potion she may use.
    fn witch_moves(&self) -> Option<usize> {
        let witch = self.living_role(Role::Witch)?;
        let can_save = self.antidote && self.tonight.knifed.is_some_and(|seat| seat != witch);
        (can_save || self.poison).then_some(witch)
    }

    fn nightfall(&mut self) {
        self.tonight = Tonight::default();
        self.announce(format!(
            "第{}夜，天黑了。夜里有事做的人看私聊。",
            self.night
        ));
        let night = self.night;
        for seat in self.acting() {
            let line = match self.roles[seat] {
                Role::Werewolf => {
                    format!("第{night}夜：选今晚刀谁。狼队友各自选，票多的那个被刀，一样多随机。")
                }
                Role::Seer => format!("第{night}夜：选今晚验谁。"),
                _ => format!("第{night}夜：选今晚守谁，不能连着两晚守同一个人。"),
            };
            self.tell(seat, line);
        }
        self.phases += 1;
        self.phase = Phase::Night { step: Step::Act };
    }

    /// The first part of the night is done: the knife falls, and the witch
    /// is told where while she has her antidote.
    fn to_witch(&mut self, rolls: &mut Rolls) {
        let (top, _) = self.tonight.knives.most();
        self.tonight.knifed = match top.len() {
            0 => None,
            1 => Some(top[0]),
            _ => Some(top[rolls.range(0..=(top.len() as u64 - 1)) as usize]),
        };
        if let Some(witch) = self.living_role(Role::Witch) {
            let night = self.night;
            let line = match (self.antidote, self.tonight.knifed) {
                (true, Some(seat)) if seat == witch => {
                    format!("第{night}夜：被刀的是你自己，解药不能救自己。")
                }
                (true, Some(seat)) => format!("第{night}夜：被刀的是{}。", self.name(seat)),
                (true, None) => format!("第{night}夜：今晚没人被刀。"),
                (false, _) => format!("第{night}夜：解药用过了，看不到今晚刀了谁。"),
            };
            let line = if self.witch_moves().is_some() {
                format!("{line}要用药吗？")
            } else {
                line
            };
            self.tell(witch, line);
        }
        self.phases += 1;
        self.phase = Phase::Night { step: Step::Witch };
    }

    /// Dawn: who died is told (not their roles), then their last words (the
    /// first night) and the hunter's shot, then the day.
    fn dawn(&mut self) {
        let Tonight {
            guarded,
            knifed,
            saved,
            poisoned,
            ..
        } = self.tonight.clone();
        self.guarded_last = guarded;
        let mut dead: Vec<usize> = Vec::new();
        if let Some(seat) = knifed {
            let guarded = guarded == Some(seat);
            // Guarded and saved both: dies all the same.
            if guarded == saved {
                dead.push(seat);
            }
        }
        if let Some(seat) = poisoned
            && !dead.contains(&seat)
        {
            dead.push(seat);
        }
        dead.sort_unstable();
        for seat in &dead {
            self.alive[*seat] = false;
        }
        self.moments += 1;
        if dead.is_empty() {
            self.announce("天亮了，昨晚是平安夜。".to_string());
        } else {
            let line = format!("天亮了，昨晚死的是{}。", self.names(&dead));
            self.announce(line);
        }
        if self.check_over() {
            return;
        }
        let mut queue = Vec::new();
        for seat in dead {
            if self.night == 1 {
                queue.push(Then::Words(seat));
            }
            if self.has_hunter() {
                queue.push(Then::Shoot(seat));
            }
        }
        self.after(queue, Next::Day);
    }

    fn after(&mut self, queue: Vec<Then>, next: Next) {
        self.phases += 1;
        self.phase = Phase::After {
            queue,
            next,
            aimed: false,
            aim: None,
        };
        self.go_on();
    }

    /// The next of what comes after the dead; none left, on.
    fn go_on(&mut self) {
        let Phase::After { queue, next, .. } = &self.phase else {
            return;
        };
        if !queue.is_empty() {
            return;
        }
        match *next {
            Next::Day => self.day(),
            Next::Night => {
                self.night += 1;
                self.nightfall();
            }
        }
    }

    fn done_with_first(&mut self) {
        if let Phase::After {
            queue, aimed, aim, ..
        } = &mut self.phase
            && !queue.is_empty()
        {
            queue.remove(0);
            *aimed = false;
            *aim = None;
        }
        self.phases += 1;
        self.go_on();
    }

    fn day(&mut self) {
        let players = self.seats.len();
        let first = (self.start + self.night as usize - 1) % players.max(1);
        let order: Vec<usize> = (0..players)
            .map(|offset| (first + offset) % players)
            .filter(|seat| self.alive[*seat])
            .collect();
        self.announce(format!(
            "第{}天：从{}开始，按顺序每人发言一次。",
            self.night,
            self.name(order[0])
        ));
        self.speak(order, false);
    }

    fn speak(&mut self, order: Vec<usize>, tied: bool) {
        self.phases += 1;
        self.phase = Phase::Speak { order, at: 0, tied };
    }

    fn next_speaker(&mut self) {
        let Phase::Speak { order, at, tied } = &mut self.phase else {
            return;
        };
        *at += 1;
        self.phases += 1;
        if *at < order.len() {
            return;
        }
        let (among, tied) = (order.clone(), *tied);
        self.vote(among, tied);
    }

    fn vote(&mut self, among: Vec<usize>, tied: bool) {
        let alive = self.living();
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

    /// Votes counted: one with the most is out and leaves last words; a
    /// tie, once more among them; tied again or nobody voted, nobody.
    fn count(&mut self) {
        let Phase::Vote {
            among, votes, tied, ..
        } = &self.phase
        else {
            return;
        };
        let (among, votes, tied) = (among.clone(), votes.clone(), *tied);
        let (top, counts) = votes.most();
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
        let heading = format!("第{}天投票：{}", self.night, tally.join("、"));
        match top.len() {
            1 => {
                let seat = top[0];
                let line = format!("{heading}，{}出局。", self.name(seat));
                self.announce(line);
                self.alive[seat] = false;
                self.moments += 1;
                if self.check_over() {
                    return;
                }
                let mut queue = vec![Then::Words(seat)];
                if self.has_hunter() {
                    queue.push(Then::Shoot(seat));
                }
                self.after(queue, Next::Night);
            }
            0 => {
                self.announce(format!("{heading}，没人投票，今天没人出局。"));
                self.after(Vec::new(), Next::Night);
            }
            _ if !tied => {
                let line = format!(
                    "{heading}，{}平票：他们再各说一句，其他人在他们中间再投。",
                    self.names(&top)
                );
                self.announce(line);
                self.speak(top, true);
            }
            _ => {
                self.announce(format!("{heading}，又平票，今天没人出局。"));
                self.after(Vec::new(), Next::Night);
            }
        }
    }

    /// Whether it is over (屠边); if so, every role is told.
    fn check_over(&mut self) -> bool {
        let living = |good: &dyn Fn(Role) -> bool| {
            (0..self.seats.len())
                .filter(|seat| self.alive[*seat] && good(self.roles[*seat]))
                .count()
        };
        let wolves = living(&|role| role == Role::Werewolf);
        let gods = living(&Role::god);
        let villagers = living(&|role| role == Role::Villager);
        let wolves_won = if wolves == 0 {
            false
        } else if gods == 0 || villagers == 0 {
            true
        } else {
            return false;
        };
        let roles: Vec<String> = (0..self.seats.len())
            .map(|seat| format!("{} {}", self.name(seat), self.roles[seat].name()))
            .collect();
        self.announce(format!(
            "{}赢了。身份：{}。",
            if wolves_won { "狼人" } else { "好人" },
            roles.join("、")
        ));
        self.moments += 1;
        self.phases += 1;
        self.phase = Phase::Over { wolves_won };
        true
    }

    /// Who `seat` may pick among at night, as it is let: the living, but
    /// for a werewolf not its own, for the seer not itself, for the guard
    /// not whom it guarded last night.
    fn night_targets(&self, seat: usize) -> Vec<usize> {
        let role = self.roles[seat];
        self.living()
            .into_iter()
            .filter(|other| match role {
                Role::Werewolf => self.roles[*other] != Role::Werewolf,
                Role::Seer => *other != seat,
                Role::Guard => self.guarded_last != Some(*other),
                _ => false,
            })
            .collect()
    }

    /// Whom the witch may poison: the living but herself.
    fn poisonable(&self, witch: usize) -> Vec<usize> {
        if !self.poison {
            return Vec::new();
        }
        self.living()
            .into_iter()
            .filter(|seat| *seat != witch)
            .collect()
    }

    /// What the witch may do: let it go, save, poison.
    fn witch_choices(&self, witch: usize) -> Vec<(String, Option<WitchMove>)> {
        let mut choices = vec![("不用药".to_string(), None)];
        if self.antidote
            && let Some(seat) = self.tonight.knifed.filter(|seat| *seat != witch)
        {
            choices.push((
                format!("用解药救{}", self.name(seat)),
                Some(WitchMove::Save),
            ));
        }
        for seat in self.poisonable(witch) {
            choices.push((
                format!("毒{}", self.name(seat)),
                Some(WitchMove::Poison(seat)),
            ));
        }
        choices
    }

    /// Whom the hunter may shoot: the living (it is out).
    fn shootable(&self) -> Vec<usize> {
        self.living()
    }

    /// The board has a hunter (from seven players).
    fn has_hunter(&self) -> bool {
        self.roles.contains(&Role::Hunter)
    }

    /// Whom the wait after the dead is on now waits for: the dead seat when
    /// it is the hunter, not poisoned, and yet to choose; else nobody, and
    /// it waits all the same.
    fn shooting(&self) -> Option<usize> {
        let Phase::After {
            queue,
            aimed: false,
            ..
        } = &self.phase
        else {
            return None;
        };
        let Some(Then::Shoot(seat)) = queue.first() else {
            return None;
        };
        (self.roles[*seat] == Role::Hunter && self.tonight.poisoned != Some(*seat)).then_some(*seat)
    }

    /// The wait after the dead is over: the hunter's shot, if one was
    /// taken, is told now.
    fn shot(&mut self) {
        let Phase::After {
            queue,
            aim: Some(target),
            ..
        } = &self.phase
        else {
            return;
        };
        let Some(Then::Shoot(hunter)) = queue.first().copied() else {
            return;
        };
        let target = *target;
        self.alive[target] = false;
        self.moments += 1;
        let line = format!(
            "{}是猎人，开枪带走了{}。",
            self.name(hunter),
            self.name(target)
        );
        self.announce(line);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WitchMove {
    Save,
    Poison(usize),
}

/// Not to move, at night or for the hunter: the last choice.
const NO_KNIFE: &str = "不刀";
const NO_GUARD: &str = "不守";
const NO_SHOT: &str = "不开枪";
const NO_VOTE: &str = "弃票";

impl Rules for Werewolf {
    type State = Werewolf;
    const KIND: &'static str = KIND;
    const RULES: &'static str = "In 狼人杀 each player was dealt a role in secret: werewolves (who know each other), villagers, and gods (the seer, who learns at night whether one player is a werewolf; the witch, with one potion to save whom the werewolves knifed and one to poison someone; the hunter, who may shoot someone when knifed or voted out; and with nine or more the guard, who guards one player a night). At night the werewolves knife one player; by day those left speak in turn and vote one out. The good side wins when no werewolf is left; the werewolves win when every god or every villager is out. Roles are not told when someone dies.";
    const NEWS_IN_PRIVATE: bool = true;

    fn seats(game: &Self) -> Vec<Seat> {
        game.seats.clone()
    }

    fn facts(game: &Self) -> Vec<Fact> {
        game.log.clone()
    }

    fn choices(game: &Self, seat: usize) -> Vec<String> {
        match &game.phase {
            Phase::Night { step: Step::Act } if game.acting().contains(&seat) => {
                let mut choices: Vec<String> = game
                    .night_targets(seat)
                    .into_iter()
                    .map(|other| game.name(other).to_string())
                    .collect();
                match game.roles[seat] {
                    Role::Werewolf => choices.push(NO_KNIFE.into()),
                    Role::Guard => choices.push(NO_GUARD.into()),
                    _ => {}
                }
                choices
            }
            Phase::Night { step: Step::Witch } if game.witch_moves() == Some(seat) => game
                .witch_choices(seat)
                .into_iter()
                .map(|(choice, _)| choice)
                .collect(),
            Phase::After { .. } => match game.shooting() {
                Some(hunter) if hunter == seat => {
                    let mut choices: Vec<String> = game
                        .shootable()
                        .into_iter()
                        .map(|other| game.name(other).to_string())
                        .collect();
                    choices.push(NO_SHOT.into());
                    choices
                }
                _ => Vec::new(),
            },
            Phase::Vote {
                among,
                voters,
                votes,
                ..
            } if voters.contains(&seat) && !votes.has(seat) => {
                let mut choices: Vec<String> = among
                    .iter()
                    .filter(|other| **other != seat)
                    .map(|other| game.name(*other).to_string())
                    .collect();
                choices.push(NO_VOTE.into());
                choices
            }
            _ => Vec::new(),
        }
    }

    fn take(game: &mut Self, seat: usize, index: usize, rolls: &mut Rolls) {
        if index >= Self::choices(game, seat).len() {
            return;
        }
        match game.phase.clone() {
            Phase::Night { step: Step::Act } => {
                let targets = game.night_targets(seat);
                let target = targets.get(index).copied();
                let night = game.night;
                match game.roles[seat] {
                    Role::Werewolf => {
                        game.tonight.knives.cast(seat, target);
                        let picked = match target {
                            Some(other) => format!("刀{}", game.name(other)),
                            None => NO_KNIFE.to_string(),
                        };
                        let mates: Vec<usize> = game
                            .wolves()
                            .filter(|mate| *mate != seat && game.alive[*mate])
                            .collect();
                        for mate in mates {
                            let line =
                                format!("第{night}夜：狼队友{}选了{picked}。", game.name(seat));
                            game.tell(mate, line);
                        }
                    }
                    Role::Seer => {
                        if let Some(other) = target {
                            let what = if game.roles[other] == Role::Werewolf {
                                "狼人"
                            } else {
                                "好人"
                            };
                            let line = format!("第{night}夜验{}：{what}。", game.name(other));
                            game.tell(seat, line);
                        }
                    }
                    _ => game.tonight.guarded = target,
                }
                game.tonight.acted.push(seat);
                if game.acting().is_empty() {
                    game.to_witch(rolls);
                }
            }
            Phase::Night { step: Step::Witch } => {
                match game.witch_choices(seat)[index].1 {
                    Some(WitchMove::Save) => {
                        game.antidote = false;
                        game.tonight.saved = true;
                    }
                    Some(WitchMove::Poison(other)) => {
                        game.poison = false;
                        game.tonight.poisoned = Some(other);
                    }
                    None => {}
                }
                game.dawn();
            }
            // Kept till the wait is over: it goes on all the same.
            Phase::After { .. } => {
                let target = game.shootable().get(index).copied();
                if let Phase::After { aimed, aim, .. } = &mut game.phase {
                    *aimed = true;
                    *aim = target;
                }
            }
            Phase::Vote { among, .. } => {
                let open: Vec<usize> = among
                    .iter()
                    .copied()
                    .filter(|other| *other != seat)
                    .collect();
                if let Phase::Vote { votes, voters, .. } = &mut game.phase {
                    votes.cast(seat, open.get(index).copied());
                    if voters.iter().all(|voter| votes.has(*voter)) {
                        game.count();
                    }
                }
            }
            _ => {}
        }
    }

    fn over(game: &Self) -> bool {
        matches!(game.phase, Phase::Over { .. })
    }

    fn waiting(game: &Self) -> Waiting {
        match &game.phase {
            Phase::Night { step: Step::Act } => Waiting::Choose(game.acting()),
            Phase::Night { step: Step::Witch } => {
                Waiting::Choose(game.witch_moves().into_iter().collect())
            }
            Phase::After { queue, .. } => match queue.first() {
                Some(Then::Words(seat)) => Waiting::Speak(*seat),
                Some(Then::Shoot(_)) => Waiting::Choose(game.shooting().into_iter().collect()),
                None => Waiting::Choose(Vec::new()),
            },
            Phase::Speak { order, at, .. } => Waiting::Speak(order[*at]),
            Phase::Vote { voters, votes, .. } => Waiting::Choose(
                voters
                    .iter()
                    .copied()
                    .filter(|voter| !votes.has(*voter))
                    .collect(),
            ),
            Phase::Over { .. } => Waiting::Over,
        }
    }

    fn phase(game: &Self) -> u64 {
        game.phases
    }

    fn seconds(game: &Self) -> f64 {
        match game.phase {
            Phase::Night { step: Step::Witch } if game.witch_moves().is_none() => IDLE_SECONDS,
            Phase::After { ref queue, .. } if matches!(queue.first(), Some(Then::Shoot(_))) => {
                SHOT_SECONDS
            }
            _ => SECONDS,
        }
    }

    fn spoke(game: &mut Self, seat: usize, text: &str) {
        let text = text.trim();
        match &game.phase {
            Phase::Speak { order, at, .. } if order.get(*at) == Some(&seat) => {
                let line = format!("{}：{text}", game.name(seat));
                game.log.push(Fact::heard(line));
                game.next_speaker();
            }
            Phase::After { queue, .. } if queue.first() == Some(&Then::Words(seat)) => {
                let line = format!("{}的遗言：{text}", game.name(seat));
                game.log.push(Fact::heard(line));
                game.done_with_first();
            }
            _ => {}
        }
    }

    fn timed_out(game: &mut Self, rolls: &mut Rolls) {
        match game.phase.clone() {
            // Who did not move at night does nothing tonight.
            Phase::Night { step: Step::Act } => game.to_witch(rolls),
            Phase::Night { step: Step::Witch } => game.dawn(),
            Phase::After { queue, .. } => {
                if let Some(Then::Words(seat)) = queue.first() {
                    let line = format!("{}没有留遗言。", game.name(*seat));
                    game.announce(line);
                }
                game.shot();
                if game.check_over() {
                    return;
                }
                game.done_with_first();
            }
            Phase::Speak { order, at, .. } => {
                let line = format!("{}没有发言。", game.name(order[at]));
                game.announce(line);
                game.next_speaker();
            }
            Phase::Vote { voters, votes, .. } => {
                let missing: Vec<usize> = voters
                    .iter()
                    .copied()
                    .filter(|voter| !votes.has(*voter))
                    .collect();
                if !missing.is_empty() {
                    let line = format!("{}没有投票，算弃票。", game.names(&missing));
                    game.announce(line);
                }
                game.count();
            }
            Phase::Over { .. } => {}
        }
    }

    fn moments(game: &Self) -> usize {
        game.moments
    }

    fn public_wait(game: &Self) -> bool {
        matches!(game.phase, Phase::Speak { .. } | Phase::Vote { .. })
            || matches!(&game.phase, Phase::After { queue, .. } if matches!(queue.first(), Some(Then::Words(_))))
    }

    fn speak_turn(game: &Self) -> Option<(String, String)> {
        match &game.phase {
            Phase::After { .. } => Some((
                String::new(),
                " 留遗言（一分钟；说「等等」可以多等一分钟）。".to_string(),
            )),
            _ => Some((
                "轮到 ".to_string(),
                " 发言了（一分钟；说「等等」可以多等一分钟）。".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::{
        Audience, narration_prompt, player_prompt, public_view, seat_view, speaker_prompt,
    };

    fn seats(n: usize, hers: Option<usize>) -> Vec<Seat> {
        (0..n)
            .map(|seat| Seat {
                who: format!("telegram:{seat}"),
                name: format!("玩家{seat}"),
                hers: hers == Some(seat),
            })
            .collect()
    }

    fn seat_of(game: &Werewolf, role: Role) -> usize {
        game.roles.iter().position(|known| *known == role).unwrap()
    }

    /// `seat` picks `target` by name, or the move named `choice`.
    fn pick(game: &mut Werewolf, seat: usize, choice: &str, rolls: &mut Rolls) {
        let index = Werewolf::choices(game, seat)
            .iter()
            .position(|known| known == choice)
            .unwrap_or_else(|| panic!("{choice} not among {:?}", Werewolf::choices(game, seat)));
        Werewolf::take(game, seat, index, rolls);
    }

    /// Everyone acting tonight lets it go but `moves`.
    fn night(game: &mut Werewolf, moves: &[(usize, String)], rolls: &mut Rolls) {
        for (seat, choice) in moves {
            pick(game, *seat, choice, rolls);
        }
        if matches!(game.phase, Phase::Night { step: Step::Act }) {
            Werewolf::timed_out(game, rolls);
        }
    }

    #[test]
    fn boards_by_players() {
        for players in FEWEST..=MOST {
            let roles = board(players);
            assert_eq!(roles.len(), players);
            let count = |role: Role| roles.iter().filter(|known| **known == role).count();
            assert_eq!(
                count(Role::Werewolf),
                match players {
                    6 | 7 => 2,
                    8..=10 => 3,
                    _ => 4,
                }
            );
            assert_eq!(count(Role::Seer), 1);
            assert_eq!(count(Role::Witch), 1);
            assert_eq!(count(Role::Hunter), usize::from(players >= 7));
            assert_eq!(count(Role::Guard), usize::from(players >= 9));
            assert!(count(Role::Villager) >= 2);
        }
        assert_eq!(
            board_line(&board(9)),
            "3 狼人、预言家、女巫、猎人、守卫、2 村民"
        );
    }

    #[test]
    fn werewolves_know_each_other_and_nobody_else_does() {
        let mut rolls = Rolls::new(5, Vec::new());
        let game = Werewolf::deal(seats(9, None), &mut rolls);
        let wolves: Vec<usize> = game.wolves().collect();
        for seat in 0..9 {
            let known = seat_view::<Werewolf>(&game, seat).facts().join("\n");
            assert!(known.contains(&format!("你的身份：{}", game.roles[seat].name())));
            let mates = seat_view::<Werewolf>(&game, seat)
                .facts()
                .iter()
                .find(|fact| fact.contains("狼队友"))
                .cloned()
                .unwrap_or_default();
            for wolf in &wolves {
                let named = mates.contains(&game.seats[*wolf].name);
                assert_eq!(
                    named,
                    game.roles[seat] == Role::Werewolf && seat != *wolf,
                    "seat {seat} and wolf {wolf}"
                );
            }
        }
        assert!(
            !public_view::<Werewolf>(&game)
                .facts()
                .join("\n")
                .contains("你的身份")
        );
    }

    #[test]
    fn the_seer_alone_learns_what_she_checked() {
        let mut rolls = Rolls::new(9, Vec::new());
        let mut game = Werewolf::deal(seats(6, None), &mut rolls);
        let seer = seat_of(&game, Role::Seer);
        let wolf = seat_of(&game, Role::Werewolf);
        let name = game.seats[wolf].name.clone();
        pick(&mut game, seer, &name, &mut rolls);
        let line = format!("第1夜验{name}：狼人。");
        assert!(seat_view::<Werewolf>(&game, seer).facts().contains(&line));
        for seat in (0..6).filter(|seat| *seat != seer) {
            assert!(!seat_view::<Werewolf>(&game, seat).facts().contains(&line));
        }
    }

    #[test]
    fn the_witch_saves_once_never_herself_and_one_potion_a_night() {
        let mut rolls = Rolls::new(11, Vec::new());
        let mut game = Werewolf::deal(seats(6, None), &mut rolls);
        let witch = seat_of(&game, Role::Witch);
        let wolves: Vec<usize> = game.wolves().collect();
        let victim = (0..6)
            .find(|seat| game.roles[*seat] == Role::Villager)
            .unwrap();
        let victim_name = game.seats[victim].name.clone();
        let knife: Vec<(usize, String)> = wolves
            .iter()
            .map(|wolf| (*wolf, victim_name.clone()))
            .collect();
        night(&mut game, &knife, &mut rolls);
        let told = seat_view::<Werewolf>(&game, witch).facts().join("\n");
        assert!(told.contains(&format!("被刀的是{victim_name}")));
        let choices = Werewolf::choices(&game, witch);
        assert!(choices.contains(&format!("用解药救{victim_name}")));
        assert!(
            !choices
                .iter()
                .any(|choice| choice.contains(&game.seats[witch].name) && choice.contains("救"))
        );
        pick(
            &mut game,
            witch,
            &format!("用解药救{victim_name}"),
            &mut rolls,
        );
        assert!(game.alive[victim] && !game.antidote && game.poison);
        assert!(
            game.log
                .iter()
                .any(|fact| fact.text == "天亮了，昨晚是平安夜。")
        );
        // Played to the next night: no antidote, so she is not told where
        // the knife fell; she may only poison.
        while !matches!(game.phase, Phase::Night { .. }) {
            match Werewolf::waiting(&game) {
                Waiting::Speak(seat) => Werewolf::spoke(&mut game, seat, "过"),
                _ => Werewolf::timed_out(&mut game, &mut rolls),
            }
        }
        night(&mut game, &knife, &mut rolls);
        let choices = Werewolf::choices(&game, witch);
        assert!(choices.iter().all(|choice| !choice.contains("救")));
        assert!(choices.iter().any(|choice| choice.starts_with('毒')));
        let told = seat_view::<Werewolf>(&game, witch).facts().join("\n");
        assert!(told.contains("第2夜：解药用过了"));
    }

    #[test]
    fn the_witch_knifed_cannot_save_herself() {
        let mut rolls = Rolls::new(13, Vec::new());
        let mut game = Werewolf::deal(seats(6, None), &mut rolls);
        let witch = seat_of(&game, Role::Witch);
        let name = game.seats[witch].name.clone();
        let knife: Vec<(usize, String)> = game.wolves().map(|wolf| (wolf, name.clone())).collect();
        night(&mut game, &knife, &mut rolls);
        assert!(
            Werewolf::choices(&game, witch)
                .iter()
                .all(|choice| !choice.contains("救"))
        );
        Werewolf::timed_out(&mut game, &mut rolls);
        assert!(!game.alive[witch]);
    }

    #[test]
    fn guarded_and_saved_both_dies_and_not_the_same_guard_twice() {
        let mut rolls = Rolls::new(17, Vec::new());
        let mut game = Werewolf::deal(seats(9, None), &mut rolls);
        let guard = seat_of(&game, Role::Guard);
        let witch = seat_of(&game, Role::Witch);
        let victim = (0..9)
            .find(|seat| game.roles[*seat] == Role::Villager)
            .unwrap();
        let name = game.seats[victim].name.clone();
        let mut moves: Vec<(usize, String)> =
            game.wolves().map(|wolf| (wolf, name.clone())).collect();
        moves.push((guard, name.clone()));
        night(&mut game, &moves, &mut rolls);
        pick(&mut game, witch, &format!("用解药救{name}"), &mut rolls);
        assert!(!game.alive[victim], "guarded and saved: dies");
        assert_eq!(game.guarded_last, Some(victim));
        while !matches!(game.phase, Phase::Night { .. }) {
            match Werewolf::waiting(&game) {
                Waiting::Speak(seat) => Werewolf::spoke(&mut game, seat, "过"),
                _ => Werewolf::timed_out(&mut game, &mut rolls),
            }
        }
        if game.alive[guard] {
            let choices = Werewolf::choices(&game, guard);
            assert!(!choices.contains(&name));
        }
    }

    #[test]
    fn a_guarded_one_knifed_lives_and_is_not_guarded_the_next_night() {
        let mut rolls = Rolls::new(19, Vec::new());
        let mut game = Werewolf::deal(seats(9, None), &mut rolls);
        let guard = seat_of(&game, Role::Guard);
        let name = game.seats[guard].name.clone();
        let mut moves: Vec<(usize, String)> =
            game.wolves().map(|wolf| (wolf, name.clone())).collect();
        moves.push((guard, name.clone()));
        night(&mut game, &moves, &mut rolls);
        Werewolf::timed_out(&mut game, &mut rolls);
        assert!(game.alive[guard]);
        while !matches!(game.phase, Phase::Night { .. }) {
            match Werewolf::waiting(&game) {
                Waiting::Speak(seat) => Werewolf::spoke(&mut game, seat, "过"),
                _ => Werewolf::timed_out(&mut game, &mut rolls),
            }
        }
        let choices = Werewolf::choices(&game, guard);
        assert!(!choices.contains(&name) && choices.contains(&NO_GUARD.to_string()));
    }

    #[test]
    fn the_hunter_shoots_when_voted_out_not_when_poisoned() {
        let mut rolls = Rolls::new(23, Vec::new());
        let mut game = Werewolf::deal(seats(8, None), &mut rolls);
        let hunter = seat_of(&game, Role::Hunter);
        let witch = seat_of(&game, Role::Witch);
        let hunter_name = game.seats[hunter].name.clone();
        // Night one: nobody knifed; the witch poisons the hunter, who
        // leaves last words and then is waited on like anyone, with no
        // shot to take.
        night(&mut game, &[], &mut rolls);
        if witch != hunter {
            pick(&mut game, witch, &format!("毒{hunter_name}"), &mut rolls);
            assert!(!game.alive[hunter]);
            Werewolf::spoke(&mut game, hunter, "过");
            assert_eq!(Werewolf::waiting(&game), Waiting::Choose(Vec::new()));
            assert_eq!(Werewolf::seconds(&game), SHOT_SECONDS);
            assert!(Werewolf::choices(&game, hunter).is_empty());
        }

        let mut rolls = Rolls::new(29, Vec::new());
        let mut game = Werewolf::deal(seats(8, None), &mut rolls);
        let hunter = seat_of(&game, Role::Hunter);
        let hunter_name = game.seats[hunter].name.clone();
        night(&mut game, &[], &mut rolls);
        Werewolf::timed_out(&mut game, &mut rolls);
        while let Waiting::Speak(seat) = Werewolf::waiting(&game) {
            Werewolf::spoke(&mut game, seat, "过");
        }
        for voter in game.living() {
            if voter != hunter {
                pick(&mut game, voter, &hunter_name, &mut rolls);
            } else {
                pick(&mut game, voter, NO_VOTE, &mut rolls);
            }
        }
        assert!(!game.alive[hunter]);
        // Last words, then the shot, told when the wait is over.
        assert_eq!(Werewolf::waiting(&game), Waiting::Speak(hunter));
        Werewolf::spoke(&mut game, hunter, "我是猎人");
        assert_eq!(Werewolf::waiting(&game), Waiting::Choose(vec![hunter]));
        let wolf = game.wolves().next().unwrap();
        let wolf_name = game.seats[wolf].name.clone();
        let phase = Werewolf::phase(&game);
        pick(&mut game, hunter, &wolf_name, &mut rolls);
        assert!(game.alive[wolf]);
        assert_eq!(Werewolf::phase(&game), phase);
        assert_eq!(Werewolf::waiting(&game), Waiting::Choose(Vec::new()));
        Werewolf::timed_out(&mut game, &mut rolls);
        assert!(!game.alive[wolf]);
        assert!(
            game.log
                .iter()
                .any(|fact| fact.text == format!("{hunter_name}是猎人，开枪带走了{wolf_name}。"))
        );
    }

    /// What the table sees of a game from the deal to the second night,
    /// `victim` knifed, poisoned or voted out the first time (`how`) and
    /// everyone else letting each move go: each line told to all, every
    /// name as 某人 and the tally left out, and each wait that ran its whole
    /// time, how long.
    fn seen(seed: u64, victim: Role, how: &str) -> Vec<String> {
        let mut rolls = Rolls::new(seed, Vec::new());
        let mut game = Werewolf::deal(seats(8, None), &mut rolls);
        let victim = seat_of(&game, victim);
        let target = game.seats[victim].name.clone();
        let names: Vec<String> = game.seats.iter().map(|seat| seat.name.clone()).collect();
        let mut seen = Vec::new();
        let mut told = 0;
        let mut steps = 0;
        while !(game.night == 2 || Werewolf::over(&game)) {
            steps += 1;
            assert!(steps < 200);
            for fact in &game.log[told..] {
                if fact.to == Audience::All {
                    let mut line = fact.text.clone();
                    for name in &names {
                        line = line.replace(name.as_str(), "某人");
                    }
                    // The tally goes by seat: whose count is where is not
                    // the point here.
                    if line.contains("投票：")
                        && let Some((heading, _)) = line.split_once('：')
                        && let Some((_, outcome)) = line.rsplit_once('，')
                    {
                        line = format!("{heading}：……，{outcome}");
                    }
                    seen.push(line);
                }
            }
            told = game.log.len();
            let (phase, seconds) = (Werewolf::phase(&game), Werewolf::seconds(&game));
            match Werewolf::waiting(&game) {
                Waiting::Speak(seat) => Werewolf::spoke(&mut game, seat, "过"),
                Waiting::Choose(seats) => {
                    for seat in seats {
                        let choices = Werewolf::choices(&game, seat);
                        let wanted = match (&game.phase, game.roles[seat]) {
                            (Phase::Vote { .. }, _) if how == "vote" => target.clone(),
                            (Phase::Night { .. }, Role::Werewolf) if how == "knife" => {
                                target.clone()
                            }
                            (Phase::Night { .. }, Role::Witch) if how == "poison" => {
                                format!("毒{target}")
                            }
                            (Phase::Night { .. }, Role::Witch) => "不用药".to_string(),
                            _ => choices.last().cloned().unwrap_or_default(),
                        };
                        let index = choices
                            .iter()
                            .position(|choice| *choice == wanted)
                            .unwrap_or(choices.len() - 1);
                        Werewolf::take(&mut game, seat, index, &mut rolls);
                        if Werewolf::phase(&game) != phase {
                            break;
                        }
                    }
                }
                _ => unreachable!(),
            }
            if Werewolf::phase(&game) == phase {
                seen.push(format!("（等满 {seconds} 秒）"));
                Werewolf::timed_out(&mut game, &mut rolls);
            }
        }
        assert!(!game.alive[victim], "{how}: never out");
        seen
    }

    /// Knifed, poisoned or voted out, a hunter (who lets the shot go) and a
    /// villager leave the table the same: the same lines told to all, in
    /// the same messages, with the same waits between them.
    #[test]
    fn a_death_looks_the_same_whether_or_not_it_is_the_hunter() {
        for seed in [29, 41, 43] {
            for how in ["knife", "poison", "vote"] {
                let hunter = seen(seed, Role::Hunter, how);
                let villager = seen(seed, Role::Villager, how);
                assert_eq!(hunter, villager, "seed {seed}, {how}");
                assert!(
                    hunter
                        .iter()
                        .any(|line| line == &format!("（等满 {SHOT_SECONDS} 秒）")),
                    "seed {seed}, {how}: {hunter:?}"
                );
            }
        }
    }

    #[test]
    fn a_tie_is_spoken_again_and_voted_among_the_tied() {
        let mut rolls = Rolls::new(31, Vec::new());
        let mut game = Werewolf::deal(seats(6, None), &mut rolls);
        night(&mut game, &[], &mut rolls);
        Werewolf::timed_out(&mut game, &mut rolls);
        while let Waiting::Speak(seat) = Werewolf::waiting(&game) {
            Werewolf::spoke(&mut game, seat, "过");
        }
        let names: Vec<String> = game.seats.iter().map(|seat| seat.name.clone()).collect();
        for (voter, target) in [(0, 1), (1, 0), (2, 0), (3, 1), (4, 1), (5, 0)] {
            pick(&mut game, voter, &names[target], &mut rolls);
        }
        assert!(
            matches!(game.phase, Phase::Speak { tied: true, ref order, .. } if order == &vec![0, 1])
        );
        Werewolf::spoke(&mut game, 0, "不是我");
        Werewolf::spoke(&mut game, 1, "也不是我");
        assert_eq!(Werewolf::waiting(&game), Waiting::Choose(vec![2, 3, 4, 5]));
        for (voter, target) in [(2, 0), (3, 1), (4, 0), (5, 1)] {
            pick(&mut game, voter, &names[target], &mut rolls);
        }
        assert!(game.alive.iter().all(|alive| *alive));
        assert!(game.log.iter().any(|fact| fact.text.contains("又平票")));
        assert!(matches!(game.phase, Phase::Night { .. }));
        assert_eq!(game.night, 2);
    }

    /// Whatever the seed, the number of players and which seat is hers, at
    /// every step: nothing she is asked as her seat (to speak, to move)
    /// holds what was told to another seat alone, unless it was told to
    /// her as well; nothing she is asked as the game's voice holds what
    /// was told to any seat alone; and no role but by the hunter's own shot
    /// is told to all before the end.
    #[test]
    fn what_another_seat_was_told_never_reaches_her() {
        for seed in 0..400u64 {
            let mut rolls = Rolls::new(seed, Vec::new());
            let players = FEWEST + rolls.range(0..=(MOST - FEWEST) as u64) as usize;
            let hers = rolls.range(0..=(players as u64 - 1)) as usize;
            let mut game = Werewolf::deal(seats(players, Some(hers)), &mut rolls);
            let mut steps = 0;
            while !Werewolf::over(&game) && steps < 2000 {
                steps += 1;
                let view = seat_view::<Werewolf>(&game, hers);
                let (s1, i1) = speaker_prompt("她", NAME, Werewolf::RULES, HOW_TO_SPEAK, &view);
                let (s2, i2) = player_prompt("她", NAME, Werewolf::RULES, &view);
                let (s3, i3) = narration_prompt("她", NAME, &public_view::<Werewolf>(&game), &[]);
                let as_seat = [&s1, &i1, &s2, &i2].map(|s| s.as_str()).join("\n");
                let as_voice = [s3, i3].join("\n");
                for fact in &game.log {
                    let Audience::Seat(other) = fact.to else {
                        continue;
                    };
                    let shared = view.facts().contains(&fact.text);
                    let quoted = serde_json::to_string(&fact.text).unwrap();
                    let quoted = &quoted[1..quoted.len() - 1];
                    if other != hers && !shared {
                        assert!(
                            !as_seat.contains(quoted),
                            "seed {seed}: seat {other}'s {:?} reached her seat {hers}",
                            fact.text
                        );
                    }
                    assert!(
                        !as_voice.contains(quoted),
                        "seed {seed}: {:?} reached the voice",
                        fact.text
                    );
                }
                let public = public_view::<Werewolf>(&game).facts().join("\n");
                assert!(
                    !public.contains("身份："),
                    "seed {seed}: roles told before the end"
                );
                match Werewolf::waiting(&game) {
                    Waiting::Speak(seat) => {
                        if rolls.range(0..=4) == 0 {
                            Werewolf::timed_out(&mut game, &mut rolls);
                        } else {
                            Werewolf::spoke(&mut game, seat, "我是好人");
                        }
                    }
                    Waiting::Choose(seats) if !seats.is_empty() && rolls.range(0..=7) != 0 => {
                        let seat = seats[rolls.range(0..=(seats.len() as u64 - 1)) as usize];
                        let open = Werewolf::choices(&game, seat).len();
                        let index = rolls.range(0..=(open as u64 - 1)) as usize;
                        Werewolf::take(&mut game, seat, index, &mut rolls);
                    }
                    Waiting::Choose(_) => Werewolf::timed_out(&mut game, &mut rolls),
                    Waiting::Judge { .. } => unreachable!("no judging"),
                    Waiting::Over => break,
                }
            }
            assert!(Werewolf::over(&game), "seed {seed}: never over");
            let end = public_view::<Werewolf>(&game).facts().join("\n");
            for seat in 0..players {
                assert!(end.contains(&format!(
                    "{} {}",
                    game.seats[seat].name,
                    game.roles[seat].name()
                )));
            }
        }
    }

    #[test]
    fn the_werewolves_win_when_every_god_is_out() {
        let mut rolls = Rolls::new(37, Vec::new());
        let mut game = Werewolf::deal(seats(6, None), &mut rolls);
        let seer = seat_of(&game, Role::Seer);
        let witch = seat_of(&game, Role::Witch);
        game.alive[seer] = false;
        let name = game.seats[witch].name.clone();
        let knife: Vec<(usize, String)> = game.wolves().map(|wolf| (wolf, name.clone())).collect();
        night(&mut game, &knife, &mut rolls);
        Werewolf::timed_out(&mut game, &mut rolls);
        assert_eq!(game.phase, Phase::Over { wolves_won: true });
        assert!(game.outcome().unwrap().starts_with("狼人赢了"));
    }
}
