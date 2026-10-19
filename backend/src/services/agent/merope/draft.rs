//! Before what she wrote goes out (see `myriad_merope::draft`): whether it
//! still fits, asked only when something came while she wrote it.

use std::time::Duration;

/// Someone is waiting on her answer: a quick look, not a second thought.
const LOOK_FOR: Duration = Duration::from_secs(8);

/// What her group mind asks of a model (see `myriad_merope::group`): its
/// prompt as the core put it, in the voice it names, billed to `owner`; a
/// JSON answer to `schema`, or what she says to the whole prompt (`input`).
/// The raw answer, or how it failed (`unavailable`, `timeout`, `cut`,
/// `invalid`, `failed`).
#[allow(clippy::too_many_arguments)]
pub async fn ask(
    how: myriad_merope::group::AskKind,
    voice: &str,
    owner: i32,
    operation: &str,
    within: Duration,
    system: &str,
    input: &str,
    schema_name: &str,
    schema: &serde_json::Value,
) -> Result<String, &'static str> {
    use super::call::{Failure, Voice};
    use myriad_merope::group::AskKind;
    let operation: &'static str = match operation {
        "draft_still" => "draft_still",
        "group_making_sense" => "group_making_sense",
        "group_chime" => "group_chime",
        "meme" => "meme",
        "group_stranger" => "group_stranger",
        "group_member" => "group_member",
        "soup_judge" => "soup_judge",
        "soup_start" => "soup_start",
        "undercover_words" => "undercover_words",
        "undercover_judge" => "undercover_judge",
        "undercover_voice" => "undercover_voice",
        "undercover_her_move" => "undercover_her_move",
        "werewolf_voice" => "werewolf_voice",
        "werewolf_her_move" => "werewolf_her_move",
        "avalon_voice" => "avalon_voice",
        "avalon_her_move" => "avalon_her_move",
        "jubensha_outline" => "jubensha_outline",
        "jubensha_clues" => "jubensha_clues",
        "jubensha_scripts" => "jubensha_scripts",
        "jubensha_check" => "jubensha_check",
        "jubensha_answer" => "jubensha_answer",
        "jubensha_voice" => "jubensha_voice",
        _ => "group_mind",
    };
    let voice = match voice {
        "hers" => Voice::Hers,
        "hersAtLength" => Voice::HersAtLength,
        "thorough" => Voice::Thorough,
        _ => Voice::Judge,
    };
    let class = |failure: Failure| match failure {
        Failure::Unavailable => "unavailable",
        Failure::Timeout => "timeout",
        Failure::StreamInterrupted => "cut",
        Failure::InvalidOutput => "invalid",
        _ => "failed",
    };
    let ask = super::call::Ask::new(voice, owner, operation).within(within);
    match how {
        AskKind::Say => {
            #[cfg(test)]
            if let Some(said) = crate::services::channel_group::g0::say(operation, input).await {
                return said;
            }
            ask.model()
                .await
                .map_err(class)?
                .say(input)
                .await
                .map_err(class)
        }
        AskKind::Json => ask
            .json_raw(system, input, schema_name, schema)
            .await
            .map_err(class),
        // Pictures are looked at through `look`; nothing asks this way yet.
        AskKind::Look => Err("unavailable"),
    }
}

/// Whether `draft`, her answer to `said`, still fits now that `came` arrived
/// while she wrote it (who, what, oldest first). Nothing came: it does. No
/// answer in time: it goes, as it would have before she looked.
pub async fn still_fits(owner: i32, said: &str, draft: &str, came: &[(String, String)]) -> bool {
    if came.is_empty() {
        return true;
    }
    let answer = super::call::Ask::new(super::call::Voice::Judge, owner, "draft_still")
        .within(LOOK_FOR)
        .json_raw(
            &myriad_merope::draft::system(),
            &myriad_merope::draft::input(said, draft, came),
            myriad_merope::draft::SCHEMA_NAME,
            &myriad_merope::draft::schema(),
        )
        .await;
    let send = answer
        .ok()
        .and_then(|raw| myriad_merope::draft::parse(&raw))
        .unwrap_or(true);
    tracing::info!(
        owner,
        came = came.len(),
        send,
        "[Merope] looked at what she wrote again"
    );
    send
}

/// The look on the site's judgment model, a few times each: held back when
/// the talk moved past it, sent when what came does not touch it.
/// `cargo test -p myriad-backend --bin myriad-backend -- --ignored
/// draft_still_on_the_sites_model --nocapture` (reads the site's database,
/// asks its model).
#[tokio::test]
#[ignore = "reads the site's database and asks its judgment model"]
async fn draft_still_on_the_sites_model() {
    let _db = crate::services::agent::semantic_eval::load_configured_lite().await;
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(LOOK_FOR))
        .await
        .expect("judgment model");
    let cases: [(&str, &str, &[(&str, &str)], bool); 4] = [
        (
            "周六几点集合",
            "九点吧，老地方见",
            &[("阿红", "刚说过了 十点 在东门")],
            false,
        ),
        (
            "帮我想个猫的名字",
            "叫小橘怎么样",
            &[("them", "算了不用了 我已经想好了")],
            false,
        ),
        (
            "你今天吃啥了",
            "吃了碗面，你呢",
            &[("them", "我刚吃完火锅")],
            true,
        ),
        (
            "这首歌好听吗",
            "副歌那段挺上头的",
            &[("小周", "哈哈哈哈")],
            true,
        ),
    ];
    let mut right = 0;
    for (said, draft, came, expect) in cases {
        let came: Vec<(String, String)> = came
            .iter()
            .map(|(who, what)| (who.to_string(), what.to_string()))
            .collect();
        for _ in 0..3 {
            let send = judge
                .analyze_json(
                    &myriad_merope::draft::system(),
                    &myriad_merope::draft::input(said, draft, &came),
                    myriad_merope::draft::SCHEMA_NAME,
                    Some(&myriad_merope::draft::schema()),
                )
                .await
                .ok()
                .and_then(|raw| myriad_merope::draft::parse(&raw));
            println!("{said} → {draft} | came {came:?} | send {send:?} (expect {expect})");
            right += usize::from(send == Some(expect));
        }
    }
    println!("{right}/12 as expected");
}
