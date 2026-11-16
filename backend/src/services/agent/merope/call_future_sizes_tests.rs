//! How big the futures of her model calls are, each against a budget.
//!
//! The site runs a debug build, where each async function's poll frame keeps
//! room on the stack for the futures it awaits. A layer wrapped around a
//! future by an async function that takes it (`reporting(fut)`,
//! `with_site_ai_ledger(fut)`) holds it twice, so every such layer roughly
//! doubles what is under it, and every caller up the chain grows with it.
//! One unboxed layer around every model call (181101a29) once took a chain
//! of hers past a 2 MiB stack. Budgets are about one and a half times what
//! the futures measured with the layers boxed (2026-11-17, debug, aarch64:
//! 2640, 4768, 5616, 10352, 37096 (unboxed) and 4912 bytes, in the order below). The
//! Work turn's layers are not boxed yet: its budget comes down once they are.

use std::mem::size_of;

fn returned<A, R>(_: fn(A) -> R) -> usize {
    size_of::<R>()
}

fn returned3<A, B, C, R>(_: fn(A, B, C) -> R) -> usize {
    size_of::<R>()
}

fn returned4<A, B, C, D, R>(_: fn(A, B, C, D) -> R) -> usize {
    size_of::<R>()
}

fn returned5<A, B, C, D, E, R>(_: fn(A, B, C, D, E) -> R) -> usize {
    size_of::<R>()
}

fn returned6<A, B, C, D, E, F, R>(_: fn(A, B, C, D, E, F) -> R) -> usize {
    size_of::<R>()
}

#[test]
fn her_call_futures_stay_within_their_budgets() {
    type Db = &'static sea_orm::DatabaseConnection;
    type At = chrono::DateTime<chrono::FixedOffset>;
    let measured = [
        (
            "call::Model::text",
            returned3::<&'static super::Model, &'static str, &'static str, _>(super::Model::text),
            4 * 1024,
        ),
        (
            "call::Ask::json_raw",
            returned5::<
                super::Ask,
                &'static str,
                &'static str,
                &'static str,
                &'static serde_json::Value,
                _,
            >(super::Ask::json_raw),
            7 * 1024,
        ),
        (
            "chat_remember::settle::settle",
            returned4::<Db, i32, &'static crate::services::agent::memory::unified::Audience, At, _>(
                super::super::chat_remember::settle::settle,
            ),
            8 * 1024,
        ),
        (
            "Agent::process_chat_with_progress",
            returned6::<
                &'static crate::services::agent::Agent,
                crate::services::agent::UserRequest,
                tokio::sync::mpsc::Sender<crate::services::agent::AgentProgressEvent>,
                Option<super::super::MoodTransition>,
                String,
                Option<At>,
                _,
            >(crate::services::agent::Agent::process_chat_with_progress),
            16 * 1024,
        ),
        (
            "Agent::process_work_with_progress",
            returned5::<
                &'static crate::services::agent::Agent,
                crate::services::agent::UserRequest,
                tokio::sync::mpsc::Sender<crate::services::agent::AgentProgressEvent>,
                Option<super::super::MoodTransition>,
                String,
                _,
            >(crate::services::agent::Agent::process_work_with_progress),
            56 * 1024,
        ),
        (
            "memory_jobs::tick",
            returned::<sea_orm::DatabaseConnection, _>(super::super::memory_jobs::tick),
            8 * 1024,
        ),
    ];
    let over: Vec<String> = measured
        .iter()
        .inspect(|(name, size, budget)| eprintln!("{name}: {size} bytes (budget {budget})"))
        .filter(|(_, size, budget)| size > budget)
        .map(|(name, size, budget)| format!("{name}: {size} bytes, budget {budget}"))
        .collect();
    assert!(
        over.is_empty(),
        "a future grew past its budget: {over:?}. A new layer wrapped around a \
         future (an async fn that takes one) must be boxed (`Box::pin`); see \
         this module's docs. Right after a toolchain upgrade, measure again \
         before changing a budget"
    );
}
