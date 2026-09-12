use super::*;

#[test]
fn accept_matches_follow_activity_id() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &candidates,
    );
    assert_eq!(
        got,
        Some((
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            false
        ))
    );
}

#[test]
fn accept_matches_activity_id_trailing_slash() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1/",
        "https://B.example/users/bob",
        &candidates,
    );
    assert!(got.is_some());
}

#[test]
fn accept_fallback_single_pending_to_actor() {
    let candidates = vec![(
        "https://a.example/activities/missing".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://other/activities/x",
        "https://b.example/users/bob/",
        &candidates,
    );
    assert_eq!(
        got.map(|(id, _, already)| (id, already)),
        Some(("https://a.example/activities/missing".into(), false))
    );
}

#[test]
fn accept_fallback_ambiguous_does_not_match() {
    let candidates = vec![
        (
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        ),
        (
            "https://a.example/activities/2".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        ),
    ];
    let got = resolve_follow_accept_target(
        "https://unknown/activities/z",
        "https://b.example/users/bob",
        &candidates,
    );
    assert!(got.is_none());
}

#[test]
fn accept_rejects_actor_mismatch_even_with_id() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://evil.example/users/mallory",
        &candidates,
    );
    assert!(got.is_none());
}

#[test]
fn accept_idempotent_already_accepted_by_id() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "accepted".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &candidates,
    );
    assert_eq!(got.map(|(_, _, already)| already), Some(true));
}

#[test]
fn accept_matches_username_case_drift_on_same_host() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/Bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &candidates,
    );
    assert!(got.is_some());
}

#[test]
fn accept_rejects_same_host_different_user_even_with_id() {
    // Multi-user instances share a host. activity_id is not a capability:
    // Carol must not Accept Alice→Bob by citing Bob's Follow id.
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/carol",
        &candidates,
    );
    assert!(got.is_none());
    // Substring username tricks (/users/bob.extra) must also fail.
    let got2 = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob.extra",
        &candidates,
    );
    assert!(got2.is_none());
}

#[test]
fn accept_unique_id_same_host_path_drift_alias() {
    // Same host+user under non-standard Accept.actor path form.
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/@bob",
        &candidates,
    );
    assert!(got.is_some());
}

#[test]
fn accept_matches_nested_follow_object_id() {
    let activity = serde_json::json!({
        "type": "Accept",
        "actor": "https://b.example/users/bob",
        "object": {
            "type": "Follow",
            "id": "https://a.example/activities/1",
            "actor": "https://a.example/users/alice",
            "object": "https://b.example/users/bob"
        }
    });
    assert_eq!(
        extract_accept_object_id(&activity),
        "https://a.example/activities/1"
    );
    assert_eq!(extract_accept_object_type(&activity), "Follow");
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let follow_id = extract_accept_object_id(&activity);
    let got =
        resolve_follow_accept_target(&follow_id, activity["actor"].as_str().unwrap(), &candidates);
    assert!(got.is_some());
}

#[test]
fn accept_matches_string_object_with_query_and_fragment() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://A.example/activities/1/?utm=1#section",
        "https://b.example/users/bob",
        &candidates,
    );
    assert!(got.is_some());
}

#[test]
fn accept_idempotent_already_accepted_with_query_drift() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "accepted".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1?retry=1",
        "https://b.example/users/bob",
        &candidates,
    );
    assert_eq!(got.map(|(_, _, already)| already), Some(true));
}

#[test]
fn accept_ambiguous_id_hits_on_same_host_no_silent_pick() {
    let candidates = vec![
        (
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        ),
        (
            "https://a.example/activities/1/?x=1".into(), // normalizes to same id
            "https://b.example/users/bob".into(),         // same user, duplicate id rows
            "pending".into(),
        ),
    ];
    // Accept.actor is neither authorized nor username-compatible uniquely
    // across ambiguous id rows → must not silent-pick.
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/other",
        &candidates,
    );
    assert!(got.is_none());
    let got2 = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &candidates,
    );
    // Step 1 iterates and returns the first actor-auth hit (non-ambiguous by design).
    assert!(got2.is_some());
}

