//! The pieces of Work she hands off in one private chat.
//!
//! Each piece is a lane of its own, kept under `{chat}#{n}`: its own Work
//! session, run, pending prompt and outbox, so delivery, retries and
//! recovery work per piece exactly as they did for the chat's one Work. The
//! chat's own record keeps her chat with them. Several can be under way at
//! once; how many is hers to weigh, from how long each has taken.
//!
//! What decides where something goes is plain here (`route`, `act`,
//! `label`, `line`), so the scheduling evaluation holds the same rules.

use super::*;
use crate::services::agent::delegate::HandOff;

const LANE: char = '#';
/// What of a piece's request is shown in its label.
const LABEL_CHARS: usize = 12;
/// What of a result she reads back.
const RESULT_CHARS: usize = 200;
/// Done pieces she still sees, and for how long.
const DONE_SHOWN: usize = 3;
const DONE_FOR_SECS: i64 = 24 * 60 * 60;

pub(crate) fn lane_key(chat: &str, number: u32) -> String {
    format!("{chat}{LANE}{number}")
}

/// The chat and number of a piece's key; none for a chat's own key.
pub(crate) fn split_lane(key: &str) -> Option<(&str, u32)> {
    let (chat, number) = key.rsplit_once(LANE)?;
    Some((chat, number.parse().ok()?))
}

/// The chat a key belongs to, a piece's or its own.
pub(crate) fn chat_of(key: &str) -> &str {
    split_lane(key).map_or(key, |(chat, _)| chat)
}

/// The next piece's number: one past the highest, from 1.
pub(crate) fn next_number(numbers: impl IntoIterator<Item = u32>) -> u32 {
    numbers.into_iter().max().unwrap_or(0) + 1
}

/// How a piece stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Standing {
    Doing { progress: Option<u8> },
    Waiting { question: String },
    Done { result: Option<String> },
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Piece {
    pub number: u32,
    pub asked: String,
    pub started_at: Option<i64>,
    pub standing: Standing,
}

impl Piece {
    pub(crate) fn unfinished(&self) -> bool {
        matches!(
            self.standing,
            Standing::Doing { .. } | Standing::Waiting { .. }
        )
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn took(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    match minutes {
        0 => "不到 1 分钟".into(),
        1..60 => format!("{minutes} 分钟"),
        _ => format!("{} 小时 {} 分钟", minutes / 60, minutes % 60),
    }
}

/// A piece as she and `/status` see it, on one line.
pub(crate) fn line(piece: &Piece, now: i64) -> String {
    let head = format!("#{}「{}」", piece.number, one_line(&piece.asked));
    match &piece.standing {
        Standing::Doing { progress } => {
            let took = piece
                .started_at
                .map(|at| format!("，已办 {}", took(now - at)))
                .unwrap_or_default();
            let progress = progress
                .filter(|progress| *progress > 0)
                .map(|progress| format!("（{progress}%）"))
                .unwrap_or_default();
            format!("{head}在办{took}{progress}")
        }
        Standing::Waiting { question } => {
            format!("{head}在等对方回答：{}", one_line(question))
        }
        Standing::Done { result: Some(result) } => {
            let result: String = one_line(result).chars().take(RESULT_CHARS).collect();
            format!("{head}办完了：{result}")
        }
        Standing::Done { result: None } => format!("{head}办完了"),
        Standing::Stopped => format!("{head}被叫停了"),
    }
}

/// Which pieces she sees: every unfinished one, and the latest few done
/// lately, so she knows how they came back. In the order handed off.
pub(crate) fn shown(mut pieces: Vec<Piece>, now: i64) -> Vec<Piece> {
    pieces.sort_by_key(|piece| piece.number);
    let mut done: Vec<u32> = pieces
        .iter()
        .filter(|piece| !piece.unfinished())
        .filter(|piece| {
            piece
                .started_at
                .is_some_and(|at| now - at < DONE_FOR_SECS)
        })
        .map(|piece| piece.number)
        .collect();
    done = done.split_off(done.len().saturating_sub(DONE_SHOWN));
    pieces
        .into_iter()
        .filter(|piece| piece.unfinished() || done.contains(&piece.number))
        .collect()
}

/// Where what they write goes, when it is not a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// The one piece waiting for an answer takes it as its answer.
    Answer(u32),
    /// She reads it: chat, a new piece, or which piece it is for.
    Her,
}

/// Only one waiting: it is that one's answer. More: she places it.
pub(crate) fn route(pieces: &[Piece]) -> Route {
    let mut waiting = pieces
        .iter()
        .filter(|piece| matches!(piece.standing, Standing::Waiting { .. }));
    match (waiting.next(), waiting.next()) {
        (Some(piece), None) => Route::Answer(piece.number),
        _ => Route::Her,
    }
}

/// What one of her lines does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Act {
    Start(String),
    /// The answer to piece N's question.
    Answer(u32, String),
    /// More for piece N to go on, into its run as it is.
    Steer(u32, String),
    /// Piece N is done: it picks up again in its own session, where it ended.
    Continue(u32, String),
    Stop(u32),
    /// For a piece that is over or never was: nothing.
    Nothing,
}

