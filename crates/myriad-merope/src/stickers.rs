//! Her stickers: pictures of her she made herself, to send in chat apps the
//! way people send stickers.
//!
//! She makes one when she wants to send a sticker and has none that fits,
//! and later of a running joke in a group. Each keeps what it shows, what it
//! means and where it belongs: a reaction goes anywhere, a group's joke
//! only to that group, where it lands. She picks by what it means, from the
//! few offered to her, by number; she never names one she does not have.
//! Making one draws a picture, so there are only so many a month.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Stickers she can make in a month.
pub const MONTHLY: usize = 30;
/// Longest what a sticker shows, and what it means, as she describes them.
pub const PICTURE_CHARS: usize = 200;
pub const MEANING_CHARS: usize = 80;
/// Drawn square; sent smaller.
pub const SIZE: u32 = 1024;

const OPEN: &str = "[[sticker:";
const CLOSE: &str = "]]";

/// One of her stickers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sticker {
    pub id: String,
    /// What it shows, as she described it.
    pub picture: String,
    /// What it means: when she would send it.
    pub meaning: String,
    /// The group it belongs to (`telegram:-100123`), or none: anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    pub asset_id: String,
    /// Who it was drawn as (see [`identity_key`]): one drawn of how she
    /// looked before is not her any more.
    pub identity: String,
    pub made_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub sent: u32,
}

/// What she wants done with a sticker in her reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Send the one numbered so among those offered (from 1).
    Send(u8),
    /// Make a new one: what it shows, what it means.
    Make { picture: String, meaning: String },
}

/// Her reply without its sticker line, and what she wants done.
pub fn split_sticker_directive(raw: &str) -> (String, Option<Choice>) {
    let mut spoken = raw.to_string();
    let mut choice = None;
    while let Some(start) = spoken.find(OPEN) {
        let inner_at = start + OPEN.len();
        let Some(close) = spoken[inner_at..].find(CLOSE) else {
            break;
        };
        let inner = spoken[inner_at..inner_at + close].trim().to_string();
        spoken.replace_range(start..inner_at + close + CLOSE.len(), "");
        if let Some(next) = parse_choice(&inner) {
            choice = Some(next);
        }
    }
    let spoken = spoken
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    (spoken, choice)
}

fn parse_choice(inner: &str) -> Option<Choice> {
    if let Some(rest) = inner.strip_prefix("new") {
        let mut parts = rest.trim_start_matches([' ', ':', '：', '|']).split('|');
        let picture: String = parts.next()?.trim().chars().take(PICTURE_CHARS).collect();
        let meaning: String = parts.next()?.trim().chars().take(MEANING_CHARS).collect();
        return (!picture.is_empty() && !meaning.is_empty())
            .then_some(Choice::Make { picture, meaning });
    }
    inner
        .parse::<u8>()
        .ok()
        .filter(|number| *number > 0)
        .map(Choice::Send)
}

/// The stickers offered to her this turn, numbered, and whether she can make
/// a new one (with how many are left this month).
pub fn format_sticker_section(meanings: &[String], left_this_month: usize) -> Option<String> {
    if meanings.is_empty() && left_this_month == 0 {
        return None;
    }
    let mut section = "## Stickers\nStickers of your own, pictures of you. Send one only where a person would, not every time. To send one, put [[sticker:N]] on its own last line, N its number here; a sticker can also be your whole reply, with nothing else.".to_string();
    if !meanings.is_empty() {
        let lines: Vec<String> = meanings
            .iter()
            .enumerate()
            .map(|(index, meaning)| format!("{}. {meaning}", index + 1))
            .collect();
        section.push('\n');
        section.push_str(&myriad_agent_rules::untrusted_block(
            "your_stickers",
            &lines.join("\n"),
        ));
    }
    if left_this_month > 0 {
        section.push_str(&format!(
            "\nIf none of yours fits and you want one, you can make a new one: [[sticker:new|what it shows|what it means]] on its own last line. What it shows is you, in a pose and an expression, with any prop it needs; no words in it. What it means is when you would send it. It takes a little while to make, so if you are making one, say something first. You can make {left_this_month} more this month."
        ));
    }
    Some(section)
}

