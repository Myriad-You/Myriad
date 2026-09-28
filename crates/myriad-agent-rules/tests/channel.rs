//! Public boundary of the channel transport adapter.
//!
//! These tests do not simulate the QQ Gateway or Telegram HTTP. They prove
//! that inbound text becomes a Work request or a pairing reply, and that a
//! finished turn is delivered, dropped, or failed visibly.

use myriad_agent_rules::channel::{
    CHANNEL_HELP_REPLY, CHANNEL_IMAGE_LIMIT, CONFIRM_HINT, ChannelCommand, ChannelEvent,
    ChannelImageRef, ConnectFailure, ConnectFailureKind, DISCORD_CHANNEL_TYPE_DM,
    DISCORD_CHANNEL_TYPE_GROUP_DM, DISCORD_DIRECT_MESSAGES, DISCORD_PAIRING_TAKEN_REPLY,
    DISCORD_TEXT_LIMIT, DeliveryContext, DeliveryPlan, FEISHU_CARD_ACTION_TRIGGER,
    FEISHU_MESSAGE_RECEIVE_V1, FEISHU_PAIRING_TAKEN_REPLY, FEISHU_TEXT_LIMIT, GROUP_AND_C2C_EVENT,
    InboundC2cText, InboundDecision, PAIRING_INVALID_REPLY, PAIRING_OK_REPLY,
    PAIRING_REQUIRED_REPLY, PAIRING_TAKEN_REPLY, PANEL_REQUIRED_REPLY, PENDING_EXPIRED_REPLY,
    PENDING_STALE_REPLY, PairingBindResult, PairingLookup, PendingDecision, PendingKind,
    PendingOption, PendingPrompt, TELEGRAM_CALLBACK_INPUT, TELEGRAM_CALLBACK_NO,
    TELEGRAM_CALLBACK_YES, TELEGRAM_INPUT_BUTTON, TELEGRAM_PAIRING_TAKEN_REPLY,
    TELEGRAM_TEXT_LIMIT, TelegramCallbackAction, TelegramPrivateInbound, WorkerIntent,
    channel_can_finish, clarify_base_input, clarify_followup, classify_connect_failure,
    classify_discord_rest, classify_feishu_handshake, classify_feishu_token_code,
    classify_gateway_close, collect_channel_image_urls, decide_pending_reply,
    discord_dm_capabilities, discord_private_component_from_create,
    discord_private_text_from_create, discord_reply_markup, discord_worker_intent,
    encode_pairing_code, encode_pending_id, ensure_pending_id, extract_pairing_code,
    feishu_dm_capabilities, feishu_identity_keys, feishu_photo_messages, feishu_reply_markup,
    feishu_text_from_content, feishu_token_needs_refresh, feishu_worker_intent,
    format_channel_result, format_pairing_code, format_pending_prompt, ingest_c2c_text,
    ingest_channel_text, next_passive_seq, outbound_idempotency_key, pairing_bind_reply,
    pairing_bind_reply_for, panel_entry_reply, parse_access_token_response, parse_channel_command,
    parse_discord_channel_type, parse_feishu_api_code, parse_feishu_card_callback,
    parse_feishu_event_envelope, parse_feishu_message_receive, parse_feishu_tenant_token,
    parse_feishu_ws_endpoint, parse_gateway_url_response, parse_qq_c2c_images, parse_qq_file_info,
    parse_telegram_bot_identity, parse_telegram_callback, parse_telegram_file_path,
    parse_telegram_ok_payload, parse_telegram_private_inbounds, parse_telegram_private_texts,
    pending_prompt_from_model_json, plan_delivery, qq_c2c_capabilities, qq_token_needs_refresh,
    session_key, should_deliver_sequence, split_channel_text, task_started_reply,
    telegram_callback_action, telegram_dm_capabilities, telegram_inline_keyboard,
    telegram_max_update_id, telegram_reply_markup, telegram_retry_after, telegram_worker_intent,
    truncate_feishu_text, truncate_telegram_text, worker_intent,
};

fn text(msg_id: &str, openid: &str, content: &str) -> InboundC2cText {
    InboundC2cText {
        msg_id: msg_id.to_string(),
        user_openid: openid.to_string(),
        content: content.to_string(),
        images: Vec::new(),
    }
}