pub(crate) fn act(hand_off: &HandOff, pieces: &[Piece]) -> Act {
    let standing = |work: u32| {
        pieces
            .iter()
            .find(|piece| piece.number == work)
            .map(|piece| &piece.standing)
    };
    match hand_off {
        HandOff::New { text } => Act::Start(text.clone()),
        HandOff::To { work, text } => match standing(*work) {
            Some(Standing::Waiting { .. }) => Act::Answer(*work, text.clone()),
            Some(Standing::Doing { .. }) => Act::Steer(*work, text.clone()),
            Some(Standing::Done { .. }) => Act::Continue(*work, text.clone()),
            _ => Act::Nothing,
        },
        HandOff::Stop { work } => match standing(*work) {
            Some(Standing::Waiting { .. } | Standing::Doing { .. }) => Act::Stop(*work),
            _ => Act::Nothing,
        },
    }
}

/// `【#N 做什么】`, on what a piece sends while another is not done.
pub(crate) fn label(number: u32, asked: &str) -> String {
    let asked = one_line(asked);
    let mut short: String = asked.chars().take(LABEL_CHARS).collect();
    if asked.chars().count() > LABEL_CHARS {
        short.push('…');
    }
    format!("【#{number} {short}】")
}

pub(crate) fn labelled(number: u32, asked: &str, others_unfinished: bool, content: &str) -> String {
    if others_unfinished {
        format!("{}\n{content}", label(number, asked))
    } else {
        content.to_string()
    }
}

/// Her answer to a piece's choice question, as the choice: she may write
/// `2（19:00）` or `订 19:00 的`; a leading choice number, or the one choice
/// named in it, is what she means. Anything else stays as she wrote it.
pub(crate) fn her_choice(prompt: &PendingPrompt, text: &str) -> String {
    let text = text.trim();
    if prompt.options.is_empty() {
        return text.to_string();
    }
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    if let Ok(number) = digits.parse::<usize>()
        && (1..=prompt.options.len()).contains(&number)
        && !text[digits.len()..].starts_with([':', '：', '.', '点'])
    {
        return number.to_string();
    }
    let named: Vec<usize> = prompt
        .options
        .iter()
        .enumerate()
        .filter(|(_, option)| {
            text.contains(option.value.as_str())
                || (!option.label.trim().is_empty() && text.contains(option.label.as_str()))
        })
        .map(|(index, _)| index + 1)
        .collect();
    match named.as_slice() {
        [number] => number.to_string(),
        _ => text.to_string(),
    }
}

/// A piece's question carries its number in its id, and so in its buttons.
pub(crate) fn stamp(number: u32, mut prompt: PendingPrompt) -> PendingPrompt {
    ensure_pending_id(&mut prompt);
    if number_of_prompt(&prompt.id).is_none() {
        prompt.id = format!("{number}-{}", prompt.id);
    }
    prompt
}

pub(crate) fn number_of_prompt(id: &str) -> Option<u32> {
    id.split_once('-')?.0.parse().ok()
}

/// One piece as kept.
pub(super) struct Lane {
    pub(super) number: u32,
    pub(super) key: String,
    pub(super) stored: StoredSession,
}

/// The pieces of this chat as kept, in order.
pub(super) async fn lanes(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    chat: &str,
) -> Vec<Lane> {
    let rows = shared_registry::list(db, platform.session_ns(), Some(user_id), None)
        .await
        .unwrap_or_default();
    let mut lanes: Vec<Lane> = rows
        .into_iter()
        .filter_map(|row| {
            let (of, number) = split_lane(&row.record_id)?;
            if of != chat {
                return None;
            }
            Some(Lane {
                number,
                key: row.record_id.clone(),
                stored: serde_json::from_value(row.payload).ok()?,
            })
        })
        .collect();
    lanes.sort_by_key(|lane| lane.number);
    lanes
}

