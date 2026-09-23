# Federation development notes

Operational notes for Myriad’s ActivityPub + MFP stack. Product inventory lives
in the federation-universe scratch; this file is **how the code behaves** for
developers working on ensure-keys, Accept, delivery, and the dual-instance suite.

Production Compose runs federation HTTP, WebSockets and outbound delivery in
`federation-worker`. Proxy routes this domain with
`PROXY_FEDERATION_UPSTREAM=http://federation-worker:1103`. Web owns migrations
and does not register these handlers. Path table:
[PORTS.md](../deployment/PORTS.md). Process boundary:
[RUNTIME_ISOLATION.md](../deployment/RUNTIME_ISOLATION.md).

## Federation gate (egress location)

Federation is not an admin toggle. On boot the process probes the machine's
**egress location** (public IP geolocation, bypassing the outbound proxy) and
holds the result in memory only. It is never written to `configurations`.

- **Closed** only on a positive reading that this server is in a blocked
  region (currently ISO `CN`; HK / MO / TW are distinct codes). All
  `federation:*` grants then evaluate false for every role, including admins.
- **Fail-open** if the geolocation provider is unreachable, rate-limited, or
  unparseable. The unresolved probe window also reads as enabled. Outbound
  delivery is the exception: it waits for the reading before the first drain
  so a blocked server never emits an Activity on a merely-pending gate.
- When the gate is closed, `federation-worker` exits with status 0 (Compose
  `restart: on-failure` does not loop an idle worker). Web and
  `persona-worker` keep running and do not register the federation router.
  On a combined / native process the gate middleware answers **404** for
  federation paths. On official Compose the public path is
  proxy → `federation-worker`; a missing upstream is **502**, not web 404.

Entry points: `backend/src/services/federation_gate.rs`,
`backend/src/services/server_location.rs`, worker exit in
`backend/src/federation/worker.rs`. Diagnostics expose `federation_gate`
without credentials.

## Inbox auth hardening (MYR-022 / MYR-023)

| Concern | Behaviour |
|---------|-----------|
| **MYR-022 actor cache poison** | Signature verification resolves the remote Actor via trusted DB cache **or** an **ephemeral** HTTP fetch. Unauthenticated remote documents are **not** written to `federation_remote_actors` until the HTTP Signature verifies. Failed signatures drop the ephemeral material. |
| **MYR-023 replay** | HTTP `Date` skew alone (`HTTP_DATE_MAX_SKEW` = 5m) is weak. After signature and trust checks, the inbox claims a durable receipt keyed by signer, normalized activity id, and local inbox scope. The exact signed body digest is bound to that identity. A committed success answers `202 Accepted` without re-running handlers; reuse of the same identity with different bytes is `409 Conflict`. |

Entry points: `fetch_remote_actor_for_verify` / `persist_verified_remote_actor` in
`actor.rs`; `claim_receipt` / `finish_receipt` in `inbox/receipt.rs`; wired in
`inbox/receive.rs`.

### Public inbox resource boundary (MYR-002 / #306)

The actor identity needed for true HTTP Signature verification is inside the
Activity JSON. The pre-parse gate can therefore validate only the signature
header shape, `Date` freshness, and the signed raw-body `Digest`; it does not
authenticate the actor. A request that passes this cheap gate is still parsed
before actor resolution and signature verification. The mitigation bounds that
unavoidable unauthenticated work; it does not eliminate it.

Default-profile bounds are:

| Limit | Value | Rejection |
| --- | ---: | --- |
| message payload | 4 MiB | 413; use chunked transfer for larger data |
| public inbox body | 8 MiB | 413 |
| concurrent raw inbox bodies | 32 MiB | 429; peer should retry |
| complete parsed inbox trees | 4 requests | 429; peer should retry |
| JSON nesting | 32 levels | 400 |
| JSON structural items | 65,536 | 400 |
| one encoded JSON string | 6 MiB | 400 |

`INBOX_PARSE_CONCURRENCY` names the admission mechanism, not a short CPU-only
critical section. The permit is deliberately held while the parsed
`serde_json::Value` remains alive: through actor fetch, signature/trust checks,
receipt transaction, and handler execution. Releasing it immediately after
`serde_json::from_slice` would allow more complete trees to coexist and defeat
the memory bound. Consequently, four slow remote lookups or DB/handler paths can
make a fifth delivery receive 429 even when raw-byte budget remains. Operators
should monitor sustained inbox 429s; peers must treat them as retryable.

The compile-time and unit checks establish deterministic size/concurrency
bounds. They are not a 1 GiB-host stress test or production load proof.

## Durable inbox transaction boundary

`federation_inbox_receipts` is coordination state, not an ActivityPub content
projection. It intentionally remains separate from `federation_activities`:

- not every accepted inbox activity creates a `federation_activities` row;
- rejected activities and same-id/different-body conflicts must never be
  represented as accepted activity content;
- the receipt must be claimed before handler writes, while the activity table
  is an output of only some handlers;
- adding receipt lifecycle columns and indexes to the populated activity table
  would be a heavier migration and would couple unrelated retention policies.

Migration `012_federation_inbox_receipts` therefore creates one isolated table
with a composite primary key and no secondary indexes or data rewrite. The
in-transaction `processing` row is never committed: accepted/rejected outcome,
handler DB effects, and transactional delivery-queue writes commit together.
A retryable error rolls the entire transaction back. PostgreSQL's unique-key
conflict serialization prevents concurrent execution; no lease/reclaim state is
needed.