fn window(msg_id: &str) -> DeliveryContext {
    DeliveryContext {
        inbound_msg_id: Some(msg_id.to_string()),
        passive_window_open: true,
        remaining_passive_replies: 4,
        typing: false,
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
fn private_chat_capabilities_send_final_images() {
    let caps = qq_c2c_capabilities();
    assert!(telegram_dm_capabilities().inbound_text);
    assert!(telegram_dm_capabilities().outbound_final_text);
    assert!(telegram_dm_capabilities().outbound_image);
    assert!(!telegram_dm_capabilities().outbound_edit);
    assert!(!telegram_dm_capabilities().outbound_streaming_draft);
    assert!(telegram_dm_capabilities().interactive);
    assert!(telegram_dm_capabilities().inbound_callback);
    assert!(caps.inbound_text);
    assert!(caps.outbound_final_text);
    assert!(caps.inbound_media);
    assert!(telegram_dm_capabilities().inbound_media);
    assert!(discord_dm_capabilities().inbound_media);
    assert!(!caps.inbound_callback);
    assert!(!caps.outbound_markdown);
    assert!(caps.outbound_image);
    assert!(!caps.outbound_edit);
    assert!(!caps.outbound_streaming_draft);
    assert!(caps.interactive);
    assert!(!caps.frontend_action);
    assert!(discord_dm_capabilities().outbound_image);
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
fn task_start_is_one_notice_only_without_typing() {
    let qq = window("m4b");
    assert_eq!(
        plan_delivery(&ChannelEvent::TaskStarted { total_steps: 3 }, &qq),
        DeliveryPlan::PassiveText {
            content: task_started_reply(3),
            msg_id: "m4b".into(),
            image_urls: Vec::new(),
        }
    );
    assert!(task_started_reply(3).contains("共 3 步"));
    assert!(!task_started_reply(1).contains("步，"));

    let typing = DeliveryContext {
        inbound_msg_id: None,
        passive_window_open: false,
        remaining_passive_replies: 0,
        typing: true,
    };
    assert_eq!(
        plan_delivery(&ChannelEvent::TaskStarted { total_steps: 3 }, &typing),
        DeliveryPlan::Drop
    );
}

#[test]
fn final_answer_becomes_one_c2c_text() {
    let plan = plan_delivery(
        &ChannelEvent::Answer {
            message: "票已订好".into(),
            image_urls: Vec::new(),
        },
        &window("m5"),
    );
    assert_eq!(
        plan,
        DeliveryPlan::PassiveText {
            content: "票已订好".into(),
            msg_id: "m5".into(),
            image_urls: Vec::new(),
        }
    );
}

#[test]
fn final_answer_with_images_keeps_text_and_image_urls() {
    let urls = vec!["/api/phantasi/image-cache/ab/abcd.png".into()];
    let plan = plan_delivery(
        &ChannelEvent::Answer {
            message: "图片已经生成好了".into(),
            image_urls: urls.clone(),
        },
        &window("m-img"),
    );
    assert_eq!(
        plan,
        DeliveryPlan::PassiveText {
            content: "图片已经生成好了".into(),
            msg_id: "m-img".into(),
            image_urls: urls,
        }
    );
}

#[test]
fn collect_channel_image_urls_reads_envelope_and_step_history() {
    let urls = collect_channel_image_urls(&serde_json::json!({
        "data": {
            "format": "image",
            "value": { "url": "/api/phantasi/image-cache/ab/abcd.png" }
        },
        "task": {
            "stepHistory": [
                { "imageUrl": "https://cdn.example/a.png" },
                { "imageUrl": "/api/phantasi/image-cache/ab/abcd.png" },
                { "imageUrl": "javascript:alert(1)" },
                { "imageUrl": "data:image/png;base64,xx" }
            ]
        }
    }));
    assert_eq!(
        urls,
        vec![
            "/api/phantasi/image-cache/ab/abcd.png",
            "https://cdn.example/a.png",
        ]
    );
    assert!(collect_channel_image_urls(&serde_json::json!({ "message": "ok" })).is_empty());
    let capped = collect_channel_image_urls(&serde_json::json!({
        "task": {
            "stepHistory": [
                { "imageUrl": "https://cdn.example/1.png" },
                { "imageUrl": "https://cdn.example/2.png" },
                { "imageUrl": "https://cdn.example/3.png" },
                { "imageUrl": "https://cdn.example/4.png" },
                { "imageUrl": "https://cdn.example/5.png" }
            ]
        }
    }));
    assert_eq!(capped.len(), CHANNEL_IMAGE_LIMIT);
    assert_eq!(
        capped.last().map(String::as_str),
        Some("https://cdn.example/4.png")
    );
}

#[test]
fn collect_channel_image_urls_keeps_public_assets_and_drops_private_content() {
    let urls = collect_channel_image_urls(&serde_json::json!({
        "task": {
            "stepHistory": [
                { "imageUrl": "/media/assets/11111111-1111-1111-1111-111111111111/generated.png" },
                { "imageUrl": "/api/media/7/content" }
            ]
        }
    }));
    assert_eq!(
        urls,
        vec!["/media/assets/11111111-1111-1111-1111-111111111111/generated.png"]
    );
}

#[test]
fn qq_file_info_comes_from_upload_json() {
    assert_eq!(
        parse_qq_file_info(200, r#"{"file_info":"INFO_1","ttl":600}"#).unwrap(),
        "INFO_1"
    );
    assert_eq!(
        parse_qq_file_info(200, r#"{"data":{"file_info":"INFO_2"}}"#).unwrap(),
        "INFO_2"
    );
    assert!(parse_qq_file_info(400, r#"{"message":"bad"}"#).is_err());
    assert!(parse_qq_file_info(200, r#"{"ok":true}"#).is_err());
}

#[test]
fn inbound_images_are_kept_and_non_images_drop() {
    let qq = parse_qq_c2c_images(&serde_json::json!({
        "attachments": [
            {
                "url": "https://cdn.example/a.png",
                "content_type": "image/png",
                "filename": "a.png",
                "size": 12
            },
            {
                "url": "https://cdn.example/n.txt",
                "content_type": "text/plain",
                "filename": "n.txt"
            },
            { "url": "javascript:alert(1)", "content_type": "image/png" }
        ]
    }));
    assert_eq!(qq.len(), 1);
    assert_eq!(qq[0].url, "https://cdn.example/a.png");
    assert_eq!(qq[0].name, "a.png");
    assert_eq!(qq[0].mime, "image/png");
    assert_eq!(qq[0].size, 12);

    let photo = serde_json::json!({
        "ok": true,
        "result": [{
            "update_id": 20,
            "message": {
                "message_id": 8,
                "from": {"id": 1001},
                "chat": {"id": 1001, "type": "private"},
                "caption": "看看这张",
                "photo": [
                    {"file_id": "small", "width": 10, "height": 10},
                    {"file_id": "big", "width": 100, "height": 80, "file_size": 2048}
                ]
            }
        }]
    });
    let texts = parse_telegram_private_texts(200, &photo.to_string()).expect("photo");
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].text, "看看这张");
    assert_eq!(texts[0].images.len(), 1);
    assert_eq!(texts[0].images[0].url, "tg:big");
    assert_eq!(texts[0].images[0].size, 2048);
    assert_eq!(
        parse_telegram_file_path(
            200,
            r#"{"ok":true,"result":{"file_path":"photos/big.jpg"}}"#
        )
        .as_deref(),
        Ok("photos/big.jpg")
    );

    let dm = serde_json::json!({
        "id": "11",
        "channel_id": "22",
        "channel_type": 1,
        "author": { "id": "33", "bot": false },
        "content": "",
        "attachments": [{
            "id": "att1",
            "filename": "cat.png",
            "content_type": "image/png",
            "size": 44,
            "url": "https://cdn.discordapp.com/attachments/1/2/cat.png"
        }]
    });
    let inbound = discord_private_text_from_create(&dm, "99").expect("image dm");
    assert_eq!(inbound.text, "");
    assert_eq!(inbound.images.len(), 1);
    assert_eq!(inbound.images[0].name, "cat.png");
}

#[test]
fn paired_image_only_c2c_still_starts_work() {
    let mut event = text("m-img", "openid-b", "");
    event.images = vec![ChannelImageRef {
        url: "https://cdn.example/a.png".into(),
        name: "a.png".into(),
        mime: "image/png".into(),
        size: 12,
    }];
    let decision = ingest_c2c_text(&event, PairingLookup::Paired { user_id: 7 }, false);
    match decision {
        InboundDecision::StartWork { input, user_id, .. } => {
            assert_eq!(user_id, 7);
            assert!(input.is_empty());
        }
        other => panic!("{other:?}"),
    }
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
        typing: false,
    };
    assert_eq!(
        plan_delivery(
            &ChannelEvent::Answer {
                message: "做好了".into(),
                image_urls: Vec::new(),
            },
            &closed
        ),
        DeliveryPlan::ActiveText {
            content: "做好了".into(),
            image_urls: Vec::new(),
        }
    );

    let exhausted = DeliveryContext {
        inbound_msg_id: Some("m7".into()),
        passive_window_open: true,
        remaining_passive_replies: 0,
        typing: false,
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
            image_urls: Vec::new(),
        }
    );

    let proactive = DeliveryContext {
        inbound_msg_id: None,
        passive_window_open: false,
        remaining_passive_replies: 0,
        typing: false,
    };
    assert_eq!(
        plan_delivery(
            &ChannelEvent::Answer {
                message: "测一条".into(),
                image_urls: Vec::new(),
            },
            &proactive
        ),
        DeliveryPlan::ActiveText {
            content: "测一条".into(),
            image_urls: Vec::new(),
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
    assert!(discord_dm_capabilities().inbound_text);
    assert_eq!(DISCORD_TEXT_LIMIT, 2000);
}

#[test]
fn discord_dm_create_is_kept_and_guild_is_dropped() {
    let dm = serde_json::json!({
        "id": "11",
        "channel_id": "22",
        "channel_type": DISCORD_CHANNEL_TYPE_DM,
        "author": { "id": "33", "bot": false },
        "content": "hello"
    });
    let inbound = discord_private_text_from_create(&dm, "99").expect("dm");
    assert_eq!(inbound.author_id, "33");
    assert!(inbound.images.is_empty());
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
fn discord_group_dm_and_missing_channel_type_are_dropped() {
    let group = serde_json::json!({
        "id": "11",
        "channel_id": "22",
        "channel_type": DISCORD_CHANNEL_TYPE_GROUP_DM,
        "author": { "id": "33", "bot": false },
        "content": "hello"
    });
    assert!(discord_private_text_from_create(&group, "99").is_none());
    let via_channel = serde_json::json!({
        "id": "11",
        "channel_id": "22",
        "channel": { "id": "22", "type": DISCORD_CHANNEL_TYPE_GROUP_DM },
        "author": { "id": "33", "bot": false },
        "content": "hello"
    });
    assert!(discord_private_text_from_create(&via_channel, "99").is_none());
    let missing = serde_json::json!({
        "id": "11",
        "channel_id": "22",
        "author": { "id": "33", "bot": false },
        "content": "hello"
    });
    assert!(discord_private_text_from_create(&missing, "99").is_none());
    assert_eq!(
        parse_discord_channel_type(r#"{"id":"22","type":1}"#),
        Some(1)
    );
    assert_eq!(
        parse_discord_channel_type(r#"{"id":"22","type":3}"#),
        Some(DISCORD_CHANNEL_TYPE_GROUP_DM)
    );

    let dm_button = serde_json::json!({
        "type": 3,
        "id": "i1",
        "token": "tok",
        "channel_id": "22",
        "channel": { "id": "22", "type": DISCORD_CHANNEL_TYPE_DM },
        "user": { "id": "33" },
        "data": { "custom_id": "y:abc" },
        "message": { "id": "m1" }
    });
    let component = discord_private_component_from_create(&dm_button).expect("dm button");
    assert_eq!(component.author_id, "33");
    assert_eq!(component.custom_id, "y:abc");

    let group_button = serde_json::json!({
        "type": 3,
        "id": "i1",
        "token": "tok",
        "channel_id": "22",
        "channel_type": DISCORD_CHANNEL_TYPE_GROUP_DM,
        "user": { "id": "33" },
        "data": { "custom_id": "y:abc" },
        "message": { "id": "m1" }
    });
    assert!(discord_private_component_from_create(&group_button).is_none());
    let bare_button = serde_json::json!({
        "type": 3,
        "id": "i1",
        "token": "tok",
        "channel_id": "22",
        "user": { "id": "33" },
        "data": { "custom_id": "y:abc" },
        "message": { "id": "m1" }
    });
    assert!(discord_private_component_from_create(&bare_button).is_none());
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
    assert_eq!(parse_channel_command("/start"), Some(ChannelCommand::Help));
    assert_eq!(parse_channel_command("帮助"), Some(ChannelCommand::Help));
    assert_eq!(parse_channel_command("请停止订票"), None);
    assert_eq!(parse_channel_command("帮助我订票"), None);
    assert!(CHANNEL_HELP_REPLY.contains("当前任务"));
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
    assert!(
        parse_telegram_private_texts(200, body)
            .expect("texts")
            .is_empty()
    );
}

#[test]
fn feishu_p2p_text_parses_open_id_and_content() {
    let event = serde_json::json!({
        "sender": {
            "sender_id": { "open_id": "ou_user", "union_id": "on_x", "user_id": "u_1" },
            "sender_type": "user",
            "tenant_key": "t"
        },
        "message": {
            "message_id": "om_msg",
            "root_id": "",
            "parent_id": "",
            "chat_id": "oc_chat",
            "chat_type": "p2p",
            "message_type": "text",
            "content": "{\"text\":\"帮我查天气\"}",
            "create_time": "1603977298000"
        }
    });
    let parsed = parse_feishu_message_receive("evt-1", &event).expect("p2p text");
    assert_eq!(parsed.event_id, "evt-1");
    assert_eq!(parsed.message_id, "om_msg");
    assert_eq!(parsed.open_id, "ou_user");
    assert_eq!(
        parsed.identity_keys,
        vec!["ou_user".to_string(), "u_1".to_string()]
    );
    assert_eq!(parsed.chat_id, "oc_chat");
    assert_eq!(parsed.content, "帮我查天气");
    assert!(parsed.images.is_empty());
    assert_eq!(parsed.inbound().user_openid, "ou_user");
    assert_eq!(parsed.chat_id_key(), "oc_chat");
}

#[test]
fn feishu_identity_keys_prefer_open_id_and_keep_user_id_alias() {
    assert_eq!(
        feishu_identity_keys(Some("ou_user"), Some("u_1")),
        vec!["ou_user".to_string(), "u_1".to_string()]
    );
    assert_eq!(
        feishu_identity_keys(None, Some("u_emp")),
        vec!["u_emp".to_string()]
    );
    assert_eq!(
        feishu_identity_keys(Some("ou_user"), Some("ou_user")),
        vec!["ou_user".to_string()]
    );
    assert!(feishu_identity_keys(None, None).is_empty());
    assert!(feishu_identity_keys(Some("  "), Some("")).is_empty());
}

#[test]
fn feishu_missing_open_id_falls_back_to_user_id() {
    let event = serde_json::json!({
        "sender": {
            "sender_id": { "user_id": "u_emp" },
            "sender_type": "user"
        },
        "message": {
            "message_id": "om_msg",
            "chat_id": "oc_chat",
            "chat_type": "p2p",
            "message_type": "text",
            "content": "{\"text\":\"hi\"}"
        }
    });
    let parsed = parse_feishu_message_receive("evt-2", &event).expect("fallback");
    assert_eq!(parsed.open_id, "u_emp");
    assert_eq!(parsed.identity_keys, vec!["u_emp".to_string()]);
}

#[test]
fn feishu_group_and_bot_messages_drop() {
    let group = serde_json::json!({
        "sender": {
            "sender_id": { "open_id": "ou_g" },
            "sender_type": "user"
        },
        "message": {
            "message_id": "om_g",
            "chat_id": "oc_g",
            "chat_type": "group",
            "message_type": "text",
            "content": "{\"text\":\"群\"}"
        }
    });
    assert!(parse_feishu_message_receive("e", &group).is_none());

    let bot = serde_json::json!({
        "sender": {
            "sender_id": { "open_id": "ou_bot" },
            "sender_type": "app"
        },
        "message": {
            "message_id": "om_b",
            "chat_id": "oc_b",
            "chat_type": "p2p",
            "message_type": "text",
            "content": "{\"text\":\"bot\"}"
        }
    });
    assert!(parse_feishu_message_receive("e", &bot).is_none());
}

#[test]
fn feishu_image_message_becomes_image_ref() {
    let event = serde_json::json!({
        "sender": {
            "sender_id": { "open_id": "ou_img" },
            "sender_type": "user"
        },
        "message": {
            "message_id": "om_img",
            "chat_id": "oc_img",
            "chat_type": "p2p",
            "message_type": "image",
            "content": "{\"image_key\":\"img_abc\"}"
        }
    });
    let parsed = parse_feishu_message_receive("evt-img", &event).expect("image");
    assert!(parsed.content.is_empty());
    assert_eq!(parsed.images.len(), 1);
    assert_eq!(parsed.images[0].url, "feishu:om_img/img_abc");
}

#[test]
fn feishu_unknown_message_type_drops() {
    let file = serde_json::json!({
        "sender": {
            "sender_id": { "open_id": "ou_file" },
            "sender_type": "user"
        },
        "message": {
            "message_id": "om_file",
            "chat_id": "oc_file",
            "chat_type": "p2p",
            "message_type": "file",
            "content": "{\"file_key\":\"file_abc\"}"
        }
    });
    assert!(parse_feishu_message_receive("e", &file).is_none());

    let post = serde_json::json!({
        "sender": {
            "sender_id": { "open_id": "ou_post" },
            "sender_type": "user"
        },
        "message": {
            "message_id": "om_post",
            "chat_id": "oc_post",
            "chat_type": "p2p",
            "message_type": "post",
            "content": "{\"zh_cn\":{\"title\":\"x\",\"content\":[]}}"
        }
    });
    assert!(parse_feishu_message_receive("e", &post).is_none());

    let sticker = serde_json::json!({
        "sender": {
            "sender_id": { "open_id": "ou_sticker" },
            "sender_type": "user"
        },
        "message": {
            "message_id": "om_sticker",
            "chat_id": "oc_sticker",
            "chat_type": "p2p",
            "message_type": "sticker",
            "content": "{\"file_key\":\"sticker_abc\"}"
        }
    });
    assert!(parse_feishu_message_receive("e", &sticker).is_none());
}

#[test]
fn feishu_card_callback_parses_operator_and_value_data() {
    let event = serde_json::json!({
        "operator": { "open_id": "ou_op", "user_id": "u_op" },
        "action": { "value": { "data": "y:prompt1" }, "tag": "button" },
        "context": { "open_message_id": "om_c", "open_chat_id": "oc_c" }
    });
    let callback = parse_feishu_card_callback("evt-cb", &event).expect("callback");
    assert_eq!(callback.event_id, "evt-cb");
    assert_eq!(callback.open_id, "ou_op");
    assert_eq!(
        callback.identity_keys,
        vec!["ou_op".to_string(), "u_op".to_string()]
    );
    assert_eq!(callback.chat_id, "oc_c");
    assert_eq!(callback.data, "y:prompt1");
    assert!(
        parse_feishu_card_callback("e", &serde_json::json!({ "operator": { "open_id": "ou" } }))
            .is_none()
    );

    let fallback = parse_feishu_card_callback(
        "evt-cb-user",
        &serde_json::json!({
            "operator": { "user_id": "u_only" },
            "action": { "value": { "data": "y:prompt1" } },
            "context": { "open_chat_id": "oc_c" }
        }),
    )
    .expect("user_id callback");
    assert_eq!(fallback.open_id, "u_only");
    assert_eq!(fallback.identity_keys, vec!["u_only".to_string()]);
}

#[test]
fn feishu_reply_markup_renders_card_buttons() {
    let prompt = choice_prompt();
    let markup = feishu_reply_markup(&prompt).expect("markup");
    let elements = markup
        .get("elements")
        .and_then(|v| v.as_array())
        .expect("elements");
    assert_eq!(elements.len(), 2);
    let button = &elements[0];
    assert_eq!(button.get("tag").and_then(|v| v.as_str()), Some("button"));
    let data = button
        .get("value")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_str())
        .expect("callback data");
    assert!(data.starts_with("o:") && data.ends_with(":0"), "{data}");
}

#[test]
fn feishu_photo_messages_follow_image_with_card() {
    let image_only = feishu_photo_messages("img_key", None);
    assert_eq!(image_only.len(), 1);
    assert_eq!(image_only[0].0, "image");
    assert_eq!(
        image_only[0].1.get("image_key").and_then(|v| v.as_str()),
        Some("img_key")
    );

    let markup = feishu_reply_markup(&choice_prompt()).expect("markup");
    let with_card = feishu_photo_messages("img_key", Some(markup.clone()));
    assert_eq!(with_card.len(), 2);
    assert_eq!(with_card[0].0, "image");
    assert_eq!(with_card[1].0, "interactive");
    assert_eq!(with_card[1].1, markup);
}

#[test]
fn feishu_capabilities_open_buttons_and_images_but_not_edit() {
    let caps = feishu_dm_capabilities();
    assert!(caps.inbound_text);
    assert!(caps.inbound_media);
    assert!(caps.inbound_callback);
    assert!(caps.outbound_final_text);
    assert!(caps.outbound_image);
    assert!(caps.interactive);
    assert!(!caps.outbound_edit);
    assert!(!caps.outbound_streaming_draft);
    assert!(!caps.outbound_markdown);
    assert!(!caps.frontend_action);
}

#[test]
fn feishu_text_cap_and_pairing_taken_reply() {
    assert_eq!(truncate_feishu_text("短").len(), "短".len());
    assert!(
        truncate_feishu_text(&"长".repeat(FEISHU_TEXT_LIMIT + 10))
            .chars()
            .count()
            <= FEISHU_TEXT_LIMIT
    );
    assert_eq!(
        pairing_bind_reply_for(PairingBindResult::OpenidTaken, "feishu"),
        FEISHU_PAIRING_TAKEN_REPLY
    );
    assert_eq!(FEISHU_MESSAGE_RECEIVE_V1, "im.message.receive_v1");
    assert_eq!(FEISHU_CARD_ACTION_TRIGGER, "card.action.trigger");
}

#[test]
fn feishu_text_content_keeps_raw_on_bad_json() {
    assert_eq!(feishu_text_from_content("{\"text\":\"hi\"}"), "hi");
    assert_eq!(feishu_text_from_content("plain"), "plain");
    assert_eq!(feishu_text_from_content("{\"other\":1}"), "{\"other\":1}");
    assert_eq!(feishu_text_from_content(""), "");
}

#[test]
fn feishu_worker_intent_matches_qq_shape() {
    assert_eq!(feishu_worker_intent(true, "cli_a", true), WorkerIntent::Run);
    assert_eq!(
        feishu_worker_intent(true, "cli_a", false),
        WorkerIntent::Stop
    );
    assert_eq!(
        feishu_worker_intent(false, "cli_a", true),
        WorkerIntent::Stop
    );
}

#[test]
fn feishu_tenant_token_classifies_credential_and_transient() {
    assert_eq!(
        classify_feishu_token_code(99991664),
        ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_feishu_token_code(99999),
        ConnectFailureKind::Transient
    );
    assert!(feishu_token_needs_refresh(99991663));
    assert!(!feishu_token_needs_refresh(99991664));
    let (token, ttl) = parse_feishu_tenant_token(
        200,
        r#"{"code":0,"tenant_access_token":"t-abc","expire":7200}"#,
    )
    .expect("token");
    assert_eq!(token, "t-abc");
    assert_eq!(ttl, 7200);
    assert_eq!(
        parse_feishu_tenant_token(200, r#"{"code":99991664,"msg":"invalid"}"#),
        Err(ConnectFailureKind::Permanent)
    );
    assert_eq!(
        parse_feishu_tenant_token(503, "busy"),
        Err(ConnectFailureKind::Transient)
    );
}

#[test]
fn feishu_ws_endpoint_and_handshake_and_envelope() {
    let (url, service_id, ping) = parse_feishu_ws_endpoint(
        200,
        r#"{"code":0,"data":{"URL":"wss://open.feishu.cn/ws?service_id=7","ClientConfig":{"PingInterval":60}}}"#,
    )
    .expect("endpoint");
    assert!(url.contains("service_id=7"));
    assert_eq!(service_id, 7);
    assert_eq!(ping, 60);
    assert_eq!(
        classify_feishu_handshake(403, 0),
        ConnectFailureKind::Permanent
    );
    assert_eq!(
        classify_feishu_handshake(514, 1_000_040_350),
        ConnectFailureKind::Transient
    );
    assert_eq!(
        classify_feishu_handshake(514, 0),
        ConnectFailureKind::Permanent
    );
    let payload = serde_json::json!({
        "schema": "2.0",
        "header": {
            "event_id": "evt-1",
            "event_type": "im.message.receive_v1"
        },
        "event": { "message": { "chat_type": "p2p" } }
    });
    let (kind, id, event) =
        parse_feishu_event_envelope(payload.to_string().as_bytes()).expect("envelope");
    assert_eq!(kind, FEISHU_MESSAGE_RECEIVE_V1);
    assert_eq!(id, "evt-1");
    assert_eq!(
        event
            .get("message")
            .and_then(|m| m.get("chat_type"))
            .and_then(|v| v.as_str()),
        Some("p2p")
    );
    assert_eq!(
        parse_feishu_api_code(200, r#"{"code":99991663}"#),
        Err(ConnectFailureKind::Transient)
    );
}

#[test]
fn answer_buttons_are_bound_to_task_not_only_question_text() {
    let mut first = myriad_agent_rules::channel::PendingPrompt {
        id: String::new(),
        kind: myriad_agent_rules::channel::PendingKind::Answer {
            task_id: "task-a".into(),
            question_id: "pre_param:1:city".into(),
            question_type: "free_text".into(),
        },
        question: "城市？".into(),
        options: Vec::new(),
        expires_at_unix: None,
    };
    let mut second = first.clone();
    second.kind = myriad_agent_rules::channel::PendingKind::Answer {
        task_id: "task-b".into(),
        question_id: "pre_param:1:city".into(),
        question_type: "free_text".into(),
    };
    myriad_agent_rules::channel::ensure_pending_id(&mut first);
    myriad_agent_rules::channel::ensure_pending_id(&mut second);
    assert_ne!(first.id, second.id);
}

mod telegram_groups {
    use myriad_agent_rules::channel::{TelegramBotIdentity, parse_telegram_group_messages};

    fn bot() -> TelegramBotIdentity {
        TelegramBotIdentity {
            id: 777,
            first_name: "小灯".into(),
            username: Some("xiaodeng_bot".into()),
        }
    }

    fn body(messages: &[&str]) -> String {
        let updates: Vec<String> = messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                format!(r#"{{"update_id": {}, "message": {message}}}"#, 100 + index)
            })
            .collect();
        format!(r#"{{"ok": true, "result": [{}]}}"#, updates.join(","))
    }

    #[test]
    fn a_group_line_is_addressed_by_mention_text_mention_command_or_reply() {
        let body = body(&[
            // @mention of the bot, after an emoji (UTF-16 offsets).
            r#"{"message_id": 1, "from": {"id": 11, "first_name": "阿明"}, "chat": {"id": -100, "type": "supergroup"},
                "text": "🎉 @XiaoDeng_bot 周五去哪", "entities": [{"type": "mention", "offset": 3, "length": 13}]}"#,
            // text_mention of the bot by id.
            r#"{"message_id": 2, "from": {"id": 12, "first_name": "小红"}, "chat": {"id": -100, "type": "supergroup"},
                "text": "小灯 你觉得呢", "entities": [{"type": "text_mention", "offset": 0, "length": 2, "user": {"id": 777}}]}"#,
            // Reply to one of her messages.
            r#"{"message_id": 3, "from": {"id": 13, "username": "zhou"}, "chat": {"id": -100, "type": "group"},
                "text": "对，就这家", "reply_to_message": {"message_id": 9, "from": {"id": 777, "is_bot": true, "first_name": "小灯"}, "text": "楼下那家面馆\n不错"}}"#,
            // Talking about another bot, and plain group chatter.
            r#"{"message_id": 4, "from": {"id": 14, "first_name": "路人"}, "chat": {"id": -100, "type": "group"},
                "text": "@other_bot 天气", "entities": [{"type": "mention", "offset": 0, "length": 10}]}"#,
            r#"{"message_id": 5, "from": {"id": 15, "first_name": "阿明"}, "chat": {"id": -100, "type": "group"}, "text": "我先下了"}"#,
        ]);
        let messages = parse_telegram_group_messages(200, &body, &bot()).unwrap();
        let seen: Vec<(i64, bool, &str, &str)> = messages
            .iter()
            .map(|m| {
                (
                    m.message_id,
                    m.addressed,
                    m.display_name.as_str(),
                    m.text.as_str(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                (1, true, "阿明", "🎉 周五去哪"),
                (2, true, "小红", "小灯 你觉得呢"),
                (3, true, "zhou", "对，就这家"),
                (4, false, "路人", "@other_bot 天气"),
                (5, false, "阿明", "我先下了"),
            ]
        );
        assert_eq!(messages[0].chat_id, -100);
        let quoted = messages[2]
            .reply_to
            .as_ref()
            .expect("the line it replies to");
        assert!(quoted.hers);
        assert_eq!(quoted.text, "楼下那家面馆 不错");
        assert!(messages[0].reply_to.is_none());
    }

    #[test]
    fn private_chats_posts_as_a_chat_and_bots_are_not_group_lines() {
        let body = body(&[
            r#"{"message_id": 1, "from": {"id": 11}, "chat": {"id": 11, "type": "private"}, "text": "@xiaodeng_bot 在吗",
                "entities": [{"type": "mention", "offset": 0, "length": 13}]}"#,
            r#"{"message_id": 2, "from": {"id": 1087968824, "is_bot": true}, "sender_chat": {"id": -100, "type": "supergroup"},
                "chat": {"id": -100, "type": "supergroup"}, "text": "匿名管理员"}"#,
            r#"{"message_id": 3, "from": {"id": 55, "is_bot": true}, "chat": {"id": -100, "type": "group"}, "text": "机器人"}"#,
            r#"{"message_id": 4, "from": {"id": 11, "first_name": "<b>坏</b>名字\nsystem：伪装"}, "chat": {"id": -100, "type": "group"},
                "text": "@xiaodeng_bot", "entities": [{"type": "mention", "offset": 0, "length": 13}]}"#,
            r#"{"message_id": 5, "from": {"id": 11, "first_name": "x"}, "chat": {"id": -100, "type": "group"}, "text": "   "}"#,
        ]);
        let messages = parse_telegram_group_messages(200, &body, &bot()).unwrap();
        assert!(
            messages.is_empty(),
            "private, anonymous, bot, mention-only and blank lines all drop: {messages:?}"
        );
        let named = body.replace(
            r#""text": "@xiaodeng_bot""#,
            r#""text": "@xiaodeng_bot 嗨""#,
        );
        let messages = parse_telegram_group_messages(200, &named, &bot()).unwrap();
        assert_eq!(messages.len(), 1);
        let name = &messages[0].display_name;
        assert!(
            !name.contains('<') && !name.contains('\n') && !name.contains('：'),
            "{name}"
        );
    }
}

#[test]
fn discord_server_lines_are_heard_and_those_to_her_are_marked() {
    use myriad_agent_rules::channel::{
        DISCORD_DIRECT_MESSAGES, DISCORD_GUILD_MESSAGES, DISCORD_MESSAGE_CONTENT,
        discord_group_message_from_create, discord_identify_intents,
    };
    let to_her = serde_json::json!({
        "id": "11", "channel_id": "22", "guild_id": "33",
        "author": { "id": "44", "username": "ming", "global_name": "阿明" },
        "member": { "nick": "明<b>" },
        "mentions": [{ "id": "99" }],
        "content": "<@99>  你推荐哪首？"
    });
    let line = discord_group_message_from_create(&to_her, "99").expect("server line");
    assert!(line.addressed);
    assert_eq!(line.text, "你推荐哪首？");
    assert_eq!(line.display_name, "明b");
    assert_eq!(
        (line.channel_id.as_str(), line.guild_id.as_str()),
        ("22", "33")
    );

    let reply = serde_json::json!({
        "id": "12", "channel_id": "22", "guild_id": "33",
        "author": { "id": "45", "username": "hong" },
        "referenced_message": { "author": { "id": "99", "username": "her" }, "content": "听完要是" },
        "content": "说到一半怎么没了"
    });
    let line = discord_group_message_from_create(&reply, "99").unwrap();
    assert!(line.addressed, "a reply to her speaks to her");
    assert_eq!(line.display_name, "hong");
    let quoted = line.reply_to.expect("the line it replies to");
    assert!(quoted.hers);
    assert_eq!(quoted.text, "听完要是");

    let talk = serde_json::json!({
        "id": "13", "channel_id": "22", "guild_id": "33",
        "author": { "id": "45", "username": "hong" },
        "content": "今天好热"
    });
    assert!(
        !discord_group_message_from_create(&talk, "99")
            .unwrap()
            .addressed
    );

    for dropped in [
        serde_json::json!({"id": "1", "channel_id": "22", "author": {"id": "45"}, "content": "dm"}),
        serde_json::json!({"id": "1", "channel_id": "22", "guild_id": "33", "author": {"id": "46", "bot": true}, "content": "bot"}),
        serde_json::json!({"id": "1", "channel_id": "22", "guild_id": "33", "webhook_id": "7", "author": {"id": "46"}, "content": "hook"}),
        serde_json::json!({"id": "1", "channel_id": "22", "guild_id": "33", "author": {"id": "99"}, "content": "her own"}),
        serde_json::json!({"id": "1", "channel_id": "22", "guild_id": "33", "author": {"id": "45"}, "content": "<@99>"}),
    ] {
        assert!(
            discord_group_message_from_create(&dropped, "99").is_none(),
            "{dropped}"
        );
    }

    assert_eq!(
        discord_identify_intents(true),
        DISCORD_DIRECT_MESSAGES | DISCORD_GUILD_MESSAGES | DISCORD_MESSAGE_CONTENT
    );
    assert_eq!(discord_identify_intents(false) & DISCORD_MESSAGE_CONTENT, 0);
}

/// OneBot v11 wire 层：宽松解析是移植的核心价值。
/// 各协议端（Lagrange / NapCat / LLOneBot）随时加字段，解不开的事件必须降级，绝不报错。
mod onebot_wire {
    use myriad_agent_rules::onebot::wire::{
        Inbound, RawEventJson, RespJson, WireMessage, value_as_string,
    };
    use serde_json::json;

    /// NapCat 真实私聊事件（`messagePostFormat: "array"`）。
    fn private_text_event() -> &'static str {
        r#"{
          "time": 1790000000, "self_id": 10001, "post_type": "message",
          "message_type": "private", "sub_type": "friend",
          "message_id": 1234567890, "user_id": 20002,
          "message": [{"type":"text","data":{"text":"帮我看一下"}},
                      {"type":"image","data":{"file":"x.jpg","url":"https://example.com/x.jpg"}}],
          "raw_message": "帮我看一下", "font": 0,
          "sender": {"user_id":20002,"nickname":"某人","card":""}
        }"#
    }

    #[test]
    fn private_event_parses_into_text_and_image_segments() {
        let event: RawEventJson =
            serde_json::from_str(private_text_event()).expect("a real event must decode");
        assert_eq!(event.post_type.as_deref(), Some("message"));
        assert_eq!(event.message_type.as_deref(), Some("private"));
        assert_eq!(event.sub_type.as_deref(), Some("friend"));
        assert_eq!(event.user_id, Some(20002));
        assert_eq!(event.self_id, 10001);
        assert_eq!(event.message_id, Some(1234567890));
        assert_eq!(
            event.sender.as_ref().and_then(|s| s.nickname.as_deref()),
            Some("某人")
        );

        let WireMessage::Array(segments) = event.message.expect("message present") else {
            panic!("array format must stay an array");
        };
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].kind, "text");
        assert_eq!(segments[0].str_field("text").as_deref(), Some("帮我看一下"));
        assert_eq!(segments[1].kind, "image");
        assert_eq!(
            segments[1].str_field("url").as_deref(),
            Some("https://example.com/x.jpg")
        );
    }

    /// 这是移植过来的最重要一条保证：未知顶层字段与未知段类型都不能让解析失败。
    #[test]
    fn unknown_fields_and_unknown_segments_degrade_instead_of_failing() {
        let raw = r#"{
          "post_type":"message","message_type":"private","user_id":1,
          "totally_new_field":{"nested":[1,2,3]},
          "message":[{"type":"brand_new_segment","data":{"x":1}}]
        }"#;
        let event: RawEventJson = serde_json::from_str(raw).expect("unknown fields must not fail");
        assert_eq!(event.user_id, Some(1));
        assert!(
            event.extra.contains_key("totally_new_field"),
            "the escape hatch keeps the whole payload"
        );

        let WireMessage::Array(segments) = event.message.expect("message present") else {
            panic!("array stays an array");
        };
        assert_eq!(segments[0].kind, "brand_new_segment");
    }

    #[test]
    fn sloppy_wire_types_are_read_leniently() {
        let raw = r#"{"message":[{"type":"image","data":{"file":12345,"size":"999"}}]}"#;
        let event: RawEventJson = serde_json::from_str(raw).expect("sloppy fields decode");
        let WireMessage::Array(segments) = event.message.expect("message present") else {
            panic!("array stays an array");
        };
        assert_eq!(
            segments[0].str_field("file").as_deref(),
            Some("12345"),
            "a JSON number reads as a string"
        );
        assert_eq!(
            segments[0].i64_field("size"),
            Some(999),
            "a numeric string reads as i64"
        );

        assert_eq!(value_as_string(&json!(true)).as_deref(), Some("true"));
        assert_eq!(value_as_string(&json!(7)).as_deref(), Some("7"));
        assert_eq!(value_as_string(&json!([1])), None);
    }

    #[test]
    fn cq_string_format_still_decodes() {
        let raw = r#"{"message":"[CQ:image,file=x.jpg]","message_type":"private"}"#;
        let event: RawEventJson = serde_json::from_str(raw).expect("string format decodes");
        assert!(matches!(event.message, Some(WireMessage::Cq(_))));
    }

    /// 入站帧 demux：只有 `echo` 能判别响应与事件。
    #[test]
    fn echo_is_the_only_discriminator_between_response_and_event() {
        let event: Inbound =
            serde_json::from_str(private_text_event()).expect("a real event must decode");
        assert!(
            matches!(event, Inbound::Event(_)),
            "no echo → an event, not a response"
        );

        let response = r#"{"status":"ok","retcode":0,"data":{"message_id":999},"echo":"uuid-1"}"#;
        let inbound: Inbound = serde_json::from_str(response).expect("a response must decode");
        let Inbound::Resp(envelope) = inbound else {
            panic!("an echo → a response");
        };
        assert_eq!(envelope.echo.as_str(), Some("uuid-1"));

        let full: RespJson = serde_json::from_str(response).expect("full response decodes");
        assert_eq!(full.status, "ok");
        assert_eq!(full.data["message_id"], 999);
    }

    /// NapCat `OB11Response.createResponse` 的原样成功帧：`message` 与 `wording` 同时出现。
    /// 解不开它，每个动作都等不到回执，超时后重试会把同一条消息反复发出去。
    #[test]
    fn napcat_response_with_message_and_wording_decodes() {
        let ok = r#"{"status":"ok","retcode":0,"data":{"message_id":-2147480000},"message":"","wording":"","echo":"uuid-2","stream":"normal-action"}"#;
        let full: RespJson = serde_json::from_str(ok).expect("a NapCat response decodes");
        assert_eq!(full.status, "ok");
        assert_eq!(full.retcode, 0);
        assert_eq!(full.echo, json!("uuid-2"));
        assert_eq!(full.data["message_id"], -2147480000i64);
    }

    /// NapCat `websocket-server.ts` 的 `authorize` 原样发出的拒绝帧：`echo` 是 `null`，不是缺失。
    /// 它按响应解出来，所以拒绝必须在响应分支里认出来，否则 token 写错会被当成断线无限重连。
    #[test]
    fn napcat_token_refusal_is_a_response_that_refuses_the_connection() {
        use myriad_agent_rules::onebot::rules::onebot_frame_refuses_connection;
        let refusal = r#"{"status":"failed","retcode":1403,"data":null,"message":"token验证失败","wording":"token验证失败","echo":null,"stream":"normal-action"}"#;
        let inbound: Inbound = serde_json::from_str(refusal).expect("the refusal decodes");
        let Inbound::Resp(envelope) = inbound else {
            panic!("`echo: null` is still a response");
        };
        let full: RespJson = serde_json::from_str(refusal).expect("full refusal decodes");
        assert!(onebot_frame_refuses_connection(
            &envelope.echo,
            &full.status,
            full.retcode
        ));

        // One refused action carries our echo; it fails that action, not the socket.
        assert!(!onebot_frame_refuses_connection(
            &json!("uuid-1"),
            "failed",
            1403
        ));
        // A send failure (NapCat 1200) with no echo is not a credential problem.
        assert!(!onebot_frame_refuses_connection(
            &json!(null),
            "failed",
            1200
        ));
        assert!(!onebot_frame_refuses_connection(&json!(null), "ok", 0));
    }

    /// 站点内全链路是字符串，但 int64 必须先完整保住，不能经浮点。
    #[test]
    fn int64_survives_without_going_through_a_float() {
        let raw = r#"{"user_id":9007199254740993,"message":[]}"#;
        let event: RawEventJson = serde_json::from_str(raw).expect("a large id decodes");
        let user_id = event.user_id.expect("user_id present");
        assert_eq!(user_id, 9007199254740993i64);
        assert_eq!(user_id.to_string(), "9007199254740993");
    }
}

/// OneBot 私聊事件解码成 Myriad 入站。解不开就 `None`，不 panic。
mod onebot_decode {
    use myriad_agent_rules::channel::{CHANNEL_IMAGE_LIMIT, QQ_TEXT_LIMIT};
    use myriad_agent_rules::onebot::decode::{decode_private_inbound, decode_segments_to_text};
    use myriad_agent_rules::onebot::wire::WireSegment;
    use serde_json::{Map, Value, json};

    fn private_text_event() -> &'static str {
        r#"{
          "time": 1790000000, "self_id": 10001, "post_type": "message",
          "message_type": "private", "sub_type": "friend",
          "message_id": 1234567890, "user_id": 20002,
          "message": [{"type":"text","data":{"text":"帮我看一下"}},
                      {"type":"image","data":{"file":"x.jpg","url":"https://example.com/x.jpg"}}],
          "raw_message": "帮我看一下", "font": 0,
          "sender": {"user_id":20002,"nickname":"某人","card":""}
        }"#
    }

    fn with_message(message_type: &str, sub_type: &str, post_type: &str) -> String {
        json!({
            "post_type": post_type,
            "message_type": message_type,
            "sub_type": sub_type,
            "message_id": 1,
            "user_id": 20002,
            "message": [{"type":"text","data":{"text":"hi"}}]
        })
        .to_string()
    }

    #[test]
    fn private_text_becomes_inbound() {
        let inbound = decode_private_inbound(private_text_event()).expect("private text");
        assert_eq!(inbound.user_openid, "20002");
        assert_eq!(inbound.msg_id, "1234567890");
        assert_eq!(inbound.content, "帮我看一下");
    }

    #[test]
    fn group_message_is_not_a_private_inbound() {
        assert!(decode_private_inbound(&with_message("group", "normal", "message")).is_none());
    }

    #[test]
    fn cq_string_private_message_is_not_an_inbound() {
        use myriad_agent_rules::onebot::decode::private_message_is_cq_string;
        let raw = r#"{"post_type":"message","message_type":"private","sub_type":"friend","user_id":1,"message":"[CQ:image,file=x.jpg]"}"#;
        assert!(private_message_is_cq_string(raw));
        assert!(decode_private_inbound(raw).is_none());
    }

    #[test]
    fn group_at_is_addressed_and_plain_talk_is_not() {
        use myriad_agent_rules::onebot::decode::decode_group_inbound;
        let at = json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 555,
            "user_id": 20002,
            "self_id": 10001,
            "message_id": 9,
            "sender": {"nickname": "某人", "card": "群名片"},
            "message": [
                {"type": "at", "data": {"qq": "10001"}},
                {"type": "text", "data": {"text": " 在吗 "}}
            ]
        });
        let line = decode_group_inbound(&at.to_string(), 10001).expect("at");
        assert!(line.addressed);
        assert_eq!(line.group_id, "555");
        assert_eq!(line.user_id, "20002");
        assert_eq!(line.display_name, "群名片");
        assert_eq!(line.text, "在吗");

        let plain = json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 555,
            "user_id": 20002,
            "message_id": 10,
            "message": [{"type": "text", "data": {"text": "闲聊"}}]
        });
        let line = decode_group_inbound(&plain.to_string(), 10001).expect("plain");
        assert!(!line.addressed);
        assert_eq!(line.text, "闲聊");
    }

    #[test]
    fn group_reply_to_her_is_addressed() {
        use myriad_agent_rules::onebot::decode::decode_group_inbound;
        let raw = json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 555,
            "user_id": 20002,
            "message_id": 11,
            "message": [
                {"type": "reply", "data": {"id": "8", "user_id": "10001", "text": "上一句"}},
                {"type": "text", "data": {"text": "接着说"}}
            ]
        });
        let line = decode_group_inbound(&raw.to_string(), 10001).expect("reply");
        assert!(line.addressed);
        let quoted = line.reply_to.expect("quote");
        assert!(quoted.hers);
        assert_eq!(quoted.text, "上一句");
        assert!(line.reply_id.is_none());
    }

    /// NapCat 的回复段只有 `id`（`api/msg.ts` 只写 `data: { id }`）。解码认不出是不是她，
    /// 只留下 id 给工人用 `get_msg` 反查。
    #[test]
    fn napcat_reply_carries_only_an_id_to_look_up() {
        use myriad_agent_rules::onebot::decode::decode_group_inbound;
        let raw = json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 555,
            "user_id": 20002,
            "message_id": 12,
            "message": [
                {"type": "reply", "data": {"id": "-2147480000"}},
                {"type": "text", "data": {"text": "接着说"}}
            ]
        });
        let line = decode_group_inbound(&raw.to_string(), 10001).expect("reply");
        assert!(
            !line.addressed,
            "an id alone does not say who was replied to"
        );
        assert!(line.reply_to.is_none());
        assert_eq!(line.reply_id.as_deref(), Some("-2147480000"));
    }

    #[test]
    fn looked_up_reply_says_whether_it_was_hers() {
        use myriad_agent_rules::onebot::decode::decode_replied_message;
        let hers = json!({
            "self_id": 10001,
            "user_id": 10001,
            "message_id": -2147480000,
            "message_type": "group",
            "sender": {"user_id": 10001, "nickname": "她", "card": ""},
            "message": [{"type": "text", "data": {"text": " 上一句 "}}]
        });
        let quoted = decode_replied_message(&hers, 10001).expect("quote");
        assert!(quoted.hers);
        assert_eq!(quoted.text, "上一句");
        assert_eq!(quoted.name, "她");

        let someone = json!({
            "user_id": 20003,
            "sender": {"user_id": 20003, "nickname": "别人", "card": "群名片"},
            "message": [{"type": "text", "data": {"text": "别人的话"}}]
        });
        let quoted = decode_replied_message(&someone, 10001).expect("quote");
        assert!(!quoted.hers);
        assert_eq!(quoted.name, "群名片");

        let cq = json!({"user_id": 10001, "message": "[CQ:face,id=1]", "raw_message": "原文"});
        assert_eq!(
            decode_replied_message(&cq, 10001).expect("raw text").text,
            "原文"
        );
        assert!(decode_replied_message(&json!(null), 10001).is_none());
    }

    #[test]
    fn her_own_group_line_is_dropped() {
        use myriad_agent_rules::onebot::decode::decode_group_inbound;
        let raw = json!({
            "post_type": "message",
            "message_type": "group",
            "group_id": 555,
            "user_id": 10001,
            "message": [{"type": "text", "data": {"text": "我自己"}}]
        });
        assert!(decode_group_inbound(&raw.to_string(), 10001).is_none());
    }

    #[test]
    fn message_sent_is_dropped() {
        assert!(
            decode_private_inbound(&with_message("private", "friend", "message_sent")).is_none()
        );
    }

    #[test]
    fn group_temp_session_is_dropped() {
        assert!(decode_private_inbound(&with_message("private", "group", "message")).is_none());
    }

    #[test]
    fn text_and_images_are_both_kept() {
        let inbound = decode_private_inbound(private_text_event()).expect("mixed");
        assert_eq!(inbound.content, "帮我看一下");
        assert_eq!(inbound.images.len(), 1);
        assert_eq!(inbound.images[0].url, "https://example.com/x.jpg");
        assert_eq!(inbound.images[0].name, "x.jpg");
        assert_eq!(inbound.images[0].mime, "image/jpeg");
        assert_eq!(inbound.images[0].size, 0);
    }

    #[test]
    fn image_without_http_url_is_skipped() {
        let missing = json!({
            "post_type": "message",
            "message_type": "private",
            "sub_type": "friend",
            "message_id": 1,
            "user_id": 20002,
            "message": [
                {"type":"image","data":{"file":"a.jpg"}},
                {"type":"image","data":{"file":"b.jpg","url":"file:///tmp/b.jpg"}}
            ]
        });
        let inbound = decode_private_inbound(&missing.to_string()).expect("textless private");
        assert!(inbound.images.is_empty());
        assert!(inbound.content.is_empty());
    }

    #[test]
    fn images_are_capped_at_the_shared_limit() {
        let segments: Vec<_> = (0..6)
            .map(|index| {
                json!({
                    "type": "image",
                    "data": {
                        "file": format!("p{index}.png"),
                        "url": format!("https://example.com/{index}.png"),
                        "file_size": index
                    }
                })
            })
            .collect();
        let raw = json!({
            "post_type": "message",
            "message_type": "private",
            "sub_type": "other",
            "message_id": 7,
            "user_id": 20002,
            "message": segments
        });
        let inbound = decode_private_inbound(&raw.to_string()).expect("images");
        assert_eq!(inbound.images.len(), CHANNEL_IMAGE_LIMIT);
        assert_eq!(inbound.images[0].url, "https://example.com/0.png");
        assert_eq!(inbound.images[3].name, "p3.png");
        assert_eq!(inbound.images[3].mime, "image/png");
        assert_eq!(inbound.images[3].size, 3);
    }

    #[test]
    fn long_text_is_truncated_not_dropped() {
        let over = "字".repeat(QQ_TEXT_LIMIT + 40);
        let raw = json!({
            "post_type": "message",
            "message_type": "private",
            "sub_type": "friend",
            "message_id": 8,
            "user_id": 20002,
            "message": [{"type":"text","data":{"text": over}}]
        });
        let inbound = decode_private_inbound(&raw.to_string()).expect("long text");
        let len = inbound.content.chars().count();
        assert!(!inbound.content.is_empty());
        assert!(len <= QQ_TEXT_LIMIT, "len={len}");
        assert!(len < over.chars().count());
    }

    #[test]
    fn int64_ids_keep_full_precision() {
        let raw = r#"{
            "post_type":"message","message_type":"private","sub_type":"friend",
            "message_id":1,"user_id":9007199254740993,"message":[]
        }"#;
        let inbound = decode_private_inbound(raw).expect("large user id");
        assert_eq!(inbound.user_openid, "9007199254740993");
        assert_eq!(inbound.msg_id, "1");
    }

    #[test]
    fn malformed_json_returns_none() {
        assert!(decode_private_inbound("not json").is_none());
    }

    #[test]
    fn unknown_event_type_returns_none() {
        let raw = r#"{"post_type":"notice","notice_type":"friend_add","user_id":1}"#;
        assert!(decode_private_inbound(raw).is_none());
    }

    #[test]
    fn decode_segments_to_text_joins_only_text_pieces() {
        let text = |value: &str| WireSegment {
            kind: "text".into(),
            data: Map::from_iter([("text".into(), Value::String(value.into()))]),
        };
        let image = WireSegment {
            kind: "image".into(),
            data: Map::new(),
        };
        assert_eq!(
            decode_segments_to_text(&[text("a"), image, text("b")]),
            "ab"
        );
        assert_eq!(decode_segments_to_text(&[]), "");
    }

    /// 回归：NapCat 实测会发出大于 `i32::MAX` 的 `message_id`（官方 issue #213 的报文里
    /// 是 `3425462029`）。OneBot 规范写的是 int32，但按 int32 读会让整条事件解不开，
    /// 于是用户的消息被静默丢弃。宽松层跟现实，不跟规范字面。
    #[test]
    fn napcat_message_id_beyond_i32_is_not_dropped() {
        let raw = r#"{"post_type":"message","message_type":"private","sub_type":"friend","message_id":3425462029,"user_id":10001,"message":[{"type":"text","data":{"text":"hi"}}]}"#;
        let inbound = decode_private_inbound(raw)
            .expect("a real NapCat message_id must not drop the whole event");
        assert_eq!(inbound.msg_id, "3425462029");
        assert_eq!(inbound.content, "hi");
    }
}

