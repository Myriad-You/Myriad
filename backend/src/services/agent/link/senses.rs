//! Her senses on the app: what a child sees and hears for her. Its
//! `presence` frames carry the same observations a page's report does
//! (`perception`, `musicStatus`); the host keeps those only as far as the
//! child said in `hello` that it senses them, so a child that never said it
//! looks at a window, a pointer, music or the screen tells her nothing of
//! them. Where the person is using her decides whose senses she has
//! (`consciousness::senses_presence`); a turn said on the app is answered
//! with that app's.

use serde_json::Value;

/// What a child said it senses for her.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Senses {
    window: bool,
    pointer: bool,
    music: bool,
    screen: bool,
    voice: bool,
}

impl Senses {
    /// From the capabilities the host took in `hello`.
    pub(super) fn of(capabilities: &[String]) -> Self {
        let has = |name: &str| capabilities.iter().any(|capability| capability == name);
        Self {
            window: has("perception.window"),
            pointer: has("perception.pointer"),
            music: has("perception.music"),
            screen: has("perception.screen"),
            voice: has("voice.in"),
        }
    }

    /// Whether an observation of this kind is one the child senses.
    /// `presence` rows are the report itself: always.
    fn senses(&self, kind: &str) -> bool {
        match kind {
            "page" | "surface" => self.window,
            "pointer" => self.pointer,
            "music" => self.music,
            "screen" => self.screen,
            "voice" => self.voice,
            "presence" => true,
            _ => false,
        }
    }

    /// A `presence` body with only what the child senses.
    pub(super) fn admit(&self, body: &Value) -> Value {
        let mut body = body.clone();
        let Some(object) = body.as_object_mut() else {
            return body;
        };
        if let Some(rows) = object.get_mut("perception").and_then(Value::as_array_mut) {
            rows.retain(|row| {
                row.get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| self.senses(kind))
            });
            for row in rows.iter_mut() {
                if row.get("kind").and_then(Value::as_str) == Some("screen") {
                    keep_screen_fields(row);
                }
            }
        }
        if !self.music {
            object.remove("musicStatus");
        }
        body
    }
}

/// A look at the screen says only what `screenFields` lets it: a fact the
/// list does not name is deleted (and counted, never its value). A chat
/// window's row (its facts carry who) keeps only the first line of its
/// summary and no screen_text: a child from before put others' words there.
/// Facts that are not an object, or a summary that is not text, are deleted
/// whole.
fn keep_screen_fields(row: &mut Value) {
    let t = "presence";
    let Some(row) = row.as_object_mut() else {
        return;
    };
    if row.get("safeFacts").is_some_and(|facts| !facts.is_object()) {
        row.remove("safeFacts");
        super::counts::screen_key_deleted(t, "safeFacts");
    }
    if row
        .get("summary")
        .is_some_and(|summary| !summary.is_string())
    {
        row.remove("summary");
        super::counts::screen_key_deleted(t, "summary");
    }
    let chat = row
        .get_mut("safeFacts")
        .and_then(Value::as_object_mut)
        .is_some_and(|facts| keep_facts(t, facts, false));
    if chat
        && let Some(Value::String(summary)) = row.get_mut("summary")
        && let Some(first) = cut_to_first_line(summary)
    {
        *summary = first;
        super::counts::screen_key_deleted(t, "summary");
    }
}

/// A text up to its first line break of any kind (`\n`, `\r`, a vertical
/// tab, a form feed, NEL, U+2028, U+2029); `None` when it has none.
fn cut_to_first_line(text: &str) -> Option<String> {
    let at = text.find([
        '\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
    ])?;
    Some(text[..at].to_string())
}

