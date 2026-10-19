//! G0 scenes of 剧本杀 at a group's table, through the site's executor: a
//! whole game on Telegram from buttons and on QQ from typed numbers, the
//! theme voted on (a tie picked by chance), questions to her in the group
//! and in private, and a wrong vote. Writing that goes wrong and restarts
//! are in `jubensha_writing`.

use super::seats::{line_on, whisper_in};
use super::undercover::remembered;
use super::*;
use myriad_merope::group::Seating;
use myriad_merope::hosting::{SeatGame, Waiting};
use myriad_merope::jubensha::{Jubensha, PICKS, Phase, sample};

pub(super) fn table(venue: &str) -> Option<Seating> {
    with_group(venue, |group| group.seating.clone()).flatten()
}

pub(super) fn game(seating: &Seating) -> Option<&Jubensha> {
    match seating.game.as_ref()? {
        SeatGame::Jubensha(game) => Some(game),
        _ => None,
    }
}

/// The story she writes, step by step.
pub(super) fn written(world: &mut World, players: usize) {
    let [outline, clues, scripts, check] = sample(players);
    answer(world, "jubensha_outline", &outline);
    answer(world, "jubensha_clues", &clues);
    answer(world, "jubensha_scripts", &scripts);
    answer(world, "jubensha_check", &check);
}

/// The character `seat` plays, by name.
fn plays(game: &Jubensha, seat: usize) -> String {
    let outline = game.drafts.outline.as_ref().unwrap();
    outline.characters[game.cast[seat]].name.clone()
}

/// How the scene's players play: the theme as `theme` has it, read at
/// once, each search a place with a clue it may find (its own seat's
/// first), the first private clue held shown in the first talk and then
/// done when `done`, the vote for whom `vote` names.
pub(super) fn player<'a>(
    theme: &'a dyn Fn(usize) -> usize,
    done: bool,
    vote: &'a dyn Fn(&Jubensha, usize) -> &'static str,
) -> impl Fn(&Seating, usize) -> Option<String> + 'a {
    move |seating, seat| {
        let game = game(seating)?;
        let choices = seating.game.as_ref()?.seat_view(seat).choices().to_vec();
        Some(match &game.phase {
            Phase::Theme { .. } => theme(seat).to_string(),
            Phase::Search { round } => {
                let clues = &game.drafts.clues.as_ref()?.clues;
                let outline = game.drafts.outline.as_ref()?;
                let me = &outline.characters[game.cast[seat]].id;
                let shown = |id: &str| {
                    clues
                        .iter()
                        .position(|clue| clue.id == id)
                        .is_some_and(|index| game.shown[index])
                };
                let first = if game.picks[seat] == PICKS {
                    seat
                } else {
                    seat + 1
                };
                (0..outline.locations.len())
                    .map(|offset| (first + offset) % outline.locations.len())
                    .find(|place| {
                        clues.iter().enumerate().any(|(index, clue)| {
                            game.found[index].is_none()
                                && clue.place == outline.locations[*place].id
                                && clue.round <= *round
                                && !clue.excluded.contains(me)
                                && (clue.requires.is_empty() || shown(&clue.requires))
                        })
                    })?
                    .to_string()
            }
            Phase::Discuss { round, offered, .. } => match offered[seat].first() {
                Some(card) if *round == 1 && !game.shown[*card] => "1".into(),
                _ if done => "0".into(),
                _ => return None,
            },
            Phase::Vote { .. } => {
                let name = vote(game, seat);
                choices
                    .iter()
                    .position(|choice| choice.starts_with(name))
                    .unwrap_or(choices.len() - 1)
                    .to_string()
            }
            _ => "0".into(),
        })
    }
}