/// OneBot 私聊出站编码。只产出请求体，不带 echo。
mod onebot_encode {
    use myriad_agent_rules::channel::CHANNEL_IMAGE_LIMIT;
    use myriad_agent_rules::onebot::encode::{
        encode_get_msg, encode_group_message, encode_image_segment, encode_private_message,
        encode_text_segment, encode_typing, plan_private_delivery,
    };
    use serde_json::json;

    #[test]
    fn text_segment_has_the_exact_wire_shape() {
        assert_eq!(
            encode_text_segment("hi"),
            json!({"type":"text","data":{"text":"hi"}})
        );
    }

    #[test]
    fn image_segment_uses_file_not_url() {
        let segment = encode_image_segment("https://example.com/a.png");
        assert_eq!(segment["data"]["file"], json!("https://example.com/a.png"));
        assert!(segment["data"].get("url").is_none());
    }

    #[test]
    fn private_message_action_wraps_segments_in_params() {
        let payload =
            encode_private_message("10001", &[encode_text_segment("hi")]).expect("request");
        assert_eq!(payload["action"], "send_private_msg");
        assert_eq!(payload["params"]["user_id"], 10001);
        assert!(payload["params"]["message"].is_array());
        assert!(payload.get("echo").is_none());
    }

    #[test]
    fn user_id_is_a_json_number_not_a_string() {
        let payload =
            encode_private_message("10001", &[encode_text_segment("hi")]).expect("request");
        assert!(payload["params"]["user_id"].is_i64());
        assert!(!payload["params"]["user_id"].is_string());
        assert_eq!(payload["params"]["user_id"].as_i64(), Some(10001));
    }

