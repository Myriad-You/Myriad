//! Memory bench only (`MEROPE_MEMORY_BENCH_WRITE_THINKING`): her writing
//! asked to think otherwise than production asks, to see what its thinking
//! buys. Unset, it is written as production writes it (the model's own
//! thinking, `call::Voice::Keeping`); `off`: thinking turned off. Compiled
//! into test builds only, so production asks as it always has. Meant for
//! `her_memory_on_longmemeval`: the long background bench writes partly
//! outside it and does not record it, so it is not set there.

use super::AiProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Thinking {
    /// Thinking turned off.
    Off,
}

tokio::task_local! {
    /// Set while a bench question is written so.
    static WRITTEN: Thinking;
}

/// What `MEROPE_MEMORY_BENCH_WRITE_THINKING` asks for; unset, production's.
pub(crate) fn asked() -> Option<Thinking> {
    match std::env::var("MEROPE_MEMORY_BENCH_WRITE_THINKING").as_deref() {
        Err(_) | Ok("") => None,
        Ok("off") => Some(Thinking::Off),
        Ok(other) => panic!("MEROPE_MEMORY_BENCH_WRITE_THINKING is off or unset, not {other}"),
    }
}

/// `fut`, its calls to Anthropic's own API asked to think as
/// [`asked`] says; unset, as is.
pub(crate) async fn when_asked<F: std::future::Future>(fut: F) -> F::Output {
    match asked() {
        Some(thinking) => WRITTEN.scope(thinking, fut).await,
        None => fut.await,
    }
}

/// Whether the call now is inside [`when_asked`]: a refusal of what was
/// sent then is not remembered, since production never sends it so.
pub(super) fn active() -> bool {
    WRITTEN.try_with(|_| ()).is_ok()
}

/// A body about to go to `provider`, inside [`when_asked`]: its thinking
/// and effort as asked.
pub(super) fn apply(provider: AiProvider, body: &mut serde_json::Value) {
    if provider != AiProvider::Anthropic {
        return;
    }
    if let Ok(thinking) = WRITTEN.try_with(|thinking| *thinking) {
        set(thinking, body);
    }
}

fn set(thinking: Thinking, body: &mut serde_json::Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    match thinking {
        // Haiku 5.5 takes `disabled` at effort high or below; with no
        // effort it is the model's own.
        Thinking::Off => object.insert("thinking".into(), serde_json::json!({"type": "disabled"})),
    };
    let emptied = object
        .get_mut("output_config")
        .and_then(serde_json::Value::as_object_mut)
        .is_some_and(|config| {
            config.remove("effort");
            config.is_empty()
        });
    if emptied {
        object.remove("output_config");
    }
}

#[tokio::test]
async fn only_anthropic_bodies_inside_the_scope_are_asked_otherwise() {
    let deep = || {
        serde_json::json!({"thinking": {"type": "adaptive"},
            "output_config": {"effort": "high", "format": {"type": "json_schema"}}})
    };
    let mut outside = deep();
    apply(AiProvider::Anthropic, &mut outside);
    assert_eq!(outside, deep(), "outside the scope nothing changes");
    let within = |thinking| async move {
        WRITTEN
            .scope(thinking, async {
                let (mut anthropic, mut other) = (deep(), deep());
                let mut light = serde_json::json!({"output_config": {"effort": "low"}});
                apply(AiProvider::Anthropic, &mut anthropic);
                apply(AiProvider::OpenAI, &mut other);
                apply(AiProvider::Anthropic, &mut light);
                (anthropic, other, light)
            })
            .await
    };
    let (anthropic, other, light) = within(Thinking::Off).await;
    assert_eq!(
        anthropic,
        serde_json::json!({"thinking": {"type": "disabled"},
            "output_config": {"format": {"type": "json_schema"}}})
    );
    assert_eq!(other, deep(), "other providers are left as asked");
    assert_eq!(light, serde_json::json!({"thinking": {"type": "disabled"}}));
}
