//! Prompt string shaping. No I/O.

use crate::SANITIZE_PROMPT_MAX_CHARS;

/// Sanitize user text for model prompts (drop control chars except newline, cap length).
pub fn sanitize_prompt_input(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(SANITIZE_PROMPT_MAX_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Merge role identity text into systemPrompt (pure string combine).
pub fn merge_system_prompt(existing: &str, addition: &str) -> String {
    if existing.is_empty() {
        addition.to_string()
    } else if addition.is_empty() {
        existing.to_string()
    } else {
        format!("{}\n\n{}", addition, existing)
    }
}

/// Append memory reference block onto systemPrompt.
pub fn append_memory_to_system_prompt(existing: &str, memory: &str) -> String {
    if existing.is_empty() {
        format!("参考记忆（仅供参考，不要照搬）：\n{memory}")
    } else {
        format!("{existing}\n\n参考记忆（仅供参考，不要照搬）：\n{memory}")
    }
}

/// Take the last N conversation messages (oldest-first order preserved).
pub fn take_recent_conversation_messages<T: Clone>(history: &[T], max: usize) -> Vec<T> {
    history
        .iter()
        .rev()
        .take(max)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_drops_controls_and_caps_length() {
        let dirty = "hello\x00\nworld\t";
        let clean = sanitize_prompt_input(dirty);
        assert!(!clean.contains('\0'));
        assert!(!clean.contains('\t'));
        assert!(clean.contains('\n'));
        assert_eq!(
            sanitize_prompt_input(&"a".repeat(SANITIZE_PROMPT_MAX_CHARS + 50))
                .chars()
                .count(),
            SANITIZE_PROMPT_MAX_CHARS
        );
    }

    #[test]
    fn merge_and_memory_and_history_window() {
        assert_eq!(merge_system_prompt("", "role"), "role");
        assert_eq!(merge_system_prompt("keep", ""), "keep");
        assert!(append_memory_to_system_prompt("base", "mem").contains("参考记忆"));
        let msgs = vec![1, 2, 3, 4, 5];
        assert_eq!(take_recent_conversation_messages(&msgs, 3), vec![3, 4, 5]);
        assert_eq!(take_recent_conversation_messages(&msgs, 10), msgs);
    }
}