    #[test]
    fn user_id_keeps_int64_precision() {
        let payload = encode_private_message("9007199254740993", &[encode_text_segment("hi")])
            .expect("large id");
        assert_eq!(
            payload["params"]["user_id"].as_i64(),
            Some(9007199254740993i64)
        );
    }

    #[test]
    fn bad_user_id_returns_none() {
        let segment = encode_text_segment("hi");
        assert!(encode_private_message("", std::slice::from_ref(&segment)).is_none());
        assert!(encode_private_message("abc", std::slice::from_ref(&segment)).is_none());
        assert!(encode_private_message("1.5", &[segment]).is_none());
    }

    #[test]
    fn empty_segments_return_none() {
        assert!(encode_private_message("10001", &[]).is_none());
    }

    #[test]
    fn group_message_action_uses_group_id() {
        let payload = encode_group_message("555", &[encode_text_segment("hi")]).expect("group");
        assert_eq!(payload["action"], "send_group_msg");
        assert_eq!(payload["params"]["group_id"], 555);
        assert!(payload.get("echo").is_none());
        assert!(encode_group_message("abc", &[encode_text_segment("hi")]).is_none());
    }

    #[test]
    fn plain_text_plan_sends_one_text_segment() {
        let payload = plan_private_delivery("10001", "hi", &[]).expect("text");
        assert_eq!(
            payload["params"]["message"],
            json!([{"type":"text","data":{"text":"hi"}}])
        );
    }

