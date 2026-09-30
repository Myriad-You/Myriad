//! Her memory at its full size: half a year of her own talks beside others'.

use super::*;

/// What she keeps of one person tops out at a thousand memories (older,
/// weaker ones fade past it): whether recall still finds what answers a
/// question when the rest of those thousand are about other things. The
/// memories come from a finished bench report (`memories` of each row), so
/// nothing is extracted again: each question is answered twice from the
/// same memories, once alone and once among others' up to the cap, with
/// the same cues. `MEROPE_MEMORY_SCALE=<report> MEROPE_MEMORY_BENCH=<its
/// questions> MEROPE_MEMORY_BENCH_REPORT=<new file>`, optional
/// `MEROPE_MEMORY_BENCH_PER_TYPE`.
///
/// The thousand build up over half a year, the question's own scattered
/// among the rest, and the question alone has its own at the same dates:
/// the others are the only difference. Those dates are the timeline's, not
/// the conversations', so questions about when are left out of answering.
/// With `MEROPE_MEMORY_SCALE_LABELS=<file>` (id → `needed`, the indices of
/// its memories answering rests on) only recall is measured, from their
/// words alone and with cues, and no answer is asked for. Reports from
/// before concepts were kept beside memories have none, and recall also
/// searches them: what is found from those is lower than production for
/// both, alike.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_memory_at_its_full_size() {
    use crate::services::agent::memory::unified::MemoryKind;
    // `MEROPE_MEMORY_SCALE_FULL`: how many she keeps of the person, when
    // not the cap.
    let full: usize = std::env::var("MEROPE_MEMORY_SCALE_FULL")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000);
    let source: Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_SCALE").expect("scale source"))
            .unwrap(),
    )
    .unwrap();
    let questions: Vec<Question> = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_BENCH").expect("questions")).unwrap(),
    )
    .unwrap();
    let report_path = std::env::var("MEROPE_MEMORY_BENCH_REPORT").expect("report path");
    assert!(
        !std::path::Path::new(&report_path).exists(),
        "report must not exist"
    );
    let per_type: usize = std::env::var("MEROPE_MEMORY_BENCH_PER_TYPE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(7);
    let rows = source["rows"].as_array().expect("rows");
    let mut taken: BTreeMap<String, usize> = BTreeMap::new();
    let chosen: Vec<(Question, Vec<String>)> = rows
        .iter()
        .filter(|row| row["type"] != "single-session-assistant")
        .filter_map(|row| {
            let id = row["id"].as_str()?;
            let question = questions.iter().find(|q| q.question_id == id)?.clone();
            let count = taken.entry(question.question_type.clone()).or_default();
            *count += 1;
            (*count <= per_type).then(|| {
                let memories = row["memories"]
                    .as_array()
                    .map(|all| {
                        all.iter()
                            .filter_map(|m| m.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                (question, memories)
            })
        })
        .collect();
    let everyone: Vec<String> = rows
        .iter()
        .flat_map(|row| row["memories"].as_array().cloned().unwrap_or_default())
        .filter_map(|m| m.as_str().map(str::to_string))
        .collect();
    // Concepts kept beside each memory, where the report has them.
    let concepts_of: std::collections::HashMap<String, Value> = rows
        .iter()
        .flat_map(|row| {
            let memories = row["memories"].as_array().cloned().unwrap_or_default();
            let concepts = row["concepts"].as_array().cloned().unwrap_or_default();
            memories.into_iter().zip(concepts)
        })
        .filter_map(|(memory, concepts)| Some((memory.as_str()?.to_string(), concepts)))
        .collect();
    let labels: Option<BTreeMap<String, Vec<usize>>> = std::env::var("MEROPE_MEMORY_SCALE_LABELS")
        .ok()
        .map(|path| {
            let raw: BTreeMap<String, Value> =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            raw.into_iter()
                .map(|(id, label)| {
                    let needed = label["needed"]
                        .as_array()
                        .map(|all| {
                            all.iter()
                                .filter_map(|index| index.as_u64().map(|index| index as usize))
                                .collect()
                        })
                        .unwrap_or_default();
                    (id, needed)
                })
                .collect()
        });
    let config_db = super::super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
    embedding_as_asked().await;
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("judgment model");
    let lite = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("Lite model")
    .with_light_thinking();
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "memory_scale").await;
    let db = isolated.db.clone();
    // Kept as production keeps a memory, in one insert: remembering them one
    // by one checks the whole store each time.
    let keep = |user_id: i32, dated: Vec<(String, chrono::DateTime<chrono::FixedOffset>)>| {
        let db = db.clone();
        let concepts_of = &concepts_of;
        async move {
            use sea_orm::EntityTrait;
            let audience = Audience::private(user_id);
            let rows: Vec<crate::models::entities::agent_memories::ActiveModel> = dated
                .into_iter()
                .map(|(content, at)| {
                    let concepts = concepts_of.get(&content).cloned().unwrap_or(json!([]));
                    (content, concepts, at)
                })
                .map(|(content, concepts, at)| {
                    crate::models::entities::agent_memories::ActiveModel {
                        id: Set(format!("mem_{}", uuid::Uuid::new_v4().simple())),
                        user_id: Set(Some(user_id)),
                        kind: Set(MemoryKind::Fact.as_str().into()),
                        content: Set(content),
                        evidence: Set(None),
                        speaker: Set("user".into()),
                        source: Set("chat".into()),
                        venue: Set(audience.venue()),
                        audience: Set(json!(audience.members())),
                        concepts: Set(concepts),
                        importance: Set(0.5),
                        access_count: Set(0),
                        last_accessed_at: Set(None),
                        valid_from: Set(at),
                        invalid_at: Set(None),
                        invalid_reason: Set(None),
                        created_at: Set(at),
                        updated_at: Set(at),
                    }
                })
                .collect();
            for chunk in rows.chunks(200) {
                crate::models::entities::agent_memories::Entity::insert_many(chunk.to_vec())
                    .exec(&db)
                    .await
                    .unwrap();
            }
        }
    };
    let chosen: Vec<(Question, Vec<String>)> = chosen
        .into_iter()
        .filter(|(question, _)| labels.is_some() || question.question_type != "temporal-reasoning")
        .collect();
    // `MEROPE_MEMORY_SCALE_CUES=<file>`: the cues thought of for each
    // question, read when there and written after.
    let cue_path = std::env::var("MEROPE_MEMORY_SCALE_CUES").ok();
    let cue_cache: std::sync::Mutex<BTreeMap<String, String>> = std::sync::Mutex::new(
        cue_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
    );
    let outcomes: Vec<Value> = futures::stream::iter(chosen)
        .map(|(question, own)| {
            let (db, judge, lite, everyone, labels, cue_cache) =
                (db.clone(), &judge, &lite, &everyone, &labels, &cue_cache);
            let keep = &keep;
            async move {
                let alone = new_user(&db, &format!("alone-{}", question.question_id)).await;
                let among = new_user(&db, &format!("among-{}", question.question_id)).await;
                let others: Vec<String> = everyone
                    .iter()
                    .filter(|m| !own.contains(m))
                    .take(full.saturating_sub(own.len()))
                    .cloned()
                    .collect();
                let (own_dated, others_dated) =
                    over_half_a_year(&question.question_id, &own, &others);
                keep(alone, own_dated.clone()).await;
                keep(among, others_dated).await;
                keep(among, own_dated).await;
                embed_all(&db, alone).await;
                embed_all(&db, among).await;
                // Thought of once and kept, so runs compare recall and not
                // what the cues happened to be.
                let thought = cue_cache
                    .lock()
                    .ok()
                    .and_then(|cache| cache.get(&question.question_id).cloned());
                let raw = match thought {
                    Some(raw) => Some(raw),
                    None => {
                        let raw = judge
                            .analyze_json(
                                &myriad_merope::remembering::system(),
                                &myriad_merope::remembering::input(
                                    &question.question,
                                    None,
                                    &question.question_date,
                                ),
                                myriad_merope::remembering::SCHEMA_NAME,
                                Some(&myriad_merope::remembering::schema()),
                            )
                            .await
                            .ok();
                        if let (Some(raw), Ok(mut cache)) = (&raw, cue_cache.lock()) {
                            cache.insert(question.question_id.clone(), raw.clone());
                        }
                        raw
                    }
                };
                let cues = raw.and_then(|raw| myriad_merope::remembering::parse(&raw));
                if let Some(labels) = labels {
                    let needed: Vec<&String> = labels
                        .get(&question.question_id)
                        .map(|indices| indices.iter().filter_map(|i| own.get(*i)).collect())
                        .unwrap_or_default();
                    let found = |recalled: &[String]| {
                        needed
                            .iter()
                            .filter(|memory| {
                                recalled.iter().any(|line| line.contains(memory.as_str()))
                            })
                            .count()
                    };
                    let mut row = json!({"id":question.question_id,"type":question.question_type,
                        "needed":needed.len(),"cued":cues.is_some()});
                    for (name, user_id) in [("alone", alone), ("among", among)] {
                        for (way, with) in [("plain", None), ("cued", cues.as_ref())] {
                            let recalled = recalled_for(
                                &db,
                                &question,
                                user_id,
                                &Audience::private(user_id),
                                with,
                                THOROUGH,
                            )
                            .await;
                            row[format!("{name}_{way}")] = json!(found(&recalled));
                        }
                    }
                    // Where each needed memory stands in the whole ranking,
                    // from their words and at best over the cues: just past
                    // the budget is a matter of room, far down of matching.
                    let place = |query: String| {
                        let db = db.clone();
                        async move {
                            super::super::store::recall_remembered_split(
                                &db,
                                among,
                                &Audience::private(among),
                                Some(&query),
                                full,
                                &Priming::default(),
                                0.0,
                            )
                            .await
                            .map(|(recalled, _)| recalled.named)
                            .unwrap_or_default()
                        }
                    };
                    let mut rankings = vec![place(question.question.clone()).await];
                    for cue in cues.iter().flat_map(|cues| cues.cues.iter()) {
                        rankings.push(place(cue.clone()).await);
                    }
                    let at = |ranking: &[String], memory: &str| {
                        ranking.iter().position(|line| line.contains(memory))
                    };
                    row["places"] = json!(
                        needed
                            .iter()
                            .map(|memory| json!({
                                "plain": at(&rankings[0], memory),
                                "best": rankings.iter().filter_map(|ranking| at(ranking, memory)).min(),
                            }))
                            .collect::<Vec<_>>()
                    );
                    println!("{row}");
                    return row;
                }
                let small = answer_one(
                    &db,
                    judge,
                    lite,
                    &question,
                    alone,
                    &Audience::private(alone),
                    cues.as_ref(),
                    THOROUGH,
                    false,
                    &[],
                )
                .await;
                let large = answer_one(
                    &db,
                    judge,
                    lite,
                    &question,
                    among,
                    &Audience::private(among),
                    cues.as_ref(),
                    THOROUGH,
                    false,
                    &[],
                )
                .await;
                let kept = small
                    .recalled
                    .iter()
                    .filter(|line| large.recalled.contains(line))
                    .count();
                println!(
                    "{} {} alone={:?} among={:?} kept {kept}/{}",
                    question.question_type,
                    question.question_id,
                    small.correct,
                    large.correct,
                    small.recalled.len()
                );
                json!({"id":question.question_id,"type":question.question_type,
                    "alone":small.correct,"among":large.correct,
                    "recalledAlone":small.recalled.len(),"stillRecalled":kept,
                    "responseAmong":large.answer})
            }
        })
        .buffer_unordered(at_once())
        .collect()
        .await;
    isolated.drop().await;
    println!(
        "embedding requests that failed or timed out: {}",
        crate::services::agent::memory::meaning::failed()
    );
    if let (Some(path), Ok(cache)) = (&cue_path, cue_cache.lock()) {
        std::fs::write(path, serde_json::to_string_pretty(&*cache).unwrap()).unwrap();
    }
    if labels.is_some() {
        // Per type: memories needed, then found alone and among, from their
        // words and with cues; and questions with all of them found.
        let mut summary: BTreeMap<String, [usize; 9]> = BTreeMap::new();
        for row in &outcomes {
            let slot = summary
                .entry(row["type"].as_str().unwrap_or_default().to_string())
                .or_default();
            let needed = row["needed"].as_u64().unwrap_or(0) as usize;
            slot[0] += needed;
            for (at, key) in ["alone_plain", "alone_cued", "among_plain", "among_cued"]
                .iter()
                .enumerate()
            {
                let found = row[*key].as_u64().unwrap_or(0) as usize;
                slot[1 + at] += found;
                slot[5 + at] += usize::from(needed > 0 && found == needed);
            }
        }
        for (kind, slot) in &summary {
            println!(
                "{kind:<28} needed {:>3}  found alone {}/{}  among {}/{}  complete alone {}/{}  among {}/{}",
                slot[0], slot[1], slot[2], slot[3], slot[4], slot[5], slot[6], slot[7], slot[8]
            );
        }
        std::fs::write(
            &report_path,
            serde_json::to_string_pretty(&json!({"summary":summary,"rows":outcomes})).unwrap(),
        )
        .unwrap();
        return;
    }
    let count = |key: &str| outcomes.iter().filter(|row| row[key] == true).count();
    let (alone, among) = (count("alone"), count("among"));
    let recalled: usize = outcomes
        .iter()
        .filter_map(|row| row["recalledAlone"].as_u64())
        .sum::<u64>() as usize;
    let still: usize = outcomes
        .iter()
        .filter_map(|row| row["stillRecalled"].as_u64())
        .sum::<u64>() as usize;
    println!(
        "alone {alone} among {among} of {}; recalled lines kept {still}/{recalled}",
        outcomes.len()
    );
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&json!({"alone":alone,"among":among,"total":outcomes.len(),
            "recalledAlone":recalled,"stillRecalled":still,"rows":outcomes}))
        .unwrap(),
    )
    .unwrap();
}

