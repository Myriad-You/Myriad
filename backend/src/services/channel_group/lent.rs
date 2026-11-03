//! A group lent to her mind on a PC (`her_log::lend`): while she is not on
//! the site and her mind is on a PC that can think with it, a group's new
//! line goes there instead of to the group mind here, and what she says
//! back is sent in the group as her.

use super::*;
use crate::services::her_log::lend::{self as her, Place};

/// Lines before the new one lent with it, at most.
const RECENT: usize = 20;

/// Lend `message` to her mind on a PC; whether it was (else the group mind
/// here takes it, as before).
pub(super) async fn lend(message: &GroupLine) -> bool {
    if !away() {
        return false;
    }
    let Ok(db) = db() else {
        return false;
    };
    let Some(device) = her::mind_on_a_pc(&db).await else {
        return false;
    };
    let venue = message.venue();
    let recent = with_group(&venue, |group| {
        let theirs: Vec<her::Message> = group
            .lines
            .iter()
            .filter(|line| !line.hers)
            .filter_map(|line| {
                let id = line.message_id.clone()?;
                (id != message.message_id).then(|| her::Message {
                    id,
                    from: line.from.clone(),
                    name: Some(line.name.clone()),
                    text: line.said(),
                    at_ms: line.at.timestamp_millis(),
                    to_her: line.addressed,
                    mine: false,
                })
            })
            .collect();
        let skip = theirs.len().saturating_sub(RECENT);
        theirs[skip..].to_vec()
    })
    .unwrap_or_default();
    let text = if message.text.trim().is_empty() && !message.images.is_empty() {
        "[图片]".to_string()
    } else {
        message.text.clone()
    };
    let one = her::Message {
        id: message.message_id.clone(),
        from: Some(message.from.clone()),
        name: Some(message.display_name.clone()),
        text,
        at_ms: now().timestamp_millis(),
        to_her: message.addressed,
        mine: false,
    };
    let name = title(&venue).await;
    her::lend(
        &device,
        &her::group_audience(&venue),
        Place::Group {
            venue,
            thread: message.thread,
        },
        name.as_deref(),
        &[one],
        &recent,
    )
}

/// Her line from her mind on a PC, sent in `venue` (in `thread`) as she
/// sends her own, quoting `reply_to` when it names a line there.
pub(crate) async fn say_lent(
    venue: &str,
    thread: Option<i64>,
    reply_to: Option<&str>,
    text: &str,
) -> Result<(), &'static str> {
    let (slug, chat) = venue.split_once(':').ok_or("no such group")?;
    let mut line = line_to(&myriad_merope::group::Target {
        platform: slug.to_string(),
        chat: chat.to_string(),
        message_id: reply_to.unwrap_or_default().to_string(),
        thread,
    })
    .ok_or("no such platform")?;
    line.thread = thread;
    let pieces = [myriad_agent_rules::mentions::Piece::Text(text.to_string())];
    send_reply(&line, &token(venue), &pieces, reply_to.is_some())
        .await
        .map_err(|_| "not sent")
}
