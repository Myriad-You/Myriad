//! Her speaking prompts live in `myriad_merope::speaking`: pure text, no
//! storage. Re-exported here; only reading her persona row is the backend's.

use crate::models::entities::agent_persona;

pub use myriad_merope::speaking::*;

/// [`myriad_merope::speaking::format_persona`] for her stored persona.
pub fn format_persona(persona: &agent_persona::Model) -> Option<String> {
    myriad_merope::speaking::format_persona(&persona.name, &persona.personality)
}

#[cfg(test)]
mod whole_prompt {
    use myriad_merope::consistency::{
        PROMPT_RECORDING_VERSION, PromptMessage, PromptRecording, prompt_diff,
    };
    use myriad_merope::personality::flatten_saved;
    use myriad_merope::speaking::{Home, format_persona, site_line, soul};

    fn recording(end: &str, soul: String) -> PromptRecording {
        PromptRecording {
            version: PROMPT_RECORDING_VERSION,
            scene: "private/soul".into(),
            end: end.into(),
            core: None,
            input: serde_json::json!({ "name": "瞳", "site": "星海" }),
            messages: vec![PromptMessage {
                role: "system".into(),
                text: soul,
            }],
        }
    }

    /// Her soul as the site put it together before (the page's flattened
    /// personality, the contract, the site's name), set beside the core's
    /// one soul: the same word for word, but for her personality's labels,
    /// now in her own language rather than the page reader's.
    #[test]
    fn the_site_soul_changes_only_where_her_personality_is_labelled_in_her_language() {
        let saved = serde_json::json!({
            "language": "ja-JP",
            "summary": "話は短い。",
            "temperament": ["落ち着いている"],
            "speechStyle": "先に答える。"
        });
        let before = |page: &str| {
            format!(
                "{}\n\n{}",
                format_persona("瞳", page).unwrap(),
                site_line("星海")
            )
        };
        let now = soul("瞳", &flatten_saved(&saved, "ja-JP"), &Home::site("星海")).unwrap();
        // Read on a page in her language: nothing differs.
        let same_page = "気質：落ち着いている\n話し方：先に答える。\n\n話は短い。";
        let diff = prompt_diff(
            &recording("site-before", before(same_page)),
            &recording("site", now.clone()),
        );
        assert!(diff.same_scene && diff.same_input);
        assert!(diff.differ.is_empty(), "{diff:#?}");
        // Read on a Chinese page: only the labelled lines differ.
        let zh_page = "气质：落ち着いている\n表达：先に答える。\n\n話は短い。";
        let diff = prompt_diff(
            &recording("site-before", before(zh_page)),
            &recording("site", now),
        );
        println!("{}", serde_json::to_string_pretty(&diff).unwrap());
        let changed: Vec<(&str, &str)> = diff
            .differ
            .iter()
            .map(|piece| {
                (
                    piece.left.as_deref().unwrap_or_default(),
                    piece.right.as_deref().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            changed
                .iter()
                .map(|(left, right)| format!("{left}{right}"))
                .collect::<String>(),
            "气质：落ち着いている\n表达：先に答える。気質：落ち着いている\n話し方：先に答える。"
        );
    }
}
