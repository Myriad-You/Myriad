//! Glancing at talk that did not call her, and her judgment of whether to
//! speak up or only react.

use super::*;

pub(crate) fn look_loop(cx: &mut Cx, mut errand: Errand) {
    let now = cx.now();
    let Kind::Look { id, .. } = &errand.kind else {
        return;
    };
    let Some(pending) = cx.group.pending.as_ref() else {
        return;
    };
    let latest = pending.message_id == *id;
    let overdue = cx
        .group
        .unjudged_since
        .is_some_and(|since| now - since >= seconds(MAX_WAIT_SECONDS));
    if !latest && !overdue {
        return;
    }
    if cx.group.judging || cx.group.busy {
        let key = cx.wake(SETTLE_SECONDS);
        errand.doing = Doing::Settling;
        park_errand(cx, errand, key);
        return;
    }
    cx.group.judging = true;
    cx.group.unjudged_since = None;
    let Some(message) = cx.group.pending.take() else {
        return;
    };
    if let Kind::Look { message: held, .. } = &mut errand.kind {
        *held = Some(message.clone());
    }
    let mut judging = Box::new(Judging {
        message,
        stage: JudgeStage::Identifying,
        owner: None,
        conversation: Vec::new(),
        here: Here::default(),
        had: Vec::new(),
        would_tell: Vec::new(),
        own_time: None,
    });
    match judge_start(cx, &mut judging) {
        Ok(awaiting) => {
            errand.doing = Doing::Judging(judging);
            park_errand(cx, errand, awaiting);
        }
        Err(decided) => judged(cx, errand, decided),
    }
}

pub(crate) fn judge_start(cx: &mut Cx, judging: &mut Judging) -> Result<String, Option<Decision>> {
    let now = cx.now();
    if stopped_answering(cx.group, &judging.message, now) || muted_now(cx.group, now) {
        return Err(None);
    }
    judging.stage = JudgeStage::Identifying;
    let from = judging.message.from.clone();
    Ok(cx.effect(|id| Effect::Identify { id, from }))
}

/// The pictures in the recent talk she has not seen yet.
pub(crate) fn unseen(group: &Group) -> Vec<Picture> {
    let lines = group.lines.len();
    group
        .lines
        .iter()
        .skip(lines.saturating_sub(CONVERSATION_LINES))
        .filter(|line| !line.hers)
        .flat_map(|line| {
            line.images
                .iter()
                .enumerate()
                .filter(|(index, _)| line.seen.get(*index).is_none_or(Option::is_none))
                .map(|(index, image)| Picture {
                    message_id: line.message_id.clone(),
                    index,
                    image: image.clone(),
                })
                .collect::<Vec<_>>()
        })
        .take(PICTURES_AT_ONCE)
        .collect()
}

/// Whether the picture `key` is the group's: sent there often enough, by
/// more than one of them.
pub(crate) fn the_groups_picture(group: &Group, key: &str) -> bool {
    group
        .pictures
        .get(key)
        .is_some_and(|sent| *sent >= PICTURE_SENT_FOR_THE_GROUP)
        && group
            .picture_senders
            .get(key)
            .is_some_and(|senders| senders.len() >= 2)
}

/// What she saw, put on the lines; a picture several of them keep sending
/// there is the group's.
pub(crate) fn apply_seen(cx: &mut Cx, looked: Vec<Looked>) {
    for looked in looked {
        let key = cx
            .group
            .lines
            .iter()
            .find(|line| !line.hers && line.message_id == looked.message_id)
            .and_then(|line| line.images.get(looked.index))
            .map(|image| image.key.clone());
        if let Some(key) = key
            && the_groups_picture(cx.group, &key)
        {
            cx.out.effects.push(Effect::PictureAgain {
                key,
                seen: looked.seen.clone(),
            });
        }
        if let Some(line) = cx
            .group
            .lines
            .iter_mut()
            .find(|line| !line.hers && line.message_id == looked.message_id)
        {
            if line.seen.len() <= looked.index {
                line.seen.resize(looked.index + 1, None);
            }
            line.seen[looked.index] = Some(looked.seen);
        }
    }
}

