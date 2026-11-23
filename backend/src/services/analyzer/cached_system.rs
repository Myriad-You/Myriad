//! The part of a system prompt that is the same on every call of a kind,
//! cached by the provider: written once, then read at a tenth of its price
//! while calls keep coming. Only for the memory writing steps; her talk is
//! not cached (its prompt is put together anew each turn).

use super::types::AiProvider;
use serde_json::{Value, json};

/// How a kind of call's system prompt is cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedSystem {
    /// Where the part that changes from call to call begins: its first
    /// appearance in the system prompt. `None`: the whole of it is the same.
    pub until: Option<&'static str>,
    /// What OpenRouter keeps calls of this kind on one provider by, so that
    /// they reach the one holding their cache. A fixed name for the kind of
    /// call (`memory-remember`), never anything of whom it is for: it is
    /// sent out.
    pub session: &'static str,
}

/// Bytes of `system` that are the same on every call: up to `until`, or
/// all of it. `None` when there is nothing before it, or it is not there.
pub(super) fn stable_len(system: &str, until: Option<&str>) -> Option<usize> {
    let len = match until {
        Some(marker) => system.find(marker)?,
        None => system.len(),
    };
    (len > 0).then_some(len)
}

/// `system` as text blocks, the cache's end after the part that is the
/// same; `None` when it has none.
fn blocks(system: &str, until: Option<&str>) -> Option<Value> {
    let len = stable_len(system, until)?;
    let (same, rest) = system.split_at(len);
    let mut blocks = vec![json!({
        "type": "text",
        "text": same,
        "cache_control": {"type": "ephemeral"},
    })];
    if !rest.is_empty() {
        blocks.push(json!({"type": "text", "text": rest}));
    }
    Some(Value::Array(blocks))
}

/// The host `base_url` names; none when it names none.
fn host(base_url: &str) -> Option<String> {
    reqwest::Url::parse(base_url.trim())
        .ok()?
        .host_str()
        .map(str::to_ascii_lowercase)
}

/// Anthropic's own API: the one it is asked at unless told another. A
/// gateway speaking its protocol may take a system prompt only as a string.
fn anthropics_own(base_url: Option<&str>) -> bool {
    base_url.is_none_or(|url| host(url).as_deref() == Some("api.anthropic.com"))
}

/// Through OpenRouter to a Claude model: the gateway that passes the cache
/// marks on to Anthropic (and to Bedrock and Vertex, which serve the same
/// models). Other OpenAI-compatible gateways are sent the prompt as it was.
fn openrouter_claude(base_url: Option<&str>, model: &str) -> bool {
    base_url
        .and_then(host)
        .is_some_and(|host| host == "openrouter.ai" || host.ends_with(".openrouter.ai"))
        && model.contains("claude")
}