/// How each piece of this chat stands now, with its lane.
pub(super) async fn pieces(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    chat: &str,
) -> Vec<(Lane, Piece)> {
    let lanes = lanes(db, platform, user_id, chat).await;
    if lanes.is_empty() {
        return Vec::new();
    }
    let tasks = Agent::new(db.clone()).await.get_user_tasks(user_id).await;
    let mut pieces = Vec::with_capacity(lanes.len());
    for lane in lanes {
        let live = tasks.iter().find(|task| {
            is_cancellable_task_status(&task.status)
                && session_id_from_lane_id(task.lane_id.as_deref()).as_deref()
                    == Some(lane.stored.session_id.as_str())
        });
        let standing = if lane.stored.stopped {
            Standing::Stopped
        } else if let Some(pending) = load_pending(db, platform, &lane.key).await {
            Standing::Waiting {
                question: format_pending_prompt(&pending.prompt),
            }
        } else if live.is_some() || is_active(&lane.key).await {
            Standing::Doing {
                progress: live.map(|task| task.progress),
            }
        } else {
            Standing::Done {
                result: came_back(db, &lane.stored.session_id).await,
            }
        };
        let piece = Piece {
            number: lane.number,
            asked: lane.stored.asked.clone(),
            started_at: lane.stored.started_at,
            standing,
        };
        pieces.push((lane, piece));
    }
    pieces
}

/// What the helper last said in a piece's Work session.
async fn came_back(db: &DatabaseConnection, session_id: &str) -> Option<String> {
    let history =
        crate::services::agent::sessions::load_session_history(db, session_id, 2, false)
            .await
            .ok()?;
    history
        .last()
        .filter(|message| message.role == "assistant")
        .map(|message| message.content.clone())
}

/// A new piece: its number, its own Work session, kept before it starts.
pub(super) async fn open(
    db: &DatabaseConnection,
    sink: &ChannelSink,
    user_id: i32,
    chat: &str,
    asked: &str,
) -> Option<Lane> {
    let platform = sink.platform();
    let number = next_number(
        lanes(db, platform, user_id, chat)
            .await
            .iter()
            .map(|lane| lane.number),
    );
    let session_id = crate::services::agent::sessions::ensure_session(
        db,
        None,
        user_id,
        AgentInteractionMode::Work,
    )
    .await
    .ok()?;
    let key = lane_key(chat, number);
    let stored = StoredSession {
        session_id,
        binding: Some(sink.binding.clone()),
        address: Some(sink.transport.address()),
        asked: asked.to_string(),
        started_at: Some(crate::clock::now().timestamp()),
        ..Default::default()
    };
    put_session(db, platform, user_id, &key, stored.clone())
        .await
        .ok()?;
    Some(Lane {
        number,
        key,
        stored,
    })
}

/// Stop one piece, and only it: its run, its tasks, what it had waiting.
pub(super) async fn stop(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    lane: &Lane,
) {
    stop_delivery(&lane.key).await;
    cancel_session_tasks(db, user_id, &lane.stored.session_id).await;
    clear_pending(db, platform, &lane.key).await;
    clear_outbound(db, platform, &lane.key).await;
    if let Some(mut stored) = load_session(db, platform, &lane.key).await {
        stored.stopped = true;
        stored.last_run_id = None;
        stored.last_event_seq = 0;
        let _ = put_session(db, platform, user_id, &lane.key, stored).await;
    }
}

/// Stop every piece and forget them: a new conversation numbers from 1.
pub(super) async fn forget_all(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    chat: &str,
) {
    for lane in lanes(db, platform, user_id, chat).await {
        stop(db, platform, user_id, &lane).await;
        let _ = shared_registry::take::<StoredSession>(db, platform.session_ns(), &lane.key).await;
    }
}

/// What a piece sends, labelled when another piece of the chat is not done.
pub(super) async fn label_for(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    key: &str,
    content: &str,
) -> String {
    let Some((chat, number)) = split_lane(key) else {
        return content.to_string();
    };
    let mut asked = String::new();
    let mut others = false;
    for lane in lanes(db, platform, user_id, chat).await {
        if lane.number == number {
            asked = lane.stored.asked;
            continue;
        }
        if !others && !lane.stored.stopped {
            others = is_active(&lane.key).await
                || load_pending(db, platform, &lane.key).await.is_some();
        }
    }
    labelled(number, &asked, others, content)
}

