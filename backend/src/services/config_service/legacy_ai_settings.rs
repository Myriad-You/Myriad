//! One-way upgrade from the old AI settings. Each text tier used to carry a
//! provider, two model names (Gemini and OpenAI-compatible), its own key and
//! endpoint, and Lite an on/off switch; the judgment and embedding models
//! were named after Lite. Images and speech each had a provider beside their
//! source, and keys and endpoints of their own. Now a tier, images and
//! speech are each a source (plus their models), and keys live in the
//! shared vault.
//!
//! Runs on stored settings at startup, before defaults are seeded
//! (`db::schema_check`), and on a settings backup being restored
//! (`api::config::backup`). Delete once no instance or kept backup predates
//! it.

use std::collections::HashMap;

use serde_json::{Value as JsonValue, json};

/// The text tiers' old keys.
const TIER_KEYS: &[&str] = &[
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

/// Images' and speech's old keys.
const SERVICE_KEYS: &[&str] = &[
    "ai_image_provider",
    "ai_image_openai_api_key",
    "ai_image_openai_base_url",
    "ai_image_openrouter_api_key",
    "ai_image_volcengine_api_key",
    "ai_image_volcengine_base_url",
    "speech_provider",
    "speech_reuse_text_credentials",
    "speech_openai_api_key",
    "speech_openai_base_url",
    "speech_openrouter_api_key",
];

/// Every old key. None of them is stored after the upgrade.
pub(crate) fn legacy_keys() -> impl Iterator<Item = &'static str> {
    TIER_KEYS.iter().chain(SERVICE_KEYS).copied()
}

pub(crate) fn is_legacy_key(key: &str) -> bool {
    legacy_keys().any(|old| old == key)
}

const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
const OPENAI_BASE: &str = "https://api.openai.com/v1";
const VOLCENGINE_BASE: &str = "https://ark.cn-beijing.volces.com/api/v3";

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

/// A shared OpenAI endpoint has to be there and not OpenRouter's.
fn usable_openai_base(url: &str) -> bool {
    !url.trim().is_empty() && !is_openrouter(url)
}

/// Read as the old parser did: the stored string, else the old default.
fn text(stored: &HashMap<String, JsonValue>, key: &str, default: &str) -> String {
    stored
        .get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or(default)
        .to_string()
}

/// The stored string, trimmed, when there is one.
fn filled(stored: &HashMap<String, JsonValue>, key: &str) -> Option<String> {
    stored
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn blank(stored: &HashMap<String, JsonValue>, key: &str) -> bool {
    stored
        .get(key)
        .and_then(JsonValue::as_str)
        .is_none_or(|value| value.trim().is_empty())
}

/// What the old settings in `stored` come to in the new keys. Values a
/// key already holds in `stored` are kept, except a blank source. Empty
/// when nothing old is there. The caller removes [`legacy_keys`].
pub(crate) fn upgrade(stored: &HashMap<String, JsonValue>) -> HashMap<String, JsonValue> {
    let mut out = HashMap::new();
    if TIER_KEYS.iter().any(|key| stored.contains_key(*key)) {
        upgrade_tiers(stored, &mut out);
    }
    if SERVICE_KEYS.iter().any(|key| stored.contains_key(*key)) {
        // After the tiers: a vault key they filled is already there.
        let mut seen = stored.clone();
        seen.extend(out.clone());
        upgrade_services(&seen, &mut out);
    }
    out
}

fn upgrade_tiers(stored: &HashMap<String, JsonValue>, out: &mut HashMap<String, JsonValue>) {
    let text = |key: &str, default: &str| text(stored, key, default);
    let filled = |key: &str| filled(stored, key);
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
        fill(out, format!("{prefix}ai_model"), json!(model.trim()));

        // A blank source was inferred from the provider and the endpoint
        // (the tier's own, else Standard's); now it is written down.
        let source_key = format!("{prefix}ai_source");
        if blank(stored, &source_key) {
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
                filled(&format!("{prefix}openai_api_key")),
            )
        })
        .collect();
    let mut fold_key = |vault: &str, before: &[&str], legacy: Option<String>| {
        let unset = |key: &&str| filled(key).is_none();
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
        .find_map(filled),
    );
    // Standard's endpoint was the last fallback of the shared OpenAI one.
    let shared_base_set = usable_openai_base(&text("provider_openai_base_url", OPENAI_BASE))
        || ["speech_openai_base_url", "ai_image_openai_base_url"]
            .into_iter()
            .any(|key| usable_openai_base(&text(key, OPENAI_BASE)));
    if !shared_base_set && usable_openai_base(&standard_base) {
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
            fill(out, new.to_string(), json!(text(old, "").trim()));
        }
    }
}

