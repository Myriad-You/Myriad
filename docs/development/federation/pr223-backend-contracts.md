# PR #223 federation backend contracts (tests)

Living checklist for unit tests that pin ensure-keys / Accept / delivery / rotate
behavior. Prefer pure tests (no DB) so CI stays fast.

## Keys

| Contract | Test location |
|----------|----------------|
| ensure never treats live PEM as missing | `actor::tests::ensure_never_treats_live_pem_as_missing` |
| rotate requires JSON `confirm: true` only | `actor::tests::key_rotation_confirm_gate` |
| decrypt failure must not re-generate | `delivery::tests::missing_keys_error_is_detected` |
| empty username key load is permanent dead | `delivery::tests::unrecoverable_key_load_only_empty_username` |
| stored keyId preferred except Move | `delivery::tests::resolve_signing_key_id_prefers_stored_except_move` |

## Follow Accept

| Contract | Test location |
|----------|----------------|
| nested Follow object id | `inbox::tests::accept_matches_nested_follow_object_id` |
| same-host different user rejected | `inbox::tests::accept_rejects_same_host_different_user_even_with_id` |
| path alias `/@bob` and `/ap/users/bob` | `accept_unique_id_same_host_path_drift_alias`, `accept_path_alias_ap_users_form_matches_stored_users_url` |
| expanded actor `{id}` | `extract_activity_actor_id_string_and_expanded`, `accept_matches_with_expanded_actor_object` |
| object array / Link href | `extract_accept_object_id_from_array_and_link` |

## Delivery cancel / retry

| Contract | Test location |
|----------|----------------|
| cancel dead is idempotent; delivered rejected | `cancel_status_idempotent_dead_rejects_delivered` |
| bulk retry-dead skips `cancelled:%` | `user_cancelled_error_classifier` + suite `deploy_retry_dead_skips_user_cancel` |
| single-id retry may revive cancelled | `single_retry_may_surface_revived_cancelled_flag` |
| retry status gates | `retry_status_allows_dead_and_pending_only` |

## Host attribution

| Contract | Test location |
|----------|----------------|
| `POST keys/rotate` → federation:write | `host_attribution::federation_keys_rotate_and_cancel_pending_are_write` |
| delivery list/stats read; retry/cancel write | same |

## Multi-instance suite isolation

See header of `scripts/dev/federation-multi-instance-suite.sh` for PORT/DB
isolation when multiple agents share a host.