/// Carry out her lines, in the order she wrote them.
#[allow(clippy::too_many_arguments)]
pub(super) async fn carry_out(
    db: &DatabaseConnection,
    user_id: i32,
    hand_offs: &[HandOff],
    their_words: &str,
    images: Option<Value>,
    sink: &ChannelSink,
    chat: &str,
) {
    let platform = sink.platform();
    for hand_off in hand_offs {
        let pieces = pieces(db, platform, user_id, chat).await;
        let plain: Vec<Piece> = pieces.iter().map(|(_, piece)| piece.clone()).collect();
        let lane_of = |work: u32| pieces.iter().find(|(lane, _)| lane.number == work);
        match act(hand_off, &plain) {
            Act::Start(instruction) => {
                let Some(lane) = open(db, sink, user_id, chat, &instruction).await else {
                    let _ = sink.send_text("没能开始这件事，请稍后再试。").await;
                    continue;
                };
                start_work_run(
                    db.clone(),
                    user_id,
                    lane.stored.session_id.clone(),
                    &work_input(&instruction, their_words),
                    images.clone(),
                    sink.clone(),
                    &lane.key,
                )
                .await;
            }
            Act::Answer(work, answer) => {
                let Some((lane, _)) = lane_of(work) else {
                    continue;
                };
                let Some(pending) = load_pending(db, platform, &lane.key).await else {
                    continue;
                };
                let answer = her_choice(&pending.prompt, &answer);
                answer_pending(
                    db,
                    user_id,
                    &lane.key,
                    lane.stored.session_id.clone(),
                    pending,
                    &answer,
                    sink.clone(),
                )
                .await;
            }
            Act::Steer(work, text) => {
                let Some((lane, _)) = lane_of(work) else {
                    continue;
                };
                steer(db, user_id, &lane.stored.session_id, &work_input(&text, their_words)).await;
            }
            Act::Continue(work, text) => {
                let Some((lane, _)) = lane_of(work) else {
                    continue;
                };
                start_work_run(
                    db.clone(),
                    user_id,
                    lane.stored.session_id.clone(),
                    &work_input(&text, their_words),
                    images.clone(),
                    sink.clone(),
                    &lane.key,
                )
                .await;
            }
            Act::Stop(work) => {
                if let Some((lane, _)) = lane_of(work) {
                    stop(db, platform, user_id, lane).await;
                }
            }
            Act::Nothing => {
                info!(?hand_off, "[Channel] her line is for no piece under way");
            }
        }
    }
}

/// More for a running piece: into its tool loop as it is (see `work_loop`).
async fn steer(db: &DatabaseConnection, user_id: i32, session_id: &str, text: &str) -> bool {
    let tasks = Agent::new(db.clone()).await.get_user_tasks(user_id).await;
    let Some(task) = tasks.into_iter().find(|task| {
        is_cancellable_task_status(&task.status)
            && session_id_from_lane_id(task.lane_id.as_deref()).as_deref() == Some(session_id)
    }) else {
        warn!("[Channel] no task under way to take more for this piece");
        return false;
    };
    crate::services::agent::executor::enqueue_steering(db, &task.task_id, text.to_string())
        .await
        .is_ok()
}

/// The answer to a parked question: it resumes, is asked again (what they
/// said kept for the piece), or has expired.
pub(super) async fn answer_pending(
    db: &DatabaseConnection,
    user_id: i32,
    key: &str,
    session_id: String,
    pending: StoredPending,
    input: &str,
    sink: ChannelSink,
) {
    let platform = sink.platform();
    if pending
        .expected_user_id
        .is_some_and(|expected| expected != user_id)
    {
        let _ = sink.send_text(PENDING_STALE_REPLY).await;
        return;
    }
    match decide_pending_reply(&pending.prompt, input, crate::clock::now().timestamp()) {
        PendingDecision::Reask { reply } => {
            // Not an answer, but meant for this piece: it goes on with it.
            if split_lane(key).is_some()
                && let PendingKind::Answer { task_id, .. } = &pending.prompt.kind
            {
                let _ = crate::services::agent::executor::enqueue_steering(
                    db,
                    task_id,
                    input.to_string(),
                )
                .await;
            }
            let reply = label_for(db, platform, user_id, key, &reply).await;
            let _ = sink.send_prompt(&reply, &pending.prompt).await;
        }
        PendingDecision::Expired { reply } => {
            clear_pending(db, platform, key).await;
            let _ = sink.send_text(&reply).await;
        }
        PendingDecision::Resume { kind, answer, .. } => {
            if take_pending_if_id(db, platform, key, &pending.prompt.id)
                .await
                .is_none()
            {
                let _ = sink.send_text(PENDING_STALE_REPLY).await;
                return;
            }
            resume_pending(
                db.clone(),
                session_id,
                key,
                user_id,
                kind,
                answer,
                sink,
                input,
                pending,
            )
            .await;
        }
    }
}

