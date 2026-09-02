//! Unit tests for delivery queue classification and retry helpers.

use crate::federation::types::key_id;

use super::queue_and_query::*;

const WEEK_SECS: i64 = 7 * 24 * 60 * 60;

#[test]
fn revocation_needs_both_a_sustained_count_and_a_week_long_streak() {
    // A count alone is not enough: a 20-row fan-out can fail this many times
    // inside a single worker tick while the peer is merely restarting.
    for failures in [i32::MIN, -1, 0, 1, 5, 12, 19, 20, 500] {
        assert!(
            !should_revoke_relationships(failures, 0),
            "instantaneous burst of {failures} must not revoke"
        );
        assert!(!should_revoke_relationships(failures, WEEK_SECS - 1));
    }
    // Elapsed time alone is not enough either: a peer we barely talk to can be
    // failing for months without ever accumulating the count.
    for failures in [i32::MIN, -1, 0, 1, 5, 12, 19] {
        assert!(
            !should_revoke_relationships(failures, WEEK_SECS * 52),
            "only {failures} failures must not revoke however old the streak is"
        );
    }
    assert!(should_revoke_relationships(20, WEEK_SECS));
    assert!(should_revoke_relationships(21, WEEK_SECS + 1));
}

#[test]
fn revocation_reason_tracks_the_thresholds_it_describes() {
    let reason = domain_revocation_reason();
    // `retry_all_dead_for_user` skips anything with this prefix, so losing it
    // would silently make revoked rows retryable again.
    assert!(reason.starts_with("cancelled:"));
    assert!(is_user_cancelled_delivery_error(Some(&reason)));
    // Derived from the constants rather than restated, so the two cannot drift.
    assert!(reason.contains("20 unreachable delivery attempts"));
    assert!(reason.contains("7+ days"));
}

#[test]
fn only_dns_failures_from_client_preparation_count_as_remote_failures() {
    assert!(outbound_client_error_counts_as_remote_failure(
        "DNS resolution failed"
    ));
    assert!(outbound_client_error_counts_as_remote_failure(
        "DNS resolution returned no addresses"
    ));
    for local_error in [
        "Invalid URL",
        "Only HTTP and HTTPS URLs are allowed",
        "URL credentials are not allowed",
        "Target resolves to no public addresses",
        "HTTP client error",
    ] {
        assert!(!outbound_client_error_counts_as_remote_failure(local_error));
    }
}

#[test]
fn retry_backoff_grows_exponentially_and_is_capped() {
    // 低位不加抖动，可精确断言
    assert_eq!(retry_backoff_secs(0), 1);
    assert_eq!(retry_backoff_secs(1), 2);
    // 高位落在 base ±25% 内
    for attempts in 3..=16 {
        let base = 2i64.pow(attempts as u32).min(86_400);
        let got = retry_backoff_secs(attempts);
        let spread = base / 4;
        assert!(
            got >= (base - spread).max(1) && got <= (base + spread).min(86_400),
            "attempts={attempts} base={base} got={got}"
        );
    }
    // 永不超过 24h，永不为 0；且极大 attempts 不会溢出 panic
    for attempts in [17, 32, 64, i32::MAX] {
        let got = retry_backoff_secs(attempts);
        assert!((1..=86_400).contains(&got), "attempts={attempts} got={got}");
    }
}

#[test]
fn retry_backoff_jitter_actually_spreads_a_batch() {
    // 同一实例宕机时积压的投递必须算出不同的 next_retry_at，
    // 否则对方一恢复就被我们同时打一轮
    let seen: std::collections::HashSet<i64> = (0..64).map(|_| retry_backoff_secs(10)).collect();
    assert!(seen.len() > 1, "backoff must not be deterministic at scale");
}
use super::*;
use serde_json::json;

#[test]
fn move_signing_uses_old_actor_base() {
    let act = json!({
        "type": "Move",
        "actor": "https://old.example/users/alice",
        "object": "https://old.example/users/alice",
        "target": "https://new.example/users/alice",
    });
    let (base, user) = signing_identity_for_activity("Move", &act, "https://new.example", "alice");
    assert_eq!(base, "https://old.example");
    assert_eq!(user, "alice");
}

