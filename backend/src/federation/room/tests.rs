//! Room unit tests (mechanical move from room.rs).
use super::helpers::*;
use super::members::{
    home_servers_match, may_promote_private_room_to_public, validate_remote_public_room_doc,
};
use super::stickers::{parse_room_stickers, stickers_to_json, ROOM_STICKER_MAX_DATA_LEN};
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
    let big = format!(
        "data:image/png;base64,{}",
        "A".repeat(ROOM_STICKER_MAX_DATA_LEN)
    );
    let too_big = format!(
        "data:image/png;base64,{}",
        "B".repeat(ROOM_STICKER_MAX_DATA_LEN)
    );
    // `big` length is prefix + max => > MAX; craft exact edge
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
    let _ = big; // silence unused if compiler optimizes
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
    require_remote_inbox, room_join_authorized, room_join_effective_role, RoomJoinAuth,
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
    // join_public_room gate is `invite_policy == "open" || is_public`.
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
    // ...but can under member-invite / open, matching invite_to_room.
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
    // Mirrors the `_ if !is_admin_role(..)` arm in invite_to_room.
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
fn room_join_never_overwrites_an_existing_role() {
    // A pending invite carrying role=admin must survive the invitee's accept,
    // and a member must not be able to re-announce itself upward.
    assert_eq!(room_join_effective_role(Some("admin"), "member"), "admin");
    assert_eq!(room_join_effective_role(Some("owner"), "member"), "owner");
    assert_eq!(room_join_effective_role(Some("member"), "owner"), "member");
}
