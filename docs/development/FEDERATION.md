# Federation development notes

Operational notes for Myriad’s ActivityPub + MFP stack. Product inventory lives
in the federation-universe scratch; this file is **how the code behaves** for
developers working on ensure-keys, Accept, delivery, and the dual-instance suite.

## Inbox auth hardening (MYR-022 / MYR-023)

| Concern | Behaviour |
|---------|-----------|
| **MYR-022 actor cache poison** | Signature verification resolves the remote Actor via trusted DB cache **or** an **ephemeral** HTTP fetch. Unauthenticated remote documents are **not** written to `federation_remote_actors` until the HTTP Signature verifies. Failed signatures drop the ephemeral material. |
| **MYR-023 replay** | HTTP `Date` skew alone (`HTTP_DATE_MAX_SKEW` = 5m) is weak. After successful verify, a process-local short-lived dedup cache records body SHA-256 digests and normalized activity ids for ~10m (`replay_dedup_ttl`). Replays answer `202 Accepted` without re-running handlers. Legitimate peer retries within the window stay idempotent. |

Entry points: `fetch_remote_actor_for_verify` / `persist_verified_remote_actor` in `actor.rs`; `is_replay_or_record` in `replay.rs`; wired in `inbox/receive.rs`.

## Keys: ensure vs rotate

| Path | When | Effect |
|------|------|--------|
| **ensure** (`ensure_user_federation_keys`) | Outbound sign / identity cold start | Generate **only** if PEM missing/empty. Never silent-rotate live keys. |
| **rotate** `POST /api/federation/keys/rotate` | Explicit user/admin action | Requires body `{ "confirm": true }`. New RSA keypair, store `key_id`, best-effort `Update(Person)` fan-out. |

Gate helper: `rotation_confirm_accepted` in `backend/src/federation/actor.rs`
(boolean `true` only — strings/numbers rejected).

Host surface (stacked with this branch family):

- REST: `federationApi.rotateKeys({ confirm: true })`
- Bridge: `federation.rotateKeys` → `federation:write`
- Host UI: Config → Federation → **Identity & keys**

## Accept matching (Follow)

`resolve_follow_accept_target` / `extract_accept_object_id` /
`extract_activity_actor_id` in `inbox.rs`:

1. Normalized activity id + Accept.actor is the remote peer (`same_actor_or_user`)
2. Unique id + **same host + username-compatible** path drift (`/@bob` or `/ap/users/bob` vs `/users/bob`)
3. Exactly one **pending** outgoing to Accept.actor

Never: cross-host username-only, same-host different user with only an id leak,
silent multi-pick.

Actor and Accept.object may be string IRIs, expanded `{ id }`, AS2 `Link` (`href`),
or a single-level array of those forms.

## Delivery queue

Statuses: `pending` → `delivering` → `delivered` | `dead`.

| API | Behavior |
|-----|----------|
| `POST …/delivery/{id}/cancel` | pending/delivering → dead, `error_message = 'cancelled: by user'` |
| `POST …/delivery/cancel-pending` | bulk cancel (same message) |
| `POST …/delivery/{id}/retry` | dead/pending (not delivered/in-progress) |
| `POST …/delivery/retry-dead` | bulk requeue **skipping** `cancelled:…` rows (`skipped_cancelled` in JSON) |
| `DELETE …/delivery/{id}` | purge a **dead** row (dismiss clutter; pending/delivered rejected) |
| `POST …/delivery/purge-dead` | bulk delete dead rows; `?cancelled_only=true` keeps only `cancelled:%` |

Classifier: `is_user_cancelled_delivery_error` — exact `cancelled: by user` (case-insensitive)
or any `cancelled:` prefix (room/channel teardown). Peer errors that merely mention
“cancelled” mid-string do **not** match.

Resource teardown: `cancel_pending_deliveries_for_resource` **excludes** `RoomDissolve` /
`ChannelClose` (and `myriad:` variants) so dissolve/close fan-out is never self-cancelled.
`delete_room` cancels stale traffic **before** dissolve enqueue.

Signing: prefer **stored** `key_id` except `Move` (old actor base). ensure-on-sign
only when keys are missing; decrypt failures never regenerate.

Host UI: Config → Federation → **Outbound delivery queue** (stats, list, retry/cancel).

## Multi-instance suite

Script: `scripts/dev/federation-multi-instance-suite.sh`

- `wait_delivery_side` treats `dead` as failure **unless** `error_message` matches
  `cancelled:%` (user/API cancels left by prior cases must not poison later waits).
- Case `deploy_retry_dead_skips_user_cancel` seeds one user-cancel dead + one real
  dead and asserts bulk retry-dead keeps the cancel dead.

Lab dual backends: ports 18080/18081, DBs `myriad_fed_a` / `myriad_fed_b`.
Override `PORT_*` / `DB_*` / `SCRATCH_DIR` when multiple agents share a host (see script header).

## File transfer amplification budgets (MYR-008)

Chunked federation transfers keep a large per-file product ceiling and use
**admission control** instead of removing the feature:

| Limit | Value | Where |
|-------|-------|--------|
| `MAX_FILE_SIZE` | **20 GiB** | product ceiling (unchanged) |
| `MAX_CONCURRENT_TRANSFERS` | **64** open (`pending`/`in-progress`) | process-wide |
| `MAX_CONCURRENT_TRANSFERS_PER_USER` | **16** | local initiator/owner |
| `MAX_CONCURRENT_TRANSFER_BYTES` | **64 GiB** sum of open `file_size` | rejects absurd concurrent full-size pile-ups |
| `MAX_IN_FLIGHT_CHUNK_BYTES` | **128 MiB** decoded | concurrent chunk handlers |
| `TRANSFER_CHUNK_SIZE` | **4 MiB** raw | single chunk |

Constants live in `backend/src/federation/limits.rs`; admission in
`file_transfer.rs` (`admit_new_transfer`, `admit_chunk_bytes`). Over budget
returns **503** (retry later), not a permanent ban.

## Related docs / fixtures

- Permission lockstep: `docs/development/tapp/fixtures/action_permissions.json`,
  `host_route_permissions.json` (includes `keys/rotate`, `delivery/cancel-pending`)
- Tapp host attribution: `federation_host_attribution` on federation router
- PR lineage: `fix/federation-ensure-keys` (ensure, Accept harden, rotate API, cancel races)
- MCP transport residual (no OS sandbox yet): `backend/src/services/agent/mcp/mod.rs`