/// Who she is drawn as: her name, how she looks, and the portrait she is
/// drawn from. A sticker made under another key is of how she looked before.
pub fn identity_key(name: &str, visual_profile: &Value, portrait_asset_id: &str) -> String {
    let identity = serde_json::json!({
        "name": name.trim(),
        "look": crate::visual_contract::appearance_visual_profile(visual_profile),
        "portrait": portrait_asset_id.trim(),
    });
    hex_digest(identity.to_string().as_bytes())
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

const SCHOOL: &str = "One die-cut chibi sticker of a single character, drawn in the same souvenir-sticker language as the attached style reference: a Q-style super-deformed character with a big head and a small body, large glossy jewel eyes with layered irises and bright catchlights, tiny simplified nose and mouth, soft round cheeks, hair built from a few broad tapered ribbon masses, thin colored linework, pastel high-key palette, and clean cel-to-gradient shading with a luminous finish. It is a chat sticker: one clear, exaggerated expression and pose that reads at a glance.";

const ANCHOR: &str = "The attached portrait is the immutable anchor for who this character is, not for how they are posed or dressed in this sticker. Keep the same person: hair color and cut, eye color, skin tone and signature ornaments carry over unchanged. Do not redesign the identity or change the character's age.";

const CUT: &str = "Finish it as a physical die-cut sticker: one thick uniform white cut border tracing the whole silhouette, a soft narrow drop shadow just outside it, and nothing else. The character and border sit fully inside the square with even margins; nothing is cropped at the edge. Outside the border is fully transparent: no backdrop, no frame, no second character. No text, letters, numbers, speech bubbles, captions, watermark or signature anywhere.";

const READABILITY: &str = "It will be shown small in a chat: one clear silhouette, strong value contrast, no fine detail that vanishes when downscaled.";

/// The prompt for drawing a sticker of her: who she is from her visual
/// profile, what this one shows as she described it.
pub fn sticker_prompt(name: &str, visual_profile: &Value, picture: &str) -> String {
    let identity = crate::visual_prompt::normalize_visual_identity_for_prompt(visual_profile)
        .and_then(|identity| crate::visual_design::flatten_visual_identity(&identity))
        .unwrap_or(Value::Null);
    let mut parts = vec![SCHOOL.to_string(), ANCHOR.to_string()];
    let picture: String = picture
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(PICTURE_CHARS)
        .collect();
    parts.push(format!("This sticker shows: {picture}"));
    parts.push(CUT.to_string());
    let name: String = name.trim().chars().take(50).collect();
    if !name.is_empty() {
        parts.push(format!("Identity name: {name}."));
    }
    let facts = crate::sticker_avatar::identity_facts(&identity);
    if !facts.is_empty() {
        parts.push(format!(
            "Identity facts that must survive the restyle:\n{}",
            facts.join("\n")
        ));
    }
    parts.push(READABILITY.to_string());
    parts.join("\n\n")
}

/// The month a sticker was made in, for the monthly count.
pub fn month_of(at: chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&chrono::Local).format("%Y-%m").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn her_sticker_line_is_taken_out_of_what_she_says() {
        let (spoken, choice) = split_sticker_directive("哈哈哈\n[[sticker:2]]");
        assert_eq!(spoken, "哈哈哈");
        assert_eq!(choice, Some(Choice::Send(2)));
        let (spoken, choice) = split_sticker_directive("[[sticker:1]]");
        assert_eq!(spoken, "");
        assert_eq!(choice, Some(Choice::Send(1)));
        let (spoken, choice) =
            split_sticker_directive("等我做一个\n[[sticker:new|抱着空鸟笼叹气|没等到对的]]");
        assert_eq!(spoken, "等我做一个");
        assert_eq!(
            choice,
            Some(Choice::Make {
                picture: "抱着空鸟笼叹气".into(),
                meaning: "没等到对的".into()
            })
        );
        assert_eq!(split_sticker_directive("[[sticker:0]]").1, None);
        assert_eq!(split_sticker_directive("[[sticker:new|只有画面]]").1, None);
        assert_eq!(split_sticker_directive("没有表情包").1, None);
    }

    #[test]
    fn she_is_offered_hers_by_number_and_making_only_while_there_are_some_left() {
        let section =
            format_sticker_section(&["够格了，认了".into(), "不是这个".into()], 3).unwrap();
        assert!(section.contains("1. 够格了，认了") && section.contains("2. 不是这个"));
        assert!(section.contains("your_stickers"));
        assert!(section.contains("make 3 more this month"));
        let none_left = format_sticker_section(&["不是这个".into()], 0).unwrap();
        assert!(!none_left.contains("[[sticker:new"));
        assert!(format_sticker_section(&[], 0).is_none());
        assert!(
            format_sticker_section(&[], 2)
                .unwrap()
                .contains("[[sticker:new")
        );
    }

    #[test]
    fn a_sticker_is_drawn_of_her_as_she_looks_now() {
        let profile = json!({"visualIdentity": {"hairShape": "短鲍伯，粉转薰衣草渐变"}});
        let prompt = sticker_prompt("若泉 绮羽", &profile, "抱着空鸟笼叹气");
        assert!(prompt.contains("This sticker shows: 抱着空鸟笼叹气"));
        assert!(prompt.contains("No text"));
        assert!(prompt.contains("若泉 绮羽"));
        let key = identity_key("若泉 绮羽", &profile, "asset-1");
        assert_eq!(key, identity_key(" 若泉 绮羽 ", &profile, "asset-1"));
        assert_ne!(key, identity_key("若泉 绮羽", &profile, "asset-2"));
        let at: chrono::DateTime<chrono::Utc> = "2026-09-28T10:00:00Z".parse().unwrap();
        assert_eq!(month_of(at).len(), 7);
    }
}