    #[test]
    fn typing_action_uses_set_input_status() {
        let payload = encode_typing("10001", true).expect("typing");
        assert_eq!(payload["action"], "set_input_status");
        assert_eq!(payload["params"]["user_id"], 10001);
        assert_eq!(payload["params"]["event_type"], 1);
        assert!(encode_typing("10001", false).is_none());
        assert!(encode_typing("not-a-number", true).is_none());
    }

    #[test]
    fn markdown_looking_text_still_goes_out_as_a_text_segment() {
        let payload = plan_private_delivery("10001", "| a |\n| --- |", &[]).expect("text");
        assert_eq!(payload["params"]["message"][0]["type"], "text");
    }

    #[test]
    fn get_msg_looks_up_a_reply_id() {
        let payload = encode_get_msg("-2147480000").expect("lookup");
        assert_eq!(payload["action"], "get_msg");
        assert_eq!(payload["params"]["message_id"], -2147480000i64);
        assert!(payload.get("echo").is_none());
        assert!(encode_get_msg("abc").is_none());
    }

    #[test]
    fn empty_text_with_no_images_returns_none() {
        assert!(plan_private_delivery("10001", "", &[]).is_none());
    }

    #[test]
    fn text_and_images_are_both_encoded() {
        let urls = vec![
            "https://example.com/a.png".to_string(),
            "https://example.com/b.png".to_string(),
        ];
        let payload = plan_private_delivery("10001", "hi", &urls).expect("mixed");
        let message = payload["params"]["message"].as_array().expect("array");
        assert_eq!(message.len(), 3);
        assert_eq!(message[0]["type"], "text");
        assert_eq!(message[1]["type"], "image");
        assert_eq!(message[1]["data"]["file"], "https://example.com/a.png");
        assert_eq!(message[2]["data"]["file"], "https://example.com/b.png");
    }

