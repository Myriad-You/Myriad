//! Slash commands in a private chat: status of their Work, and cancelling it.

use super::*;

pub(super) async fn handle_command(
    db: &DatabaseConnection,
    user_id: i32,
    platform: ChannelPlatform,
    session_key: &str,
    session_id: &str,
    command: ChannelCommand,
    sink: &ChannelSink,
) {
    match command {
        ChannelCommand::Stop => {
            stop_delivery(session_key).await;
            cancel_session_tasks(db, user_id, session_id).await;
            clear_pending(db, platform, session_key).await;
            clear_outbound(db, platform, session_key).await;
            if let Some(mut stored) = load_session(db, platform, session_key).await {
                stored.last_run_id = None;
                stored.last_event_seq = 0;
                let _ = put_session(db, platform, user_id, session_key, stored).await;
            }
            let _ = sink.send_text(CHANNEL_STOP_REPLY).await;
        }
        ChannelCommand::NewConversation => {
            stop_delivery(session_key).await;
            cancel_session_tasks(db, user_id, session_id).await;
            clear_pending(db, platform, session_key).await;
            clear_outbound(db, platform, session_key).await;
            match crate::services::agent::sessions::ensure_session(
                db,
                None,
                user_id,
                AgentInteractionMode::Work,
            )
            .await
            {
                Ok(new_id) => {
                    let _ = put_session(
                        db,
                        platform,
                        user_id,
                        session_key,
                        StoredSession {
                            session_id: new_id,
                            last_run_id: None,
                            last_event_seq: 0,
                            original_input: String::new(),
                            binding: None,
                            address: None,
                            chat_session_id: None,
                        },
                    )
                    .await;
                    let _ = sink.send_text(CHANNEL_NEW_SESSION_REPLY).await;
                }
                Err(error) => {
                    warn!(%error, "channel new session failed");
                    let _ = sink.send_text("没能开新对话，请稍后再试。").await;
                }
            }
        }
        ChannelCommand::Status => {
            let reply = status_reply(db, user_id, platform, session_key, session_id).await;
            let _ = sink.send_text(&reply).await;
        }
        ChannelCommand::Help => {
            let _ = sink.send_text(CHANNEL_HELP_REPLY).await;
        }
    }
}

pub(super) async fn status_reply(
    db: &DatabaseConnection,
    user_id: i32,
    platform: ChannelPlatform,
    session_key: &str,
    session_id: &str,
) -> String {
    if let Some(pending) = load_pending(db, platform, session_key).await {
        return format!("当前待答：\n{}", format_pending_prompt(&pending.prompt));
    }
    let agent = Agent::new(db.clone()).await;
    let tasks = agent.get_user_tasks(user_id).await;
    let live = tasks.into_iter().find(|task| {
        is_cancellable_task_status(&task.status)
            && session_id_from_lane_id(task.lane_id.as_deref()).as_deref() == Some(session_id)
    });
    match live {
        Some(task) => format!(
            "正在办：{}（{}%）\n任务 {}",
            task_status_label(&task.status),
            task.progress,
            task.task_id
        ),
        None => {
            if session_id.is_empty() {
                "现在没有进行中的任务。".to_string()
            } else {
                format!("现在没有进行中的任务。会话 {session_id}。")
            }
        }
    }
}

pub(super) fn task_status_label(status: &myriad_agent_rules::TaskStatus) -> &'static str {
    match status {
        myriad_agent_rules::TaskStatus::Pending => "排队中",
        myriad_agent_rules::TaskStatus::Running => "执行中",
        myriad_agent_rules::TaskStatus::WaitingForInput => "等待回答",
        myriad_agent_rules::TaskStatus::Paused => "已暂停",
        myriad_agent_rules::TaskStatus::Completed => "已完成",
        myriad_agent_rules::TaskStatus::Failed => "失败",
        myriad_agent_rules::TaskStatus::Cancelled => "已取消",
    }
}

pub(super) async fn cancel_session_tasks(db: &DatabaseConnection, user_id: i32, session_id: &str) {
    if session_id.is_empty() {
        return;
    }
    let agent = Agent::new(db.clone()).await;
    for task in agent.get_user_tasks(user_id).await {
        if !is_cancellable_task_status(&task.status) {
            continue;
        }
        if session_id_from_lane_id(task.lane_id.as_deref()).as_deref() != Some(session_id) {
            continue;
        }
        let _ = crate::services::agent::run::cancel_task_and_wake(db, user_id, &task.task_id).await;
    }
}