The inbox scope is part of the key. A peer may deliver the same public activity
to several actor inboxes when it does not use `sharedInbox`; processing Alice's
copy must not suppress Bob's. Shared-inbox delivery uses its own scope and the
existing per-user/activity uniqueness constraints keep fan-out idempotent.

### Inbound Move

Verification fetches the old and new actor documents over remote HTTP, so it
runs as a preflight after signature verification and before the receipt is
claimed: `actor` = signer = `object`, a distinct `target`, the old document's
`movedTo` = target and the new document's `alsoKnownAs` containing the old id.
The new actor is resolved through the actor cache in the same preflight. Only
the follow rewrite (row locks + savepoint merge), the Move activity record and
the receipt completion then run in one DB transaction; any failure rolls all
of it back. Unreachable documents are a retryable `503` with no receipt; link
mismatches are a permanent `400`.

### Inbound FileChunk

A same-version peer's `myriad:FileChunk` runs on the receipt transaction. The
per-transfer advisory lock serializes it with every other writer of that
transfer; the chunk is written at the offset owned by `chunks_completed`
(a half-written tail is truncated and rewritten, a complete earlier attempt is
byte-compared), the file and any new directory entries are synced, and only
then does the progress update commit together with the receipt. A rolled-back
receipt leaves the synced bytes in place for the retry to verify; the database
never records progress ahead of durable file data. The file I/O runs in its own
task holding a per-transfer in-process lock, so a cancelled request cannot let
a retry write the same `.part` while the orphaned write is still running
(separate replicas on shared storage are not covered by that lock — uncertain).
Live-UI progress is broadcast only after the receipt commits.

Missing transfer metadata or an out-of-order chunk is a retryable `503`; a
wrong sender, a closed transfer or a malformed chunk is a permanent `4xx`; a
chunk already committed under another activity id is accepted without effect.

## Keys: ensure vs rotate

| Path | When | Effect |
|------|------|--------|
| **ensure** (`ensure_user_federation_keys`) | Outbound sign / identity cold start | Generate **only** if PEM missing/empty. Never silent-rotate live keys. |
| **rotate** `POST /api/federation/keys/rotate` | Explicit user/admin action | Requires body `{ "confirm": true }`. New RSA keypair, store `key_id`, best-effort `Update(Person)` fan-out. |

Gate helper: `rotation_confirm_accepted` in `backend/src/federation/actor.rs`
(boolean `true` only — strings/numbers rejected).

Host surface (stacked with this branch family):

- REST: `federationApi.rotateKeys({ confirm: true })`
- Bridge: `federation.rotateKeys` → `federation:post`
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

Remote health per domain (`federation_instances.failure_count` + `failing_since`)
tracks one *unreachable* streak. Each outbound attempt falls into three buckets:

| Bucket | Examples | Effect on the streak |
| --- | --- | --- |
| unreachable | DNS failure, connection refused, timeout, HTTP 5xx/408/429 | increments; stamps `failing_since` if unset |
| rejected | permanent 4xx, `not_found` / `not_member` bodies | **neutral** — the peer answered, so it is alive |
| local | URL/SSRF policy, HTTP-client prep, key/signing/DB failures, trust-policy rejection, lease reclaim, a remote human not replying | never reaches domain health |

Any successful HTTP acknowledgement clears both `failure_count` and `failing_since`.

Revocation needs **both** halves of the gate: ≥ 20 unreachable attempts **and** a
streak ≥ 7 days old. A count alone is not a signal — the worker drains 20 rows per
15 s tick, so one fan-out can burn any count while a peer merely restarts; and
`max_attempts` (12) means no single queue row can reach the count by itself. A
neutral rejection cannot advance the gate, so a live peer declining one object can
never tear down its whole domain.

When it fires, one transaction removes active follows and room memberships, closes
DM channels, cancels open DM transfers plus transfers in rooms hosted by the failed
domain, and cancels the domain's **unfinished** (`pending` / `delivering`) deliveries
with a `cancelled:` reason derived from the thresholds. Rows that already reached a
terminal state are untouched: a dead-letter keeps its real cause and stays eligible
for `retry-dead`. Every owner of a cancelled row gets a `federation.domain_revoked`
notification, because the bulk sweep bypasses the per-row dead-letter path. Locally
hosted rooms keep other members; for a room hosted by the failed domain, local
memberships are removed. Cached actors, room/channel messages, activities, and
completed deliveries remain as local history. This is a liveness teardown, not a
trust block; revocation resets the streak so a later relationship starts fresh.
`idx_delivery_queue_target_domain` (`LOWER(target_domain), status`) keeps the sweep
off a full scan of retained queue history.

Resource teardown: `cancel_pending_deliveries_for_resource` **excludes** `RoomDissolve` /
`ChannelClose` (and `myriad:` variants) so dissolve/close fan-out is never self-cancelled.
`delete_room` cancels stale traffic **before** dissolve enqueue.

Signing: prefer **stored** `key_id` except `Move` (old actor base). ensure-on-sign
only when keys are missing; decrypt failures never regenerate.

Host UI: Config → Federation → **Outbound delivery queue** (stats, list, retry/cancel).

## Multi-instance suite

Script: `scripts/extra/federation-suite.sh`

Read-only FK orphan report (does not mutate): `scripts/extra/federation-fk-orphan-report.sql`

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