/// Every memory of `user_id` given what it means, as recall would over time.
pub(super) async fn embed_all(db: &sea_orm::DatabaseConnection, user_id: i32) {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let rows = crate::models::entities::agent_memories::Entity::find()
        .filter(crate::models::entities::agent_memories::Column::UserId.eq(user_id))
        .all(db)
        .await
        .unwrap_or_default();
    crate::services::agent::memory::meaning::fill(db, &rows).await;
}

/// One person's memories as they build up over half a year: `own`
/// scattered among `others` in an order `seed` fixes, each dated at its
/// place in time.
#[allow(clippy::type_complexity)]
pub(super) fn over_half_a_year(
    seed: &str,
    own: &[String],
    others: &[String],
) -> (
    Vec<(String, chrono::DateTime<chrono::FixedOffset>)>,
    Vec<(String, chrono::DateTime<chrono::FixedOffset>)>,
) {
    use std::hash::{Hash, Hasher};
    let place = |text: &str| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (seed, text).hash(&mut hasher);
        hasher.finish()
    };
    let mut all: Vec<(bool, &String)> = own
        .iter()
        .map(|memory| (true, memory))
        .chain(others.iter().map(|memory| (false, memory)))
        .collect();
    all.sort_by_key(|(_, memory)| place(memory));
    let now = chrono::Utc::now().fixed_offset();
    let count = all.len().max(1) as i32;
    let step = chrono::Duration::days(180) / count;
    let (mut hers, mut theirs) = (Vec::new(), Vec::new());
    for (position, (own, memory)) in all.into_iter().enumerate() {
        let at = now - step * (count - position as i32);
        if own {
            hers.push((memory.clone(), at));
        } else {
            theirs.push((memory.clone(), at));
        }
    }
    (hers, theirs)
}