#[test]
fn accept_rejects_evil_host_same_username() {
    // Cross-host username-only must never match.
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://evil.example/users/bob",
        &candidates,
    );
    assert!(got.is_none());
    // Fallback with wrong host id also fails
    let got2 = resolve_follow_accept_target(
        "https://unknown/activities/z",
        "https://evil.example/users/bob",
        &candidates,
    );
    assert!(got2.is_none());
}

#[test]
fn extract_accept_object_id_from_string() {
    let activity = serde_json::json!({
        "type": "Accept",
        "actor": "https://b.example/users/bob",
        "object": "https://a.example/activities/9"
    });
    assert_eq!(
        extract_accept_object_id(&activity),
        "https://a.example/activities/9"
    );
    assert_eq!(extract_accept_object_type(&activity), "");
}

#[test]
fn extract_accept_object_id_from_array_and_link() {
    // AS2 multi-value object array (first non-empty id wins).
    let arr = serde_json::json!({
        "type": "Accept",
        "actor": "https://b.example/users/bob",
        "object": [
            {"type": "Follow", "id": "https://a.example/activities/arr-1"}
        ]
    });
    assert_eq!(
        extract_accept_object_id(&arr),
        "https://a.example/activities/arr-1"
    );
    assert_eq!(extract_accept_object_type(&arr), "Follow");

    // Link object uses href.
    let link = serde_json::json!({
        "type": "Accept",
        "actor": "https://b.example/users/bob",
        "object": {"type": "Link", "href": "https://a.example/activities/link-1"}
    });
    assert_eq!(
        extract_accept_object_id(&link),
        "https://a.example/activities/link-1"
    );
}

#[test]
fn extract_activity_actor_id_string_and_expanded() {
    let plain = serde_json::json!({
        "actor": "https://b.example/users/bob"
    });
    assert_eq!(
        extract_activity_actor_id(&plain),
        "https://b.example/users/bob"
    );
    let expanded = serde_json::json!({
        "actor": {
            "type": "Person",
            "id": "https://b.example/users/bob",
            "preferredUsername": "bob"
        }
    });
    assert_eq!(
        extract_activity_actor_id(&expanded),
        "https://b.example/users/bob"
    );
    let empty = serde_json::json!({ "actor": {} });
    assert!(extract_activity_actor_id(&empty).is_empty());
}

#[test]
fn extract_activity_actor_id_link_href_and_string_array() {
    let link = serde_json::json!({
        "actor": {"type": "Link", "href": "https://b.example/users/bob"}
    });
    assert_eq!(
        extract_activity_actor_id(&link),
        "https://b.example/users/bob"
    );
    let arr = serde_json::json!({
        "actor": ["", "  https://b.example/users/bob  "]
    });
    assert_eq!(
        extract_activity_actor_id(&arr),
        "https://b.example/users/bob"
    );
    let arr_obj = serde_json::json!({
        "actor": [
            {"type": "Person"},
            {"type": "Person", "id": "https://b.example/users/carol"}
        ]
    });
    assert_eq!(
        extract_activity_actor_id(&arr_obj),
        "https://b.example/users/carol"
    );
}

#[test]
fn extract_accept_object_skips_empty_array_entries() {
    let activity = serde_json::json!({
        "type": "Accept",
        "actor": "https://b.example/users/bob",
        "object": ["", "   ", "https://a.example/activities/keep"]
    });
    assert_eq!(
        extract_accept_object_id(&activity),
        "https://a.example/activities/keep"
    );
}

#[test]
fn extract_accept_object_type_from_array_first_typed() {
    let activity = serde_json::json!({
        "type": "Accept",
        "actor": "https://b.example/users/bob",
        "object": [
            {"id": "https://a.example/activities/x"},
            {"type": "Follow", "id": "https://a.example/activities/y"}
        ]
    });
    assert_eq!(extract_accept_object_type(&activity), "Follow");
    assert_eq!(
        extract_accept_object_id(&activity),
        "https://a.example/activities/x"
    );
    let link_only = serde_json::json!({
        "object": {"type": "Link", "href": "https://a.example/activities/z"}
    });
    // Link has a type but Accept routing treats pure Link object type as reported.
    assert_eq!(extract_accept_object_type(&link_only), "Link");
}

