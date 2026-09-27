//! Room unit tests (mechanical move from room.rs).
use super::helpers::*;
use super::members::{
    home_servers_match, may_promote_private_room_to_public, validate_remote_public_room_doc,
};
use super::stickers::{ROOM_STICKER_MAX_DATA_LEN, parse_room_stickers, stickers_to_json};
use super::*;
use serde_json::json;

#[test]
fn public_transition_is_one_way() {
    assert!(validate_public_transition(false, None).is_ok());
    assert!(validate_public_transition(false, Some(true)).is_ok());
    assert!(validate_public_transition(false, Some(false)).is_ok()); // still private
    assert!(validate_public_transition(true, Some(true)).is_ok()); // no-op
    assert!(validate_public_transition(true, None).is_ok());
    assert!(validate_public_transition(true, Some(false)).is_err());
}

#[test]
fn parse_room_join_ref_bare_and_shareable() {
    let (id, home) = parse_room_join_ref("rm_6297d497-1ecb-494c-9abe-5247585c75a9");
    assert_eq!(id, "rm_6297d497-1ecb-494c-9abe-5247585c75a9");
    assert!(home.is_none());

    let (id, home) =
        parse_room_join_ref("rm_6297d497-1ecb-494c-9abe-5247585c75a9@example.com:8443");
    assert_eq!(id, "rm_6297d497-1ecb-494c-9abe-5247585c75a9");
    assert_eq!(home.as_deref(), Some("example.com:8443"));

    let (id, home) =
        parse_room_join_ref("myriad:room:rm_6297d497-1ecb-494c-9abe-5247585c75a9@127.0.0.1:1103");
    assert_eq!(id, "rm_6297d497-1ecb-494c-9abe-5247585c75a9");
    assert_eq!(home.as_deref(), Some("127.0.0.1:1103"));
}

#[test]
fn home_servers_match_normalizes_scheme_and_case() {
    assert!(home_servers_match(
        "Example.COM:8443",
        "https://example.com:8443/"
    ));
    assert!(home_servers_match(
        "127.0.0.1:1103",
        "http://127.0.0.1:1103"
    ));
    assert!(!home_servers_match("evil.example", "good.example"));
    assert!(!home_servers_match("", "example.com"));
}

fn sample_public_info(home: &str, owner: &str) -> PublicRoomInfo {
    PublicRoomInfo {
        room_id: "rm_aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
        name: "Public".into(),
        description: None,
        avatar_url: None,
        owner_actor: owner.into(),
        home_server: home.into(),
        invite_policy: "open".into(),
        max_members: 50,
        is_public: true,
        member_count: 1,
        game: None,
    }
}

#[test]
fn validate_remote_public_doc_rejects_private_and_bad_id() {
    let mut info = sample_public_info("evil.example", "https://evil.example/users/x");
    info.is_public = false;
    assert!(validate_remote_public_room_doc(&info, "evil.example").is_err());

    info.is_public = true;
    info.room_id = "not-a-room".into();
    assert!(validate_remote_public_room_doc(&info, "evil.example").is_err());
}

#[test]
fn validate_remote_public_doc_rejects_home_mismatch() {
    // Attacker hosts card but document claims victim home — blocked.
    let info = sample_public_info("victim.example", "https://victim.example/users/owner");
    let err = validate_remote_public_room_doc(&info, "evil.example").unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
}

#[test]
fn validate_remote_public_doc_accepts_matching_home() {
    let info = sample_public_info("peer.example:8443", "https://peer.example:8443/users/owner");
    let (home, policy, max) =
        validate_remote_public_room_doc(&info, "https://peer.example:8443").unwrap();
    assert!(home_servers_match(&home, "peer.example:8443"));
    assert_eq!(policy, "open");
    assert_eq!(max, 50);
}

#[test]
fn validate_remote_public_doc_defaults_bad_invite_policy() {
    let mut info = sample_public_info("peer.example", "https://peer.example/users/o");
    info.invite_policy = "not-a-policy".into();
    let (_, policy, _) = validate_remote_public_room_doc(&info, "peer.example").unwrap();
    assert_eq!(policy, "open");
}

