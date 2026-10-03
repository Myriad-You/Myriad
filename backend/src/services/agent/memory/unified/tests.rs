use super::*;

#[test]
fn a_record_cut_short_is_made_whole_and_none_is_cut_again() {
    // As the site kept them, cut at 400 inside the last field.
    let song = r#"{"key":"song:netease:1","thing":{"kind":"song","id":"1","name":"恋人を射ち堕とした日","artist":"Sound Horizon"},"reaction":"liked","tell":true,"heard":"Length 3:58. About 122 BPM, pulse clarity 0.53, spectral centroid 2162"#;
    let whole: serde_json::Value = serde_json::from_str(&mend_cut_record(song).unwrap()).unwrap();
    assert_eq!(whole["reaction"], "liked");
    assert_eq!(whole["tell"], true);
    assert_eq!(whole["thing"]["artist"], "Sound Horizon");
    assert!(whole.get("heard").is_none());
    // Cut inside a nested record: the whole field goes, not half of it.
    let inquiry = r#"{"key":"inquiry:q","thing":{"kind":"inquiry","question":"补血草为什么不褪色，\"苞片\"是什么？"},"reaction":"liked","explored":{"expected":"想查清楚","sources":[],"compared":{"surprise":"none","new":"","alr"#;
    let whole: serde_json::Value =
        serde_json::from_str(&mend_cut_record(inquiry).unwrap()).unwrap();
    assert_eq!(whole["reaction"], "liked");
    assert!(whole.get("explored").is_none());
    assert!(
        whole["thing"]["question"]
            .as_str()
            .unwrap()
            .contains("\"苞片\"")
    );
    // Whole ones and what is not a record are left alone.
    assert_eq!(mend_cut_record(r#"{"a":1}"#), None);
    assert_eq!(mend_cut_record("听他说的"), None);

    // From now on, evidence past the cap loses its largest field whole.
    let big = serde_json::json!({
        "key": "song:netease:1",
        "thing": { "kind": "song" },
        "reaction": "moved",
        "heard": "x".repeat(MAX_EVIDENCE_CHARS),
    })
    .to_string();
    let kept: serde_json::Value = serde_json::from_str(&bounded_evidence(&big)).unwrap();
    assert_eq!(kept["reaction"], "moved");
    assert!(kept.get("heard").is_none());
    assert_eq!(bounded_evidence("短的"), "短的");
}

#[test]
fn what_she_noted_in_passing_is_recalled_when_named_not_as_filler() {
    let mut played = row(
        "played",
        "常在 Steam 上玩《Hades》，最近一次是 09-25",
        0.3,
        10,
    );
    played.source = "presence".into();
    let rows = vec![played, row("cat", "养了一只猫叫年糕", 0.6, 1_000)];
    let filler: Vec<String> = rank(rows.clone(), None, 8)
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(filler, vec!["cat".to_string()]);
    let named: Vec<String> = rank(rows, Some("Hades"), 8)
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert!(named.contains(&"played".to_string()));
}

#[test]
fn what_she_looked_up_comes_back_when_the_talk_comes_to_it_not_every_turn() {
    // One failed search, put before her every turn, became a story she
    // kept retelling to whatever they said.
    let mut looked = row(
        "looked",
        "搜出来的净是些乱七八糟的鱼名和乐队八卦，根本没查到这首歌听起来到底什么样",
        0.4,
        10,
    );
    looked.source = "lookup".into();
    let rows = vec![looked, row("cat", "养了一只猫叫年糕", 0.6, 1_000)];
    let filler: Vec<String> = rank(rows.clone(), Some("说说开心的事吧"), 8)
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert!(!filler.contains(&"looked".to_string()));
    let named: Vec<String> = rank(rows, Some("你后来查到那首歌了吗"), 8)
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert!(named.contains(&"looked".to_string()));
}

#[test]
fn what_stands_out_by_meaning_is_named_even_with_no_word_shared() {
    // Asked for cultural events; the memory is about language exchanges.
    let rows = vec![
        row("exchange", "想参加语言交换活动，练口语", 0.5, 1_000),
        row("weather", "明天下雨", 0.5, 10),
        row("events", "上周去看了一个展览活动", 0.5, 100),
    ];
    let query = Some("推荐点这周末的文化活动");
    let by_meaning: std::collections::HashMap<String, f64> = [("exchange".to_string(), 3.1)].into();
    let (chosen, _, _) = rank_marked(rows, query, 2, &Priming::default(), 0.0, &by_meaning);
    let chosen: Vec<(String, bool)> = chosen
        .into_iter()
        .map(|(row, brought)| (row.id, brought))
        .collect();
    assert!(chosen.contains(&("exchange".to_string(), false)));
    // Found both ways comes before found one way; the first counts 1.
    let fused = named_by_words_or_meaning(&[1.0, 0.5, 0.0], &[0.0, 2.5, 3.0]);
    assert_eq!(fused[1], 1.0);
    assert!(fused[0] > 0.0 && fused[2] > 0.0 && fused[0] < 1.0);
    assert_eq!(named_by_words_or_meaning(&[0.0], &[0.0]), vec![0.0]);
}

fn recalled(mut row: agent_memories::Model, times: i32, ago_secs: i64) -> agent_memories::Model {
    row.access_count = times;
    row.last_accessed_at = Some((Utc::now() - chrono::Duration::seconds(ago_secs)).fixed_offset());
    row
}

#[test]
fn with_nothing_named_what_she_often_recalls_comes_before_what_she_never_does() {
    const DAY: i64 = 86_400;
    let rows = vec![
        row("new", "他今天换了新手机", 0.5, 60),
        row("untouched", "他上个月说过想学吉他", 0.5, 30 * DAY),
        recalled(row("often", "他养了一只叫年糕的猫", 0.5, 30 * DAY), 6, DAY),
    ];
    let order: Vec<String> = rank(rows, None, 3).into_iter().map(|row| row.id).collect();
    assert_eq!(order, ["new", "often", "untouched"]);
}

#[test]
fn an_old_memory_named_outright_still_comes_to_mind() {
    const DAY: i64 = 86_400;
    let rows = vec![
        row("guitar", "他说过想学吉他", 0.5, 300 * DAY),
        recalled(row("cat", "他养了一只叫年糕的猫", 0.5, DAY), 9, 60),
    ];
    let named: Vec<String> = rank(rows, Some("吉他"), 1)
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(named, ["guitar"]);
}

#[test]
fn of_two_as_important_the_one_never_recalled_fades_first() {
    const DAY: i64 = 86_400;
    let untouched = row("untouched", "他说过想学吉他", 0.5, 30 * DAY);
    let often = recalled(row("often", "他养了一只叫年糕的猫", 0.5, 60 * DAY), 6, DAY);
    let minor = recalled(row("minor", "他提过一次天气", 0.1, DAY), 9, 60);
    let mut rows = [often, untouched, minor];
    rows.sort_by(fade_order);
    let order: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    // Importance first; then the one she never thinks of.
    assert_eq!(order, ["minor", "untouched", "often"]);
}

fn row(id: &str, content: &str, importance: f64, age_secs: i64) -> agent_memories::Model {
    let at = (Utc::now() - chrono::Duration::seconds(age_secs)).fixed_offset();
    agent_memories::Model {
        id: id.into(),
        user_id: Some(7),
        kind: "fact".into(),
        content: content.into(),
        evidence: None,
        speaker: "user".into(),
        source: "chat".into(),
        venue: "private".into(),
        audience: json!([]),
        concepts: json!([]),
        importance,
        access_count: 0,
        last_accessed_at: None,
        valid_from: at,
        invalid_at: None,
        invalid_reason: None,
        created_at: at,
        updated_at: at,
    }
}

#[test]
fn a_memory_is_said_only_where_everyone_present_was_there() {
    assert!(audience_admits(&[7], &Audience::private(7)));
    assert!(!audience_admits(&[7], &Audience::private(8)));
    let both = Audience {
        members: vec![7, 8],
        venue: Venue::Private,
    };
    assert!(audience_admits(&[7, 8, 9], &both));
    assert!(!audience_admits(&[7], &both));
    assert!(!audience_admits(
        &[7],
        &Audience {
            members: vec![],
            venue: Venue::Private,
        }
    ));
}

#[test]
fn a_group_hears_only_what_was_said_in_that_group() {
    let here = Audience::group("telegram:-100123", 7);
    assert_eq!(here.venue(), "group:telegram:-100123");
    let private = row("p", "养了一只猫", 0.5, 0);
    assert!(
        !admits(&private, &here),
        "a private fact stays out of a group"
    );
    let mut learned_here = row("g", "群里说过周五聚餐", 0.5, 0);
    learned_here.venue = here.venue();
    learned_here.audience = json!([8]);
    assert!(
        admits(&learned_here, &here),
        "whoever said it, the group heard it"
    );
    let mut elsewhere = learned_here.clone();
    elsewhere.venue = "group:telegram:-100999".into();
    assert!(!admits(&elsewhere, &here), "another group is other people");
    assert!(
        !admits(&learned_here, &Audience::private(8)),
        "what a group heard stays in the group"
    );
    assert!(admits(&private, &Audience::private(7)));
    let long = Audience::group("x".repeat(200), 7);
    assert!(long.venue().chars().count() <= MAX_VENUE_CHARS);
}

#[test]
fn rows_without_a_recorded_audience_stay_with_their_person() {
    assert_eq!(audience_of(&row("a", "x", 0.5, 0)), vec![7]);
    let mut shared = row("b", "x", 0.5, 0);
    shared.audience = json!([7, 8]);
    assert_eq!(audience_of(&shared), vec![7, 8]);
}

const DAY: i64 = 86_400;

#[test]
fn matching_rows_rank_first_and_ties_keep_recency() {
    let rows = vec![
        row("new", "likes jasmine tea", 0.5, 0),
        row("mid", "works night shifts", 0.5, 10 * DAY),
        row("old", "prefers saffron tea", 0.5, 20 * DAY),
    ];
    let ranked: Vec<String> = rank(rows.clone(), Some("tea"), 8)
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(ranked, vec!["new", "old"]);
    let recent: Vec<String> = rank(rows, None, 2).into_iter().map(|row| row.id).collect();
    assert_eq!(recent, vec!["new", "mid"]);
    let noisy = vec![
        row("blank", "  ", 0.5, 0),
        row("dup1", "likes tea", 0.5, 1),
        row("dup2", " likes  tea", 0.5, 2),
        row("other", "has a cat", 0.5, 3),
    ];
    let kept: Vec<String> = rank(noisy, None, 2).into_iter().map(|row| row.id).collect();
    assert_eq!(kept, vec!["dup1", "other"]);
}

fn ranked(facts: &[&str], query: Option<&str>, limit: usize) -> Vec<String> {
    // A day apart each, so nothing is linked by having been learned together.
    let rows = facts
        .iter()
        .enumerate()
        .map(|(age, text)| row(&age.to_string(), text, 0.5, age as i64 * DAY))
        .collect();
    rank(rows, query, limit)
        .into_iter()
        .map(|row| row.content)
        .collect()
}

/// Chinese matches by bigram; repeating a word is not extra evidence;
/// punctuation is not evidence; nothing overlapping keeps recency.
#[test]
fn overlap_is_counted_by_distinct_words_and_bigrams() {
    let facts = ["晚上想打独立游戏", "早上喝美式", "讨厌早会"];
    assert_eq!(
        ranked(&facts, Some("今晚打游戏吗"), 2),
        vec!["晚上想打独立游戏"]
    );
    assert_eq!(
        ranked(&facts, Some("完全无关的天气"), 2),
        vec!["晚上想打独立游戏", "早上喝美式"]
    );
    assert_eq!(
        ranked(
            &["tea", "saffron milk"],
            Some("tea tea tea saffron milk"),
            1
        ),
        vec!["saffron milk"]
    );
    assert_eq!(
        ranked(&["喝水", "咖啡"], Some("喝喝喝咖啡"), 1),
        vec!["咖啡"]
    );
    assert_eq!(
        ranked(&["likes coffee", "prefers tea。"], Some("天气。"), 1),
        vec!["likes coffee"]
    );
    assert!(ranked(&facts, Some("tea"), 0).is_empty());
    assert_eq!(
        ranked(&["天气好就去跑步", "今天要加班"], Some("今天几点下班"), 2),
        vec!["今天要加班"],
        "sharing 天 alone is not a match"
    );
}

fn about(mut row: agent_memories::Model, concepts: &[&str]) -> agent_memories::Model {
    row.concepts = json!(
        concepts
            .iter()
            .map(|name| Concept {
                name: name.to_string(),
                aliases: Vec::new(),
            })
            .collect::<Vec<_>>()
    );
    row
}

#[test]
fn what_is_named_brings_its_associations_along() {
    let rows = vec![
        about(row("cat", "养了一只猫叫年糕", 0.5, 0), &["猫", "年糕"]),
        about(row("vet", "年糕上周打了疫苗", 0.5, 30 * DAY), &["年糕"]),
        row("same-chat", "那天刚搬完家", 0.5, 60 + 40 * DAY),
        about(row("tea", "喜欢茉莉花茶", 0.5, 40 * DAY), &["茶"]),
        row("shift", "上夜班", 0.5, 50 * DAY),
    ];
    let ids = |query: &str, limit: usize| -> Vec<String> {
        rank(rows.clone(), Some(query), limit)
            .into_iter()
            .map(|row| row.id)
            .collect()
    };
    assert_eq!(
        ids("猫最近怎么样", 8),
        vec!["cat", "vet"],
        "年糕 links the vaccine to the cat"
    );
    assert_eq!(
        ids("茉莉花茶", 8),
        vec!["tea", "same-chat"],
        "learned a minute apart"
    );
    assert_eq!(ids("猫最近怎么样", 1), vec!["cat"]);
    assert_eq!(
        ids("明日预报", 2),
        vec!["cat", "vet"],
        "nothing named: recency, no association"
    );
}

#[test]
fn what_was_named_and_what_it_brought_to_mind_are_told_apart() {
    let rows = vec![
        about(row("cat", "养了一只猫叫年糕", 0.5, 0), &["猫", "年糕"]),
        about(row("vet", "年糕上周打了疫苗", 0.5, 30 * DAY), &["年糕"]),
    ];
    let (chosen, _, filler) = rank_marked(
        rows.clone(),
        Some("猫怎么样"),
        8,
        &Priming::default(),
        1.0,
        &std::collections::HashMap::new(),
    );
    let marks: Vec<(String, bool)> = chosen
        .into_iter()
        .map(|(row, brought)| (row.id, brought))
        .collect();
    assert_eq!(marks, vec![("cat".into(), false), ("vet".into(), true)]);
    assert!(filler.is_empty(), "both came to mind");
    // A topic only lingering from before is not a new association.
    let (lingering, _, _) = rank_marked(
        rows,
        Some("明日预报"),
        8,
        &Priming::with("cat", 0.8),
        1.0,
        &std::collections::HashMap::new(),
    );
    assert!(lingering.iter().all(|(_, brought)| !brought));
    // Nothing named: what is there is recent context, shown but not recalled.
    let (shown, _, filler) = rank_marked(
        vec![
            row("a", "最近在学吉他", 0.5, 0),
            row("b", "明天下雨", 0.5, DAY),
        ],
        None,
        8,
        &Priming::default(),
        0.0,
        &std::collections::HashMap::new(),
    );
    assert_eq!(shown.len(), 2);
    assert_eq!(filler.len(), 2);
}

#[test]
fn a_topic_carries_over_a_turn_that_does_not_name_it_then_fades() {
    let rows = vec![
        row("news", "最近在学吉他", 0.5, 0),
        about(
            row("cat", "养了一只猫叫年糕", 0.5, 10 * DAY),
            &["猫", "年糕"],
        ),
        about(row("vet", "年糕上周打了疫苗", 0.5, 20 * DAY), &["年糕"]),
        row("tea", "喜欢茉莉花茶", 0.5, 30 * DAY),
    ];
    let ids = |chosen: Vec<agent_memories::Model>| -> Vec<String> {
        chosen.into_iter().map(|row| row.id).collect()
    };
    let (first, primed) = rank_primed(rows.clone(), Some("猫怎么样"), 3, &Priming::default(), 1.0);
    assert_eq!(ids(first), vec!["cat", "vet"]);
    assert!(primed.of("cat") > 0.0);

    // "它又吐了" names nothing, yet the cat is still on the mind; the
    // rest of the budget is ordinary recent context.
    let (second, primed) = rank_primed(rows.clone(), Some("它又吐了"), 3, &primed, 1.0);
    let second = ids(second);
    assert_eq!(second[0], "cat");
    assert!(second.contains(&"news".to_string()));
    assert_eq!(second.len(), 3);

    // Without the talk renewing it, it is gone within a few turns.
    let mut primed = primed;
    for _ in 0..3 {
        primed = rank_primed(rows.clone(), Some("它又吐了"), 3, &primed, 1.0).1;
    }
    assert!(primed.is_empty(), "{primed:?}");
    let (cold, _) = rank_primed(rows, Some("明日预报"), 2, &primed, 1.0);
    assert_eq!(ids(cold), vec!["news", "cat"], "back to recency");
}

#[test]
fn a_primed_memory_no_longer_admitted_cannot_seed() {
    let rows = vec![row("left", "喜欢茉莉花茶", 0.5, 0)];
    let (chosen, next) = rank_primed(rows, Some("它又吐了"), 3, &Priming::with("gone", 1.0), 1.0);
    assert_eq!(chosen.len(), 1, "recency");
    assert!(next.is_empty());
}

#[test]
fn she_is_curious_where_she_knows_a_little_not_nothing_or_plenty() {
    let rows = vec![
        about(row("guitar", "最近在学吉他", 0.5, 0), &["吉他"]),
        about(row("cat1", "养了一只猫叫年糕", 0.5, DAY), &["猫", "年糕"]),
        about(
            row("cat2", "年糕上周打了疫苗", 0.5, 2 * DAY),
            &["猫", "年糕"],
        ),
        about(row("cat3", "年糕怕吸尘器", 0.5, 3 * DAY), &["猫", "年糕"]),
        about(row("cat4", "年糕喜欢晒太阳", 0.5, 4 * DAY), &["猫", "年糕"]),
    ];
    assert_eq!(
        gap_in(&rows, "今天吉他弹了一小时"),
        Some(("吉他".into(), 1))
    );
    assert_eq!(
        gap_in(&rows, "猫今天好乖"),
        None,
        "she knows plenty about the cat"
    );
    assert_eq!(gap_in(&rows, "明日预报"), None, "nothing named");
}

#[test]
fn the_least_important_and_least_used_fade_first() {
    let mut rows = [
        row("keep", "a", 0.9, 100),
        row("fade", "b", 0.2, 50),
        row("next", "c", 0.2, 10),
    ];
    rows.sort_by(fade_order);
    assert_eq!(rows[0].id, "fade");
    assert_eq!(rows[1].id, "next");
    assert_eq!(rows[2].id, "keep");
}

#[test]
fn content_is_collapsed_and_capped() {
    assert_eq!(normalize_content("  likes \n  tea "), "likes tea");
    assert_eq!(
        normalize_content(&"x".repeat(MAX_CONTENT_CHARS + 10))
            .chars()
            .count(),
        MAX_CONTENT_CHARS
    );
}