#[test]
fn accept_matches_with_expanded_actor_object() {
    // Peers that embed actor document must still authorize by id.
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let activity = serde_json::json!({
        "type": "Accept",
        "actor": {
            "type": "Person",
            "id": "https://b.example/users/bob"
        },
        "object": {
            "type": "Follow",
            "id": "https://a.example/activities/1"
        }
    });
    let actor = extract_activity_actor_id(&activity);
    let follow_id = extract_accept_object_id(&activity);
    let got = resolve_follow_accept_target(&follow_id, &actor, &candidates);
    assert!(got.is_some());
}

#[test]
fn accept_fallback_skips_already_accepted_when_id_unknown() {
    // Fallback only considers pending; an accepted-only set must not re-match.
    let candidates = vec![(
        "https://a.example/activities/old".into(),
        "https://b.example/users/bob".into(),
        "accepted".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://unknown/activities/z",
        "https://b.example/users/bob",
        &candidates,
    );
    assert!(got.is_none());
}

#[test]
fn extract_accept_object_id_trims_whitespace() {
    let activity = serde_json::json!({
        "type": "Accept",
        "actor": "https://b.example/users/bob",
        "object": "  https://a.example/activities/ws  "
    });
    assert_eq!(
        extract_accept_object_id(&activity),
        "https://a.example/activities/ws"
    );
}

#[test]
fn extract_accept_object_id_missing_or_empty() {
    assert_eq!(extract_accept_object_id(&serde_json::json!({})), "");
    assert_eq!(
        extract_accept_object_id(&serde_json::json!({"object": ""})),
        ""
    );
    assert_eq!(
        extract_accept_object_id(&serde_json::json!({"object": {}})),
        ""
    );
    assert_eq!(
        extract_accept_object_id(&serde_json::json!({"object": []})),
        ""
    );
}

#[test]
fn same_actor_or_user_matches_host_user_case() {
    assert!(same_actor_or_user(
        "https://b.example/users/Bob",
        "https://B.EXAMPLE/users/bob"
    ));
    assert!(!same_actor_or_user(
        "https://b.example/users/bob",
        "https://evil.example/users/bob"
    ));
    assert!(!same_actor_or_user(
        "https://b.example/users/bob",
        "https://b.example/users/carol"
    ));
}

#[test]
fn extract_accept_object_type_from_nested_follow() {
    let activity = serde_json::json!({
        "type": "Accept",
        "object": {"type": "Follow", "id": "https://a.example/activities/1"}
    });
    assert_eq!(extract_accept_object_type(&activity), "Follow");
    assert_eq!(
        extract_accept_object_id(&activity),
        "https://a.example/activities/1"
    );
}

#[test]
fn extract_activity_actor_id_from_link_href() {
    let activity = serde_json::json!({
        "actor": {"type": "Link", "href": "https://b.example/users/bob"}
    });
    assert_eq!(
        extract_activity_actor_id(&activity),
        "https://b.example/users/bob"
    );
}

#[test]
fn extract_iri_or_object_id_from_string_array() {
    let activity = serde_json::json!({
        "object": ["", "  https://a.example/activities/arr  "]
    });
    assert_eq!(
        extract_accept_object_id(&activity),
        "https://a.example/activities/arr"
    );
}

#[test]
fn extract_accept_object_id_trims_string_and_nested() {
    assert_eq!(
        extract_accept_object_id(&serde_json::json!({
            "object": "  https://a.example/activities/1  "
        })),
        "https://a.example/activities/1"
    );
    assert_eq!(
        extract_accept_object_id(&serde_json::json!({
            "object": {"id": "  https://a.example/activities/2  ", "type": "Follow"}
        })),
        "https://a.example/activities/2"
    );
    assert_eq!(
        extract_accept_object_type(&serde_json::json!({
            "object": {"id": "x", "type": "myriad:ChannelOpen"}
        })),
        "myriad:ChannelOpen"
    );
}

#[test]
fn same_host_username_compatible_at_handle_and_reject() {
    assert!(same_host_username_compatible(
        "https://b.example/@bob",
        "https://b.example/users/bob"
    ));
    assert!(same_host_username_compatible(
        "https://b.example/@Bob",
        "https://b.example/users/bob"
    ));
    // Different user on same host
    assert!(!same_host_username_compatible(
        "https://b.example/@carol",
        "https://b.example/users/bob"
    ));
    // Cross host
    assert!(!same_host_username_compatible(
        "https://evil.example/@bob",
        "https://b.example/users/bob"
    ));
}

#[test]
fn resolve_follow_accept_empty_candidates_and_empty_id() {
    assert!(resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &[],
    )
    .is_none());
    // Empty follow_id skips id match; only unique pending-to-actor fallback.
    let candidates = vec![(
        "https://a.example/activities/9".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target("", "https://b.example/users/bob", &candidates);
    assert_eq!(
        got.map(|(id, _, already)| (id, already)),
        Some(("https://a.example/activities/9".into(), false))
    );
    // accepted-only with empty id → no pending fallback
    let accepted_only = vec![(
        "https://a.example/activities/9".into(),
        "https://b.example/users/bob".into(),
        "accepted".into(),
    )];
    assert!(
        resolve_follow_accept_target("", "https://b.example/users/bob", &accepted_only).is_none()
    );
}

#[test]
fn resolve_follow_accept_fallback_ignores_non_pending() {
    // Wrong id + only accepted/rejected rows to actor → no match.
    let candidates = vec![
        (
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "accepted".into(),
        ),
        (
            "https://a.example/activities/2".into(),
            "https://b.example/users/bob".into(),
            "rejected".into(),
        ),
    ];
    assert!(resolve_follow_accept_target(
        "https://unknown/activities/z",
        "https://b.example/users/bob",
        &candidates,
    )
    .is_none());
}

#[test]
fn resolve_follow_accept_port_sensitive_actor_auth() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example:8443/users/bob".into(),
        "pending".into(),
    )];
    // Port mismatch → not same actor
    assert!(resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &candidates,
    )
    .is_none());
    assert!(resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example:8443/users/bob",
        &candidates,
    )
    .is_some());
}