pub(crate) fn judge_step(
    cx: &mut Cx,
    judging: &mut Judging,
    answer: Option<Answer>,
) -> Result<String, Option<Decision>> {
    match (judging.stage, answer) {
        (JudgeStage::Identifying, Some(Answer::Identified { who, owner })) => {
            let owner = match who {
                Who::Member { user_id, bound } => {
                    if !bound {
                        return Err(None);
                    }
                    user_id
                }
                Who::Stranger | Who::Unknown => owner.ok_or(None)?,
            };
            judging.owner = Some(owner);
            let pictures = unseen(cx.group);
            if pictures.is_empty() {
                return Ok(judge_ask(cx, judging));
            }
            judging.stage = JudgeStage::Looking;
            Ok(cx.effect(|id| Effect::Look { id, pictures }))
        }
        (JudgeStage::Looking, Some(Answer::Looked { seen })) => {
            apply_seen(cx, seen);
            Ok(judge_ask(cx, judging))
        }
        (JudgeStage::Views, Some(Answer::Rows { rows })) => {
            let held: Vec<crate::views::Held> = rows
                .iter()
                .map(|row| crate::views::Held {
                    content: &row.content,
                    evidence: row.evidence.as_deref(),
                    concepts: &row.concepts,
                })
                .collect();
            let talk = judging.conversation.join("\n");
            for (about, view) in crate::views::touched(&held, &talk, PER_KIND) {
                judging
                    .had
                    .push(("your_view".into(), format!("{about}: {view}")));
            }
            judging.stage = JudgeStage::Life;
            Ok(cx.effect(|id| Effect::Read {
                id,
                of: Of::Hers,
                what: ToRead::Life {
                    talk,
                    related: PER_KIND,
                },
            }))
        }
        (
            JudgeStage::Life,
            Some(Answer::Life {
                did,
                would_tell,
                now,
            }),
        ) => {
            for (what, stayed) in did {
                judging
                    .had
                    .push(("you_did".into(), format!("{what}: {stayed}")));
            }
            judging.would_tell = would_tell;
            judging.own_time = now;
            Ok(own_rows(
                cx,
                judging,
                JudgeStage::Heard,
                HEARD_SOURCE,
                HEARD_HELD,
            ))
        }
        (JudgeStage::Heard, Some(Answer::Rows { rows })) => {
            let talk = judging.conversation.join("\n");
            if !talk.trim().is_empty() {
                let documents: Vec<myriad_agent_rules::memory::lexical::Document> = rows
                    .iter()
                    .map(|row| myriad_agent_rules::memory::lexical::Document {
                        text: &row.content,
                        concepts: &row.concepts,
                    })
                    .collect();
                for index in crate::views::strongly_touched(&talk, &documents, PER_KIND) {
                    judging
                        .had
                        .push(("you_heard".into(), rows[index].content.clone()));
                }
            }
            Ok(own_rows(
                cx,
                judging,
                JudgeStage::Memes,
                crate::memes::SOURCE,
                MEMES_HELD,
            ))
        }
        (JudgeStage::Memes, Some(Answer::Rows { rows })) => {
            // Words and memes in the talk that she looked up before: she
            // knows them.
            let known: Vec<crate::memes::Known> = rows
                .iter()
                .filter_map(|row| crate::memes::read_kept(&row.content))
                .collect();
            let talk = judging.conversation.join("\n");
            for known in crate::memes::known_in(&talk, &known) {
                judging.had.push((
                    "meme".into(),
                    crate::memes::kept_line(&known.term, &known.meaning),
                ));
            }
            for told in std::mem::take(&mut judging.would_tell) {
                if !judging.had.iter().any(|(_, text)| told.contains(text)) {
                    judging.had.push(("you_would_tell".into(), told));
                }
            }
            judging.stage = JudgeStage::Soul;
            Ok(cx.effect(|id| Effect::Read {
                id,
                of: Of::Hers,
                what: ToRead::Soul,
            }))
        }
        (JudgeStage::Soul, Some(Answer::Soul { text })) => {
            let material = material(&judging.had);
            let here = crate::joining::Standing {
                last_spoke: judging.here.last_spoke.as_deref(),
                late: judging.here.late.as_deref(),
                how_it_went: judging.here.how_it_went.as_deref(),
                share: judging.here.share.as_deref(),
                shown_as: cx.group.shown_as.as_deref(),
            };
            let input = crate::joining::input(
                &judging.conversation,
                &here,
                judging.own_time.as_deref(),
                &material,
            );
            judging.stage = JudgeStage::Judging;
            let owner = judging.owner.unwrap_or_default();
            Ok(cx.effect(|id| Effect::Ask {
                id,
                how: AskKind::Json,
                voice: "hers".into(),
                owner,
                operation: "group_chime".into(),
                timeout_ms: JUDGE_WITHIN_MS,
                system: crate::joining::system(&text),
                input,
                schema_name: crate::joining::SCHEMA_NAME.into(),
                schema: crate::joining::schema(),
                turn: None,
            }))
        }
        (JudgeStage::Judging, Some(Answer::Asked { raw, .. })) => {
            let raw = raw.ok_or(None)?;
            // Words or memes in the talk she is not sure of: hers to look
            // up, whether or not she speaks.
            let terms = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim())
                .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
                .map(|value| crate::memes::unsure_terms(&value))
                .unwrap_or_default();
            learn(
                cx,
                judging.owner.unwrap_or_default(),
                judging.message.venue(),
                terms,
                &judging.conversation,
            );
            let material = material(&judging.had);
            let decision = crate::joining::parse(&raw, material.len())
                .flatten()
                .ok_or(None)?;
            // A reaction is only the emoji: nothing is said.
            if decision.why == Why::React {
                return Err(Some(Decision {
                    why: Why::React,
                    reason: decision.about,
                }));
            }
            let reason = crate::joining::reason(&decision, &material);
            Err(Some(Decision {
                why: decision.why,
                reason: match judging.here.late.as_deref() {
                    Some(ago) => format!("{reason}\n{}", crate::joining::seeing_it_late(ago)),
                    None => reason,
                },
            }))
        }
        _ => Err(None),
    }
}

