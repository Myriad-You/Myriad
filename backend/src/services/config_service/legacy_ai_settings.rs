//! One-way upgrade from the old AI settings. Each text tier used to carry a
//! provider, two model names (Gemini and OpenAI-compatible), its own key and
//! endpoint, and Lite an on/off switch; the judgment and embedding models
//! were named after Lite. Images and speech each had a provider beside their
//! source, and keys and endpoints of their own. Tencent Cloud's credentials
//! and the shared OpenAI and Volcengine endpoints had settings of their own
//! that no page edited. Now a tier, images and speech are each a source
//! (plus their models), keys live in the shared vault, and endpoints and
//! Tencent's credentials on the sources.
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

/// Tencent's credentials and the vault's endpoints, now on the sources.
const VAULT_KEYS: &[&str] = &[
    "tencent_secret_id",
    "tencent_secret_key",
    "tencent_region",
    "provider_openai_base_url",
    "provider_volcengine_base_url",
];

/// Every old key. None of them is stored after the upgrade.
pub(crate) fn legacy_keys() -> impl Iterator<Item = &'static str> {
    TIER_KEYS
        .iter()
        .chain(SERVICE_KEYS)
        .chain(VAULT_KEYS)
        .copied()
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
    // Each stage after the ones before: what they filled is already there.
    if SERVICE_KEYS.iter().any(|key| stored.contains_key(*key)) {
        let mut seen = stored.clone();
        seen.extend(out.clone());
        upgrade_services(&seen, &mut out);
    }
    let mut seen = stored.clone();
    seen.extend(out.clone());
    if VAULT_KEYS.iter().any(|key| seen.contains_key(*key)) {
        upgrade_vault(&seen, &mut out);
    }
    // A stage may fill an old key a later one moves on (the shared OpenAI
    // endpoint onto its sources); none is written back.
    out.retain(|key, _| !is_legacy_key(key));
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

