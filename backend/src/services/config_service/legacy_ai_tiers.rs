//! One-way upgrade from the old text-model settings. Each tier used to
//! carry a provider, two model names (Gemini and OpenAI-compatible), its own
//! key and endpoint, and Lite an on/off switch; the judgment and embedding
//! models were named after Lite. Now a tier is a source and one model, and
//! keys live in the shared vault.
//!
//! Runs on stored settings at startup, before defaults are seeded
//! (`db::schema_check`), and on a settings backup being restored
//! (`api::config::backup`). Delete once no instance or kept backup predates
//! it.

use std::collections::HashMap;

use serde_json::{Value as JsonValue, json};

/// Every old key. None of them is stored after the upgrade.
pub(crate) const LEGACY_KEYS: &[&str] = &[
    "ai_provider",
    "gemini_api_key",
    "gemini_model",
    "openai_api_key",
    "openai_model",
    "openai_base_url",
    "lite_enabled",
    "lite_ai_provider",
    "lite_gemini_api_key",
    "lite_gemini_model",
    "lite_openai_api_key",
    "lite_openai_model",
    "lite_openai_base_url",
    "lite_judge_model",
    "lite_embedding_model",
    "pro_ai_provider",
    "pro_gemini_api_key",
    "pro_gemini_model",
    "pro_openai_api_key",
    "pro_openai_model",
    "pro_openai_base_url",
];

const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";

/// A tier's key prefix and its old factory model names (Gemini, OpenAI).
const TIERS: [(&str, &str, &str); 3] = [
    ("", "gemini-3.8-flash", "minimax/minimax-m3"),
    ("lite_", "", ""),
    (
        "pro_",
        "gemini-3.1-pro-preview",
        "anthropic/claude-opus-5.5",
    ),
];

fn is_openrouter(url: &str) -> bool {
    url.to_ascii_lowercase().contains("openrouter.ai")
}

