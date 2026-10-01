//! Being muted in a group: she knows it, as what happened there, and says
//! nothing there until it lifts.

use super::*;
use myriad_agent_rules::onebot::decode::Muted;

/// Muted with no end given (a whole-group mute): as long as a group can
/// mute anyone, unless lifted first.
const UNTIL_LIFTED: chrono::Duration = chrono::Duration::days(30);

/// She was muted in a group, or it was lifted, by `by` (their platform id;
/// empty if not known). Kept in mind, and told in the group's talk as what
/// happened, by whom, the way the group saw it. While a mute holds she
/// answers nothing there and does not speak up. A restart forgets the mute
/// (not the line): what she says meanwhile is only refused by the platform.
pub async fn muted(venue: &str, by: &str, muted: Muted, everyone: bool) {
    let now = chrono::Utc::now();
    let until = match muted {
        Muted::For(seconds) => {
            Some(now + chrono::Duration::seconds(seconds.min(i64::MAX as u64) as i64))
        }
        Muted::UntilLifted => Some(now + UNTIL_LIFTED),
        Muted::Lifted => None,
    };
    with_group(venue, |group| {
        if everyone {
            group.everyone_muted_until = until;
        } else {
            group.muted_until = until;
        }
    });
    let name = (!by.is_empty())
        .then(|| {
            people(venue)
                .into_iter()
                .find(|(_, from)| from == by)
                .map(|(name, _)| name)
        })
        .flatten()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "群管理".to_string());
    info!(%venue, ?muted, everyone, "[Group] her mute there changed");
    remember_line(
        venue,
        Line {
            at: now,
            message_id: None,
            name,
            from: (!by.is_empty()).then(|| by.to_string()),
            text: told(muted, everyone),
            hers: false,
            addressed: false,
            images: Vec::new(),
            seen: Vec::new(),
        },
    )
    .await;
}

/// The mute as the group saw it happen.
pub(super) fn told(muted: Muted, everyone: bool) -> String {
    match (muted, everyone) {
        (Muted::For(seconds), false) => format!("（把你禁言了{}）", how_long(seconds)),
        (Muted::For(seconds), true) => format!("（全员禁言{}）", how_long(seconds)),
        (Muted::UntilLifted, false) => "（把你禁言了）".to_string(),
        (Muted::UntilLifted, true) => "（开了全员禁言）".to_string(),
        (Muted::Lifted, false) => "（解除了你的禁言）".to_string(),
        (Muted::Lifted, true) => "（解除了全员禁言）".to_string(),
    }
}

fn how_long(seconds: u64) -> String {
    let minutes = seconds.div_ceil(60);
    match minutes {
        0..=89 => format!(" {minutes} 分钟"),
        90..=2159 => format!(" {} 小时", (minutes + 30) / 60),
        _ => format!(" {} 天", (minutes + 720) / 1440),
    }
}

/// Whether she cannot speak in the group now.
pub(super) fn is_muted(venue: &str) -> bool {
    let now = chrono::Utc::now();
    with_group(venue, |group| muted_now(group, now)).unwrap_or(false)
}

pub(super) fn muted_now(group: &Group, now: chrono::DateTime<chrono::Utc>) -> bool {
    [group.muted_until, group.everyone_muted_until]
        .into_iter()
        .flatten()
        .any(|until| until > now)
}
