//! G0 scenes of 剧本杀 while she writes it and when the site restarts: a
//! story that does not hold written again, writing given up after ten
//! minutes, most asking to stop, and a restart while she writes and another
//! mid-game.

use super::jubensha::{game, play_out, player, private_stays_private, sends, table, written};
use super::seats::whisper_in;
use super::undercover::remembered;
use super::*;
use myriad_merope::jubensha::{Jubensha, Phase, sample};

/// The first outline has two culprits and is written again, told what was
/// wrong; the check then finds the clues unfair and the clues and the
/// parts are written again; it begins. While they read, three of the four
/// say to stop: at more than half the truth is told at once.
#[tokio::test(start_paused = true)]
async fn g0_jubensha_written_again_then_most_ask_to_stop() {
    let chat = -77_153;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "jubensha_written_again_then_most_ask_to_stop") else {
        return;
    };
    said(&mut world, 2.5, "好\n[[game:jubensha]]");
    let [outline, clues, scripts, check] = sample(4);
    let mut two: serde_json::Value = serde_json::from_str(&outline).unwrap();
    two["characters"][0]["culprit"] = json!(true);
    answer(&mut world, "jubensha_outline", &two.to_string());
    answer(&mut world, "jubensha_outline", &outline);
    answer(&mut world, "jubensha_clues", &clues);
    answer(&mut world, "jubensha_scripts", &scripts);
    answer(
        &mut world,
        "jubensha_check",
        r#"{"problems":[{"part":"clues","severity":"fatal","what":"凶手只能靠私密线索推出"}]}"#,
    );
    answer(&mut world, "jubensha_clues", &clues);
    answer(&mut world, "jubensha_scripts", &scripts);
    answer(&mut world, "jubensha_check", &check);
    g0::enter(world);
    hear(line(chat, 1, 1121, "阿明", "@bot 剧本杀", true)).await;
    wait(37.5).await;
    let people = [
        (1121, "阿明"),
        (1122, "小红"),
        (1123, "阿强"),
        (1124, "小李"),
    ];
    for (id, (from, name)) in people.into_iter().enumerate() {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        200.0,
        true,
        &|seating, seat| {
            matches!(game(seating)?.phase, Phase::Theme { .. }).then(|| (seat % 2).to_string())
        },
    )
    .await;
    assert!(matches!(
        table(&venue)
            .as_ref()
            .and_then(game)
            .map(|game| &game.phase),
        Some(Phase::Read { .. })
    ));
    for (id, (from, name)) in people.into_iter().take(3).enumerate() {
        hear(line(chat, 40 + id as i64, from, name, "结束剧本杀", false)).await;
        wait(1.0).await;
    }
    wait(10.0).await;
    assert!(table(&venue).is_none());
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    let outlines: Vec<&serde_json::Value> = world
        .log
        .iter()
        .filter(|entry| {
            entry["kind"] == "model" && entry["what"]["operation"] == "jubensha_outline"
        })
        .collect();
    assert_eq!(outlines.len(), 2);
    assert!(
        outlines[1]
            .to_string()
            .contains("exactly one character must be the culprit")
    );
    let clue_asks = world
        .log
        .iter()
        .filter(|entry| entry["kind"] == "model" && entry["what"]["operation"] == "jubensha_clues")
        .count();
    assert_eq!(clue_asks, 2, "the clues written again after the check");
    let told = sends(&world.log);
    assert!(
        told.iter()
            .any(|line| line.contains("检查时发现有地方说不通"))
    );
    assert!(told.iter().any(|line| line.contains("想结束（2/4")));
    assert!(
        told.iter()
            .any(|line| line.contains("复盘：") && line.contains("凶手没被找出来"))
    );
    assert!(
        world
            .log
            .iter()
            .any(|entry| entry["kind"] == "remembered" && entry.to_string().contains("中途结束"))
    );
    g0::check("jubensha_written_again_then_most_ask_to_stop", &world.log);
    schema.drop_schema();
}

/// Writing may take ten minutes in all: each step comes slowly, the check
/// is still on when the time is up, and she says this time it is not on.
/// Nothing is remembered of a game that never began.
#[tokio::test(start_paused = true)]
async fn g0_jubensha_is_given_up_after_ten_minutes() {
    let chat = -77_154;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "jubensha_is_given_up_after_ten_minutes") else {
        return;
    };
    said(&mut world, 2.5, "好\n[[game:jubensha]]");
    written(&mut world, 4);
    for operation in [
        "jubensha_outline",
        "jubensha_clues",
        "jubensha_scripts",
        "jubensha_check",
    ] {
        world
            .delays
            .entry(operation.to_string())
            .or_default()
            .push_back(170.0);
    }
    g0::enter(world);
    hear(line(chat, 1, 1131, "阿明", "@bot 剧本杀", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        (1131, "阿明"),
        (1132, "小红"),
        (1133, "阿强"),
        (1134, "小李"),
    ]
    .into_iter()
    .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    // Nobody votes on the theme: picked after its minute.
    wait(900.0).await;
    assert!(table(&venue).is_none());
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    let told = sends(&world.log);
    assert!(told.iter().any(|line| line.contains("没人投题材")));
    assert!(told.iter().any(|line| line.contains("本子写得太久了")));
    assert!(
        !world
            .log
            .iter()
            .any(|entry| entry["kind"] == "remembered" && entry.to_string().contains("剧本杀")),
        "nothing kept of a game that never began"
    );
    g0::check("jubensha_is_given_up_after_ten_minutes", &world.log);
    schema.drop_schema();
}

