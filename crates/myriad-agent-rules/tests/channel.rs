//! Public boundary of the QQ C2C transport adapter.
//!
//! These tests do not simulate the QQ Gateway. They only prove that a C2C
//! text event becomes a Work request or a pairing reply, and that a finished
//! turn is delivered, dropped, or failed visibly.

use myriad_agent_rules::channel::{
    classify_connect_failure, ingest_c2c_text, next_passive_seq, outbound_idempotency_key,
    plan_delivery, qq_c2c_capabilities, session_key, worker_intent, ChannelEvent, ConnectFailure,
    DeliveryContext, DeliveryPlan, InboundC2cText, InboundDecision, PairingLookup, WorkerIntent,
    PANEL_REQUIRED_REPLY, PAIRING_REQUIRED_REPLY,
};

fn text(msg_id: &str, openid: &str, content: &str) -> InboundC2cText {
    InboundC2cText {
        msg_id: msg_id.to_string(),
        user_openid: openid.to_string(),
        content: content.to_string(),
    }
}

fn window(msg_id: &str) -> DeliveryContext {
    DeliveryContext {
        inbound_msg_id: Some(msg_id.to_string()),
        passive_window_open: true,
        remaining_passive_replies: 4,
    }
}

#[test]
fn unpaired_c2c_text_asks_to_pair_and_does_not_start_work() {
    let decision = ingest_c2c_text(&text("m1", "openid-a", "帮我查天气"), PairingLookup::Unpaired, false);
    match decision {
        InboundDecision::PairingRequired {
            user_openid,
            reply,
            msg_id,
        } => {
            assert_eq!(user_openid, "openid-a");
            assert_eq!(msg_id, "m1");
            assert_eq!(reply, PAIRING_REQUIRED_REPLY);
            assert!(reply.contains("去站点配对"));
        }
        other => panic!("expected pairing reply, got {other:?}"),
    }
}

#[test]
fn paired_c2c_text_becomes_a_stable_work_request() {
    let first = ingest_c2c_text(
        &text("m2", "openid-b", "帮我订一张票"),
        PairingLookup::Paired { user_id: 7 },
        false,
    );
    let second = ingest_c2c_text(
        &text("m3", "openid-b", "改成明天"),
        PairingLookup::Paired { user_id: 7 },
        false,
    );
    let InboundDecision::StartWork {
        user_id,
        input,
        mode,
        session_key: key,
        msg_id,
    } = first
    else {
        panic!("expected work request");
    };
    assert_eq!(user_id, 7);
    assert_eq!(input, "帮我订一张票");
    assert_eq!(mode, "work");
    assert_eq!(msg_id, "m2");
    assert_eq!(key, session_key("qq", "openid-b"));
    let InboundDecision::StartWork {
        session_key: key2, ..
    } = second
    else {
        panic!("expected work request");
    };
    assert_eq!(key, key2);
}

#[test]
fn same_message_id_does_not_start_another_run() {
    let decision = ingest_c2c_text(
        &text("m-dup", "openid-c", "再发一次"),
        PairingLookup::Paired { user_id: 3 },
        true,
    );
    assert_eq!(decision, InboundDecision::Duplicate { msg_id: "m-dup".into() });
}

#[test]
fn session_key_strips_colons_so_openid_cannot_collide() {
    assert_ne!(
        session_key("qq", "user:1"),
        session_key("qq:user", "1")
    );
    assert_eq!(session_key("qq", "user:1"), "qq:user_1");
}

#[test]
fn first_cut_capabilities_are_text_only() {
    let caps = qq_c2c_capabilities();
    assert!(caps.inbound_text);
    assert!(caps.outbound_final_text);
    assert!(!caps.inbound_media);
    assert!(!caps.inbound_callback);
    assert!(!caps.outbound_markdown);
    assert!(!caps.outbound_image);
    assert!(!caps.outbound_edit);
    assert!(!caps.outbound_streaming_draft);
    assert!(!caps.interactive);
    assert!(!caps.frontend_action);
    assert!(!caps.performance);
    assert!(!caps.outfit);
}