/// Things of hers per kind that the talk may touch.
const PER_KIND: usize = 3;
/// Her views, and what she heard, matched against.
const VIEWS_HELD: u64 = 300;
const HEARD_SOURCE: &str = "heard";
const HEARD_HELD: u64 = 300;
/// What she knows of words and memes, read back: the latest this many.
const MEMES_HELD: u64 = 500;
/// Her own judgment, thinking a little: answered without thinking, she took
/// a line for someone it was not meant for.
const JUDGE_WITHIN_MS: u64 = 30_000;

fn own_rows(
    cx: &mut Cx,
    judging: &mut Judging,
    stage: JudgeStage,
    source: &str,
    limit: u64,
) -> String {
    judging.stage = stage;
    let source = source.to_string();
    cx.effect(|id| Effect::Read {
        id,
        of: Of::Hers,
        what: ToRead::OwnRows { source, limit },
    })
}

/// What she has, as the judgment's rules take it.
fn material(had: &[(String, String)]) -> Vec<crate::joining::Material> {
    had.iter()
        .map(|(kind, text)| crate::joining::Material {
            kind: match kind.as_str() {
                "your_view" => "your_view",
                "you_did" => "you_did",
                "you_heard" => "you_heard",
                "meme" => "meme",
                _ => "you_would_tell",
            },
            text: text.clone(),
        })
        .collect()
}

pub(crate) fn judge_ask(cx: &mut Cx, judging: &mut Judging) -> String {
    let now = cx.now();
    let lines = transcript(cx.group, None, now);
    let conversation = conversation(&lines, CONVERSATION_LINES);
    let late = cx
        .group
        .lines
        .iter()
        .rev()
        .find(|line| !line.hers)
        .map(|line| age(now, line.at_micros()))
        .filter(|age| *age >= chrono::Duration::minutes(LATE_MINUTES))
        .map(crate::doing::ago_text);
    let last_spoke = cx
        .group
        .lines
        .iter()
        .rev()
        .find(|line| line.hers)
        .map(|line| crate::doing::ago_text(age(now, line.at_micros())));
    let how_it_went = crate::joining::how_it_went(
        cx.group.spoke_up.len(),
        cx.group.spoke_up.iter().filter(|one| one.taken).count(),
    );
    let share = {
        let recent: Vec<&Line> = cx
            .group
            .lines
            .iter()
            .skip(cx.group.lines.len().saturating_sub(CONVERSATION_LINES))
            .collect();
        let hers = recent.iter().filter(|line| line.hers).count();
        let others = recent
            .iter()
            .filter(|line| !line.hers)
            .map(|line| line.from.as_deref().unwrap_or(&line.name))
            .collect::<std::collections::HashSet<_>>()
            .len();
        crate::joining::share_of_talk(recent.len(), hers, others)
    };
    judging.conversation = conversation;
    judging.here = Here {
        last_spoke,
        late,
        how_it_went,
        share,
    };
    judging.had.clear();
    own_rows(cx, judging, JudgeStage::Views, VIEWS_OWN, VIEWS_HELD)
}

const VIEWS_OWN: &str = "view";

pub(crate) fn judged(cx: &mut Cx, mut errand: Errand, decided: Option<Decision>) {
    let now = cx.now();
    cx.group.judging = false;
    // Going on with what she said, without calling her: taken up.
    if decided
        .as_ref()
        .is_some_and(|decided| decided.why == Why::Answer)
    {
        taken_up(cx.group, now);
    }
    let Kind::Look {
        message,
        why,
        spoke,
        ..
    } = &mut errand.kind
    else {
        return;
    };
    let Some(target) = message.clone() else {
        chain_end(cx, errand);
        return;
    };
    match decided {
        // No words, only a reaction on the line.
        Some(Decision {
            why: Why::React,
            reason,
        }) => {
            let to = target.target();
            let emoji = reason.clone();
            let awaiting = cx.effect(|id| Effect::React { id, to, emoji });
            errand.doing = Doing::Reacting { emoji: reason };
            park_errand(cx, errand, awaiting);
        }
        Some(decided) => {
            if begin(cx.group, now).is_ok() {
                *why = Some(decided.why);
                *spoke = true;
                start_answer(cx, errand, target, Some(decided.reason), false);
            } else {
                chain_end(cx, errand);
            }
        }
        None => chain_end(cx, errand),
    }
}