#[test]
fn may_promote_private_requires_home_match() {
    // Private local room on this instance: evil home cannot promote.
    assert!(!may_promote_private_room_to_public(
        "127.0.0.1:1103",
        "evil.example"
    ));
    // Federated stub whose home went public: matching home OK.
    assert!(may_promote_private_room_to_public(
        "peer.example",
        "https://peer.example/"
    ));
    // Empty home (legacy) may promote.
    assert!(may_promote_private_room_to_public("", "peer.example"));
}

#[test]
fn parse_room_join_ref_public_url() {
    let (id, home) = parse_room_join_ref(
        "https://peer.example:8443/api/federation/public/rooms/rm_aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    );
    assert_eq!(id, "rm_aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    assert_eq!(home.as_deref(), Some("peer.example:8443"));
}

#[test]
fn non_empty_room_name_trims_and_rejects_blank() {
    assert_eq!(
        non_empty_room_name(Some(" 测试群 ")).as_deref(),
        Some("测试群")
    );
    assert_eq!(non_empty_room_name(Some("   ")), None);
    assert_eq!(non_empty_room_name(Some("")), None);
    assert_eq!(non_empty_room_name(None), None);
}

#[test]
fn resolve_invite_room_name_prefers_real_name() {
    let room_id = "rm_08355abcdef";
    assert_eq!(resolve_invite_room_name(Some("测试群"), room_id), "测试群");
    assert_eq!(
        resolve_invite_room_name(Some("  "), room_id),
        "Room rm_08355"
    );
    assert_eq!(resolve_invite_room_name(None, room_id), "Room rm_08355");
}

#[test]
fn is_missing_or_fallback_detects_placeholder() {
    let room_id = "rm_08355abcdef";
    assert!(is_missing_or_fallback_room_name("", room_id));
    assert!(is_missing_or_fallback_room_name("  ", room_id));
    assert!(is_missing_or_fallback_room_name("Room rm_08355", room_id));
    assert!(!is_missing_or_fallback_room_name("测试群", room_id));
    assert!(!is_missing_or_fallback_room_name("Room other", room_id));
}

#[test]
fn invite_object_name_parsing_matches_handle_room_invite() {
    // Mirrors handle_room_invite: read object.name, treat blank as missing.
    // "rm_abc12345" → first 8 chars = "rm_abc12"
    let room_id = "rm_abc12345";
    let with_name = json!({"id": room_id, "name": "  测试群  "});
    let name = non_empty_room_name(with_name.get("name").and_then(|v| v.as_str()));
    assert_eq!(resolve_invite_room_name(name.as_deref(), room_id), "测试群");

    let blank = json!({"id": room_id, "name": "  "});
    let name = non_empty_room_name(blank.get("name").and_then(|v| v.as_str()));
    assert_eq!(
        resolve_invite_room_name(name.as_deref(), room_id),
        "Room rm_abc12"
    );

    let missing = json!({"id": room_id});
    let name = non_empty_room_name(missing.get("name").and_then(|v| v.as_str()));
    assert_eq!(
        resolve_invite_room_name(name.as_deref(), room_id),
        "Room rm_abc12"
    );
}

#[test]
fn invite_object_game_parses_into_room_config() {
    let object = json!({
        "id": "rm_abc",
        "game": {
            "tapp_id": "com.example.chess",
            "protocol": "v1",
            "max_players": 2,
            "max_message_bytes": 65536
        }
    });
    let config = crate::federation::room::game::parse_room_game_config(Some(&json!({
        "game": object.get("game").cloned().unwrap()
    })))
    .expect("invite game");
    assert_eq!(config.tapp_id, "com.example.chess");
    assert_eq!(config.protocol, "v1");
    assert_eq!(config.max_message_bytes, Some(65536));
    assert!(
        crate::federation::room::game::parse_room_game_config(Some(&json!({
            "game": {"tapp_id": "not-an-id", "protocol": "v1"}
        })))
        .is_none()
    );
}

#[test]
fn is_admin_role_owner_and_admin_only() {
    assert!(is_admin_role("owner"));
    assert!(is_admin_role("admin"));
    assert!(!is_admin_role("member"));
    assert!(!is_admin_role("moderator"));
    assert!(!is_admin_role(""));
}

#[test]
fn fallback_room_name_uses_first_8_chars() {
    assert_eq!(fallback_room_name("rm_abcdefghij"), "Room rm_abcde");
    assert_eq!(fallback_room_name("short"), "Room short");
}

#[test]
fn validate_public_transition_one_way() {
    assert!(validate_public_transition(true, Some(false)).is_err());
    assert!(validate_public_transition(true, Some(true)).is_ok());
    assert!(validate_public_transition(false, Some(true)).is_ok());
    assert!(validate_public_transition(false, None).is_ok());
}

#[test]
fn non_empty_room_name_trims_v2() {
    assert_eq!(non_empty_room_name(Some("  hi  ")).as_deref(), Some("hi"));
    assert_eq!(non_empty_room_name(Some("   ")), None);
    assert_eq!(non_empty_room_name(None), None);
}

#[test]
fn parse_room_stickers_filters_invalid() {
    let shared = json!({
        "stickers": [
            {
                "id": "stk_ok",
                "data": "data:image/png;base64,AAAA",
                "actor": "https://example.com/users/a",
                "created_at": "2026-01-01T00:00:00Z"
            },
            {
                "id": "stk_bad_url",
                "data": "https://evil.example/x.png",
                "actor": "https://example.com/users/a",
                "created_at": "2026-01-01T00:00:00Z"
            },
            {
                "id": "",
                "data": "data:image/png;base64,BBBB",
                "actor": "https://example.com/users/a",
                "created_at": "2026-01-01T00:00:00Z"
            },
            { "not": "a sticker" }
        ]
    });
    let list = parse_room_stickers(&shared);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "stk_ok");
    assert!(list[0].data.starts_with("data:image/"));
}

#[test]
fn parse_room_stickers_rejects_oversized() {
    let too_big = format!(
        "data:image/png;base64,{}",
        "B".repeat(ROOM_STICKER_MAX_DATA_LEN)
    );
    // The prefix plus the maximum payload exceeds the cap; craft the exact edge.
    let ok_data = format!(
        "data:image/png;base64,{}",
        "C".repeat(ROOM_STICKER_MAX_DATA_LEN - "data:image/png;base64,".len())
    );
    assert!(ok_data.len() <= ROOM_STICKER_MAX_DATA_LEN);
    assert!(too_big.len() > ROOM_STICKER_MAX_DATA_LEN);
    let shared = json!({
        "stickers": [
            {
                "id": "stk_ok",
                "data": ok_data,
                "actor": "https://example.com/users/a",
                "created_at": "2026-01-01T00:00:00Z"
            },
            {
                "id": "stk_big",
                "data": too_big,
                "actor": "https://example.com/users/a",
                "created_at": "2026-01-01T00:00:00Z"
            }
        ]
    });
    let list = parse_room_stickers(&shared);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "stk_ok");
}