#[test]
fn missing_keys_error_is_detected() {
    // Exact production log text from SELECT returning no row.
    assert!(is_missing_federation_keys_error(
        "No federation keys found for user"
    ));
    assert!(is_missing_federation_keys_error(
        "No federation keys found for user and username empty (cannot ensure)"
    ));
    // Decrypt path must never be treated as missing (no re-generate).
    assert!(!is_missing_federation_keys_error(
        "Key decryption failed: AES-GCM decryption failed"
    ));
    assert!(!is_missing_federation_keys_error(
        "DB error: connection refused"
    ));
    assert!(!is_missing_federation_keys_error(
        "Key load failed (ensure): Key generation failed"
    ));
}

#[test]
fn resolve_signing_key_id_prefers_stored_except_move() {
    let stored = "https://new.example/users/alice#main-key";
    // Normal activities: stored keyId wins (post domain-move G).
    assert_eq!(
        resolve_signing_key_id("Follow", "https://old.example", "alice", Some(stored),),
        stored
    );
    assert_eq!(
        resolve_signing_key_id("Create", "https://old.example", "alice", Some("  ")),
        key_id("https://old.example", "alice")
    );
    assert_eq!(
        resolve_signing_key_id("Follow", "https://old.example", "alice", None),
        key_id("https://old.example", "alice")
    );
    // Move: always old actor origin, ignore stored (which may already be new host).
    assert_eq!(
        resolve_signing_key_id("Move", "https://old.example", "alice", Some(stored)),
        key_id("https://old.example", "alice")
    );
}

#[test]
fn unrecoverable_key_load_only_empty_username() {
    assert!(is_unrecoverable_key_load_error(
        "No federation keys found for user and username empty (cannot ensure)"
    ));
    // Decrypt: no ensure/rotate, but still backoff (not permanent here).
    assert!(!is_unrecoverable_key_load_error(
        "Key decryption failed: AES-GCM decryption failed"
    ));
    // Missing keys after ensure can still back off (race / DB blip).
    assert!(!is_unrecoverable_key_load_error(
        "No federation keys found for user"
    ));
    assert!(!is_unrecoverable_key_load_error(
            "Key load failed (ensure): Key generation failed; original: No federation keys found for user"
        ));
}

#[test]
fn non_move_signing_uses_default_base() {
    let act = json!({
        "type": "Create",
        "actor": "https://new.example/users/alice",
    });
    let (base, user) =
        signing_identity_for_activity("Create", &act, "https://new.example", "alice");
    assert_eq!(base, "https://new.example");
    assert_eq!(user, "alice");
}

#[test]
fn retry_status_allows_dead_and_pending_only() {
    assert_eq!(classify_retry_status("dead"), RetryStatusDecision::Allow);
    assert_eq!(classify_retry_status("pending"), RetryStatusDecision::Allow);
    assert_eq!(
        classify_retry_status("delivered"),
        RetryStatusDecision::AlreadyDelivered
    );
    assert_eq!(
        classify_retry_status("delivering"),
        RetryStatusDecision::InProgress
    );
    // Legacy / soft statuses: allow requeue (single-id path).
    assert_eq!(classify_retry_status("failed"), RetryStatusDecision::Allow);
    assert_eq!(
        classify_retry_status("cancelled"),
        RetryStatusDecision::Allow
    );
}

#[test]
fn single_retry_may_surface_revived_cancelled_flag() {
    // Contract for retry_delivery_item response shaping (no DB):
    // bulk retry-all skips cancelled; single-id may revive and should flag it.
    assert!(is_user_cancelled_delivery_error(Some("cancelled: by user")));
    // Non-cancelled dead letters must not set revived_cancelled.
    assert!(!is_user_cancelled_delivery_error(Some(
        "PERMANENT HTTP 401: signature failed"
    )));
    assert!(!is_user_cancelled_delivery_error(Some("suite seeded dead")));
}

