# Federation development notes

Operational notes for Myriad’s ActivityPub + MFP stack. Product inventory lives
in the federation-universe scratch; this file is **how the code behaves** for
developers working on ensure-keys, Accept, delivery, and the dual-instance suite.

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
2. Unique id + **same host + username-compatible** path drift (`/@bob` vs `/users/bob`)
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

Classifier: `is_user_cancelled_delivery_error` — exact `cancelled: by user` (case-insensitive)
or any `cancelled:` prefix (room/channel teardown). Peer errors that merely mention
“cancelled” mid-string do **not** match.

Signing: prefer **stored** `key_id` except `Move` (old actor base). ensure-on-sign
only when keys are missing; decrypt failures never regenerate.

Host UI: Config → Federation → **Outbound delivery queue** (stats, list, retry/cancel).

## Multi-instance suite

Script: `scripts/dev/federation-multi-instance-suite.sh`

- `wait_delivery_side` treats `dead` as failure **unless** `error_message` matches
  `cancelled:%` (user/API cancels left by prior cases must not poison later waits).
- Case `deploy_retry_dead_skips_user_cancel` seeds one user-cancel dead + one real
  dead and asserts bulk retry-dead keeps the cancel dead.

Lab dual backends: ports 18080/18081, DBs `myriad_fed_a` / `myriad_fed_b` (see script header).

## Related docs / fixtures

- Permission lockstep: `docs/development/tapp/fixtures/action_permissions.json`,
  `host_route_permissions.json` (includes `keys/rotate`, `delivery/cancel-pending`)
- Tapp host attribution: `federation_host_attribution` on federation router
- PR lineage: `fix/federation-ensure-keys` (ensure, Accept harden, rotate API, cancel races)