/// The facts `screenFields` names, and with `plain` only text cut to the
/// contract's length, numbers and yes/no; a chat window's (who) never its
/// screen_text. What goes is counted by key, never its value. Whether it
/// is a chat window is told by `who` as it came, before anything of it goes:
/// one whose `who` is not a plain value is still a chat. True for a chat.
fn keep_facts(t: &'static str, facts: &mut serde_json::Map<String, Value>, plain: bool) -> bool {
    let max_chars = super::contract::CONTRACT.limits.max_text_chars;
    let chat = facts.contains_key("who");
    facts.retain(|key, value| {
        let named = super::counts::screen_fact(key);
        let kept = named
            && (!plain || matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_)));
        if !kept {
            super::counts::screen_key_deleted(t, key);
        }
        kept
    });
    if plain {
        for (key, value) in facts.iter_mut() {
            if let Value::String(text) = value
                && text.chars().count() > max_chars
            {
                *text = text.chars().take(max_chars).collect();
                super::counts::screen_key_deleted(t, key);
            }
        }
    }
    if chat && facts.remove("screen_text").is_some() {
        super::counts::screen_key_deleted(t, "screen_text");
    }
    chat
}

/// What a `screen.look` result may say, built anew: its facts (an object, of
/// plain values) and the first line of its summary, cut to the contract's
/// length; anything else beside them is left out and counted. `None` when
/// it is not of that shape at all: none of it is read.
pub(super) fn look_result(result: &Value) -> Option<Value> {
    let t = "call.result";
    let given = result.as_object()?;
    let mut facts = given.get("facts")?.as_object()?.clone();
    keep_facts(t, &mut facts, true);
    let mut kept = serde_json::Map::new();
    kept.insert("facts".into(), Value::Object(facts));
    match given.get("summary") {
        Some(Value::String(summary)) => {
            let max_chars = super::contract::CONTRACT.limits.max_text_chars;
            let first = cut_to_first_line(summary).unwrap_or_else(|| summary.clone());
            let first: String = first.chars().take(max_chars).collect();
            if first.len() != summary.len() {
                super::counts::screen_key_deleted(t, "summary");
            }
            kept.insert("summary".into(), Value::String(first));
        }
        Some(_) => super::counts::screen_key_deleted(t, "summary"),
        None => {}
    }
    for key in given
        .keys()
        .filter(|key| !matches!(key.as_str(), "facts" | "summary"))
    {
        super::counts::screen_key_deleted(t, key);
    }
    Some(Value::Object(kept))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> Value {
        json!({
            "presence": { "pageVisible": true },
            "perception": [
                { "sourceId": "page", "kind": "page" },
                { "sourceId": "surface", "kind": "surface" },
                { "sourceId": "pointer", "kind": "pointer" },
                { "sourceId": "music_track", "kind": "music" },
                { "sourceId": "screen", "kind": "screen" },
                { "sourceId": "voice", "kind": "voice" },
                { "sourceId": "presence", "kind": "presence" },
                { "sourceId": "odd", "kind": "telepathy" },
            ],
            "musicStatus": { "isPlaying": true },
        })
    }

    fn sources(body: &Value) -> Vec<&str> {
        body["perception"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["sourceId"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn a_child_tells_her_only_what_it_said_it_senses() {
        let none = Senses::of(&[]).admit(&report());
        assert_eq!(sources(&none), ["presence"]);
        assert!(none.get("musicStatus").is_none());
        assert_eq!(none["presence"], json!({ "pageVisible": true }));

        let caps: Vec<String> = ["perception.window", "perception.music", "voice.in"]
            .iter()
            .map(|c| c.to_string())
            .collect();
        let some = Senses::of(&caps).admit(&report());
        assert_eq!(
            sources(&some),
            ["page", "surface", "music_track", "voice", "presence"]
        );
        assert_eq!(some["musicStatus"], json!({ "isPlaying": true }));

        let all: Vec<String> = [
            "perception.window",
            "perception.pointer",
            "perception.music",
            "perception.screen",
            "voice.in",
        ]
        .iter()
        .map(|c| c.to_string())
        .collect();
        // A kind no one knows never passes.
        assert_eq!(sources(&Senses::of(&all).admit(&report())).len(), 7);
    }

    #[test]
    fn a_look_at_the_screen_says_only_what_the_contract_lets_it() {
        let screen: Vec<String> = vec!["perception.screen".into()];
        let senses = Senses::of(&screen);
        let body = json!({
            "presence": { "pageVisible": true },
            "perception": [
                { "sourceId": "screen_look", "kind": "screen", "privacy": "consented",
                  "summary": "Xcode · Pen.swift",
                  "safeFacts": { "app": "Xcode", "screen_text": "let pen = Pen()",
                                 "seen_at": "2026-11-16T03:00:00Z", "clipboard": "secret" } },
                { "sourceId": "screen_chat", "kind": "screen", "privacy": "consented",
                  "summary": "Bob wrote about dinner\nWhat other people wrote there: see you at 7",
                  "safeFacts": { "who": "Bob", "topic": "dinner", "category": "chat",
                                 "others_text": "see you at 7", "screen_text": "see you at 7" } },
            ],
        });
        let kept = senses.admit(&body);
        let rows = kept["perception"].as_array().unwrap();
        // Another app: its filtered text stays, a key the list does not name goes.
        assert_eq!(
            rows[0]["safeFacts"],
            json!({ "app": "Xcode", "screen_text": "let pen = Pen()", "seen_at": "2026-11-16T03:00:00Z" })
        );
        assert_eq!(rows[0]["summary"], "Xcode · Pen.swift");
        // A chat window: who and about what, never what anyone else wrote.
        assert_eq!(
            rows[1]["safeFacts"],
            json!({ "who": "Bob", "topic": "dinner", "category": "chat" })
        );
        assert_eq!(rows[1]["summary"], "Bob wrote about dinner");
        let deleted = super::super::counts::screen_keys();
        for key in ["clipboard", "others_text", "screen_text", "summary"] {
            assert!(
                deleted.iter().any(|(t, k, _)| *t == "presence" && k == key),
                "{key}"
            );
        }
    }

    #[test]
    fn a_screen_row_of_another_shape_loses_what_it_cannot_say() {
        let senses = Senses::of(&["perception.screen".to_string()]);
        let body = json!({ "perception": [
            { "sourceId": "screen_look", "kind": "screen",
              "summary": { "said": "Ann: see you at 7" },
              "safeFacts": "Ann: see you at 7" },
        ]});
        let kept = senses.admit(&body);
        let row = &kept["perception"][0];
        assert_eq!(row["sourceId"], "screen_look");
        assert!(row.get("safeFacts").is_none());
        assert!(row.get("summary").is_none());
        let keys = super::super::counts::screen_keys();
        assert!(
            keys.iter()
                .any(|(t, key, _)| *t == "presence" && key == "safeFacts")
        );
    }

    #[test]
    fn a_chat_is_a_chat_by_who_as_it_came_even_where_who_cannot_stay() {
        // Who of another shape is not said, and the window is still a chat:
        // what anyone else wrote there is not read.
        let look = look_result(&json!({
            "facts": { "who": ["Ann"], "screen_text": "Ann: see you at 7", "app": "Messages" },
            "summary": "Ann\u{2028}see you at 7",
        }))
        .unwrap();
        assert_eq!(look["facts"], json!({ "app": "Messages" }));
        assert_eq!(look["summary"], "Ann");
        // Any line break ends the summary, and it is never longer than text is.
        let max_chars = super::super::contract::CONTRACT.limits.max_text_chars;
        for (summary, kept) in [("Ann\rsee you", "Ann"), ("Ann\r\nsee you", "Ann")] {
            let look = look_result(&json!({ "facts": {}, "summary": summary })).unwrap();
            assert_eq!(look["summary"], kept);
        }
        let long = "x".repeat(max_chars + 5);
        let look = look_result(&json!({ "facts": {}, "summary": long })).unwrap();
        assert_eq!(look["summary"].as_str().unwrap().chars().count(), max_chars);
        // The same on what a child senses there.
        let senses = Senses::of(&["perception.screen".to_string()]);
        let kept = senses.admit(&json!({ "perception": [
            { "sourceId": "screen_chat", "kind": "screen",
              "summary": "Ann wrote\rsee you at 7",
              "safeFacts": { "who": "Ann", "screen_text": "see you at 7" } },
        ]}));
        let row = &kept["perception"][0];
        assert_eq!(row["safeFacts"], json!({ "who": "Ann" }));
        assert_eq!(row["summary"], "Ann wrote");
    }

    #[test]
    fn a_body_without_observations_passes_as_it_is() {
        let body = json!({ "presence": { "pageVisible": false } });
        assert_eq!(Senses::default().admit(&body), body);
    }
}