/// `body` with its system prompt cached as `cached` says, where the endpoint
/// takes cache marks: Anthropic's own API, and Claude through OpenRouter.
/// Any other, and a body whose system prompt is already in blocks, is left
/// as it is.
pub(super) fn apply(
    body: &mut Value,
    provider: AiProvider,
    base_url: Option<&str>,
    model: &str,
    cached: CachedSystem,
) {
    match provider {
        AiProvider::Anthropic if anthropics_own(base_url) => {
            if let Some(blocks) = body["system"]
                .as_str()
                .and_then(|s| blocks(s, cached.until))
            {
                body["system"] = blocks;
            }
        }
        AiProvider::OpenAI if openrouter_claude(base_url, model) => {
            let Some(object) = body.as_object_mut() else {
                return;
            };
            let system = object
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .and_then(|messages| messages.first_mut())
                .filter(|first| first["role"] == "system");
            let Some(system) = system else {
                return;
            };
            if let Some(blocks) = system["content"]
                .as_str()
                .and_then(|s| blocks(s, cached.until))
            {
                system["content"] = blocks;
                object.insert("session_id".into(), json!(cached.session));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REMEMBER: CachedSystem = CachedSystem {
        until: Some("Known:\n"),
        session: "memory-remember",
    };

    fn openrouter_body() -> Value {
        json!({
            "model": "anthropic/claude-haiku-5.5",
            "messages": [
                {"role": "system", "content": "Rules.\nKnown:\n- likes tea"},
                {"role": "user", "content": "hi"},
            ],
        })
    }

    #[test]
    fn through_openrouter_the_same_part_is_cached_and_the_kind_names_its_session() {
        let mut body = openrouter_body();
        apply(
            &mut body,
            AiProvider::OpenAI,
            Some("https://openrouter.ai/api/v1"),
            "anthropic/claude-haiku-5.5",
            REMEMBER,
        );
        assert_eq!(
            body["messages"][0]["content"],
            json!([
                {"type": "text", "text": "Rules.\n", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "Known:\n- likes tea"},
            ])
        );
        assert_eq!(body["session_id"], "memory-remember");
        // What they said is sent as it was.
        assert_eq!(body["messages"][1]["content"], "hi");
    }

    #[test]
    fn anthropics_own_api_gets_system_blocks_and_no_session() {
        let mut body = json!({"model": "claude-haiku-5-5", "system": "Rules.\nKnown:\n- x",
            "messages": [{"role": "user", "content": "hi"}]});
        apply(
            &mut body,
            AiProvider::Anthropic,
            None,
            "claude-haiku-5-5",
            REMEMBER,
        );
        assert_eq!(body["system"][0]["text"], "Rules.\n");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["system"][1]["text"], "Known:\n- x");
        assert!(body.get("session_id").is_none());
    }

    /// Anthropic's protocol at another host: its system prompt stays the
    /// string it was.
    #[test]
    fn an_anthropic_shaped_gateway_is_sent_a_string() {
        let sent = |base_url: Option<&str>| {
            let mut body = json!({"model": "claude-haiku-5-5", "system": "Rules.\nKnown:\n- x",
                "messages": [{"role": "user", "content": "hi"}]});
            apply(
                &mut body,
                AiProvider::Anthropic,
                base_url,
                "claude-haiku-5-5",
                REMEMBER,
            );
            body["system"].clone()
        };
        for other in [
            "https://gateway.example/v1",
            "https://api.anthropic.com.gateway.example/v1",
            "https://gateway.example/api.anthropic.com/v1",
            "api.anthropic.com",
        ] {
            assert_eq!(sent(Some(other)), "Rules.\nKnown:\n- x", "{other}");
        }
        for own in [
            None,
            Some("https://api.anthropic.com/v1"),
            Some(" https://API.anthropic.com "),
        ] {
            assert!(sent(own).is_array(), "{own:?}");
        }
        assert!(!openrouter_claude(
            Some("https://gateway.example/openrouter.ai/v1"),
            "anthropic/claude-haiku-5.5"
        ));
        assert!(openrouter_claude(
            Some("https://openrouter.ai/api/v1"),
            "anthropic/claude-haiku-5.5"
        ));
    }

    #[test]
    fn a_whole_prompt_the_same_is_one_cached_block() {
        let mut body = openrouter_body();
        let whole = CachedSystem {
            until: None,
            session: "memory-standing",
        };
        apply(
            &mut body,
            AiProvider::OpenAI,
            Some("https://openrouter.ai/api/v1"),
            "anthropic/claude-haiku-5.5",
            whole,
        );
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["text"], "Rules.\nKnown:\n- likes tea");
    }

    #[test]
    fn other_gateways_models_and_shapes_are_sent_as_they_were() {
        for (provider, url, model) in [
            (
                AiProvider::OpenAI,
                Some("https://api.openai.com/v1"),
                "anthropic/claude-haiku-5.5",
            ),
            (
                AiProvider::OpenAI,
                Some("https://openrouter.ai/api/v1"),
                "google/gemini-3.8-flash",
            ),
            (AiProvider::Gemini, None, "gemini-3.8-flash"),
        ] {
            let mut body = openrouter_body();
            apply(&mut body, provider, url, model, REMEMBER);
            assert_eq!(body, openrouter_body(), "{model} via {url:?}");
        }
        // No marker in it, or nothing before it: nothing to cache.
        let mut body = json!({"model": "anthropic/claude-haiku-5.5",
            "messages": [{"role": "system", "content": "Known:\n- x"}]});
        let before = body.clone();
        apply(
            &mut body,
            AiProvider::OpenAI,
            Some("https://openrouter.ai/api/v1"),
            "anthropic/claude-haiku-5.5",
            REMEMBER,
        );
        assert_eq!(body, before);
        // Applied twice (a request sent again), the same.
        let mut body = openrouter_body();
        for _ in 0..2 {
            apply(
                &mut body,
                AiProvider::OpenAI,
                Some("https://openrouter.ai/api/v1"),
                "anthropic/claude-haiku-5.5",
                REMEMBER,
            );
        }
        assert_eq!(body["messages"][0]["content"].as_array().unwrap().len(), 2);
    }
}