#[test]
fn cancel_status_idempotent_dead_rejects_delivered() {
    assert_eq!(
        classify_cancel_status("dead"),
        CancelStatusDecision::AlreadyDead
    );
    assert_eq!(
        classify_cancel_status("pending"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(
        classify_cancel_status("delivering"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(
        classify_cancel_status("delivered"),
        CancelStatusDecision::AlreadyDelivered
    );
}

#[test]
fn user_cancelled_error_classifier() {
    assert!(is_user_cancelled_delivery_error(Some("cancelled: by user")));
    assert!(is_user_cancelled_delivery_error(Some(
        "cancelled: room closed"
    )));
    assert!(is_user_cancelled_delivery_error(Some("Cancelled: by user")));
    assert!(is_user_cancelled_delivery_error(Some("cancelled: suite")));
    assert!(is_user_cancelled_delivery_error(Some(
        "cancelled: channel closed"
    )));
    assert!(is_user_cancelled_delivery_error(Some("CANCELLED: by user")));
    assert!(!is_user_cancelled_delivery_error(Some("suite seeded dead")));
    assert!(!is_user_cancelled_delivery_error(Some("Key load failed")));
    assert!(!is_user_cancelled_delivery_error(Some(
        "HTTP 401 Unauthorized"
    )));
    assert!(!is_user_cancelled_delivery_error(None));
    assert!(!is_user_cancelled_delivery_error(Some("   ")));
    assert!(!is_user_cancelled_delivery_error(Some("")));
    // Substring alone must not match (real peer errors mentioning cancel).
    assert!(!is_user_cancelled_delivery_error(Some(
        "remote said: cancelled by policy"
    )));
    assert!(!is_user_cancelled_delivery_error(Some(
        "not cancelled: by user"
    )));
}

#[test]
fn retry_status_allows_failed_and_cancelled_status_strings() {
    // Historical / alternate status spellings still requeue.
    assert_eq!(classify_retry_status("failed"), RetryStatusDecision::Allow);
    assert_eq!(
        classify_retry_status("cancelled"),
        RetryStatusDecision::Allow
    );
}

#[test]
fn cancel_status_unknown_defaults_to_cancel() {
    // Defensive: unknown status is treated as cancellable rather than stuck.
    assert_eq!(
        classify_cancel_status("unknown"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(classify_cancel_status(""), CancelStatusDecision::Cancel);
}

#[test]
fn resolve_signing_key_id_trims_stored_whitespace() {
    assert_eq!(
        resolve_signing_key_id(
            "Follow",
            "https://example.com",
            "alice",
            Some("  https://example.com/users/alice#main-key  "),
        ),
        "https://example.com/users/alice#main-key"
    );
}

#[test]
fn is_missing_federation_keys_error_no_false_positive() {
    assert!(is_missing_federation_keys_error(
        "No federation keys found for user"
    ));
    // Must not treat "keys" substrings in unrelated errors as missing material.
    assert!(!is_missing_federation_keys_error(
        "HTTP 500: failed to load remote keys endpoint"
    ));
    assert!(!is_missing_federation_keys_error("signature keys mismatch"));
}

#[test]
fn move_signing_empty_actor_falls_back_to_default() {
    use serde_json::json;
    let act = json!({"type": "Move", "actor": ""});
    let (base, user) = signing_identity_for_activity("Move", &act, "https://new.example", "alice");
    assert_eq!(base, "https://new.example");
    assert_eq!(user, "alice");
}

#[test]
fn move_signing_malformed_actor_path_falls_back() {
    use serde_json::json;
    let act = json!({"type": "Move", "actor": "https://old.example/not-users/alice"});
    let (base, user) = signing_identity_for_activity("Move", &act, "https://new.example", "alice");
    assert_eq!(base, "https://new.example");
    assert_eq!(user, "alice");
}

#[test]
fn is_unrecoverable_key_load_requires_empty_username_phrase() {
    assert!(is_unrecoverable_key_load_error(
        "No federation keys found for user and username empty (cannot ensure)"
    ));
    assert!(!is_unrecoverable_key_load_error(
        "No federation keys found for user"
    ));
}

#[test]
fn classify_retry_delivering_is_in_progress() {
    assert_eq!(
        classify_retry_status("delivering"),
        RetryStatusDecision::InProgress
    );
}

/// Parity with suite `wait_delivery_side` / `dead_fail_count`:
/// dead + cancelled:% must NOT fail waiters; other dead messages must.
#[test]
fn wait_delivery_fail_filter_mirrors_suite_cancelled_exclusion() {
    // Suite SQL: status=dead AND error_message NOT ILIKE 'cancelled:%'
    // → fail wait only when is_user_cancelled is false (and there is a dead row).
    let suite_would_fail = |err: Option<&str>| !is_user_cancelled_delivery_error(err);
    assert!(!suite_would_fail(Some("cancelled: by user")));
    assert!(!suite_would_fail(Some("CANCELLED: by user")));
    assert!(!suite_would_fail(Some("cancelled: suite")));
    assert!(suite_would_fail(Some("suite seeded dead")));
    assert!(suite_would_fail(Some("Key load failed")));
    assert!(suite_would_fail(Some("HTTP 500")));
    // NULL / empty message: not a cancel — suite still counts the dead row.
    assert!(suite_would_fail(None));
    assert!(suite_would_fail(Some("")));
}

#[test]
fn bulk_retry_skip_matrix_cancelled_vs_real_dead() {
    // Documents retry_all_dead_for_user skip branch without DB.
    let cases: &[(&str, bool)] = &[
        ("cancelled: by user", true),
        ("cancelled: pending cleared", true),
        ("suite seeded dead", false),
        ("Key decryption failed", false),
        ("PERMANENT HTTP 401", false),
    ];
    for (msg, skip) in cases {
        assert_eq!(
            is_user_cancelled_delivery_error(Some(msg)),
            *skip,
            "msg={msg}"
        );
    }
}

#[test]
fn cancel_race_terminal_dead_is_success_not_conflict() {
    // When cancel UPDATE races the worker and rows_affected==0, re-read:
    // dead → already success; delivered → reject. Suite wait_delivery_side
    // only fails on non-cancelled dead (cancelled: by user is ignored).
    assert_eq!(
        classify_cancel_status("dead"),
        CancelStatusDecision::AlreadyDead
    );
    assert_eq!(
        classify_cancel_status("delivered"),
        CancelStatusDecision::AlreadyDelivered
    );
    assert!(is_user_cancelled_delivery_error(Some("cancelled: by user")));
    assert!(!is_user_cancelled_delivery_error(Some(
        "Request failed: connection refused"
    )));
}

#[test]
fn classify_cancel_status_unknown_and_failed() {
    // Unknown / failed / cancelled → treat as cancelable (best-effort).
    assert_eq!(
        classify_cancel_status("failed"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(
        classify_cancel_status("cancelled"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(classify_cancel_status(""), CancelStatusDecision::Cancel);
    assert_eq!(classify_cancel_status("DEAD"), CancelStatusDecision::Cancel); // case-sensitive
                                                                              // Canonical dead stays idempotent
    assert_eq!(
        classify_cancel_status("dead"),
        CancelStatusDecision::AlreadyDead
    );
}

#[test]
fn classify_retry_status_failed_cancelled_unknown() {
    assert_eq!(classify_retry_status("failed"), RetryStatusDecision::Allow);
    assert_eq!(
        classify_retry_status("cancelled"),
        RetryStatusDecision::Allow
    );
    assert_eq!(classify_retry_status("unknown"), RetryStatusDecision::Allow);
    assert_eq!(classify_retry_status(""), RetryStatusDecision::Allow);
    // Case: delivered is exact match only
    assert_eq!(
        classify_retry_status("Delivered"),
        RetryStatusDecision::Allow
    );
    assert_eq!(
        classify_retry_status("delivered"),
        RetryStatusDecision::AlreadyDelivered
    );
}

#[test]
fn move_signing_empty_or_malformed_actor_fallback() {
    let empty = json!({"type": "Move", "actor": ""});
    let (base, user) =
        signing_identity_for_activity("Move", &empty, "https://new.example/", "alice");
    assert_eq!(base, "https://new.example");
    assert_eq!(user, "alice");

    let bad = json!({"type": "Move", "actor": "https://old.example/not-users/alice"});
    let (base2, user2) =
        signing_identity_for_activity("Move", &bad, "https://new.example", "alice");
    assert_eq!(base2, "https://new.example");
    assert_eq!(user2, "alice");

    let nested = json!({"type": "Move", "actor": "https://old.example/users/alice/inbox"});
    let (base3, user3) =
        signing_identity_for_activity("Move", &nested, "https://new.example", "alice");
    assert_eq!(base3, "https://new.example");
    assert_eq!(user3, "alice");
}

#[test]
fn move_signing_trims_trailing_slash_on_username() {
    let act = json!({
        "type": "Move",
        "actor": "https://old.example/users/alice/",
    });
    let (base, user) = signing_identity_for_activity("Move", &act, "https://new.example", "bob");
    assert_eq!(base, "https://old.example");
    assert_eq!(user, "alice");
}

#[test]
fn non_move_signing_ignores_actor_field() {
    // Follow/Create must use default base even if actor points elsewhere.
    let act = json!({
        "type": "Follow",
        "actor": "https://old.example/users/alice",
    });
    let (base, user) =
        signing_identity_for_activity("Follow", &act, "https://new.example/", "carol");
    assert_eq!(base, "https://new.example");
    assert_eq!(user, "carol");
}

#[test]
fn classify_cancel_and_retry_pending_delivering_matrix() {
    // pending: cancel + retry both allowed
    assert_eq!(
        classify_cancel_status("pending"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(classify_retry_status("pending"), RetryStatusDecision::Allow);
    // delivering: cancel ok, retry blocked as in-progress
    assert_eq!(
        classify_cancel_status("delivering"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(
        classify_retry_status("delivering"),
        RetryStatusDecision::InProgress
    );
    // dead: cancel idempotent, retry allowed
    assert_eq!(
        classify_cancel_status("dead"),
        CancelStatusDecision::AlreadyDead
    );
    assert_eq!(classify_retry_status("dead"), RetryStatusDecision::Allow);
    // delivered: both terminal rejects
    assert_eq!(
        classify_cancel_status("delivered"),
        CancelStatusDecision::AlreadyDelivered
    );
    assert_eq!(
        classify_retry_status("delivered"),
        RetryStatusDecision::AlreadyDelivered
    );
}

#[test]
fn r37_classify_cancel_status_pending_and_delivering() {
    assert_eq!(
        classify_cancel_status("pending"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(
        classify_cancel_status("delivering"),
        CancelStatusDecision::Cancel
    );
    assert_eq!(
        classify_cancel_status("dead"),
        CancelStatusDecision::AlreadyDead
    );
    assert_eq!(
        classify_cancel_status("delivered"),
        CancelStatusDecision::AlreadyDelivered
    );
}

#[test]
fn r38_classify_retry_status_dead_vs_delivering() {
    assert_eq!(classify_retry_status("dead"), RetryStatusDecision::Allow);
    assert_eq!(
        classify_retry_status("delivering"),
        RetryStatusDecision::InProgress
    );
    assert_eq!(
        classify_retry_status("delivered"),
        RetryStatusDecision::AlreadyDelivered
    );
}

#[test]
fn r39_classify_retry_status_failed_and_cancelled_allow() {
    assert_eq!(classify_retry_status("failed"), RetryStatusDecision::Allow);
    assert_eq!(
        classify_retry_status("cancelled"),
        RetryStatusDecision::Allow
    );
}

#[test]
fn r40_is_missing_federation_keys_error_exact_substring() {
    assert!(is_missing_federation_keys_error(
        "No federation keys found for user"
    ));
    assert!(is_missing_federation_keys_error(
        "x: No federation keys found for user"
    ));
    assert!(!is_missing_federation_keys_error("No keys found for user"));
    assert!(!is_missing_federation_keys_error("Key decryption failed"));
}

#[test]
fn r41_is_user_cancelled_delivery_error_prefix() {
    assert!(is_user_cancelled_delivery_error(Some(
        "cancelled: pending by user"
    )));
    assert!(is_user_cancelled_delivery_error(Some("CANCELLED: x")));
    assert!(!is_user_cancelled_delivery_error(Some("not-cancelled: x")));
    assert!(!is_user_cancelled_delivery_error(None));
}

#[test]
fn r42_signing_identity_move_parses_old_actor() {
    let act = serde_json::json!({
        "type": "Move",
        "actor": "https://old.example/users/alice"
    });
    let (base, user) = signing_identity_for_activity("Move", &act, "https://new.example", "alice");
    assert_eq!(base, "https://old.example");
    assert_eq!(user, "alice");
}

#[test]
fn r43_resolve_signing_key_id_move_ignores_stored() {
    assert_eq!(
        resolve_signing_key_id(
            "Move",
            "https://old.example",
            "alice",
            Some("https://new.example/users/alice#main-key"),
        ),
        key_id("https://old.example", "alice")
    );
}

#[test]
fn r44_resolve_signing_key_id_follow_prefers_stored() {
    let stored = "https://new.example/users/alice#main-key";
    assert_eq!(
        resolve_signing_key_id("Follow", "https://old.example", "alice", Some(stored)),
        stored
    );
}

/// Resource teardown fan-outs must never be cancelled by resource-id LIKE.
/// Production bug: delete_room enqueued RoomDissolve then cancelled all
/// object_json LIKE %room_id% — including dissolve itself.
#[test]
fn teardown_activity_types_excluded_from_resource_cancel() {
    assert!(is_resource_teardown_activity_type("RoomDissolve"));
    assert!(is_resource_teardown_activity_type("ChannelClose"));
    assert!(is_resource_teardown_activity_type("myriad:RoomDissolve"));
    assert!(is_resource_teardown_activity_type("myriad:ChannelClose"));
    assert!(is_resource_teardown_activity_type("Myriad:RoomDissolve"));
    assert!(is_resource_teardown_activity_type(" roomdissolve "));
    assert!(is_resource_teardown_activity_type("CHANNELCLOSE"));
    // Stale traffic — must remain cancellable.
    assert!(!is_resource_teardown_activity_type("RoomMessage"));
    assert!(!is_resource_teardown_activity_type("myriad:RoomMessage"));
    assert!(!is_resource_teardown_activity_type("ChannelMessage"));
    assert!(!is_resource_teardown_activity_type("myriad:ChannelMessage"));
    assert!(!is_resource_teardown_activity_type("KeyExchange"));
    assert!(!is_resource_teardown_activity_type("myriad:KeyExchange"));
    assert!(!is_resource_teardown_activity_type("RoomInvite"));
    assert!(!is_resource_teardown_activity_type("Follow"));
    assert!(!is_resource_teardown_activity_type(""));
    assert!(!is_resource_teardown_activity_type("RoomDissolveExtra"));
}

/// Semantic contract of cancel_pending_deliveries_for_resource without DB:
/// for a given room_id, RoomDissolve stays pending; RoomMessage is cancelled.
#[test]
fn cancel_pending_for_resource_cancels_message_not_dissolve() {
    let room_id = "room-abc-123";
    // Simulated activity rows mentioning the same room id.
    let rows: &[(&str, &str)] = &[
        ("RoomDissolve", room_id),
        ("myriad:RoomDissolve", room_id),
        ("RoomMessage", room_id),
        ("myriad:RoomMessage", room_id),
        ("KeyExchange", room_id),
        ("RoomInvite", room_id),
    ];
    let would_cancel = |activity_type: &str, object_mentions_resource: bool| {
        object_mentions_resource && !is_resource_teardown_activity_type(activity_type)
    };
    for (ty, rid) in rows {
        let mentions = *rid == room_id;
        let cancel = would_cancel(ty, mentions);
        match *ty {
            "RoomDissolve" | "myriad:RoomDissolve" => {
                assert!(!cancel, "teardown {ty} must stay pending");
            }
            _ => {
                assert!(cancel, "stale traffic {ty} must be cancelled");
            }
        }
    }
    // ChannelClose path: same exclusion when channel_id matches.
    assert!(!would_cancel("ChannelClose", true));
    assert!(!would_cancel("myriad:ChannelClose", true));
    assert!(would_cancel("ChannelMessage", true));
}

#[test]
fn intentional_cancel_display_and_no_retry_helper() {
    // cancelled:… → Cancelled label, no Retry
    let cancelled_msgs = [
        "cancelled: by user",
        "cancelled: local channel closed",
        "cancelled: local room dissolved",
        "cancelled: remote room dissolved",
        "cancelled: local channel deleted",
        "CANCELLED: suite",
    ];
    for msg in cancelled_msgs {
        assert!(is_intentional_cancel_delivery_error(Some(msg)), "msg={msg}");
        assert!(
            !should_offer_retry_for_dead_error(Some(msg)),
            "no retry for {msg}"
        );
    }
    // Real failures → show Failed + Retry
    for msg in [
        "PERMANENT HTTP 401: signature failed",
        "HTTP 500: boom",
        "suite seeded dead",
        "Key load failed",
    ] {
        assert!(!is_intentional_cancel_delivery_error(Some(msg)));
        assert!(should_offer_retry_for_dead_error(Some(msg)));
    }
    assert!(should_offer_retry_for_dead_error(None));
    assert!(should_offer_retry_for_dead_error(Some("")));
    // Mid-string "cancelled" is a peer error, still retriable.
    assert!(should_offer_retry_for_dead_error(Some(
        "remote said: cancelled by policy"
    )));
}