    #[test]
    fn images_are_capped_at_the_shared_limit() {
        let urls: Vec<String> = (0..6)
            .map(|index| format!("https://example.com/{index}.png"))
            .collect();
        let payload = plan_private_delivery("10001", "", &urls).expect("images");
        let message = payload["params"]["message"].as_array().expect("array");
        assert_eq!(message.len(), CHANNEL_IMAGE_LIMIT);
        assert_eq!(message.len(), 4);
        assert_eq!(message[0]["data"]["file"], "https://example.com/0.png");
        assert_eq!(message[3]["data"]["file"], "https://example.com/3.png");
    }

    #[test]
    fn whitespace_only_text_is_treated_as_empty() {
        assert!(plan_private_delivery("10001", "   ", &[]).is_none());
    }
}

mod onebot_rules {
    use myriad_agent_rules::channel::{
        ConnectFailureKind, WorkerIntent, discord_dm_capabilities, feishu_dm_capabilities,
        qq_c2c_capabilities, telegram_dm_capabilities,
    };
    use myriad_agent_rules::onebot::rules::{
        classify_onebot_handshake, onebot_private_capabilities, onebot_worker_intent,
        onebot_worker_supports_typing as worker_supports_typing,
    };

    #[test]
    fn onebot_worker_runs_only_with_switch_url_and_token() {
        assert_eq!(
            onebot_worker_intent(true, "ws://127.0.0.1:3001", true),
            WorkerIntent::Run
        );
        assert_eq!(
            onebot_worker_intent(false, "ws://127.0.0.1:3001", true),
            WorkerIntent::Stop
        );
        assert_eq!(onebot_worker_intent(true, "", true), WorkerIntent::Stop);
        assert_eq!(onebot_worker_intent(true, "   ", true), WorkerIntent::Stop);
        assert_eq!(
            onebot_worker_intent(true, "ws://127.0.0.1:3001", false),
            WorkerIntent::Stop
        );
    }

