# Tapp host permission fixtures

Machine-readable source of truth for **speech / brew / federation** host-proxied
capabilities. Comment-only sync across stacks is not enough; tests fail on drift.

| File | What it captures | Consumers |
| ---- | ---------------- | --------- |
| `host_route_permissions.json` | HTTP method + Axum matched path → permission | Backend `host_attribution` (loaded at runtime via `include_str!`) + Rust unit tests |
| `action_permissions.json` | Sandbox bridge action → permission | Frontend `PERMISSION_MAP` consistency test + Rust permission-string checks |

## How to update

1. **Edit the fixture(s) first** (add/remove/rename routes or actions, change
   permission strings).
2. Update `TappPermission` in `crates/tapp-contract/src/permission.rs` and frontend
   `PERMISSION_LEVELS` / types if you introduced a new permission string.
3. Update frontend `permissionConfig.ts` `PERMISSION_MAP` for action changes.
4. Host route maps are loaded from `host_route_permissions.json` — no parallel
   hand-written match arms to edit for speech/brew/federation.
5. Run:

   ```bash
   # from backend/
   cargo test host_attribution

   # from frontend/
   node --experimental-strip-types --test src/tapp/runtime/permissionMapConsistency.test.ts
   ```

## Out of scope

- Non-host-proxied sandbox actions (storage, AI, etc.) still live only in
  `PERMISSION_MAP`.
- WebSocket tickets, CSP, quotas.