/// A site restart while she writes the clues (the outline written): the
/// kept table comes back with the outline, only the clues are asked again.
/// Another restart in the first talk, a question asked in private still
/// waiting on its answer: it is asked again and answered to who asked.
/// Played (let pass) to its end.
#[tokio::test(start_paused = true)]
async fn g0_jubensha_goes_on_after_restarts() {
    let chat = -77_155;
    let venue = format!("telegram:{chat}");
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "jubensha_goes_on_after_restarts") else {
        return;
    };
    world.sendable.insert(chat.to_string());
    said(&mut world, 2.5, "好\n[[game:jubensha]]");
    let [outline, clues, scripts, check] = sample(4);
    answer(&mut world, "jubensha_outline", &outline);
    // The first answer for the clues comes after the restart, to nobody.
    answer(&mut world, "jubensha_clues", &clues);
    answer(&mut world, "jubensha_clues", &clues);
    answer(&mut world, "jubensha_scripts", &scripts);
    answer(&mut world, "jubensha_check", &check);
    world
        .delays
        .entry("jubensha_clues".into())
        .or_default()
        .push_back(150.0);
    answer(
        &mut world,
        "jubensha_answer",
        r#"{"verdict":"notInYourInfo","cite":[],"say":""}"#,
    );
    answer(
        &mut world,
        "jubensha_answer",
        r#"{"verdict":"notInYourInfo","cite":[],"say":""}"#,
    );
    world
        .delays
        .entry("jubensha_answer".into())
        .or_default()
        .push_back(19.0);
    g0::enter(world);
    hear(line(chat, 1, 1141, "阿明", "@bot 剧本杀", true)).await;
    wait(37.5).await;
    for (id, (from, name)) in [
        (1141, "阿明"),
        (1142, "小红"),
        (1143, "阿强"),
        (1144, "小李"),
    ]
    .into_iter()
    .enumerate()
    {
        hear(line(chat, 2 + id as i64, from, name, "加入", false)).await;
    }
    wait(45.3).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        30.0,
        true,
        &|seating, seat| {
            matches!(game(seating)?.phase, Phase::Theme { .. }).then(|| (seat % 2).to_string())
        },
    )
    .await;
    wait(20.0).await;
    assert!(matches!(
        table(&venue)
            .as_ref()
            .and_then(game)
            .map(|game| &game.phase),
        Some(Phase::Writing { step: 1, .. })
    ));
    g0::note("restart", json!({ "while": "writing" }));
    forget(&venue);
    take_back_tables(ChannelPlatform::Telegram).await;
    // Reading, and the first round's searching, then the talk.
    let culprit = |_: &Jubensha, _: usize| "周妈";
    let theme = |_: usize| 0;
    let players = player(&theme, false, &culprit);
    let mut at = 0.0;
    while at < 3000.0 {
        play_out(
            &venue,
            ChannelPlatform::Telegram,
            &chat.to_string(),
            9.7,
            true,
            &players,
        )
        .await;
        at += 9.7;
        if table(&venue)
            .as_ref()
            .and_then(game)
            .is_some_and(|game| matches!(game.phase, Phase::Discuss { .. }))
        {
            break;
        }
    }
    whisper_in(ChannelPlatform::Telegram, "1141", "我那晚到底在哪？").await;
    wait(5.0).await;
    assert_eq!(table(&venue).map(|seating| seating.asides.len()), Some(1));
    g0::note("restart", json!({ "while": "a question waits" }));
    forget(&venue);
    take_back_tables(ChannelPlatform::Telegram).await;
    play_out(
        &venue,
        ChannelPlatform::Telegram,
        &chat.to_string(),
        6000.0,
        true,
        &|_, _| None,
    )
    .await;
    assert!(table(&venue).is_none(), "played to its end");
    remembered(&schema.db, &venue).await;
    let world = g0::leave().expect("the world of the scene");
    private_stays_private(&world.log);
    let first = world
        .log
        .iter()
        .position(|entry| entry["kind"] == "restart")
        .unwrap();
    let outlines_after = world.log[first..]
        .iter()
        .filter(|entry| {
            entry["kind"] == "model" && entry["what"]["operation"] == "jubensha_outline"
        })
        .count();
    assert_eq!(outlines_after, 0, "the outline is not written again");
    let answered = world.log.iter().any(|entry| {
        entry["kind"] == "telegram.sendMessage"
            && entry["what"]["chat_id"] == "1141"
            && entry.to_string().contains("得靠搜证和推理")
    });
    assert!(answered, "the question is answered after the restart");
    assert!(sends(&world.log).iter().any(|line| line.contains("复盘：")));
    g0::check("jubensha_goes_on_after_restarts", &world.log);
    schema.drop_schema();
}