    #[test]
    fn onebot_capabilities_open_text_and_media_but_not_markdown_or_edit() {
        let caps = onebot_private_capabilities();
        assert!(caps.inbound_text);
        assert!(caps.inbound_media);
        assert!(!caps.inbound_callback);
        assert!(caps.outbound_final_text);
        assert!(!caps.outbound_markdown);
        assert!(caps.outbound_image);
        assert!(!caps.outbound_edit);
        assert!(!caps.outbound_streaming_draft);
        assert!(caps.interactive);
        assert!(!caps.frontend_action);
        assert!(!caps.performance);
        assert!(!caps.outfit);
    }

    #[test]
    fn no_channel_sends_a_top_level_markdown_segment() {
        assert!(!onebot_private_capabilities().outbound_markdown);
        assert!(!qq_c2c_capabilities().outbound_markdown);
        assert!(!telegram_dm_capabilities().outbound_markdown);
        assert!(!discord_dm_capabilities().outbound_markdown);
        assert!(!feishu_dm_capabilities().outbound_markdown);
    }

    #[test]
    fn onebot_worker_supports_typing() {
        assert!(worker_supports_typing());
    }

    #[test]
    fn group_allowlist_is_normalized_and_rejects_typos() {
        use myriad_agent_rules::onebot::rules::normalize_onebot_group_allowlist;
        assert_eq!(
            normalize_onebot_group_allowlist(" 751307618，461758846、123 ; 123\n9").as_deref(),
            Ok("9,123,461758846,751307618")
        );
        assert_eq!(normalize_onebot_group_allowlist("  ").as_deref(), Ok(""));
        assert!(normalize_onebot_group_allowlist("123, abc").is_err());
        assert!(normalize_onebot_group_allowlist("0").is_err());
        assert!(normalize_onebot_group_allowlist("-5").is_err());
    }

