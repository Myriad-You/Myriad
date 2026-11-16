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
fn keep_screen_fields(row: &mut Value) {
    let Some(row) = row.as_object_mut() else {
        return;
    };
    let mut chat = false;
    if let Some(facts) = row.get_mut("safeFacts").and_then(Value::as_object_mut) {
        facts.retain(|key, _| {
            let kept = super::counts::screen_fact(key);
            if !kept {
                super::counts::screen_key_deleted("presence", key);
            }
            kept
        });
        chat = facts.contains_key("who");
        if chat && facts.remove("screen_text").is_some() {
            super::counts::screen_key_deleted("presence", "screen_text");
        }
    }
    if chat
        && let Some(Value::String(summary)) = row.get_mut("summary")
        && let Some((first, _)) = summary.split_once('\n')
    {
        *summary = first.to_string();
        super::counts::screen_key_deleted("presence", "summary");
    }
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
    fn a_body_without_observations_passes_as_it_is() {
        let body = json!({ "presence": { "pageVisible": false } });
        assert_eq!(Senses::default().admit(&body), body);
    }
}