/// What the helper is asked: her instruction, with their own words.
pub(crate) fn work_input(instruction: &str, their_words: &str) -> String {
    let their_words = their_words.trim();
    if their_words.is_empty() || instruction.contains(their_words) {
        instruction.to_string()
    } else {
        format!("{instruction}\n\n（对方原话：{their_words}）")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(number: u32, standing: Standing) -> Piece {
        Piece {
            number,
            asked: format!("第{number}件"),
            started_at: Some(0),
            standing,
        }
    }

    fn waiting(number: u32) -> Piece {
        piece(
            number,
            Standing::Waiting {
                question: "要几点？".into(),
            },
        )
    }

    fn doing(number: u32) -> Piece {
        piece(number, Standing::Doing { progress: None })
    }

    #[test]
    fn a_piece_has_a_key_of_its_own_in_its_chat() {
        assert_eq!(lane_key("telegram:42", 3), "telegram:42#3");
        assert_eq!(split_lane("telegram:42#3"), Some(("telegram:42", 3)));
        assert_eq!(split_lane("telegram:42"), None);
        assert_eq!(chat_of("telegram:42#3"), "telegram:42");
        assert_eq!(chat_of("telegram:42"), "telegram:42");
        assert_eq!(next_number([]), 1);
        assert_eq!(next_number([1, 3, 2]), 4);
    }

    #[test]
    fn the_one_waiting_takes_the_answer_and_more_are_hers_to_place() {
        assert_eq!(route(&[]), Route::Her);
        assert_eq!(route(&[doing(1), doing(2)]), Route::Her);
        assert_eq!(route(&[doing(1), waiting(2)]), Route::Answer(2));
        assert_eq!(route(&[waiting(1), waiting(2)]), Route::Her);
    }

    #[test]
    fn her_line_goes_to_the_piece_as_it_stands() {
        let pieces = [
            doing(1),
            waiting(2),
            piece(3, Standing::Done { result: None }),
            piece(4, Standing::Stopped),
        ];
        let to = |work, text: &str| HandOff::To {
            work,
            text: text.into(),
        };
        assert_eq!(act(&to(1, "改成周六"), &pieces), Act::Steer(1, "改成周六".into()));
        assert_eq!(act(&to(2, "是"), &pieces), Act::Answer(2, "是".into()));
        assert_eq!(
            act(&to(3, "再查一次"), &pieces),
            Act::Continue(3, "再查一次".into())
        );
        assert_eq!(act(&to(4, "接着办"), &pieces), Act::Nothing);
        assert_eq!(act(&to(9, "嗯"), &pieces), Act::Nothing);
        assert_eq!(act(&HandOff::Stop { work: 1 }, &pieces), Act::Stop(1));
        assert_eq!(act(&HandOff::Stop { work: 2 }, &pieces), Act::Stop(2));
        assert_eq!(act(&HandOff::Stop { work: 4 }, &pieces), Act::Nothing);
        assert_eq!(
            act(&HandOff::New { text: "查".into() }, &pieces),
            Act::Start("查".into())
        );
    }

    #[test]
    fn she_sees_each_piece_on_a_line() {
        let now = 3 * 60 * 60;
        let mut running = doing(2);
        running.asked = "查明天东京天气".into();
        running.started_at = Some(now - 200);
        running.standing = Standing::Doing { progress: Some(60) };
        assert_eq!(line(&running, now), "#2「查明天东京天气」在办，已办 3 分钟（60%）");
        let mut asking = waiting(3);
        asking.asked = "订周五的餐厅".into();
        asking.standing = Standing::Waiting {
            question: "要几点？\n\n1. 18:00\n2. 19:00".into(),
        };
        assert_eq!(
            line(&asking, now),
            "#3「订周五的餐厅」在等对方回答：要几点？ 1. 18:00 2. 19:00"
        );
        let back = piece(
            1,
            Standing::Done {
                result: Some("周报整理好了".into()),
            },
        );
        assert_eq!(line(&back, now), "#1「第1件」办完了：周报整理好了");
        assert_eq!(line(&piece(4, Standing::Stopped), now), "#4「第4件」被叫停了");
        running.started_at = Some(now - 2 * 60 * 60 - 5 * 60);
        assert!(line(&running, now).contains("已办 2 小时 5 分钟"));
    }

    #[test]
    fn she_sees_every_unfinished_piece_and_the_last_few_done() {
        let now = 2 * DONE_FOR_SECS;
        let done = |number, at| Piece {
            started_at: Some(at),
            ..piece(number, Standing::Done { result: None })
        };
        let pieces = vec![
            done(1, 0),
            done(2, now - 10),
            done(3, now - 9),
            done(4, now - 8),
            done(5, now - 7),
            Piece {
                started_at: Some(0),
                ..doing(6)
            },
            waiting(7),
        ];
        let shown: Vec<u32> = shown(pieces, now).iter().map(|piece| piece.number).collect();
        assert_eq!(shown, [3, 4, 5, 6, 7]);
    }

    #[test]
    fn what_a_piece_sends_says_which_when_another_is_not_done() {
        assert_eq!(
            label(2, "查一下明天东京的天气怎么样"),
            "【#2 查一下明天东京的天气怎么…】"
        );
        assert_eq!(label(1, "查天气"), "【#1 查天气】");
        assert_eq!(labelled(1, "查天气", false, "晴"), "晴");
        assert_eq!(labelled(1, "查天气", true, "晴"), "【#1 查天气】\n晴");
    }

    #[test]
    fn a_question_carries_its_piece_in_its_buttons() {
        let prompt = PendingPrompt {
            id: String::new(),
            kind: PendingKind::Answer {
                task_id: "t".into(),
                question_id: "q".into(),
                question_type: "confirmation".into(),
            },
            question: "发吗？".into(),
            options: Vec::new(),
            expires_at_unix: None,
        };
        let stamped = stamp(3, prompt);
        assert_eq!(number_of_prompt(&stamped.id), Some(3));
        assert_eq!(stamp(3, stamped.clone()).id, stamped.id);
        let rows = myriad_agent_rules::channel::telegram_inline_keyboard(&stamped);
        let data = &rows[0][0].callback_data;
        let bound = myriad_agent_rules::channel::parse_telegram_callback(data).unwrap();
        assert_eq!(number_of_prompt(&bound.prompt_id), Some(3));
        assert_eq!(
            telegram_callback_action(&stamped, data),
            TelegramCallbackAction::Resume("是".into())
        );
        // A chat's own question (from before pieces) has no number.
        assert_eq!(number_of_prompt("a1b2c3d4"), None);
    }

    #[test]
    fn her_answer_to_a_choice_is_the_choice_she_means() {
        let prompt = PendingPrompt {
            id: String::new(),
            kind: PendingKind::Answer {
                task_id: "t".into(),
                question_id: "q".into(),
                question_type: "single_choice".into(),
            },
            question: "要几点？".into(),
            options: ["18:00", "19:00"]
                .map(|time| PendingOption {
                    value: time.into(),
                    label: time.into(),
                })
                .to_vec(),
            expires_at_unix: None,
        };
        assert_eq!(her_choice(&prompt, "2（19:00）"), "2");
        assert_eq!(her_choice(&prompt, "订 19:00 的"), "2");
        assert_eq!(her_choice(&prompt, "19:00，选项2"), "2");
        assert_eq!(her_choice(&prompt, "1"), "1");
        // A time is not a choice number; two choices named is not one.
        assert_eq!(her_choice(&prompt, "19点"), "19点");
        assert_eq!(her_choice(&prompt, "18:00 或 19:00"), "18:00 或 19:00");
        assert!(matches!(
            decide_pending_reply(&prompt, &her_choice(&prompt, "2（19:00）"), 0),
            PendingDecision::Resume { answer, .. } if answer == "19:00"
        ));
    }

    #[test]
    fn the_helper_hears_their_own_words_too() {
        assert_eq!(
            work_input("查一下明天东京的天气", "明天东京会下雨吗？帮我看看"),
            "查一下明天东京的天气\n\n（对方原话：明天东京会下雨吗？帮我看看）"
        );
        assert_eq!(work_input("帮我看看", "帮我看看"), "帮我看看");
    }
}
