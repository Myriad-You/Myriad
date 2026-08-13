# Tapp host permission fixtures

Machine-readable source of truth for **speech / brew / federation** host-proxied
capabilities and the **media** action-domain split. Comment-only sync across
stacks is not enough; tests fail on drift.

| File | What it captures | Consumers |
| ---- | ---------------- | --------- |
| `host_route_permissions.json` | HTTP method + Axum matched path → permission | Backend `host_attribution` (loaded at runtime via `include_str!`) + Rust unit tests |
| `action_permissions.json` | Sandbox bridge action → permission (speech / brew / federation) | Frontend `PERMISSION_MAP` consistency test + Rust permission-string checks |
| `media_action_permissions.json` | media 域写 action → 最窄权限（control 子 action + 高层 bridge action） | Frontend `MEDIA_ACTION_PERMISSIONS` / `PERMISSION_MAP` consistency test + backend `media_control_permission` table-driven tests |

## How to update

1. **Edit the fixture(s) first** (add/remove/rename routes or actions, change
   permission strings).
2. Update backend `TappPermission` (`permission_service.rs`) and frontend
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

- Non-host-proxied sandbox actions outside the media split (storage, AI, etc.)
  still live only in `PERMISSION_MAP`.
- WebSocket tickets, CSP, quotas.