/// What the old settings in `stored` come to in the new keys. Values a
/// key already holds in `stored` are kept, except a blank source. Empty
/// when nothing old is there. The caller removes [`LEGACY_KEYS`].
pub(crate) fn upgrade(stored: &HashMap<String, JsonValue>) -> HashMap<String, JsonValue> {
    let mut out = HashMap::new();
    if !LEGACY_KEYS.iter().any(|key| stored.contains_key(*key)) {
        return out;
    }
    // Read as the old parser did: a stored string, else the old default.
    let text = |key: &str, default: &str| {
        stored
            .get(key)
            .and_then(JsonValue::as_str)
            .unwrap_or(default)
            .to_string()
    };
    let secret = |key: &str| {
        stored
            .get(key)
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let fill = |out: &mut HashMap<String, JsonValue>, key: String, value: JsonValue| {
        if !stored.contains_key(&key) {
            out.insert(key, value);
        }
    };

    let lite_switched_on = stored.get("lite_enabled").is_some_and(|value| {
        value.as_bool() == Some(true) || matches!(value.as_str(), Some("true" | "1"))
    });
    let standard_base = text("openai_base_url", OPENROUTER_BASE);
    for (prefix, gemini_default, openai_default) in TIERS {
        let provider_key = if prefix.is_empty() {
            "ai_provider".to_string()
        } else {
            format!("{prefix}ai_provider")
        };
        let gemini = text(&provider_key, "openai") == "gemini";
        let mut model = if gemini {
            text(&format!("{prefix}gemini_model"), gemini_default)
        } else {
            text(&format!("{prefix}openai_model"), openai_default)
        };
        // Lite switched off stays unused: no model.
        if prefix == "lite_" && !lite_switched_on {
            model.clear();
        }
        fill(&mut out, format!("{prefix}ai_model"), json!(model.trim()));

        // A blank source was inferred from the provider and the endpoint
        // (the tier's own, else Standard's); now it is written down.
        let source_key = format!("{prefix}ai_source");
        let blank_source = stored
            .get(&source_key)
            .and_then(JsonValue::as_str)
            .is_none_or(|slug| slug.trim().is_empty());
        if blank_source {
            let own_base = text(&format!("{prefix}openai_base_url"), OPENROUTER_BASE);
            let base = if own_base.trim().is_empty() {
                &standard_base
            } else {
                &own_base
            };
            let slug = if gemini {
                "gemini"
            } else if is_openrouter(base) {
                "openrouter"
            } else {
                "openai"
            };
            out.insert(source_key, json!(slug));
        }
    }

    // Tier keys were the last fallbacks of the shared keys: they only ever
    // counted when none before them was set, and then the first one did.
    let tier_openai_keys: Vec<(bool, Option<String>)> = ["", "lite_", "pro_"]
        .into_iter()
        .map(|prefix| {
            (
                is_openrouter(&text(&format!("{prefix}openai_base_url"), OPENROUTER_BASE)),
                secret(&format!("{prefix}openai_api_key")),
            )
        })
        .collect();
    let mut fold_key = |vault: &str, before: &[&str], legacy: Option<String>| {
        let unset = |key: &&str| secret(key).is_none();
        if unset(&vault)
            && before.iter().all(unset)
            && let Some(key) = legacy
        {
            out.insert(vault.to_string(), json!(key));
        }
    };
    fold_key(
        "provider_openrouter_api_key",
        &["speech_openrouter_api_key", "ai_image_openrouter_api_key"],
        tier_openai_keys
            .iter()
            .filter(|(openrouter, _)| *openrouter)
            .find_map(|(_, key)| key.clone()),
    );
    fold_key(
        "provider_openai_api_key",
        &["speech_openai_api_key", "ai_image_openai_api_key"],
        tier_openai_keys
            .iter()
            .filter(|(openrouter, _)| !*openrouter)
            .find_map(|(_, key)| key.clone()),
    );
    fold_key(
        "provider_gemini_api_key",
        &[],
        [
            "gemini_api_key",
            "lite_gemini_api_key",
            "pro_gemini_api_key",
        ]
        .into_iter()
        .find_map(secret),
    );
    // Standard's endpoint was the last fallback of the shared OpenAI one.
    let usable = |url: &str| !url.trim().is_empty() && !is_openrouter(url);
    let shared_base_set = [
        "provider_openai_base_url",
        "speech_openai_base_url",
        "ai_image_openai_base_url",
    ]
    .into_iter()
    .any(|key| usable(&text(key, "")));
    if !shared_base_set && usable(&standard_base) {
        out.insert(
            "provider_openai_base_url".to_string(),
            json!(standard_base.trim()),
        );
    }

    // The judgment and embedding models, named after their own option.
    for (old, new) in [
        ("lite_judge_model", "aux_judge_model"),
        ("lite_embedding_model", "aux_embedding_model"),
    ] {
        if stored.contains_key(old) {
            fill(&mut out, new.to_string(), json!(text(old, "").trim()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(pairs: &[(&str, JsonValue)]) -> HashMap<String, JsonValue> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn nothing_old_nothing_to_do() {
        assert!(upgrade(&stored(&[("ai_model", json!("m"))])).is_empty());
    }

    #[test]
    fn each_tier_becomes_its_source_and_the_model_its_provider_used() {
        let out = upgrade(&stored(&[
            ("ai_provider", json!("openai")),
            ("openai_model", json!("minimax/minimax-m3")),
            ("gemini_model", json!("gemini-unused")),
            ("openai_base_url", json!("https://openrouter.ai/api/v1")),
            ("ai_source", json!("")),
            ("lite_enabled", json!(true)),
            ("lite_ai_provider", json!("openai")),
            ("lite_openai_model", json!(" qwen3.8-omni-flash ")),
            (
                "lite_openai_base_url",
                json!("https://dashscope-intl.aliyuncs.com/v1"),
            ),
            ("lite_ai_source", json!("dashscope")),
            ("pro_ai_provider", json!("gemini")),
            ("pro_gemini_model", json!("gemini-pro")),
            ("pro_openai_model", json!("unused")),
        ]));
        assert_eq!(out["ai_model"], json!("minimax/minimax-m3"));
        assert_eq!(out["ai_source"], json!("openrouter"));
        assert_eq!(out["lite_ai_model"], json!("qwen3.8-omni-flash"));
        assert!(!out.contains_key("lite_ai_source"), "chosen source stays");
        assert_eq!(out["pro_ai_model"], json!("gemini-pro"));
        assert_eq!(out["pro_ai_source"], json!("gemini"));
    }

    #[test]
    fn a_blank_tier_endpoint_was_standards() {
        let out = upgrade(&stored(&[
            ("openai_base_url", json!("https://api.openai.com/v1")),
            ("lite_openai_base_url", json!("")),
            ("pro_openai_base_url", json!("https://openrouter.ai/api/v1")),
        ]));
        assert_eq!(out["ai_source"], json!("openai"));
        assert_eq!(out["lite_ai_source"], json!("openai"));
        assert_eq!(out["pro_ai_source"], json!("openrouter"));
    }

    #[test]
    fn lite_left_switched_off_stays_unused() {
        for switch in [None, Some(json!(false)), Some(json!("false"))] {
            let mut old = stored(&[("lite_openai_model", json!("lite/model"))]);
            if let Some(switch) = switch {
                old.insert("lite_enabled".into(), switch);
            }
            assert_eq!(upgrade(&old)["lite_ai_model"], json!(""));
        }
        let on = upgrade(&stored(&[
            ("lite_enabled", json!("true")),
            ("lite_openai_model", json!("lite/model")),
        ]));
        assert_eq!(on["lite_ai_model"], json!("lite/model"));
    }

    #[test]
    fn tier_keys_move_into_the_vault_only_where_they_were_in_use() {
        let out = upgrade(&stored(&[
            ("openai_base_url", json!("https://openrouter.ai/api/v1")),
            ("openai_api_key", json!("sk-or-standard")),
            ("lite_openai_base_url", json!("https://api.openai.com/v1")),
            ("lite_openai_api_key", json!("sk-oa-lite")),
            ("pro_gemini_api_key", json!("AIza-pro")),
            ("provider_openrouter_api_key", json!("")),
        ]));
        assert_eq!(out["provider_openrouter_api_key"], json!("sk-or-standard"));
        assert_eq!(out["provider_openai_api_key"], json!("sk-oa-lite"));
        assert_eq!(out["provider_gemini_api_key"], json!("AIza-pro"));

        // A vault key, or one before the tier keys, already won.
        let kept = upgrade(&stored(&[
            ("openai_api_key", json!("sk-or-standard")),
            ("provider_openrouter_api_key", json!("sk-vault")),
            ("lite_openai_base_url", json!("https://api.openai.com/v1")),
            ("lite_openai_api_key", json!("sk-oa-lite")),
            ("speech_openai_api_key", json!("sk-speech")),
        ]));
        assert!(!kept.contains_key("provider_openrouter_api_key"));
        assert!(!kept.contains_key("provider_openai_api_key"));
    }

    #[test]
    fn standards_endpoint_becomes_the_shared_one_when_nothing_else_was() {
        let out = upgrade(&stored(&[
            ("openai_base_url", json!("https://llm.example.com/v1")),
            ("provider_openai_base_url", json!("")),
        ]));
        assert_eq!(
            out["provider_openai_base_url"],
            json!("https://llm.example.com/v1")
        );
        let kept = upgrade(&stored(&[
            ("openai_base_url", json!("https://llm.example.com/v1")),
            (
                "provider_openai_base_url",
                json!("https://api.openai.com/v1"),
            ),
        ]));
        assert!(!kept.contains_key("provider_openai_base_url"));
    }

    #[test]
    fn judgment_and_embedding_models_take_their_own_names() {
        let out = upgrade(&stored(&[
            ("lite_judge_model", json!("openai/gpt-6-luna")),
            (
                "lite_embedding_model",
                json!("perplexity/pplx-embed-v1-0.6b"),
            ),
        ]));
        assert_eq!(out["aux_judge_model"], json!("openai/gpt-6-luna"));
        assert_eq!(
            out["aux_embedding_model"],
            json!("perplexity/pplx-embed-v1-0.6b")
        );
        // Already there (a later version wrote it): kept.
        let kept = upgrade(&stored(&[
            ("lite_judge_model", json!("old")),
            ("aux_judge_model", json!("new")),
        ]));
        assert!(!kept.contains_key("aux_judge_model"));
    }
}