#[test]
fn thinking_and_step_events_are_not_sent() {
    let ctx = window("m4");
    assert_eq!(plan_delivery(&ChannelEvent::ThinkingToken, &ctx), DeliveryPlan::Drop);
    assert_eq!(plan_delivery(&ChannelEvent::StepStarted, &ctx), DeliveryPlan::Drop);
    assert_eq!(plan_delivery(&ChannelEvent::StepCompleted, &ctx), DeliveryPlan::Drop);
    assert_eq!(plan_delivery(&ChannelEvent::Progress, &ctx), DeliveryPlan::Drop);
}

#[test]
fn final_answer_becomes_one_c2c_text() {
    let plan = plan_delivery(
        &ChannelEvent::Answer {
            message: "票已订好".into(),
        },
        &window("m5"),
    );
    assert_eq!(
        plan,
        DeliveryPlan::PassiveText {
            content: "票已订好".into(),
            msg_id: "m5".into(),
        }
    );
}

#[test]
fn confirmation_and_frontend_action_fail_visibly() {
    let ctx = window("m6");
    assert_eq!(
        plan_delivery(&ChannelEvent::ConfirmationRequired, &ctx),
        DeliveryPlan::FailVisible {
            content: PANEL_REQUIRED_REPLY.to_string(),
            msg_id: Some("m6".into()),
            passive: true,
        }
    );
    assert_eq!(
        plan_delivery(&ChannelEvent::FrontendAction, &ctx),
        DeliveryPlan::FailVisible {
            content: PANEL_REQUIRED_REPLY.to_string(),
            msg_id: Some("m6".into()),
            passive: true,
        }
    );
}

#[test]
fn missing_passive_window_sends_actively_including_without_inbound() {
    let closed = DeliveryContext {
        inbound_msg_id: Some("m7".into()),
        passive_window_open: false,
        remaining_passive_replies: 4,
    };
    assert_eq!(
        plan_delivery(
            &ChannelEvent::Answer {
                message: "做好了".into(),
            },
            &closed
        ),
        DeliveryPlan::ActiveText {
            content: "做好了".into(),
        }
    );

    let exhausted = DeliveryContext {
        inbound_msg_id: Some("m7".into()),
        passive_window_open: true,
        remaining_passive_replies: 0,
    };
    assert_eq!(
        plan_delivery(
            &ChannelEvent::Error {
                message: "失败了".into(),
            },
            &exhausted
        ),
        DeliveryPlan::ActiveText {
            content: "失败了".into(),
        }
    );

    let proactive = DeliveryContext {
        inbound_msg_id: None,
        passive_window_open: false,
        remaining_passive_replies: 0,
    };
    assert_eq!(
        plan_delivery(
            &ChannelEvent::Answer {
                message: "测一条".into(),
            },
            &proactive
        ),
        DeliveryPlan::ActiveText {
            content: "测一条".into(),
        }
    );
}

#[test]
fn passive_seq_and_outbound_idempotency_are_stable() {
    assert_eq!(next_passive_seq(None), 1);
    assert_eq!(next_passive_seq(Some(1)), 2);
    assert_eq!(
        outbound_idempotency_key("qq", "openid-d", Some("m8"), 1),
        "qq:openid-d:m8:1"
    );
    assert_eq!(
        outbound_idempotency_key("qq", "openid:d", None, 3),
        "qq:openid_d:active:3"
    );
}

#[test]
fn worker_runs_only_with_switch_and_complete_credentials() {
    assert_eq!(
        worker_intent(true, "app-1", true),
        WorkerIntent::Run
    );
    assert_eq!(
        worker_intent(false, "app-1", true),
        WorkerIntent::Stop
    );
    assert_eq!(
        worker_intent(true, "", true),
        WorkerIntent::Stop
    );
    assert_eq!(
        worker_intent(true, "app-1", false),
        WorkerIntent::Stop
    );
}

#[test]
fn connect_failures_split_permanent_from_transient() {
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus { status: 401, body: "" }),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus { status: 403, body: "" }),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::AuthRejected { close_code: 4004 }),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::AuthRejected { close_code: 4014 }),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::TokenRejected { body: "missing access_token" }),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::Transport),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus { status: 500, body: "" }),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus {
            status: 200,
            body: r#"{"code":100001,"message":"rate limited"}"#,
        }),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
}