#[test]
fn stickers_to_json_roundtrip_shape() {
    let items = vec![RoomStickerItem {
        id: "stk_1".into(),
        data: "data:image/webp;base64,QQ==".into(),
        name: Some("hi".into()),
        actor: "https://example.com/users/a".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
    }];
    let v = stickers_to_json(&items);
    let again = parse_room_stickers(&json!({ "stickers": v }));
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, "stk_1");
    assert_eq!(again[0].name.as_deref(), Some("hi"));
}

// ---------- myriad:RoomJoin authorization ----------
//
// Before this gate the handler only checked that the room existed, so any
// signed instance that learned a room_id could add itself as `owner` and then
// pass the admin check in handle_room_governance.

use super::inbox::{
    RoomJoinAuth, require_remote_inbox, room_join_authorized, room_join_effective_role,
};

/// Builder defaulting to the hostile case: a stranger self-joining a closed room.
fn join_auth(f: impl FnOnce(&mut RoomJoinAuth<'_>)) -> bool {
    let mut auth = RoomJoinAuth {
        is_self_join: true,
        announcer_is_owner: false,
        announcer_role: None,
        invite_policy: "admin-only",
        room_is_public: false,
        joining_already_on_roster: false,
    };
    f(&mut auth);
    room_join_authorized(auth)
}

#[test]
fn room_join_rejects_uninvited_stranger() {
    assert!(!join_auth(|_| {}));
}

#[test]
fn room_join_key_fanout_requires_a_remote_inbox() {
    assert!(require_remote_inbox(None).is_err());
    assert!(require_remote_inbox(Some((String::new(), "peer.example".into()))).is_err());
    assert_eq!(
        require_remote_inbox(Some((
            "https://peer.example/inbox".into(),
            "peer.example".into(),
        ))),
        Ok(("https://peer.example/inbox".into(), "peer.example".into(),))
    );
}

#[test]
fn room_join_allows_invitee_accepting() {
    // accept_room_invite fan-out: self-join, we already hold the pending row.
    assert!(join_auth(|a| a.joining_already_on_roster = true));
}

#[test]
fn room_join_allows_open_and_public_self_join() {
    // `room_join_authorized`: open invite_policy or is_public.
    assert!(join_auth(|a| a.invite_policy = "open"));
    assert!(join_auth(|a| a.room_is_public = true));
}

#[test]
fn room_join_roster_announce_requires_invite_rights() {
    // Stranger announcing someone else — the escalation path.
    assert!(!join_auth(|a| a.is_self_join = false));
    // Plain member cannot add under admin-only...
    assert!(!join_auth(|a| {
        a.is_self_join = false;
        a.announcer_role = Some("member");
    }));
    // ...but can under member-invite / open, matching invite_member.
    assert!(join_auth(|a| {
        a.is_self_join = false;
        a.announcer_role = Some("member");
        a.invite_policy = "member-invite";
    }));
    assert!(join_auth(|a| {
        a.is_self_join = false;
        a.announcer_role = Some("member");
        a.invite_policy = "open";
    }));
    // Admin and owner always may.
    assert!(join_auth(|a| {
        a.is_self_join = false;
        a.announcer_role = Some("admin");
    }));
    assert!(join_auth(|a| {
        a.is_self_join = false;
        a.announcer_is_owner = true;
    }));
}

#[test]
fn room_join_accepts_home_roster_backfill() {
    // backfill_roster_for_new_member replays the roster from the home instance:
    // owner-signed, announcing a third party the recipient has never seen, in a
    // private admin-only room. That is the shape the late joiner must accept —
    // it is the only way they learn about peers who accepted while they were
    // still pending.
    assert!(join_auth(|a| {
        a.is_self_join = false;
        a.announcer_is_owner = true;
        a.joining_already_on_roster = false;
        a.invite_policy = "admin-only";
        a.room_is_public = false;
    }));
}

#[test]
fn room_join_unknown_policy_falls_back_to_admin_only() {
    // Mirrors the `_ if !is_admin_role(..)` arm in invite_member.
    assert!(!join_auth(|a| {
        a.is_self_join = false;
        a.announcer_role = Some("member");
        a.invite_policy = "";
    }));
    assert!(join_auth(|a| {
        a.is_self_join = false;
        a.announcer_role = Some("owner");
        a.invite_policy = "nonsense";
    }));
}

#[test]
fn room_join_never_mints_privileged_roles() {
    // Self-announced promotion is the whole point of the clamp.
    assert_eq!(room_join_effective_role(None, "owner"), "member");
    assert_eq!(room_join_effective_role(None, "admin"), "member");
    assert_eq!(room_join_effective_role(None, "bogus"), "member");
    // Non-privileged seeds pass through.
    assert_eq!(room_join_effective_role(None, "member"), "member");
    assert_eq!(room_join_effective_role(None, "observer"), "observer");
}

#[test]
fn room_is_full_at_capacity() {
    assert!(room_is_full(50, 50));
    assert!(room_is_full(51, 50));
    assert!(!room_is_full(49, 50));
    assert!(!room_is_full(0, 2));
}

#[test]
fn room_capacity_decode_does_not_default() {
    let crud = include_str!("crud.rs");
    let list = crud
        .split("pub async fn list_rooms")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn get_room").next())
        .expect("list_rooms");
    let detail = crud
        .split("pub async fn get_room")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn update_room").next())
        .expect("get_room");
    assert!(
        !list.contains("max_members\").unwrap_or(50)"),
        "list must not invent capacity"
    );
    assert!(list.contains("max_members\").map_err(db_err)"));
    assert!(
        !detail.contains("max_members\").unwrap_or(50)"),
        "detail must not invent capacity"
    );
    let public = include_str!("members.rs");
    assert!(!public.contains("max_members\").unwrap_or(50)"));
}

#[test]
fn accept_invite_locks_before_reading_membership() {
    let src = include_str!("members.rs");
    let body = src
        .split("pub async fn accept_room_invite")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn ").next())
        .expect("accept_room_invite");
    let lock = body.find("lock_room_capacity").expect("lock");
    let membership = body.find("get_membership").expect("membership");
    assert!(lock < membership, "capacity lock must precede membership");
    assert!(body.contains("membership_status = 'pending'"));
    assert!(body.contains("rows_affected()"));
}

#[test]
fn create_and_admit_use_same_transaction_lock() {
    let crud = include_str!("crud.rs");
    assert!(crud.contains("db.begin()"));
    assert!(crud.contains("INSERT INTO federation_room_members"));
    let members = include_str!("members.rs");
    assert!(members.contains("assert_room_has_capacity"));
    assert!(members.contains("FOR UPDATE") || include_str!("helpers.rs").contains("FOR UPDATE"));
    let e2e = include_str!("e2e.rs");
    assert!(include_str!("helpers.rs").contains("RoomFanoutMode::RequireRoutable"));
    assert!(include_str!("helpers.rs").contains("returning_id(act_row)"));
    let invite = include_str!("members.rs");
    let invite_fn = invite
        .split("pub async fn invite_member")
        .nth(1)
        .expect("invite_member");
    assert!(invite_fn.contains("insert_and_enqueue_delivery"));
    assert!(invite_fn.contains("remote_inbox_missing"));
    assert!(!invite_fn.contains("if let Some(act_id)"));
    let reject = invite
        .split("pub async fn reject_room_invite")
        .nth(1)
        .expect("reject_room_invite");
    assert!(reject.contains("insert_and_enqueue_delivery"));
    assert!(reject.contains("db.begin()"));
    let transfer = members
        .split("pub async fn transfer_room_ownership")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn leave_room").next())
        .expect("transfer_room_ownership");
    assert!(transfer.contains("fanout_to_remote_members_required"));
    assert!(transfer.contains("db.begin()"));
    assert!(!transfer.contains("let _ = db"));
    let leave = members
        .split("pub async fn leave_room")
        .nth(1)
        .expect("leave_room");
    assert!(leave.contains("fanout_to_remote_members_required"));
    let update = crud
        .split("pub async fn update_room")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn delete_room").next())
        .expect("update_room");
    assert!(update.contains("fanout_to_remote_members_required"));
    assert!(update.contains("txn.commit()"));
    let role = members
        .split("pub async fn set_member_role")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn remove_member").next())
        .expect("set_member_role");
    assert!(role.contains("fanout_to_remote_members_required"));
    let kick = members
        .split("pub async fn remove_member")
        .nth(1)
        .and_then(|rest| rest.split("pub async fn transfer_room_ownership").next())
        .expect("remove_member");
    assert!(kick.contains("fanout_to_remote_members_required"));
    let accept = members
        .split("pub async fn accept_room_invite")
        .nth(1)
        .expect("accept_room_invite");
    assert!(accept.contains("fanout_to_remote_members_required"));
    assert!(!accept.contains("if let Err(e) = fanout_to_remote_members("));
    let pin = include_str!("messages.rs")
        .split("pub async fn pin_room_message")
        .nth(1)
        .expect("pin_room_message");
    assert!(pin.contains("fanout_to_remote_members_required"));
    let stickers = include_str!("stickers.rs");
    assert!(stickers.contains("persist_and_fanout_stickers"));
    assert!(stickers.contains("fanout_to_remote_members_required"));
    assert!(e2e.contains("fanout_to_remote_members_required"));
    assert!(
        !e2e.contains("let _ = fanout_to_remote_members"),
        "KeyExchange fanout must not be ignored after publishing the marker"
    );
    assert!(
        !e2e.contains("if !already_published"),
        "published_keys marker must not skip delivery to current members"
    );
}

#[test]
fn room_join_never_overwrites_an_existing_role() {
    // A pending invite carrying role=admin must survive the invitee's accept,
    // and a member must not be able to re-announce itself upward.
    assert_eq!(room_join_effective_role(Some("admin"), "member"), "admin");
    assert_eq!(room_join_effective_role(Some("owner"), "member"), "owner");
    assert_eq!(room_join_effective_role(Some("member"), "owner"), "member");
}

/// 真实 `send_room_message` 路径的 E2E fail-closed 行为（每个测试独立 schema，见 `test_db`）。
mod send_e2e_db {
    use axum::http::StatusCode;
    use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

    use super::super::SendRoomMessageRequest;
    use crate::federation::test_db::SchemaDb;

    struct Fixture {
        user_id: i32,
        username: String,
        room_id: String,
    }

    /// 本地用户作为 active member 的房间。`published` 是房间已发布的
    /// actor → 公钥表；`own_e2e` 是本地成员的 e2e 密钥状态。
    async fn room(
        db: &DatabaseConnection,
        published: impl FnOnce(&str) -> Option<serde_json::Value>,
        own_e2e: Option<serde_json::Value>,
    ) -> Fixture {
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let username = format!("rmsend-{tag}");
        let user_id: i32 = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ($1) RETURNING id",
                [username.clone().into()],
            ))
            .await
            .expect("insert user")
            .expect("row")
            .try_get("", "id")
            .expect("id");
        let base_url = crate::federation::types::get_base_url().await;
        let local_actor = crate::federation::types::actor_url(&base_url, &username);
        let shared = published(&local_actor)
            .map(|keys| serde_json::json!({"e2e": {"published_keys": keys}}));
        let room_id = format!("rm_{tag}");
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_rooms
               (room_id, name, owner_actor, home_server, shared_data_config)
               VALUES ($1, 'send test', $2, 'local.test', $3)"#,
            [room_id.clone().into(), local_actor.clone().into(), shared.into()],
        ))
        .await
        .expect("insert room");
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_room_members
               (room_id, actor_url, is_local, local_user_id, role, custom_permissions,
                membership_status, joined_at)
               VALUES ($1, $2, true, $3, 'member', $4, 'active', NOW())"#,
            [
                room_id.clone().into(),
                local_actor.into(),
                user_id.into(),
                own_e2e.map(|e2e| serde_json::json!({ "e2e": e2e })).into(),
            ],
        ))
        .await
        .expect("insert member");
        Fixture {
            user_id,
            username,
            room_id,
        }
    }

    async fn side_effects(db: &DatabaseConnection, f: &Fixture) -> (i64, i64) {
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT
                     (SELECT COUNT(*) FROM federation_room_messages WHERE room_id = $1)
                       AS messages,
                     (SELECT COUNT(*) FROM federation_activities WHERE user_id = $2)
                       AS activities"#,
                [f.room_id.clone().into(), f.user_id.into()],
            ))
            .await
            .expect("count")
            .expect("row");
        (
            row.try_get("", "messages").expect("messages"),
            row.try_get("", "activities").expect("activities"),
        )
    }

    async fn stored(db: &DatabaseConnection, f: &Fixture) -> (serde_json::Value, bool) {
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT payload, is_encrypted FROM federation_room_messages WHERE room_id = $1",
                [f.room_id.clone().into()],
            ))
            .await
            .expect("select")
            .expect("row");
        (
            row.try_get("", "payload").expect("payload"),
            row.try_get("", "is_encrypted").expect("flag"),
        )
    }

    fn request(encrypt: bool) -> SendRoomMessageRequest {
        SendRoomMessageRequest {
            message_type: None,
            payload: serde_json::json!({"text": "secret plaintext"}),
            thread_id: None,
            reply_to: None,
            encrypt: Some(encrypt),
        }
    }

    fn own_keys(kp: &crate::federation::e2e::E2eKeyPair) -> serde_json::Value {
        serde_json::json!({
            "local_public_key": kp.public_key,
            "local_private_key": kp.private_key,
        })
    }

    #[tokio::test]
    async fn encrypt_true_without_keys_writes_nothing() {
        let Some(fixture) = SchemaDb::new().await else {
            return;
        };
        let db = &fixture.db;
        let own = crate::federation::e2e::generate_keypair();
        let peer = crate::federation::e2e::generate_keypair();
        let own_pk = own.public_key.clone();
        let peer_pk = peer.public_key.clone();

        // 缺 session：房间从未发布 E2E 密钥
        let no_session = room(db, |_| None, Some(own_keys(&own))).await;
        // 无对端：只有自己的公钥
        let only_self = room(
            db,
            move |me: &str| Some(serde_json::json!({ me: own_pk })),
            Some(own_keys(&own)),
        )
        .await;
        // 缺 key：对端已发布，但本地成员没有自己的 e2e 密钥
        let no_own_key = room(
            db,
            |_| Some(serde_json::json!({"https://peer.example/users/p": peer_pk})),
            None,
        )
        .await;
        // 对端公钥非法：加密前即失败
        let bad_peer = room(
            db,
            |_| Some(serde_json::json!({"https://peer.example/users/p": "not-a-key"})),
            Some(own_keys(&own)),
        )
        .await;

        for (case, f) in [
            ("no_session", &no_session),
            ("only_self", &only_self),
            ("no_own_key", &no_own_key),
            ("bad_peer", &bad_peer),
        ] {
            let (status, body) = super::super::send_room_message(
                f.user_id,
                &f.username,
                &f.room_id,
                db,
                &request(true),
            )
            .await
            .expect_err("encrypt=true must fail closed");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{case}");
            assert_eq!(body.0["code"], "e2e_required", "{case}");
            assert_eq!(side_effects(db, f).await, (0, 0), "{case}: nothing written");
        }
        fixture.close().await;
    }

    #[tokio::test]
    async fn encrypt_true_with_peer_keys_stores_only_ciphertext() {
        let Some(fixture) = SchemaDb::new().await else {
            return;
        };
        let db = &fixture.db;
        let own = crate::federation::e2e::generate_keypair();
        let peer = crate::federation::e2e::generate_keypair();
        let peer_pk = peer.public_key.clone();
        let f = room(
            db,
            |_| Some(serde_json::json!({"https://peer.example/users/p": peer_pk})),
            Some(own_keys(&own)),
        )
        .await;
        let resp =
            super::super::send_room_message(f.user_id, &f.username, &f.room_id, db, &request(true))
                .await
                .expect("encrypted send");
        assert!(resp.is_encrypted);
        let (payload, is_encrypted) = stored(db, &f).await;
        assert!(is_encrypted);
        assert!(
            !payload.to_string().contains("secret plaintext"),
            "plaintext must not be stored"
        );
        assert_eq!(side_effects(db, &f).await.0, 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn encrypt_false_stores_plaintext_without_keys() {
        let Some(fixture) = SchemaDb::new().await else {
            return;
        };
        let db = &fixture.db;
        let f = room(db, |_| None, None).await;
        let resp = super::super::send_room_message(
            f.user_id,
            &f.username,
            &f.room_id,
            db,
            &request(false),
        )
        .await
        .expect("plaintext send");
        assert!(!resp.is_encrypted);
        let (payload, is_encrypted) = stored(db, &f).await;
        assert!(!is_encrypted);
        assert_eq!(payload, serde_json::json!({"text": "secret plaintext"}));
        fixture.close().await;
    }
}