/// The players at `venue`'s table every 9.7 seconds until `until` (from
/// now): the first seat introduces itself in two lines and says it is
/// done, the rest let it pass; whoever has to choose presses the button of
/// `chosen` (`buttons`) or types it.
pub(super) async fn play_out(
    venue: &str,
    platform: ChannelPlatform,
    chat: &str,
    until: f64,
    buttons: bool,
    chosen: &dyn Fn(&Seating, usize) -> Option<String>,
) {
    let mut id = 700;
    let mut at = 0.0;
    while at < until {
        wait(9.7).await;
        at += 9.7;
        let Some(seating) = table(venue) else {
            break;
        };
        let Some(on) = seating.game.as_ref() else {
            continue;
        };
        match on.waiting() {
            Waiting::Speak(0) => {
                let lines = match game(&seating).map(|game| &game.phase) {
                    Some(Phase::Intro { lines, .. }) => *lines,
                    _ => 0,
                };
                let sat = &seating.reachable[0];
                let user = sat.who.split_once(':').map_or("", |(_, user)| user);
                let text = match lines {
                    0 => "我是这家的老人了。",
                    1 => "那晚我一直在自己屋里。",
                    _ => "说完了",
                };
                id += 1;
                hear(line_on(platform, chat, id, user, &sat.name, text, false)).await;
            }
            Waiting::Choose(seats) => {
                for seat in seats {
                    let sat = &seating.reachable[seat];
                    let user = sat.who.split_once(':').map_or("", |(_, user)| user);
                    let Some(text) = chosen(&seating, seat) else {
                        continue;
                    };
                    if buttons {
                        let data = format!("seat:{}:{text}", seating.waited.unwrap_or(0));
                        let taken = pressed(platform, user, &data).await;
                        g0::note(
                            "pressed",
                            json!({ "from": sat.who, "data": data, "taken": taken }),
                        );
                    } else {
                        whisper_in(platform, user, &text).await;
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn sends(log: &[serde_json::Value]) -> Vec<String> {
    log.iter()
        .filter(|entry| entry["kind"] == "send")
        .map(|entry| entry.to_string())
        .collect()
}

/// No part of a seat's own, and nothing of the story's truth, is said in
/// the group or reaches her lines before the end tells it; she hosts it,
/// and never takes a seat.
pub(super) fn private_stays_private(log: &[serde_json::Value]) {
    let end = log
        .iter()
        .position(|entry| entry["kind"] == "send" && entry.to_string().contains("复盘："))
        .unwrap_or(log.len());
    for entry in &log[..end] {
        let voice = entry["kind"] == "model" && entry["what"]["operation"] == "jubensha_voice";
        let answer = entry["kind"] == "model" && entry["what"]["operation"] == "jubensha_answer";
        if entry["kind"] == "send" || voice {
            let text = entry.to_string();
            for secret in [
                "你的角色：",
                "你就是凶手",
                "你不想让人知道的",
                "欠了老爷的钱",
            ] {
                assert!(!text.contains(secret), "private in public: {entry}");
            }
        }
        if answer || voice {
            let text = entry.to_string();
            assert!(
                !text.contains("老爷要赶她走"),
                "the truth asked about: {entry}"
            );
        }
    }
    assert!(
        !log.iter()
            .any(|entry| entry.to_string().contains("her_move"))
    );
}

/// Someone from outside asks for 剧本杀 in a Telegram group: four join, one
/// asking for a theme, and they vote on it from buttons; she writes it
/// (saying how far she is), each is whispered their own part; they read,
/// the first introduces themselves in two lines; they search and show from
/// buttons; one asks her in the group (answered there, no turn of hers)
/// and in private (a line that would give a secret away not said); all
/// press that the talk is done; the vote catches the culprit; the truth,
/// the endings and the small goals are told, and the group remembers it.
#[tokio::test(start_paused = true)]
async fn g0_jubensha_is_played_on_telegram_with_buttons() {
    let chat = -77_151;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "jubensha_is_played_on_telegram_with_buttons") else {
        return;
    };
    said(&mut world, 2.5, "好，我来当 DM\n[[game:jubensha]]");
    written(&mut world, 4);
    answer(
        &mut world,
        "jubensha_voice",
        r#"{"say":"雨夜，老宅，大家小心。"}"#,
    );
    answer(
        &mut world,
        "jubensha_answer",
        r#"{"verdict":"answered","cite":[1],"say":"对，书房只有一扇门。"}"#,
    );
    answer(
        &mut world,
        "jubensha_answer",
        r#"{"verdict":"answered","cite":[0],"say":"茶里有安眠药。"}"#,
    );
    g0::enter(world);
    hear(line(chat, 1, 1101, "阿明", "@bot 来玩剧本杀", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        (1101, "阿明"),
        (1102, "小红"),
        (1103, "阿强"),
        (1104, "小李"),
    ]
    .into_iter()
    .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    hear(line(chat, 9, 1102, "小红", "主题：武侠", false)).await;
    wait(45.3).await;
    let culprit = |_: &Jubensha, _: usize| "周妈";
    let theme = |_: usize| 0;
    let players = player(&theme, true, &culprit);
    // Until they read.
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        400.0,
        true,
        &|seating, seat| {
            matches!(game(seating)?.phase, Phase::Theme { .. }).then(|| players(seating, seat))?
        },
    )
    .await;
    let seating = table(&venue).expect("on");
    let on = game(&seating).unwrap();
    assert_eq!(on.theme, "武侠");
    assert!(matches!(on.phase, Phase::Read { .. }));
    let asker = (0..4).find(|seat| plays(on, *seat) != "周妈").unwrap();
    let user = seating.reachable[asker]
        .who
        .split_once(':')
        .unwrap()
        .1
        .to_string();
    let name = seating.reachable[asker].name.clone();
    hear(line(
        chat,
        30,
        user.parse().unwrap(),
        &name,
        "@bot 书房是不是只有一扇门？",
        true,
    ))
    .await;
    wait(3.0).await;
    whisper_in(ChannelPlatform::Telegram, &user, "那杯茶有问题吗？").await;
    wait(5.0).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        6000.0,
        true,
        &players,
    )
    .await;
    assert!(table(&venue).is_none(), "played to its end");
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    private_stays_private(&world.log);
    let told = sends(&world.log);
    assert!(told.iter().any(|line| line.contains("对，书房只有一扇门")));
    assert!(told.iter().any(|line| line.contains("投中了")));
    assert!(told.iter().any(|line| line.contains("公开了线索")));
    let turns = world
        .log
        .iter()
        .filter(|entry| entry["kind"] == "say")
        .count();
    assert_eq!(
        turns, 1,
        "the question in the group is the game's, not a turn of hers"
    );
    assert!(
        world
            .log
            .iter()
            .filter(|entry| entry["kind"] == "telegram.sendMessage"
                && entry["what"]["chat_id"] == user.as_str())
            .all(|entry| !entry.to_string().contains("茶里有安眠药")),
        "a line giving a secret away is not whispered"
    );
    assert!(
        world
            .log
            .iter()
            .any(|entry| entry["kind"] == "telegram.sendMessage"
                && entry.to_string().contains("inline_keyboard"))
    );
    assert!(
        world
            .log
            .iter()
            .any(|entry| entry["kind"] == "remembered" && entry.to_string().contains("雨夜书房"))
    );
    g0::check("jubensha_is_played_on_telegram_with_buttons", &world.log);
    schema.drop_schema();
}

/// Four on QQ type numbers: the theme vote is two and two and she picks
/// one of those tied; they read, the rest of each wait is let pass; the
/// vote names the wrong one: the truth is told and the culprit escapes.
#[tokio::test(start_paused = true)]
async fn g0_jubensha_is_played_on_qq_with_numbers() {
    let chat = "77152";
    let venue = format!("onebot:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "jubensha_is_played_on_qq_with_numbers") else {
        return;
    };
    said(&mut world, 2.5, "好\n[[game:jubensha]]");
    written(&mut world, 4);
    g0::enter(world);
    let qq = ChannelPlatform::OneBot;
    hear(line_on(qq, chat, 1, "9201", "甲", "@bot 剧本杀", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        ("9201", "甲"),
        ("9202", "乙"),
        ("9203", "丙"),
        ("9204", "丁"),
    ]
    .iter()
    .enumerate()
    {
        hear(line_on(qq, chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    let wrong = |game: &Jubensha, seat: usize| {
        if plays(game, seat) == "林叔" {
            "阿青"
        } else {
            "林叔"
        }
    };
    let theme = |seat: usize| if seat < 2 { 0 } else { 3 };
    let players = player(&theme, false, &wrong);
    play_out(
        &venue,
        qq,
        chat,
        8000.0,
        false,
        &|seating, seat| match game(seating)?.phase {
            Phase::Theme { .. } | Phase::Read { .. } | Phase::Vote { .. } => players(seating, seat),
            _ => None,
        },
    )
    .await;
    assert!(table(&venue).is_none(), "played to its end");
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    private_stays_private(&world.log);
    let told = sends(&world.log);
    assert!(told.iter().any(|line| line.contains("平票，我挑了")));
    assert!(told.iter().any(|line| line.contains("投错了")));
    assert!(
        told.iter()
            .any(|line| line.contains("周妈第二天就离开了老宅"))
    );
    assert!(
        world
            .log
            .iter()
            .any(|entry| entry["kind"] == "remembered" && entry.to_string().contains("凶手跑了"))
    );
    g0::check("jubensha_is_played_on_qq_with_numbers", &world.log);
    schema.drop_schema();
}