/// Tencent's credentials and custom shared endpoints move onto the sources
/// that used them. With no source list stored, the list was synthesized
/// from the shared keys; it is written down as it was.
fn upgrade_vault(stored: &HashMap<String, JsonValue>, out: &mut HashMap<String, JsonValue>) {
    let openai_base = text(stored, "provider_openai_base_url", OPENAI_BASE);
    let openai_base = if usable_openai_base(&openai_base) {
        openai_base.trim().to_string()
    } else {
        OPENAI_BASE.to_string()
    };
    let volcengine_base = filled(stored, "provider_volcengine_base_url")
        .unwrap_or_else(|| VOLCENGINE_BASE.to_string());
    let tencent_id = filled(stored, "tencent_secret_id");
    let tencent_key = filled(stored, "tencent_secret_key");
    let tencent_region = filled(stored, "tencent_region");
    let custom_openai = openai_base != OPENAI_BASE;
    let custom_volcengine = volcengine_base != VOLCENGINE_BASE;
    let tencent = tencent_id.is_some() || tencent_key.is_some();
    if !custom_openai && !custom_volcengine && !tencent {
        return;
    }

    let mut sources: Vec<JsonValue> = match stored.get("ai_vendor_sources") {
        Some(JsonValue::Array(items)) => items.clone(),
        Some(JsonValue::String(raw)) => serde_json::from_str(raw).unwrap_or_default(),
        _ => Vec::new(),
    };
    let shared = |slug: &str, kind: &str, name: &str, base: &str| {
        serde_json::to_value(crate::config::AiVendorSource {
            slug: slug.to_string(),
            kind: kind.to_string(),
            display_name: name.to_string(),
            enabled: true,
            preset: slug.to_string(),
            api_format: if kind == "gemini" {
                "gemini".to_string()
            } else {
                String::new()
            },
            credential_mode: "shared".to_string(),
            shared_key_ref: Some(slug.to_string()),
            base_url: base.to_string(),
            ..Default::default()
        })
        .expect("a vendor source is JSON")
    };
    if sources.is_empty() {
        for (slug, kind, name, base) in [
            ("openrouter", "openrouter", "OpenRouter", OPENROUTER_BASE),
            ("openai", "openai", "OpenAI", openai_base.as_str()),
            ("gemini", "gemini", "Gemini", ""),
            (
                "volcengine",
                "volcengine",
                "Volcengine",
                volcengine_base.as_str(),
            ),
        ] {
            if filled(stored, &format!("provider_{slug}_api_key")).is_some() {
                sources.push(shared(slug, kind, name, base));
            }
        }
    } else {
        let kind_of = |source: &JsonValue| {
            source
                .get("kind")
                .and_then(JsonValue::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        };
        for source in &mut sources {
            let kind = kind_of(source);
            let Some(fields) = source.as_object_mut() else {
                continue;
            };
            let blank = |fields: &serde_json::Map<String, JsonValue>, key: &str| {
                fields
                    .get(key)
                    .and_then(JsonValue::as_str)
                    .is_none_or(|value| value.trim().is_empty())
            };
            // A source with no address of its own used the shared one.
            let base = match kind.as_str() {
                "openai" if custom_openai => Some(&openai_base),
                "volcengine" if custom_volcengine => Some(&volcengine_base),
                _ => None,
            };
            if let Some(base) = base
                && blank(fields, "base_url")
            {
                fields.insert("base_url".to_string(), json!(base));
            }
            // A Tencent source missing a credential used the setting's.
            if kind == "tencent" {
                for (key, value) in [
                    ("secret_id", &tencent_id),
                    ("secret_key", &tencent_key),
                    ("region", &tencent_region),
                ] {
                    if let Some(value) = value
                        && blank(fields, key)
                    {
                        fields.insert(key.to_string(), json!(value));
                    }
                }
            }
        }
    }
    let has_kind = |sources: &[JsonValue], kind: &str| {
        sources.iter().any(|source| {
            source
                .get("kind")
                .and_then(JsonValue::as_str)
                .is_some_and(|own| own.trim().eq_ignore_ascii_case(kind))
        })
    };
    // Tencent speech fell back to the setting with no Tencent source at all.
    if tencent && !has_kind(&sources, "tencent") {
        sources.push(
            serde_json::to_value(crate::config::AiVendorSource {
                slug: "tencent".to_string(),
                kind: "tencent".to_string(),
                display_name: "Tencent Cloud".to_string(),
                enabled: true,
                preset: "tencent".to_string(),
                secret_id: tencent_id.clone(),
                secret_key: tencent_key.clone(),
                region: Some(tencent_region.unwrap_or_else(|| "ap-guangzhou".to_string())),
                ..Default::default()
            })
            .expect("a vendor source is JSON"),
        );
    }
    // The built-in `openai` / `volcengine` slug with no row used the custom
    // shared endpoint too.
    for (slug, kind, name, base, custom) in [
        ("openai", "openai", "OpenAI", &openai_base, custom_openai),
        (
            "volcengine",
            "volcengine",
            "Volcengine",
            &volcengine_base,
            custom_volcengine,
        ),
    ] {
        let has_slug = sources
            .iter()
            .any(|source| source.get("slug").and_then(JsonValue::as_str) == Some(slug));
        if custom && !has_slug && filled(stored, &format!("provider_{slug}_api_key")).is_some() {
            sources.push(shared(slug, kind, name, base));
        }
    }
    out.insert("ai_vendor_sources".to_string(), JsonValue::Array(sources));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address the upgraded source list gives the source `slug`.
    fn source_base(out: &HashMap<String, JsonValue>, slug: &str) -> Option<String> {
        out.get("ai_vendor_sources")?
            .as_array()?
            .iter()
            .find(|source| source["slug"] == json!(slug))
            .and_then(|source| source["base_url"].as_str())
            .map(str::to_string)
    }

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
        assert_eq!(out["provider_openai_api_key"], json!("sk-speech"));
        // Their endpoints end up on the sources, written down from the keys.
        assert_eq!(
            source_base(&out, "volcengine").as_deref(),
            Some("https://ark.example/v3")
        );
        assert_eq!(
            source_base(&out, "openai").as_deref(),
            Some("https://speech.example/v1")
        );
        assert!(!out.contains_key("provider_openai_base_url"));
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
            ("provider_openai_api_key", json!("sk-oa")),
            ("speech_openai_base_url", json!("")),
            ("ai_image_openai_base_url", json!("")),
        ]));
        assert_eq!(
            source_base(&out, "openai").as_deref(),
            Some("https://llm.example.com/v1")
        );
        let kept = upgrade(&stored(&[
            ("openai_base_url", json!("https://llm.example.com/v1")),
            (
                "provider_openai_base_url",
                json!("https://api.openai.com/v1"),
            ),
        ]));
        assert!(!kept.contains_key("ai_vendor_sources"));
        let behind_speech = upgrade(&stored(&[
            ("openai_base_url", json!("https://llm.example.com/v1")),
            ("provider_openai_base_url", json!("")),
            ("provider_openai_api_key", json!("sk-oa")),
        ]));
        assert!(!behind_speech.contains_key("ai_vendor_sources"));
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
    #[test]
    fn tencent_credentials_and_custom_endpoints_move_onto_the_sources() {
        // No list stored: the synthesized one is written down, with Tencent.
        let out = upgrade(&stored(&[
            ("provider_openai_api_key", json!("sk-oa")),
            (
                "provider_openai_base_url",
                json!("https://llm.example.com/v1"),
            ),
            (
                "provider_volcengine_base_url",
                json!("https://ark.cn-beijing.volces.com/api/v3"),
            ),
            ("tencent_secret_id", json!("AKID")),
            ("tencent_secret_key", json!("tsecret")),
            ("tencent_region", json!("ap-shanghai")),
            ("ai_vendor_sources", json!([])),
        ]));
        let sources: Vec<crate::config::AiVendorSource> =
            serde_json::from_value(out["ai_vendor_sources"].clone()).unwrap();
        let openai = sources
            .iter()
            .find(|source| source.slug == "openai")
            .unwrap();
        assert_eq!(openai.base_url, "https://llm.example.com/v1");
        assert_eq!(openai.credential_mode, "shared");
        let tencent = sources
            .iter()
            .find(|source| source.kind == "tencent")
            .unwrap();
        assert_eq!(tencent.secret_id.as_deref(), Some("AKID"));
        assert_eq!(tencent.region.as_deref(), Some("ap-shanghai"));
        assert!(
            !sources.iter().any(|source| source.slug == "openrouter"),
            "no key, no source"
        );

        // A list stored: blanks are filled, chosen values stay.
        let out = upgrade(&stored(&[
            (
                "provider_openai_base_url",
                json!("https://llm.example.com/v1"),
            ),
            ("tencent_secret_id", json!("AKID")),
            ("tencent_secret_key", json!("tsecret")),
            (
                "ai_vendor_sources",
                json!([
                    {"slug": "work", "kind": "openai", "display_name": "Work", "enabled": true, "base_url": ""},
                    {"slug": "mine", "kind": "openai", "display_name": "Mine", "enabled": true, "base_url": "https://mine.example/v1", "extra": 1},
                    {"slug": "tc", "kind": "tencent", "display_name": "TC", "enabled": true, "secret_id": "own-id"}
                ]),
            ),
        ]));
        let sources = out["ai_vendor_sources"].as_array().unwrap();
        assert_eq!(sources[0]["base_url"], json!("https://llm.example.com/v1"));
        assert_eq!(sources[1]["base_url"], json!("https://mine.example/v1"));
        assert_eq!(
            sources[1]["extra"],
            json!(1),
            "fields the page keeps are kept"
        );
        assert_eq!(sources[2]["secret_id"], json!("own-id"));
        assert_eq!(sources[2]["secret_key"], json!("tsecret"));
        assert_eq!(sources.len(), 3);
    }

    #[test]
    fn default_endpoints_and_no_tencent_change_no_sources() {
        let out = upgrade(&stored(&[
            (
                "provider_openai_base_url",
                json!("https://api.openai.com/v1"),
            ),
            (
                "provider_volcengine_base_url",
                json!("https://ark.cn-beijing.volces.com/api/v3"),
            ),
            ("tencent_region", json!("ap-guangzhou")),
            ("tencent_secret_id", json!(null)),
        ]));
        assert!(out.is_empty(), "{out:?}");
    }
}
