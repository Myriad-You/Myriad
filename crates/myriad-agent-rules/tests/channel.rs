//! Public boundary of the channel transport adapter.
//!
//! These tests do not simulate the QQ Gateway or Telegram HTTP. They prove
//! that inbound text becomes a Work request or a pairing reply, and that a
//! finished turn is delivered, dropped, or failed visibly.

use myriad_agent_rules::channel::{
    channel_can_finish, clarify_base_input, clarify_followup, classify_connect_failure,
    classify_gateway_close, decide_pending_reply, encode_pairing_code, encode_pending_id,
    ensure_pending_id, extract_pairing_code, format_channel_result, format_pairing_code,
    format_pending_prompt, ingest_c2c_text, ingest_channel_text, next_passive_seq,
    outbound_idempotency_key, pairing_bind_reply, pairing_bind_reply_for, panel_entry_reply,
    parse_access_token_response, parse_channel_command, parse_gateway_url_response,
    parse_telegram_bot_identity, parse_telegram_callback, parse_telegram_ok_payload,
    parse_telegram_private_inbounds, parse_telegram_private_texts, pending_prompt_from_model_json,
    plan_delivery, qq_c2c_capabilities, qq_token_needs_refresh, session_key,
    should_deliver_sequence, split_channel_text, telegram_callback_action,
    telegram_dm_capabilities, telegram_inline_keyboard, telegram_max_update_id,
    telegram_reply_markup, telegram_retry_after, telegram_worker_intent, truncate_telegram_text,
    worker_intent, ChannelCommand, ChannelEvent, ConnectFailure, DeliveryContext, DeliveryPlan,
    InboundC2cText, InboundDecision, PairingBindResult, PairingLookup, PendingDecision,
    PendingKind, PendingOption, PendingPrompt, TelegramCallbackAction, TelegramPrivateInbound,
    WorkerIntent, CONFIRM_HINT, DISCORD_DIRECT_MESSAGES, DISCORD_PAIRING_TAKEN_REPLY,
    DISCORD_TEXT_LIMIT, GROUP_AND_C2C_EVENT, PAIRING_INVALID_REPLY, PAIRING_OK_REPLY,
    PAIRING_REQUIRED_REPLY, PAIRING_TAKEN_REPLY, PANEL_REQUIRED_REPLY, PENDING_EXPIRED_REPLY,
    PENDING_STALE_REPLY, TELEGRAM_CALLBACK_INPUT, TELEGRAM_CALLBACK_NO, TELEGRAM_CALLBACK_YES,
    TELEGRAM_INPUT_BUTTON, TELEGRAM_PAIRING_TAKEN_REPLY, TELEGRAM_TEXT_LIMIT, classify_discord_rest,
    discord_dm_capabilities, discord_private_text_from_create, discord_reply_markup,
    discord_worker_intent,
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
    let decision = ingest_c2c_text(
        &text("m1", "openid-a", "帮我查天气"),
        PairingLookup::Unpaired,
        false,
    );
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
fn unpaired_pairing_code_is_consumed_not_started_as_work() {
    let decision = ingest_c2c_text(
        &text("m-code", "openid-d", "ab1d-efgh"),
        PairingLookup::Unpaired,
        false,
    );
    match decision {
        InboundDecision::ConsumePairingCode {
            user_openid,
            code,
            msg_id,
        } => {
            assert_eq!(user_openid, "openid-d");
            assert_eq!(code, "AB1DEFGH");
            assert_eq!(msg_id, "m-code");
        }
        other => panic!("expected consume pairing code, got {other:?}"),
    }
    let mixed = ingest_c2c_text(
        &text("m-mix", "openid-d", "帮我查天气 AB1DEFGH"),
        PairingLookup::Unpaired,
        false,
    );
    assert!(matches!(mixed, InboundDecision::PairingRequired { .. }));
    let already_paired = ingest_c2c_text(
        &text("m-keep", "openid-d", "AB1DEFGH"),
        PairingLookup::Paired { user_id: 9 },
        false,
    );
    assert!(matches!(
        already_paired,
        InboundDecision::StartWork { user_id: 9, .. }
    ));
}

#[test]
fn pairing_code_normalizes_crockford_and_rejects_extra_words() {
    assert_eq!(
        extract_pairing_code("ab1d-efgh").as_deref(),
        Some("AB1DEFGH")
    );
    assert_eq!(
        extract_pairing_code("  AB1O EFGH  ").as_deref(),
        Some("AB10EFGH")
    );
    assert_eq!(extract_pairing_code("IL"), None);
    assert_eq!(extract_pairing_code("帮我查天气"), None);
    assert_eq!(encode_pairing_code([0, 0, 0, 0, 0]), "00000000");
    assert_eq!(format_pairing_code("AB1DEFGH"), "AB1D-EFGH");
    assert_eq!(
        pairing_bind_reply(PairingBindResult::Bound { user_id: 3 }),
        PAIRING_OK_REPLY
    );
    assert_eq!(
        pairing_bind_reply(PairingBindResult::InvalidOrExpired),
        PAIRING_INVALID_REPLY
    );
    assert_eq!(
        pairing_bind_reply(PairingBindResult::OpenidTaken),
        PAIRING_TAKEN_REPLY
    );
    assert_eq!(
        pairing_bind_reply_for(PairingBindResult::OpenidTaken, "telegram"),
        TELEGRAM_PAIRING_TAKEN_REPLY
    );
    assert_eq!(
        pairing_bind_reply_for(PairingBindResult::OpenidTaken, "discord"),
        DISCORD_PAIRING_TAKEN_REPLY
    );
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
    assert_eq!(
        decision,
        InboundDecision::Duplicate {
            msg_id: "m-dup".into()
        }
    );
}

#[test]
fn session_key_strips_colons_so_openid_cannot_collide() {
    assert_ne!(session_key("qq", "user:1"), session_key("qq:user", "1"));
    assert_eq!(session_key("qq", "user:1"), "qq:user_1");
}

#[test]
fn paired_telegram_text_uses_chat_id_session_key() {
    let decision = ingest_channel_text(
        &text("42", "1001", "帮我订一张票"),
        PairingLookup::Paired { user_id: 7 },
        false,
        "telegram",
        "1001",
    );
    let InboundDecision::StartWork {
        session_key: key, ..
    } = decision
    else {
        panic!("expected work request");
    };
    assert_eq!(key, session_key("telegram", "1001"));
}

#[test]
fn first_cut_capabilities_are_text_only() {
    let caps = qq_c2c_capabilities();
    assert!(telegram_dm_capabilities().inbound_text);
    assert!(telegram_dm_capabilities().outbound_final_text);
    assert!(!telegram_dm_capabilities().outbound_edit);
    assert!(!telegram_dm_capabilities().outbound_streaming_draft);
    assert!(telegram_dm_capabilities().interactive);
    assert!(telegram_dm_capabilities().inbound_callback);
    assert!(caps.inbound_text);
    assert!(caps.outbound_final_text);
    assert!(!caps.inbound_media);
    assert!(!caps.inbound_callback);
    assert!(!caps.outbound_markdown);
    assert!(!caps.outbound_image);
    assert!(!caps.outbound_edit);
    assert!(!caps.outbound_streaming_draft);
    assert!(caps.interactive);
    assert!(!caps.frontend_action);
    assert!(channel_can_finish(
        &telegram_dm_capabilities(),
        &ChannelEvent::ConfirmationRequired
    ));
    assert!(!channel_can_finish(
        &telegram_dm_capabilities(),
        &ChannelEvent::FrontendAction
    ));
    assert_eq!(
        panel_entry_reply(Some("ses_1"), Some("t1")),
        "请到站点打开这次办事继续。会话 ses_1，任务 t1。"
    );
    assert!(!caps.performance);
    assert!(!caps.outfit);
}

#[test]
fn thinking_and_step_events_are_not_sent() {
    let ctx = window("m4");
    assert_eq!(
        plan_delivery(&ChannelEvent::ThinkingToken, &ctx),
        DeliveryPlan::Drop
    );
    assert_eq!(
        plan_delivery(&ChannelEvent::StepStarted, &ctx),
        DeliveryPlan::Drop
    );
    assert_eq!(
        plan_delivery(&ChannelEvent::StepCompleted, &ctx),
        DeliveryPlan::Drop
    );
    assert_eq!(
        plan_delivery(&ChannelEvent::Progress, &ctx),
        DeliveryPlan::Drop
    );
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
    assert_eq!(worker_intent(true, "app-1", true), WorkerIntent::Run);
    assert_eq!(worker_intent(false, "app-1", true), WorkerIntent::Stop);
    assert_eq!(worker_intent(true, "", true), WorkerIntent::Stop);
    assert_eq!(worker_intent(true, "app-1", false), WorkerIntent::Stop);
    assert_eq!(
        telegram_worker_intent(true, "123456:ABC-DEF"),
        WorkerIntent::Run
    );
    assert_eq!(telegram_worker_intent(true, "  "), WorkerIntent::Stop);
    assert_eq!(
        telegram_worker_intent(false, "123456:ABC-DEF"),
        WorkerIntent::Stop
    );
}

#[test]
fn connect_failures_split_permanent_from_transient() {
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus {
            status: 401,
            body: ""
        }),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus {
            status: 403,
            body: ""
        }),
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
        classify_connect_failure(&ConnectFailure::TokenRejected {
            body: "missing access_token"
        }),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::Transport),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus {
            status: 500,
            body: ""
        }),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus {
            status: 200,
            body: r#"{"code":100001,"message":"rate limited"}"#,
        }),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus {
            status: 200,
            body: r#"{"code":100016,"message":"invalid appid"}"#,
        }),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_connect_failure(&ConnectFailure::HttpStatus {
            status: 409,
            body: r#"{"ok":false,"error_code":409,"description":"Conflict"}"#,
        }),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
    assert_eq!(
        classify_gateway_close(4009),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
    assert_eq!(
        classify_gateway_close(4915),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_gateway_close(4010),
        myriad_agent_rules::channel::ConnectFailureKind::Permanent
    );
    assert_eq!(GROUP_AND_C2C_EVENT, 1 << 25);
    assert_eq!(DISCORD_DIRECT_MESSAGES, 1 << 12);
    assert_eq!(
        classify_discord_rest(403, r#"{"code":50007,"message":"no"}"#),
        myriad_agent_rules::channel::ConnectFailureKind::Transient
    );
    assert_eq!(discord_worker_intent(true, "tok"), WorkerIntent::Run);
    assert_eq!(discord_dm_capabilities().inbound_text, true);
    assert_eq!(DISCORD_TEXT_LIMIT, 2000);
}

#[test]
fn discord_dm_create_is_kept_and_guild_is_dropped() {
    let dm = serde_json::json!({
        "id": "11",
        "channel_id": "22",
        "author": { "id": "33", "bot": false },
        "content": "hello"
    });
    let inbound = discord_private_text_from_create(&dm, "99").expect("dm");
    assert_eq!(inbound.author_id, "33");
    let guild = serde_json::json!({
        "id": "11",
        "channel_id": "22",
        "guild_id": "44",
        "author": { "id": "33" },
        "content": "hello"
    });
    assert!(discord_private_text_from_create(&guild, "99").is_none());
    let mut prompt = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Confirm {
            confirmation_id: "c1".into(),
        },
        question: "确认？".into(),
        options: Vec::new(),
        expires_at_unix: None,
    };
    ensure_pending_id(&mut prompt);
    let markup = discord_reply_markup(&prompt).expect("buttons");
    assert_eq!(markup.as_array().map(|rows| rows.len()), Some(1));
}

