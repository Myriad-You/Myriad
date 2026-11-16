//! What she is given to say unasked and how she puts it: her soul, the
//! sections a chat turn wears and what she already said unasked, asked of
//! her model (or her own voice), and the line kept as she wrote it.

use sea_orm::DatabaseConnection;

use super::is_trivial_line;
use crate::services::agent::consciousness::SpeakIntent;

/// What she is given to say `gist` (about `happened`) unasked: the system
/// (her soul, the sections a chat turn wears, what she already said unasked)
/// and the line asking for it.
pub(crate) async fn proactive_prompt(
    db: &DatabaseConnection,
    user_id: i32,
    gist: &str,
    happened: Option<&str>,
) -> (String, String) {
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_else(|| "You are Agent.".to_string());
    let mut sections =
        crate::services::agent::merope::speaking_prompt_for_event(db, user_id, gist).await;
    // How the last exchange left her, if it was a moment ago.
    sections.extend(
        super::super::inner::current(
            user_id,
            &crate::services::agent::memory::unified::Audience::private(user_id),
        )
        .as_deref()
        .and_then(crate::services::agent::merope::format_inner_moment_ago_section),
    );
    let mind = crate::services::agent::merope::speaking_prompt_plain(&sections);
    // What she already said unasked, as her decision saw it.
    let recent_block = super::said_lately_block(db, user_id).await;
    let system =
        super::super::speaking_prompts::compose_proactive_system(&soul, &mind, &recent_block);
    let prompt = super::super::speaking_prompts::compose_proactive_user(gist, happened);
    (system, prompt)
}

/// The line she says for `intent` (what she means, about what happened), as
/// she would put it; with `voice`, in her own voice, its sound sent as it is
/// made.
pub(super) async fn compose_line(
    db: &DatabaseConnection,
    intent: &SpeakIntent,
    voice: Option<(
        &crate::config::OmniVoice,
        &crate::services::omni_voice::VoiceOut,
    )>,
) -> Option<String> {
    let user_id = intent.user_id;
    let model = super::super::call::Ask::new(super::super::call::Voice::Hers, user_id, "speak")
        .within(std::time::Duration::from_secs(30))
        .model()
        .await
        .ok()?;
    let (system, prompt) =
        proactive_prompt(db, user_id, &intent.gist, intent.happened.as_deref()).await;
    let made = match voice {
        Some((omni, out)) => {
            let aloud = crate::services::omni_voice::said_aloud(&format!("{system}\n\n{prompt}"));
            let mut unsaid = crate::services::omni_voice::Unsaid::default();
            let speaking = crate::services::omni_voice::speak(omni, &aloud, &[], None, |piece| {
                match piece {
                    crate::services::omni_voice::OmniDelta::Text(text) => unsaid.written(&text),
                    crate::services::omni_voice::OmniDelta::Audio(sound) => {
                        if unsaid.lets_through() {
                            out.send(&sound);
                        }
                    }
                }
                async { true }
            });
            // Billed and bounded as any line of hers is (see `call::Model`).
            match tokio::time::timeout(
                std::time::Duration::from_secs(45),
                crate::services::ai_cost_ledger::with_site_ai_ledger(
                    user_id, "merope", "speak", speaking,
                ),
            )
            .await
            {
                Ok(made) => made.map_err(|error| error.to_string()),
                Err(_) => Err("her own voice ran out of time".to_string()),
            }
        }
        None => model
            .text(&system, &prompt)
            .await
            .map_err(|error| error.to_string()),
    };
    match made {
        Ok(raw) => {
            let spoken = sanitize_speech(&raw);
            (!is_trivial_line(&spoken)).then_some(spoken)
        }
        Err(error) => {
            tracing::debug!(%error, "[Merope] Lite speech failed; saying nothing");
            None
        }
    }
}

pub(super) fn sanitize_speech(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    if text.starts_with("```") {
        text = text
            .lines()
            .skip(1)
            .take_while(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join(" ");
    }
    let text = text
        .trim()
        .trim_matches(|c| c == '"' || c == '“' || c == '”')
        .trim();
    // All she said, a line a sentence as she wrote it: she is asked for one
    // or two, and what she says is hers to keep whole.
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    lines.join("\n").chars().take(SPEECH_CHARS).collect()
}

/// Bounds what is kept of a line she says unprompted, against a runaway
/// answer; far past the one or two sentences she is asked for.
const SPEECH_CHARS: usize = 600;

#[cfg(test)]
mod tests {
    #[test]
    fn what_she_says_unprompted_is_kept_whole() {
        assert_eq!(
            super::sanitize_speech("\"刚听完那首。\n\n副歌那段还在脑子里转。\""),
            "刚听完那首。\n副歌那段还在脑子里转。"
        );
        assert_eq!(super::sanitize_speech("```\n就一句\n```"), "就一句");
    }
}
