# Tapp host permission fixtures

Machine-readable facts for **speech / phantasi / federation** host-proxied
capabilities. `host_route_permissions.json` is the authority for backend host
attribution; `action_permissions.json` is a consistency fixture — sandbox action
permissions are owned by `shared/tapp_sandbox_contract.json`. Tests fail on drift.

| File | What it captures | Consumers |
| ---- | ---------------- | --------- |
| `host_route_permissions.json` | HTTP method + Axum matched path → permission | Backend `host_attribution` (loaded at runtime via `include_str!`) + Rust unit tests |
| `action_permissions.json` | Host-proxied sandbox action → permission (consistency fixture; authority is `shared/tapp_sandbox_contract.json`) | Frontend `PERMISSION_MAP` consistency test + Rust permission-string checks |

## How to update

1. **Edit the fixture(s) first** (add/remove/rename host routes, or mirror a
   host-proxied action change you are making in `shared/tapp_sandbox_contract.json`).
2. Update `TappPermission` in `crates/tapp-contract/src/permission.rs` and frontend
   `PERMISSION_LEVELS` / types if you introduced a new permission string.
3. Update `shared/tapp_sandbox_contract.json` (the sandbox action table read by
   the frontend `PERMISSION_MAP` and exported to the Tapp CLI) for action changes,
   then run `npm run sync-contract` in `tools/tapp-cli`.
4. Host route maps are loaded from `host_route_permissions.json` — no parallel
   hand-written match arms to edit for speech/phantasi/federation.
5. Run:

   ```bash
   # from backend/
   cargo test host_attribution

   # from frontend/
   node --experimental-strip-types --test src/tapp/runtime/permissionMapConsistency.test.ts
   ```

## Out of scope

- Non-host-proxied sandbox actions (storage, AI, etc.) live only in
  `shared/tapp_sandbox_contract.json`.
- WebSocket tickets, CSP, quotas.