fn upgrade_services(stored: &HashMap<String, JsonValue>, out: &mut HashMap<String, JsonValue>) {
    // A blank source went by the provider, a built-in slug's name.
    for (source_key, provider_key, factory) in [
        ("ai_image_source", "ai_image_provider", "openrouter"),
        ("speech_source", "speech_provider", "tencent"),
    ] {
        if stored.contains_key(provider_key) && blank(stored, source_key) {
            let provider = text(stored, provider_key, factory);
            let slug = match provider.trim() {
                "" => factory,
                slug => slug,
            };
            out.insert(source_key.to_string(), json!(slug));
        }
    }

    // Their keys came after the vault's: they counted only when it was empty.
    for (vault, own) in [
        (
            "provider_openrouter_api_key",
            &["speech_openrouter_api_key", "ai_image_openrouter_api_key"][..],
        ),
        (
            "provider_openai_api_key",
            &["speech_openai_api_key", "ai_image_openai_api_key"][..],
        ),
        (
            "provider_volcengine_api_key",
            &["ai_image_volcengine_api_key"][..],
        ),
    ] {
        if filled(stored, vault).is_none()
            && let Some(key) = own.iter().find_map(|key| filled(stored, key))
        {
            out.insert(vault.to_string(), json!(key));
        }
    }
    // And their endpoints after the vault's.
    if !usable_openai_base(&text(stored, "provider_openai_base_url", OPENAI_BASE))
        && let Some(base) = ["speech_openai_base_url", "ai_image_openai_base_url"]
            .into_iter()
            .map(|key| text(stored, key, OPENAI_BASE))
            .find(|base| usable_openai_base(base))
    {
        out.insert("provider_openai_base_url".to_string(), json!(base.trim()));
    }
    let vault_volcengine = text(stored, "provider_volcengine_base_url", VOLCENGINE_BASE);
    if vault_volcengine.trim().is_empty()
        && let Some(base) = filled(stored, "ai_image_volcengine_base_url")
    {
        out.insert("provider_volcengine_base_url".to_string(), json!(base));
    }
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
        // Only service keys left: the tiers' choices are not touched.
        let out = upgrade(&stored(&[
            ("lite_ai_source", json!("")),
            ("speech_provider", json!("tencent")),
            ("speech_source", json!("tencent")),
        ]));
        assert!(out.is_empty(), "{out:?}");
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
        // The speech key was the one in use, so it is the one kept.
        assert_eq!(kept["provider_openai_api_key"], json!("sk-speech"));
    }

    #[test]
    fn images_and_speech_become_their_source_and_their_keys_the_vaults() {
        let out = upgrade(&stored(&[
            ("ai_image_provider", json!("volcengine")),
            ("ai_image_source", json!("")),
            ("ai_image_volcengine_api_key", json!("volc-key")),
            (
                "ai_image_volcengine_base_url",
                json!("https://ark.example/v3"),
            ),
            ("provider_volcengine_base_url", json!("")),
            ("speech_provider", json!("openai")),
            ("speech_source", json!("")),
            ("speech_openai_api_key", json!("sk-speech")),
            ("ai_image_openai_api_key", json!("sk-image")),
            ("speech_openai_base_url", json!("https://speech.example/v1")),
            (
                "provider_openai_base_url",
                json!("https://openrouter.ai/api/v1"),
            ),
            ("speech_reuse_text_credentials", json!(true)),
        ]));
        assert_eq!(out["ai_image_source"], json!("volcengine"));
        assert_eq!(out["speech_source"], json!("openai"));
        assert_eq!(out["provider_volcengine_api_key"], json!("volc-key"));
        assert_eq!(
            out["provider_volcengine_base_url"],
            json!("https://ark.example/v3")
        );
        assert_eq!(out["provider_openai_api_key"], json!("sk-speech"));
        assert_eq!(
            out["provider_openai_base_url"],
            json!("https://speech.example/v1")
        );
        assert!(is_legacy_key("speech_reuse_text_credentials"));

        // A chosen source and a vault already set stay as they are.
        let kept = upgrade(&stored(&[
            ("speech_provider", json!("openai")),
            ("speech_source", json!("work-openai")),
            ("speech_openai_api_key", json!("sk-speech")),
            ("provider_openai_api_key", json!("sk-vault")),
        ]));
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn a_vault_key_the_tiers_filled_is_not_filled_again() {
        let out = upgrade(&stored(&[
            ("openai_base_url", json!("https://openrouter.ai/api/v1")),
            ("openai_api_key", json!("sk-or-standard")),
            ("ai_image_provider", json!("openrouter")),
        ]));
        assert_eq!(out["provider_openrouter_api_key"], json!("sk-or-standard"));
        assert_eq!(out["ai_image_source"], json!("openrouter"));
    }

    #[test]
    fn standards_endpoint_becomes_the_shared_one_when_nothing_else_was() {
        // The speech and image endpoints came first and were the official
        // one unless cleared.
        let out = upgrade(&stored(&[
            ("openai_base_url", json!("https://llm.example.com/v1")),
            ("provider_openai_base_url", json!("")),
            ("speech_openai_base_url", json!("")),
            ("ai_image_openai_base_url", json!("")),
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
        let behind_speech = upgrade(&stored(&[
            ("openai_base_url", json!("https://llm.example.com/v1")),
            ("provider_openai_base_url", json!("")),
        ]));
        assert!(!behind_speech.contains_key("provider_openai_base_url"));
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
