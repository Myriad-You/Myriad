//! Her `@name` in a group reply, as each platform mentions someone.
//!
//! She writes `@` and a name as it shows in the group's conversation. Only
//! people who spoke in the group recently can be mentioned: a mention needs
//! their platform id, and those are the ids known. Anything else stays plain
//! text, and no platform's mention-everyone form is ever produced.

use serde_json::{Value, json};

/// A piece of her reply: text, or someone mentioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Mention { id: String, name: String },
}

/// Her reply cut at each `@name` of someone in `people` (name, platform id).
/// The longest name wins where names share a start (`@小明明` over `@小明`).
pub fn split_mentions(text: &str, people: &[(String, String)]) -> Vec<Piece> {
    let mut people: Vec<&(String, String)> = people
        .iter()
        .filter(|(name, id)| !name.trim().is_empty() && !id.trim().is_empty())
        .collect();
    people.sort_by_key(|(name, _)| std::cmp::Reverse(name.chars().count()));
    let mut pieces = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['@', '＠']) {
        plain.push_str(&rest[..at]);
        let sign = rest[at..].chars().next().map_or(1, char::len_utf8);
        let after = &rest[at + sign..];
        match people
            .iter()
            .find(|(name, _)| after.starts_with(name.as_str()))
        {
            Some((name, id)) => {
                if !plain.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut plain)));
                }
                pieces.push(Piece::Mention {
                    id: id.clone(),
                    name: name.clone(),
                });
                rest = &after[name.len()..];
            }
            None => {
                plain.push_str(&rest[at..at + sign]);
                rest = after;
            }
        }
    }
    plain.push_str(rest);
    if !plain.is_empty() {
        pieces.push(Piece::Text(plain));
    }
    pieces
}

/// OneBot message segments: text, and `at` for each mention.
pub fn onebot_segments(pieces: &[Piece]) -> Vec<Value> {
    pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Text(text) => Some(json!({"type": "text", "data": {"text": text}})),
            // A QQ number only: `all` would be everyone.
            Piece::Mention { id, .. } => id
                .parse::<i64>()
                .ok()
                .map(|qq| json!({"type": "at", "data": {"qq": qq.to_string()}})),
        })
        .collect()
}

/// Telegram text with `text_mention` entities: `@name` stays in the text,
/// the entity (offsets in UTF-16 units) makes it a mention of that user.
pub fn telegram_text_and_entities(pieces: &[Piece]) -> (String, Vec<Value>) {
    let mut text = String::new();
    let mut entities = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text(plain) => text.push_str(plain),
            Piece::Mention { id, name } => {
                let shown = format!("@{name}");
                if let Ok(user) = id.parse::<i64>() {
                    entities.push(json!({
                        "type": "text_mention",
                        "offset": text.encode_utf16().count(),
                        "length": shown.encode_utf16().count(),
                        "user": {"id": user},
                    }));
                }
                text.push_str(&shown);
            }
        }
    }
    (text, entities)
}

/// Discord content with `<@id>` for each mention, and the ids it may ping.
pub fn discord_content_and_users(pieces: &[Piece]) -> (String, Vec<String>) {
    let mut content = String::new();
    let mut users = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text(plain) => content.push_str(plain),
            Piece::Mention { id, .. } if id.chars().all(|c| c.is_ascii_digit()) => {
                content.push_str(&format!("<@{id}>"));
                if !users.contains(id) {
                    users.push(id.clone());
                }
            }
            Piece::Mention { name, .. } => content.push_str(&format!("@{name}")),
        }
    }
    (content, users)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn people() -> Vec<(String, String)> {
        vec![
            ("小明".into(), "1001".into()),
            ("小明明".into(), "1002".into()),
            ("Ann Lee".into(), "1003".into()),
        ]
    }

    #[test]
    fn a_mention_is_someone_in_the_conversation() {
        let pieces = split_mentions(
            "@小明 你问的那个，@小明明也知道。@路人 呢？＠Ann Lee!",
            &people(),
        );
        assert_eq!(
            pieces,
            vec![
                Piece::Mention {
                    id: "1001".into(),
                    name: "小明".into()
                },
                Piece::Text(" 你问的那个，".into()),
                Piece::Mention {
                    id: "1002".into(),
                    name: "小明明".into()
                },
                Piece::Text("也知道。@路人 呢？".into()),
                Piece::Mention {
                    id: "1003".into(),
                    name: "Ann Lee".into()
                },
                Piece::Text("!".into()),
            ]
        );
        assert_eq!(
            split_mentions("没有人", &people()),
            vec![Piece::Text("没有人".into())]
        );
        assert_eq!(
            split_mentions("邮箱 a@b.c", &people()),
            vec![Piece::Text("邮箱 a@b.c".into())]
        );
    }

    #[test]
    fn each_platform_mentions_its_own_way_and_never_everyone() {
        let pieces = split_mentions("@小明 看这个 @全体成员", &people());
        let segments = onebot_segments(&pieces);
        assert_eq!(segments[0], json!({"type": "at", "data": {"qq": "1001"}}));
        assert_eq!(
            segments[1],
            json!({"type": "text", "data": {"text": " 看这个 @全体成员"}})
        );
        // Someone whose shown name is 全体成员 is still one person.
        let everyone = vec![("全体成员".to_string(), "all".to_string())];
        assert!(onebot_segments(&split_mentions("@全体成员", &everyone)).is_empty());

        let (text, entities) =
            telegram_text_and_entities(&split_mentions("好，@小明 你来", &people()));
        assert_eq!(text, "好，@小明 你来");
        assert_eq!(
            entities,
            vec![json!({"type": "text_mention", "offset": 2, "length": 3, "user": {"id": 1001}})]
        );

        let (content, users) =
            discord_content_and_users(&split_mentions("@小明 and @everyone", &people()));
        assert_eq!(content, "<@1001> and @everyone");
        assert_eq!(users, vec!["1001".to_string()]);
    }
}
