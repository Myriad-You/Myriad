# Federation domain migration (ActivityPub Move)

This document describes how Myriad migrates a federated instance from one domain
(base URL) to another using the **ActivityPub `Move`** activity — not env-only
rewrites and not soft-skip verification.

> **Current inbound status:** emitting local Move activities remains supported,
> but receiving a remote `Move` returns retryable `503` while its remote-document
> preflight is being separated from the durable inbox receipt transaction. The
> recovery contract is documented in
> [Federation development notes](../development/FEDERATION.md#temporarily-unavailable-handlers).

## What this implements

| Letter | Scope | What happens |
|--------|--------|----------------|
| **B** | Actor document fields | Local actors expose `alsoKnownAs` (new base) and/or `movedTo` (old base) from `federation_domain_aliases` so peers can verify Move. |
| **C** | Send Move | Admin job emits one `Move` per local user: `actor` = `object` = old actor URL, `target` = new. Signed, fan-out to **followers** via the delivery queue. |
| **D** | Receive Move | Target contract: inbox / shared-inbox accept `Move` only after **fail-closed** checks (HTTP Signature + old `movedTo` + new `alsoKnownAs`) and atomically re-point local follows. Temporarily `503` until the preflight/receipt split above is implemented. |
| **E** | Local data rewrite | After Move enqueue, rewrite **this instance’s** stored absolute URLs `old_base` → `new_base` on a **whitelist** of federation columns. **Never** third-party domains. |
| **G** | Shared keys | Same local user keeps the **same RSA keypair** (same `public_key_pem`). `keyId` host moves to the new domain `#main-key`. **No** fresh keypair per domain. |

## Actor documents (B)

| Served as | Fields |
|-----------|--------|
| **New** base (configured `base_url` or Host = new) | `id` = new actor URL; `alsoKnownAs` includes old actor URL(s) |
| **Old** base (Host still matches recorded old domain) | `id` = old actor URL; `movedTo` = new actor URL |

Both shapes are required for remote peers to verify inbound Move (D).

## Shared keys (G)

- One row in `federation_keys` per user: PEM material is **not** rotated by domain-move.
- Domain-move **retargets** `key_id` only:  
  `{old}/users/{u}#main-key` → `{new}/users/{u}#main-key`
- Actor documents always advertise the stored `publicKeyPem`. When serving under the old Host during migration, `keyId` path uses the old host so signature verification against the old actor document still works; material is identical.
- Move and post-move activities are signed with the existing user keys (`keys.rs` store). Delivery of Move uses the **old** actor origin for HTTP Signature `keyId` so peers verify against the departing document.

## Local rewrite (E)

Whitelist (prefix match on `old_base` only):

| Table | Columns |
|-------|---------|
| `federation_keys` | `key_id` (usually already handled by G) |
| `federation_remote_actors` | `actor_url`, `inbox_url`, `outbox_url`, `shared_inbox_url`, `public_key_id`, `avatar_url`, `domain` (only when actor was ours) |
| `federation_room_members` | `actor_url`, `invited_by` |
| `federation_rooms` | `owner_actor`, `home_server` |
| `federation_room_messages` | `sender_actor` |
| `federation_channel_messages` | `sender_actor` |
| `federation_delivery_queue` | `target_inbox`, `target_domain` (local targets only) |

`federation_follows` stores `remote_actor_id` FKs, not raw URLs — rows for **our** old actor stubs are updated via `federation_remote_actors.actor_url` rewrite.

Foreign URLs (other hosts) never match the `old_base` prefix and are left untouched.

## Admin job order

`POST /api/admin/federation/domain-move`

1. Validate `old_base_url` / `new_base_url`
2. dry_run: counts only (users, keys, rewrite rows)
3. **B** — store domain alias (`federation_domain_aliases`)
4. **G** — retarget `key_id` (same PEM; `regenerated_keys` always 0)
5. **C** — enqueue Move to followers per local user
6. **E** — local DB rewrite whitelist
7. Return full report: moves enqueued, shared_keys, local_rewrite, per-user rows

### Request

```http
POST /api/admin/federation/domain-move
Authorization: Bearer <admin JWT>
Content-Type: application/json

{
  "old_base_url": "https://old.example",
  "new_base_url": "https://new.example",
  "dry_run": true
}
```

### Response (shape)

```json
{
  "dry_run": false,
  "old_base_url": "https://old.example",
  "new_base_url": "https://new.example",
  "alias_stored": true,
  "total_users": 3,
  "enqueued": 3,
  "failed": 0,
  "shared_keys": {
    "users_with_keys": 3,
    "key_ids_retargeted": 3,
    "users_without_keys": 0,
    "regenerated_keys": 0
  },
  "local_rewrite": {
    "dry_run": false,
    "total_rows": 12,
    "columns": [
      { "table": "federation_room_members", "column": "actor_url", "rows": 4 }
    ],
    "foreign_urls_untouched": true
  },
  "results": [
    {
      "user_id": 1,
      "username": "alice",
      "old_actor": "https://old.example/users/alice",
      "new_actor": "https://new.example/users/alice",
      "status": "enqueued",
      "shared_key": true,
      "activity_id": "https://old.example/activities/…",
      "queued": 12
    }
  ]
}
```

## Ops checklist

1. **Keep the old domain reachable** until Move delivery completes (and a cool-down so late peers can fetch old actors with `movedTo`).
2. Prefer: old domain still live → run domain-move → set configured `base_url` to new (if not already) → keep old Host on this process → confirm peer follows → decommission old domain.
3. Always `dry_run: true` first; inspect counts for users, `key_ids_retargeted`, and rewrite rows.
4. Live run: watch delivery queues until Move rows leave `pending` / `dead`.
5. Confirm actor docs: new Host has `alsoKnownAs`; old Host has `movedTo`.

## Honest limits

| Topic | Status |
|-------|--------|
| Remotes that ignore Move | Not our bug — document for ops; no soft-skip of **our** D verification |
| Room/channel/ring **semantic** re-home on remotes | Not a full remote protocol migrate; **E** only rewrites **local** self-URLs in those tables |
| Updater / `.env` domain rewrite | Separate / out of scope |
| Soft-skip verification on D | Forbidden |
| Inbound Move while receipt-safe preflight is pending | Retryable `503`; do not acknowledge before the follow rewrite and receipt can commit atomically |

## Storage

- `federation_domain_aliases` — old_base → new_base for **B** actor fields (schema check at boot)
- `federation_keys` — shared RSA material (**G**); `key_id` host retargeted on move

## Related code

- `backend/src/federation/move_actor.rs` — B/C/E/G job, verify helpers, follow re-point, rewrite whitelist
- `backend/src/federation/inbox/receive.rs` — `handle_move` and the temporary inbound gate (D)
- `backend/src/federation/actor.rs` — actor documents + shared PEM
- `backend/src/federation/types.rs` — `Actor.also_known_as` / `Actor.moved_to`
- `backend/src/federation/delivery.rs` — Move signing identity (old actor base)
