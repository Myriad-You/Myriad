//! Unit tests for `motion.rs`: how the site asks the director and plays
//! what it says. Her judgments' own tests are in the core
//! (`myriad_merope::motion`).

use super::*;

/// Chat speaks plain prose; the director observes it on a separate task.
#[test]
fn streaming_chat_refines_actual_delivery_without_delaying_text() {
    let src = include_str!("../process_chat.rs");
    assert!(src.contains("let performance = None;"));
    let chat = src
        .find("stream_strict_lite_chat_response")
        .expect("chat lite call");
    let local_reaction = src
        .find("local_directive(&reaction_context)")
        .expect("local reaction");
    assert!(
        local_reaction < chat,
        "Chat must react before the reply stream starts"
    );
    assert!(src.contains("spawn_chat_motion_refinement("));
    let streaming = include_str!("../confirmation_and_tasks/chat_stream.rs");
    assert!(streaming.contains("emit_chat_delta(&tx, delta, speech_delivery.as_ref()).await"));
    assert!(!streaming.contains("SpeechDeliveryStream"));
    assert!(!include_str!("../chat_prompt.rs").contains("[[delivery:"));
}

#[test]
fn immediate_reaction_precedes_text_and_landing_cannot_delay_stream_close() {
    let src = include_str!("../process_chat.rs");
    let floor = src.find("local_directive(&reaction_context)").unwrap();
    let delivery = src.find("local_directive(&delivery_context)").unwrap();
    assert!(floor < delivery);
    let chat = src[floor..]
        .find("stream_strict_lite_chat_response")
        .unwrap()
        + floor;
    // What they say in chat is already in the conversation she reads: no
    // second copy of it is kept as something that happened.
    assert!(!src.contains("note_chat_diary"));
    assert!(chat < delivery);
    let finish = src
        .find("response_agent::finish_stream(&progress_tx)")
        .unwrap();
    assert!(finish < delivery);
    let stop = src.find("guard.stop().await").unwrap();
    assert!(finish < stop && stop < delivery);
    let overlay = include_str!("../motion_overlay.rs");
    assert!(overlay.contains("self.task.abort();"));
    assert!(src[delivery..].contains("performance.plan.cues.clear()"));
}

/// MOTION_TIMEOUT >= 8s; MOTION_TOTAL_TIMEOUT > MOTION_TIMEOUT.
/// Request paths must not `handle.await.ok().flatten()`.
#[test]
fn the_director_budget_clears_the_observed_success_latency() {
    assert!(MOTION_TIMEOUT >= Duration::from_secs(8));
    assert!(MOTION_TOTAL_TIMEOUT > MOTION_TIMEOUT);
    let src = concat!(
        include_str!("../process_and_recipe.rs"),
        include_str!("../process_chat.rs"),
        include_str!("../process_work.rs")
    );
    assert!(
        !src.contains("handle.await.ok().flatten()"),
        "no request path may block on the director's budget"
    );
}