#[test]
fn split_actor_host_user_shapes() {
    let (h, u) = split_actor_host_user("https://b.example/users/bob");
    assert_eq!(h, "b.example");
    assert_eq!(u, "bob");
    let (h2, u2) = split_actor_host_user("https://b.example:8443/users/Bob");
    assert_eq!(h2, "b.example:8443");
    assert_eq!(u2, "Bob");
    let (h3, u3) = split_actor_host_user("https://b.example/@bob");
    assert_eq!(h3, "b.example");
    assert_eq!(u3, ""); // non-/users/ path → empty user
    let (h4, u4) = split_actor_host_user("not-a-url");
    assert_eq!(h4, "");
    assert_eq!(u4, "");
}

#[test]
fn resolve_follow_accept_accepted_id_path_drift_idempotent() {
    // Already accepted + @handle Accept.actor still yields already=true.
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "accepted".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1/",
        "https://b.example/@bob",
        &candidates,
    );
    assert_eq!(
        got.map(|(_, remote, already)| (remote, already)),
        Some(("https://b.example/users/bob".into(), true))
    );
}

#[test]
fn resolve_follow_accept_prefers_actor_auth_over_host_only() {
    // Two remotes same host different users; id matches bob only.
    let candidates = vec![
        (
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        ),
        (
            "https://a.example/activities/other".into(),
            "https://b.example/users/carol".into(),
            "pending".into(),
        ),
    ];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &candidates,
    );
    assert_eq!(
        got.map(|(id, remote, _)| (id, remote)),
        Some((
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into()
        ))
    );
    // Carol citing bob's id must fail when she has no own pending row.
    let bob_only = vec![candidates[0].clone()];
    assert!(resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/carol",
        &bob_only,
    )
    .is_none());
}

