# Federation domain migration (ActivityPub Move)

This document describes how Myriad migrates a federated instance from one domain
(base URL) to another using the **ActivityPub `Move`** activity — not env-only
rewrites and not silent SQL rewrites of remote actor IDs.

## What this implements

| Direction | What happens |
|-----------|----------------|
| **Send (C)** | Admin API emits one `Move` per local user: `actor` = `object` = old actor URL, `target` = new actor URL. Activities are signed and fan-out to **followers** via the existing delivery queue. |
| **Receive (D)** | Inbox / shared-inbox accept `Move` only after **fail-closed** verification (HTTP Signature + old `movedTo` + new `alsoKnownAs`). Local follow graph rows that pointed at the **old** remote actor are re-pointed to the **new** one (idempotent). |

Actor documents:

- On the **new** base URL: `alsoKnownAs` includes previous actor IDs (from `federation_domain_aliases`).
- On the **old** base URL (Host still matches a recorded old domain): document id stays the old actor URL and includes `movedTo` → new actor URL.

## Ops checklist (required)

1. **Keep the old domain reachable** until Move delivery has completed (and for a cool-down so late peers can still fetch the old actor with `movedTo`). Point DNS / reverse proxy for the old host at this instance while migration runs.
2. Set the instance **configured `base_url`** to the **new** origin (or complete cut-over after enqueueing Moves — see flow below).
3. Call the admin API (dry-run first):

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

4. When the count looks right, call again with `"dry_run": false` (or omit `dry_run`).  
   Response includes a **per-user** report: `enqueued` / `failed`, `activity_id`, `queued` delivery count.
5. Watch delivery: `GET /api/federation/delivery` (as each user) or admin observability until Move rows leave `pending` / `dead`.
6. Only then retire the old domain.

### Recommended order

```
old domain still live
    → POST domain-move (stores alias + emits Move signed as old actors)
    → ensure base_url / frontend_url point at new domain
    → old Host still hits this process (serves movedTo)
    → confirm remote peers updated follows
    → decommission old domain
```

If `base_url` was already switched to the new domain, Move delivery still signs
using the **old** actor origin from the activity body so peers can verify against
the old actor document (which must remain fetchable via the old Host).

## Admin API

`POST /api/admin/federation/domain-move`

| Field | Type | Description |
|-------|------|-------------|
| `old_base_url` | string | Previous origin, no trailing slash |
| `new_base_url` | string | New origin, no trailing slash |
| `dry_run` | bool (optional) | Count only; no alias write, no enqueue |

Success body (shape):

```json
{
  "dry_run": false,
  "old_base_url": "https://old.example",
  "new_base_url": "https://new.example",
  "total_users": 3,
  "enqueued": 3,
  "failed": 0,
  "results": [
    {
      "user_id": 1,
      "username": "alice",
      "old_actor": "https://old.example/users/alice",
      "new_actor": "https://new.example/users/alice",
      "status": "enqueued",
      "activity_id": "https://old.example/activities/…",
      "queued": 12
    }
  ]
}
```

Requires **admin** JWT (`admin_middleware`).

## Receive verification (no soft-skip)

Inbound `Move` is rejected unless **all** of the following hold:

1. Valid **HTTP Signature** on the inbox request (existing path).
2. Activity `actor` matches the signed actor; `object` is the same as `actor` (old id); `target` is present and different.
3. Fresh fetch of the **old** actor document has `movedTo` equal to `target`.
4. Fresh fetch of the **new** actor document has `alsoKnownAs` containing the old id.

There is no “announce and hope” path and no silent SQL rewrite of remote IDs without a verified Move.

## What is migrated by protocol

- **Follow** relationships on receiving instances: after a verified Move, local rows in `federation_follows` that referenced the old remote actor are updated to the new remote actor (or merged if the new follow already exists).

## Honest limits (not in this change)

The following are **not** auto-migrated by Move and must be handled separately (or not at all yet):

| Area | Status |
|------|--------|
| **Rooms** (membership, history, E2E keys) | Not migrated by this PR |
| **Channels** (1:1 sessions) | Not migrated by this PR |
| **Rings** (peer mesh / gossip) | Not migrated by this PR |
| Updater / `.env` domain rewrite | Out of scope |

Room/channel/ring identifiers and remote memberships remain on old actor URLs until a future dedicated migration path exists. Follow-only migration is the ActivityPub-standard core.

## Storage

Table `federation_domain_aliases` (ensured at boot via schema check):

- `old_base_url` (unique)
- `new_base_url`
- `created_at`

Used only to render `alsoKnownAs` / `movedTo` on local actor documents after an admin domain-move.

## Related code

- `backend/src/federation/move_actor.rs` — emit, verify helpers, follow re-point
- `backend/src/federation/inbox.rs` — `handle_move`
- `backend/src/federation/actor.rs` — actor document fields
- `backend/src/federation/types.rs` — `Actor.also_known_as` / `Actor.moved_to`
- `backend/src/federation/delivery.rs` — Move signing identity (old actor base)
