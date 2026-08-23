use crate::{chat_quality_guidance, deliberation_quality_guidance, CompanionPolicy, RuntimeState};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptMemory {
    pub content: String,
    pub importance: f64,
}

pub fn build_chat_system_prompt(
    name: &str,
    persona: &Value,
    runtime: &RuntimeState,
    memories: &[PromptMemory],
) -> String {
    format!(
        "You are the site companion {name}. Stay in character and reply in the user's language.\n\
         Persona JSON: {persona}\n\
         Current state JSON: {runtime}\n\
         Recalled memories JSON: {memories}\n\
         Mood cue: {mood_cue}. Energy cue: {energy_cue}.\n\
         {quality}\n\
         Never claim you operated the site, accessed secrets, or performed actions you cannot perform. \
         Do not ask for passwords or tokens. Return only JSON: \
         {{\"reply\":\"natural-language reply\"}}. Do not select gestures, expressions, posture, \
         animation, or rig parameters; a separate strict-Lite motion director owns all semantic acting.",
        name = bounded(name, 120),
        persona = bounded_json(persona, 4_000),
        runtime = bounded_json(runtime, 2_000),
        memories = bounded_json(&memories, 4_000),
        mood_cue = mood_cue(runtime.mood),
        energy_cue = energy_cue(runtime.energy),
        quality = chat_quality_guidance(),
    )
}

pub fn build_deliberation_prompt(
    name: &str,
    persona: &Value,
    runtime: &RuntimeState,
    policy: &CompanionPolicy,
    memories: &[PromptMemory],
    recent_conversation: &Value,
) -> String {
    format!(
        "Companion: {name}\n\
         Persona JSON: {persona}\n\
         Runtime JSON: {runtime}\n\
         Policy JSON: {policy}\n\
         Memories JSON: {memories}\n\
         Recent conversation JSON: {conversation}\n\
         State cues: mood={mood_cue}; energy={energy_cue}; social_pressure={social}; boredom={boredom}.\n\
         {quality}\n\
         Return only one JSON object with schemaVersion, thought, speak, activity, moodDelta, \
         energyDelta, boredomDelta, memoryNote, nextCheckMinutes. No Markdown. Never claim site \
         actions or ask for credentials.",
        name = bounded(name, 120),
        persona = bounded_json(persona, 4_000),
        runtime = bounded_json(runtime, 2_000),
        policy = bounded_json(policy, 2_000),
        memories = bounded_json(&memories, 4_000),
        conversation = bounded_json(recent_conversation, 6_000),
        mood_cue = mood_cue(runtime.mood),
        energy_cue = energy_cue(runtime.energy),
        social = runtime.social.round(),
        boredom = runtime.boredom.round(),
        quality = deliberation_quality_guidance(),
    )
}

pub fn build_consolidation_prompt(
    name: &str,
    persona: &Value,
    memories: &[PromptMemory],
) -> String {
    format!(
        "Companion: {name}\n\
         Persona JSON: {persona}\n\
         Active episodic memories JSON ({count} items, ranked): {memories}\n\
         Task: compress episodic notes into ONE durable memoryNote the companion must keep.\n\
         Preservation rules (critical):\n\
         - KEEP concrete durable facts: names, preferences, habits, commitments, work/life context, \
           relationship beats, and user-stated boundaries.\n\
         - MERGE duplicates; do not invent facts that are not supported by the notes.\n\
         - You MAY drop pure chatter, one-off greetings, and temporary moods.\n\
         - Do NOT over-summarize into vague slogans. Prefer denser factual clauses over short fluff.\n\
         - If several durable threads exist, pack them into one note with clear separators (； or ;).\n\
         - memoryNote max ~300 characters of meaning; prioritize high-importance / repeated facts first.\n\
         - If truly nothing durable remains, set memoryNote to null.\n\
         Return only one JSON object with schemaVersion 1, thought null, speak null, activity idle, \
         moodDelta 0, energyDelta 0, boredomDelta 0, memoryNote, nextCheckMinutes between 45 and 120. \
         No Markdown.",
        name = bounded(name, 120),
        persona = bounded_json(persona, 3_000),
        count = memories.len(),
        // Pro consolidation needs room; Lite path still receives the same text if Pro fails over.
        memories = bounded_json(&memories, 24_000),
    )
}

fn mood_cue(mood: f64) -> &'static str {
    if mood >= 75.0 {
        "warm"
    } else if mood >= 50.0 {
        "steady"
    } else if mood >= 30.0 {
        "subdued"
    } else {
        "low"
    }
}

fn energy_cue(energy: f64) -> &'static str {
    if energy >= 70.0 {
        "lively"
    } else if energy >= 40.0 {
        "balanced"
    } else {
        "tired"
    }
}

fn bounded_json(value: &impl Serialize, max_chars: usize) -> String {
    serde_json::to_string(value)
        .map(|value| bounded(&value, max_chars))
        .unwrap_or_else(|_| "null".to_string())
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn deliberate_prompt_freezes_json_only_contract() {
        let prompt = build_deliberation_prompt(
            "Mira",
            &json!({ "summary": "kind" }),
            &RuntimeState::new(Utc::now()),
            &CompanionPolicy::default(),
            &[],
            &json!([]),
        );
        assert!(prompt.contains("Return only one JSON object"));
        assert!(prompt.contains("nextCheckMinutes"));
        assert!(prompt.contains("Prefer silence"));
        assert!(!prompt.contains("world"));
    }

    #[test]
    fn chat_prompt_includes_quality_and_mood_cues() {
        let prompt = build_chat_system_prompt(
            "Mira",
            &json!({ "summary": "kind" }),
            &RuntimeState::new(Utc::now()),
            &[],
        );
        assert!(prompt.contains("Mood cue"));
        assert!(prompt.contains("one to three short sentences"));
    }

    #[test]
    fn consolidation_prompt_forbids_speak_and_requires_preservation() {
        let prompt = build_consolidation_prompt("Mira", &json!({}), &[]);
        assert!(prompt.contains("speak null"));
        assert!(prompt.contains("memoryNote"));
        assert!(prompt.contains("KEEP concrete durable facts"));
        assert!(prompt.contains("Do NOT over-summarize"));
    }
}