#[test]
fn r45_same_host_username_compatible_users_path_case() {
    assert!(same_host_username_compatible(
        "https://b.example/users/Bob",
        "https://b.example/users/bob"
    ));
    assert!(!same_host_username_compatible(
        "https://b.example/users/bob",
        "https://b.example/users/carol"
    ));
}

#[test]
fn r46_same_host_username_compatible_at_handle_last_segment() {
    assert!(same_host_username_compatible(
        "https://b.example/@alice",
        "https://b.example/users/alice"
    ));
    assert!(!same_host_username_compatible(
        "https://evil.example/@alice",
        "https://b.example/users/alice"
    ));
}

#[test]
fn r47_resolve_follow_accept_target_no_candidates() {
    assert!(resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &[],
    )
    .is_none());
}

#[test]
fn r48_resolve_follow_accept_target_id_match_trailing_slash() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    assert!(resolve_follow_accept_target(
        "https://a.example/activities/1/",
        "https://b.example/users/bob",
        &candidates,
    )
    .is_some());
}

#[test]
fn r49_resolve_follow_accept_target_rejects_cross_user_same_host() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    assert!(resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/carol",
        &candidates,
    )
    .is_none());
}

#[test]
fn r50_resolve_follow_accept_target_idempotent_accepted() {
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "accepted".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/users/bob",
        &candidates,
    );
    assert_eq!(got.map(|(_, _, already)| already), Some(true));
}

#[test]
fn r51_extract_accept_object_id_string_vs_nested() {
    assert_eq!(
        extract_accept_object_id(&serde_json::json!({
            "object": "https://a.example/activities/z"
        })),
        "https://a.example/activities/z"
    );
    assert_eq!(
        extract_accept_object_id(&serde_json::json!({
            "object": {"id": "https://a.example/activities/n", "type": "Follow"}
        })),
        "https://a.example/activities/n"
    );
}

#[test]
fn r52_same_actor_or_user_rejects_different_ports() {
    assert!(!same_actor_or_user(
        "https://b.example:8443/users/bob",
        "https://b.example/users/bob"
    ));
    assert!(same_actor_or_user(
        "https://b.example:8443/users/bob",
        "https://b.example:8443/users/BOB/"
    ));
}

#[test]
fn extract_activity_actor_id_from_string_array() {
    // AS2 multi-value actor (rare for Accept, valid for some peers).
    let activity = serde_json::json!({
        "actor": ["https://b.example/users/bob", "https://b.example/users/other"]
    });
    assert_eq!(
        extract_activity_actor_id(&activity),
        "https://b.example/users/bob"
    );
    let empty_arr = serde_json::json!({ "actor": [] });
    assert!(extract_activity_actor_id(&empty_arr).is_empty());
    let blank_then_id = serde_json::json!({
        "actor": ["  ", {"id": "https://b.example/users/carol"}]
    });
    assert_eq!(
        extract_activity_actor_id(&blank_then_id),
        "https://b.example/users/carol"
    );
}

#[test]
fn extract_accept_object_id_prefers_id_over_href_on_same_object() {
    // When both id and href exist, id wins (canonical activity id).
    let activity = serde_json::json!({
        "type": "Accept",
        "object": {
            "type": "Follow",
            "id": "https://a.example/activities/id-wins",
            "href": "https://a.example/activities/href-ignored"
        }
    });
    assert_eq!(
        extract_accept_object_id(&activity),
        "https://a.example/activities/id-wins"
    );
}

#[test]
fn accept_path_alias_ap_users_form_matches_stored_users_url() {
    // Some peers emit Accept.actor as /ap/users/{name} while we store /users/{name}.
    let candidates = vec![(
        "https://a.example/activities/1".into(),
        "https://b.example/users/bob".into(),
        "pending".into(),
    )];
    let got = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/ap/users/bob",
        &candidates,
    );
    assert!(got.is_some());
    // Different user under /ap/users must still fail.
    let got_bad = resolve_follow_accept_target(
        "https://a.example/activities/1",
        "https://b.example/ap/users/carol",
        &candidates,
    );
    assert!(got_bad.is_none());
}