    #[test]
    fn only_listed_groups_are_heard_and_an_empty_list_hears_all() {
        use myriad_agent_rules::onebot::rules::onebot_group_allowed;
        assert!(onebot_group_allowed("", "555"));
        assert!(onebot_group_allowed("555,777", "555"));
        assert!(onebot_group_allowed("555，777", "777"));
        assert!(!onebot_group_allowed("555,777", "5555"));
        assert!(!onebot_group_allowed("555,777", "55"));
        // A list that cannot be read hears no group rather than every group.
        assert!(!onebot_group_allowed("555,abc", "555"));
    }

    #[test]
    fn bad_token_is_permanent_not_retried() {
        assert_eq!(
            classify_onebot_handshake(None, Some(1403)),
            ConnectFailureKind::Permanent
        );
        assert_eq!(
            classify_onebot_handshake(Some(403), None),
            ConnectFailureKind::Permanent
        );
    }

    #[test]
    fn unreachable_server_is_transient() {
        assert_eq!(
            classify_onebot_handshake(Some(503), None),
            ConnectFailureKind::Transient
        );
        assert_eq!(
            classify_onebot_handshake(None, None),
            ConnectFailureKind::Transient
        );
    }

    #[test]
    fn unknown_path_is_permanent() {
        assert_eq!(
            classify_onebot_handshake(Some(404), None),
            ConnectFailureKind::Permanent
        );
    }

    /// OneBot 标准把 1400/1401/1403/1404 对应 HTTP 400/401/403/404。
    /// 整段都是「配置或请求本身不对」，重试无用；只有 1405 往后的 14xx 才可重试。
    /// 漏掉 1404 会让路径写错时无限重试，正是这条测试要钉住的。
    #[test]
    fn the_whole_documented_retcode_range_is_permanent() {
        for retcode in 1400..=1404 {
            assert_eq!(
                classify_onebot_handshake(None, Some(retcode)),
                ConnectFailureKind::Permanent,
                "retcode {retcode} is a configuration fault"
            );
        }
        for retcode in [1405, 1499, 1500] {
            assert_eq!(
                classify_onebot_handshake(None, Some(retcode)),
                ConnectFailureKind::Transient,
                "retcode {retcode} is retryable"
            );
        }
    }

    /// 4xx/5xx 的边界，以及 retcode 优先于 status 的优先级。
    #[test]
    fn status_boundaries_and_retcode_precedence() {
        for status in [400, 499] {
            assert_eq!(
                classify_onebot_handshake(Some(status), None),
                ConnectFailureKind::Permanent,
                "status {status}"
            );
        }
        for status in [399, 500, 599, 600] {
            assert_eq!(
                classify_onebot_handshake(Some(status), None),
                ConnectFailureKind::Transient,
                "status {status}"
            );
        }
        assert_eq!(
            classify_onebot_handshake(Some(503), Some(1403)),
            ConnectFailureKind::Permanent,
            "a token rejection outranks a server-side status"
        );
    }
}

mod group_pictures {
    use myriad_agent_rules::channel::{
        GroupImage, ImageFetch, TelegramBotIdentity, discord_group_message_from_create,
        parse_telegram_group_messages,
    };

    #[test]
    fn a_telegram_photo_or_sticker_is_a_line_even_without_words() {
        let bot = TelegramBotIdentity {
            id: 777,
            first_name: "小灯".into(),
            username: Some("xiaodeng_bot".into()),
        };
        let body = r#"{"ok": true, "result": [
            {"update_id": 1, "message": {"message_id": 1, "from": {"id": 11, "first_name": "阿明"}, "chat": {"id": -100, "type": "group"},
              "photo": [{"file_id": "small", "file_unique_id": "u-small", "width": 320},
                        {"file_id": "mid", "file_unique_id": "u-mid", "width": 1280},
                        {"file_id": "big", "file_unique_id": "u-big", "width": 2560}]}},
            {"update_id": 2, "message": {"message_id": 2, "from": {"id": 12, "first_name": "小红"}, "chat": {"id": -100, "type": "group"},
              "sticker": {"file_id": "anim", "file_unique_id": "s-1", "emoji": "😂", "is_animated": true,
                          "thumbnail": {"file_id": "thumb", "file_unique_id": "t-1"}}}},
            {"update_id": 3, "message": {"message_id": 3, "from": {"id": 13, "first_name": "老周"}, "chat": {"id": -100, "type": "group"},
              "caption": "看这个", "photo": [{"file_id": "p", "file_unique_id": "u-p", "width": 800}]}}
        ]}"#;
        let lines = parse_telegram_group_messages(200, body, &bot).unwrap();
        assert_eq!(lines.len(), 3, "pictures without words are lines too");
        assert_eq!(lines[0].text, "");
        assert_eq!(
            lines[0].images,
            vec![GroupImage {
                key: "telegram:u-mid".into(),
                fetch: ImageFetch::TelegramFile {
                    file_id: "mid".into()
                },
                hint: None,
                sticker: false,
            }]
        );
        let sticker = &lines[1].images[0];
        assert!(sticker.sticker);
        assert_eq!(
            sticker.key, "telegram:s-1",
            "known by the sticker, not its thumbnail"
        );
        assert_eq!(
            sticker.fetch,
            ImageFetch::TelegramFile {
                file_id: "thumb".into()
            }
        );
        assert_eq!(sticker.hint.as_deref(), Some("😂"));
        assert_eq!(lines[2].text, "看这个");
        assert_eq!(lines[2].images.len(), 1);
    }

    #[test]
    fn a_discord_attachment_and_still_sticker_are_pictures() {
        let line = discord_group_message_from_create(
            &serde_json::json!({
                "id": "11", "channel_id": "22", "guild_id": "33",
                "author": { "id": "44", "username": "ming" },
                "content": "",
                "attachments": [
                    { "url": "https://cdn.discordapp.com/a/cat.png?ex=1", "filename": "cat.png", "size": 1234, "content_type": "image/png" },
                    { "url": "https://cdn.discordapp.com/a/a.pdf", "filename": "a.pdf", "size": 9, "content_type": "application/pdf" }
                ],
                "sticker_items": [
                    { "id": "555", "name": "wave", "format_type": 1 },
                    { "id": "556", "name": "lottie", "format_type": 3 }
                ]
            }),
            "99",
        )
        .expect("a line of pictures");
        let keys: Vec<&str> = line.images.iter().map(|image| image.key.as_str()).collect();
        assert_eq!(keys, ["discord:cat.png:1234", "discord-sticker:555"]);
        assert_eq!(
            line.images[1].fetch,
            ImageFetch::Url {
                url: "https://media.discordapp.net/stickers/555.png".into()
            }
        );
        assert!(
            discord_group_message_from_create(
                &serde_json::json!({
                    "id": "12", "channel_id": "22", "guild_id": "33",
                    "author": { "id": "44", "username": "ming" }, "content": ""
                }),
                "99"
            )
            .is_none(),
            "nothing said and nothing shown"
        );
    }

    #[test]
    fn a_qq_image_and_sticker_are_pictures() {
        let raw = r#"{"post_type":"message","message_type":"group","group_id":123,"user_id":456,"self_id":789,"message_id":1,
            "sender":{"nickname":"阿明"},
            "message":[{"type":"image","data":{"file":"ABC.jpg","url":"https://multimedia.nt.qq.com.cn/download?x=1","summary":"[动画表情]","sub_type":1}},
                       {"type":"mface","data":{"emoji_id":"e9","url":"https://gxh.vip.qq.com/e9.gif","summary":"[doge]"}},
                       {"type":"face","data":{"id":"14"}}]}"#;
        let line = myriad_agent_rules::onebot::decode::decode_group_inbound(raw, 789)
            .expect("a line of pictures");
        assert_eq!(line.text, "");
        let keys: Vec<&str> = line.images.iter().map(|image| image.key.as_str()).collect();
        assert_eq!(keys, ["qq:ABC.jpg", "qq-mface:e9"]);
        assert!(line.images.iter().all(|image| image.sticker));
        assert_eq!(line.images[1].hint.as_deref(), Some("[doge]"));
    }
}