#[test]
fn access_token_and_gateway_url_parse_without_leaking_secrets() {
    let (token, ttl) =
        parse_access_token_response(200, r#"{"access_token":"tok-abc","expires_in":7200}"#)
            .expect("token");
    assert_eq!(token, "tok-abc");
    assert_eq!(ttl, 7200);
    assert_eq!(
        parse_access_token_response(200, r#"{"code":100001,"message":"busy"}"#),
        Err(myriad_agent_rules::channel::ConnectFailureKind::Transient)
    );
    assert_eq!(
        parse_access_token_response(200, r#"{"code":100016,"message":"bad app"}"#),
        Err(myriad_agent_rules::channel::ConnectFailureKind::Permanent)
    );
    assert_eq!(
        parse_access_token_response(200, r#"{"message":"no token"}"#),
        Err(myriad_agent_rules::channel::ConnectFailureKind::Permanent)
    );
    assert_eq!(
        parse_gateway_url_response(200, r#"{"url":"wss://api.bot.qq.com/websocket"}"#).as_deref(),
        Ok("wss://api.bot.qq.com/websocket")
    );
    assert_eq!(
        parse_gateway_url_response(
            500,
            r#"{"code":11244,"message":"token not exist or expire"}"#,
        ),
        Err(myriad_agent_rules::channel::ConnectFailureKind::Permanent)
    );
    assert!(qq_token_needs_refresh(
        500,
        r#"{"code":11242,"message":"retry"}"#,
    ));
    assert_eq!(
        parse_gateway_url_response(500, r#"{"code":11242,"message":"retry"}"#),
        Err(myriad_agent_rules::channel::ConnectFailureKind::Transient)
    );
}

#[test]
fn telegram_get_updates_keeps_private_text_and_drops_groups() {
    let body = r#"{
        "ok": true,
        "result": [
            {
                "update_id": 10,
                "message": {
                    "message_id": 2,
                    "from": {"id": 1001},
                    "chat": {"id": 1001, "type": "private"},
                    "text": "帮我查天气"
                }
            },
            {
                "update_id": 11,
                "message": {
                    "message_id": 3,
                    "from": {"id": 2002},
                    "chat": {"id": -100, "type": "group"},
                    "text": "群消息"
                }
            },
            {
                "update_id": 12,
                "message": {
                    "message_id": 4,
                    "chat": {"id": 3003, "type": "private"},
                    "text": "频道投影"
                }
            }
        ]
    }"#;
    let texts = parse_telegram_private_texts(200, body).expect("updates");
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].update_id, 10);
    assert_eq!(texts[0].from_id, 1001);
    assert_eq!(texts[0].chat_id, 1001);
    assert_eq!(texts[0].text, "帮我查天气");
    assert_eq!(telegram_max_update_id(200, body).unwrap(), Some(12));
    assert_eq!(
        parse_telegram_ok_payload(
            401,
            r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#
        ),
        Err(myriad_agent_rules::channel::ConnectFailureKind::Permanent)
    );
    assert_eq!(
        parse_telegram_ok_payload(
            409,
            r#"{"ok":false,"error_code":409,"description":"Conflict"}"#
        ),
        Err(myriad_agent_rules::channel::ConnectFailureKind::Transient)
    );
}

#[test]
fn telegram_get_me_exposes_public_identity() {
    let result = parse_telegram_ok_payload(
        200,
        r#"{"ok":true,"result":{"id":101,"is_bot":true,"first_name":"站点","username":"site_bot"}}"#,
    )
    .expect("getMe");
    let identity = parse_telegram_bot_identity(&result).expect("identity");
    assert_eq!(identity.id, 101);
    assert_eq!(identity.first_name, "站点");
    assert_eq!(identity.username.as_deref(), Some("site_bot"));
    assert!(parse_telegram_bot_identity(&serde_json::json!({"id": 1})).is_none());
}

#[test]
fn telegram_retry_after_and_text_cap() {
    assert_eq!(
        telegram_retry_after(r#"{"ok":false,"error_code":429,"parameters":{"retry_after":7}}"#),
        Some(7)
    );
    assert_eq!(telegram_retry_after(r#"{"ok":false}"#), None);
    let over: String = "字".repeat(TELEGRAM_TEXT_LIMIT + 3);
    let trimmed = truncate_telegram_text(&over);
    assert_eq!(trimmed.chars().count(), TELEGRAM_TEXT_LIMIT);
    assert_eq!(truncate_telegram_text("短"), "短");
}

fn choice_prompt() -> PendingPrompt {
    let mut prompt = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Clarify {
            original_input: "订票".into(),
        },
        question: "去哪一站？".into(),
        options: vec![
            PendingOption {
                value: "shanghai".into(),
                label: "上海".into(),
            },
            PendingOption {
                value: "hangzhou".into(),
                label: "杭州".into(),
            },
        ],
        expires_at_unix: None,
    };
    ensure_pending_id(&mut prompt);
    prompt
}

#[test]
fn pending_prompt_lists_numbered_options() {
    let text = format_pending_prompt(&choice_prompt());
    assert!(text.contains("去哪一站？"));
    assert!(text.contains("1. 上海"));
    assert!(text.contains("2. 杭州"));
}

#[test]
fn pending_choice_accepts_number_or_label_not_other_text() {
    let prompt = choice_prompt();
    match decide_pending_reply(&prompt, "2", 0) {
        PendingDecision::Resume { answer, .. } => assert_eq!(answer, "hangzhou"),
        other => panic!("{other:?}"),
    }
    match decide_pending_reply(&prompt, "上海", 0) {
        PendingDecision::Resume { answer, .. } => assert_eq!(answer, "shanghai"),
        other => panic!("{other:?}"),
    }
    match decide_pending_reply(&prompt, "随便", 0) {
        PendingDecision::Reask { reply } => {
            assert!(reply.contains("1. 上海"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn pending_confirmation_never_defaults_to_yes() {
    let prompt = PendingPrompt {
        id: "c1id".into(),
        kind: PendingKind::Confirm {
            confirmation_id: "c1".into(),
        },
        question: "要删掉这篇文章吗？".into(),
        options: vec![],
        expires_at_unix: Some(100),
    };
    let text = format_pending_prompt(&prompt);
    assert!(text.contains(CONFIRM_HINT));
    match decide_pending_reply(&prompt, "是", 10) {
        PendingDecision::Resume {
            confirmed: Some(true),
            ..
        } => {}
        other => panic!("{other:?}"),
    }
    match decide_pending_reply(&prompt, "取消", 10) {
        PendingDecision::Resume {
            confirmed: Some(false),
            ..
        } => {}
        other => panic!("{other:?}"),
    }
    match decide_pending_reply(&prompt, "嗯", 10) {
        PendingDecision::Reask { .. } => {}
        other => panic!("{other:?}"),
    }
    match decide_pending_reply(&prompt, "是", 100) {
        PendingDecision::Expired { reply } => assert_eq!(reply, PENDING_EXPIRED_REPLY),
        other => panic!("{other:?}"),
    }
}

#[test]
fn clarify_followup_does_not_stack_the_previous_turn() {
    let (input, original) = clarify_followup("再来一次", "我说A");
    assert_eq!(input, "我说A");
    assert_eq!(original, "再来一次");
    let stacked = clarify_base_input("再来一次\n补充说明：我说A\n补充说明：B");
    assert_eq!(stacked, "再来一次");
    let (again, parked) = clarify_followup(stacked, "B");
    assert_eq!(again, "B");
    assert_eq!(parked, "再来一次");
}

#[test]
fn pending_free_text_keeps_the_next_message() {
    let prompt = PendingPrompt {
        id: "q1id".into(),
        kind: PendingKind::Answer {
            task_id: "t1".into(),
            question_id: "q1".into(),
            question_type: "free_text".into(),
        },
        question: "标题写什么？".into(),
        options: vec![],
        expires_at_unix: None,
    };
    match decide_pending_reply(&prompt, " 春游计划 ", 0) {
        PendingDecision::Resume { answer, .. } => assert_eq!(answer, "春游计划"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn telegram_choice_becomes_one_inline_button_per_option() {
    let prompt = choice_prompt();
    let rows = telegram_inline_keyboard(&prompt);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0].text, "上海");
    assert_eq!(rows[0][0].callback_data, format!("o:{}:0", prompt.id));
    assert_eq!(rows[1][0].callback_data, format!("o:{}:1", prompt.id));
    match telegram_callback_action(&prompt, &format!("o:{}:1", prompt.id)) {
        TelegramCallbackAction::Resume(answer) => assert_eq!(answer, "hangzhou"),
        other => panic!("{other:?}"),
    }
    let markup = telegram_reply_markup(&prompt).expect("markup");
    assert_eq!(
        markup["inline_keyboard"][0][0]["callback_data"],
        format!("o:{}:0", prompt.id)
    );
    assert_eq!(
        telegram_callback_action(&prompt, "o:other:1"),
        TelegramCallbackAction::Stale
    );
    assert_eq!(
        telegram_callback_action(&prompt, "o:1"),
        TelegramCallbackAction::Stale
    );
}

#[test]
fn telegram_confirm_and_input_buttons() {
    let mut confirm = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Confirm {
            confirmation_id: "c1".into(),
        },
        question: "要删吗？".into(),
        options: vec![],
        expires_at_unix: None,
    };
    ensure_pending_id(&mut confirm);
    let rows = telegram_inline_keyboard(&confirm);
    assert_eq!(
        rows[0][0].callback_data,
        format!("{TELEGRAM_CALLBACK_YES}:{}", confirm.id)
    );
    assert_eq!(
        rows[0][1].callback_data,
        format!("{TELEGRAM_CALLBACK_NO}:{}", confirm.id)
    );
    assert_eq!(
        telegram_callback_action(&confirm, &format!("y:{}", confirm.id)),
        TelegramCallbackAction::Resume("是".into())
    );
    assert_eq!(
        telegram_callback_action(&confirm, "y"),
        TelegramCallbackAction::Stale
    );
    let mut free = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Answer {
            task_id: "t1".into(),
            question_id: "q1".into(),
            question_type: "free_text".into(),
        },
        question: "标题写什么？".into(),
        options: vec![],
        expires_at_unix: None,
    };
    ensure_pending_id(&mut free);
    assert_eq!(
        telegram_inline_keyboard(&free)[0][0].text,
        TELEGRAM_INPUT_BUTTON
    );
    assert_eq!(
        telegram_callback_action(&free, &format!("{TELEGRAM_CALLBACK_INPUT}:{}", free.id)),
        TelegramCallbackAction::RequestInput
    );
}

#[test]
fn resume_skips_already_delivered_sequences() {
    assert!(should_deliver_sequence(None, 0));
    assert!(!should_deliver_sequence(Some(3), 3));
    assert!(should_deliver_sequence(Some(3), 4));
}

#[test]
fn long_text_splits_instead_of_dropping_the_tail() {
    let over: String = "字".repeat(TELEGRAM_TEXT_LIMIT + 8);
    let chunks = split_channel_text(&over, TELEGRAM_TEXT_LIMIT);
    assert!(chunks.len() >= 2);
    assert_eq!(chunks.concat().chars().count(), over.chars().count());
    let paragraphs = "第一段\n\n第二段很长很长很长";
    let split = split_channel_text(paragraphs, 6);
    assert!(split.iter().any(|chunk| chunk.contains("第一段")));
    assert!(split.iter().any(|chunk| chunk.contains("第二段")));
}

#[test]
fn structured_result_becomes_readable_text() {
    let table = format_channel_result(
        "查到两班车",
        Some(&serde_json::json!([
            {"from": "上海", "to": "杭州"},
            {"from": "杭州", "to": "宁波"}
        ])),
        Some(&serde_json::json!({
            "type": "table",
            "columns": [
                {"field": "from", "title": "出发"},
                {"field": "to", "title": "到达"}
            ]
        })),
    );
    assert!(table.contains("查到两班车"));
    assert!(table.contains("出发 | 到达"));
    assert!(table.contains("上海 | 杭州"));
    let chart = format_channel_result(
        "",
        Some(&serde_json::json!([{"day": "周一", "n": 3}])),
        Some(&serde_json::json!({
            "type": "chart",
            "chartType": "bar",
            "xField": "day",
            "yField": "n"
        })),
    );
    assert!(chart.contains("图表（bar）"));
    assert!(chart.contains("周一：3"));
}

#[test]
fn chat_envelope_is_not_dumped_as_key_value() {
    let with_message = format_channel_result(
        "pong，我在。",
        Some(&serde_json::json!({ "reply": "pong，我在。", "type": "chat" })),
        None,
    );
    assert_eq!(with_message, "pong，我在。");
    let reply_only = format_channel_result(
        "",
        Some(&serde_json::json!({
            "reply": "你好。",
            "type": "chat",
            "mode": "chat"
        })),
        None,
    );
    assert_eq!(reply_only, "你好。");
    assert!(!reply_only.contains("type"));
    assert!(!reply_only.contains("mode"));
}

#[test]
fn channel_commands_are_exact_tokens() {
    assert_eq!(parse_channel_command("停止"), Some(ChannelCommand::Stop));
    assert_eq!(
        parse_channel_command("/new"),
        Some(ChannelCommand::NewConversation)
    );
    assert_eq!(
        parse_channel_command("查看当前任务"),
        Some(ChannelCommand::Status)
    );
    assert_eq!(parse_channel_command("请停止订票"), None);
    assert_eq!(encode_pending_id([0, 0, 0, 1]), "00000001");
    assert!(parse_telegram_callback("y:abcd1234").is_some());
    let _ = PENDING_STALE_REPLY;
}

#[test]
fn model_json_with_suggestions_becomes_a_clarify_prompt() {
    let prompt = pending_prompt_from_model_json(
        r#"{
          "intent": "clarification",
          "message": "【步骤 1 · clarification】\n测试用，选一个城市",
          "clarifications_needed": [
            { "question": "测试用，选一个城市", "suggestions": ["东京", "京都"] }
          ]
        }"#,
        "测试脚本",
    )
    .expect("json prompt");
    assert_eq!(prompt.question, "测试用，选一个城市");
    assert_eq!(prompt.options[0].label, "东京");
    assert_eq!(prompt.options[1].label, "京都");
    assert!(matches!(prompt.kind, PendingKind::Clarify { .. }));
    assert!(pending_prompt_from_model_json("票已订好", "订票").is_none());
}

#[test]
fn telegram_get_updates_keeps_private_callback() {
    let body = r#"{
        "ok": true,
        "result": [
            {
                "update_id": 20,
                "callback_query": {
                    "id": "cb1",
                    "from": {"id": 1001},
                    "data": "o:0",
                    "message": {
                        "message_id": 5,
                        "chat": {"id": 1001, "type": "private"}
                    }
                }
            },
            {
                "update_id": 21,
                "callback_query": {
                    "id": "cb2",
                    "from": {"id": 2002},
                    "data": "o:0",
                    "message": {
                        "message_id": 6,
                        "chat": {"id": -100, "type": "group"}
                    }
                }
            }
        ]
    }"#;
    let inbounds = parse_telegram_private_inbounds(200, body).expect("updates");
    assert_eq!(inbounds.len(), 1);
    match &inbounds[0] {
        TelegramPrivateInbound::Callback(callback) => {
            assert_eq!(callback.update_id, 20);
            assert_eq!(callback.from_id, 1001);
            assert_eq!(callback.chat_id, 1001);
            assert_eq!(callback.data, "o:0");
            assert_eq!(callback.callback_query_id, "cb1");
        }
        other => panic!("{other:?}"),
    }
    assert!(parse_telegram_private_texts(200, body)
        .expect("texts")
        .is_empty());
}
