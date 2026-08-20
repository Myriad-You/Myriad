# Notification catalog

Myriad separates three concepts that previously overlapped:

- **Source**: the product surface that emitted the event. This owns the icon.
- **Event key**: the exact user-configurable event, such as success vs failure.
- **Notification type**: the persisted presentation shape used for history compatibility.

User preferences are stored on `users.notification_preferences`. New notifications are filtered
before persistence and SSE delivery. Existing history is not retroactively deleted.

## Source icons

Only eight source icons are required. Individual events reuse their source icon; priority, title,
and color communicate status, so the UI does not need 28 unrelated pictograms.

| Source key   | Product          | Icon source                          |
| ------------ | ---------------- | ------------------------------------ |
| `agent`      | Arael tasks      | `/icons/notifications/arael.webp`     |
| `heartbeat`  | Arael Heartbeat  | `/icons/notifications/heartbeat.webp` |
| `mcp`        | MCP tool servers | `/icons/notifications/mcp.webp`       |
| `brew`       | Brew             | `/icons/notifications/brew.webp`      |
| `tapp`       | Tapp runtime     | `/icons/notifications/tapp.webp`      |
| `updater`    | System updater   | `/icons/notifications/updater.webp`   |
| `federation` | Federation       | `/icons/notifications/aro.webp`       |
| `system`     | Myriad system    | `/icons/notifications/system.webp`    |

The reusable implementation lives in
`frontend/src/components/notifications/NotificationIcons.tsx`.
All eight assets are notification-scoped 512×512 transparent WebP; other
product surfaces keep their existing navigation and module icon implementations.

## Persisted presentation types

These are intentionally broader than event keys so old history remains readable:

| Source | `notification_type` values |
| --- | --- |
| Arael | `task_progress`, `task_completed`, `task_failed`, `task_cancelled`, `agent_clarification` |
| Heartbeat | `heartbeat_result` |
| MCP | `mcp_server_status` |
| Brew | `brew_new_items`, `brew_source_error` |
| Tapp | `tapp_notification` |
| Updater | `updater_status` |
| Federation | `federation_message`, `federation_follow`, `federation_invite` |
| System | `system_info` |

## Configurable events

| Source | Event key | Current producer |
| --- | --- | --- |
| Arael | `agent.task_progress` | Agent run hub progress |
| Arael | `agent.task_completed` | Agent run terminal success |
| Arael | `agent.task_failed` | Agent run terminal failure |
| Arael | `agent.task_cancelled` | User-cancelled run |
| Arael | `agent.clarification` | Run waiting for user input |
| Arael | `agent.life.platform_activity` | High-importance connected-platform activity |
| Heartbeat | `heartbeat.succeeded` | Heartbeat scheduled task success |
| Heartbeat | `heartbeat.failed` | Heartbeat scheduled task failure |
| MCP | `mcp.connected` | MCP server connection/recovery |
| MCP | `mcp.disconnected` | MCP server connection/restart failure |
| Brew | `brew.new_items` | Feed refresh with new items |
| Brew | `brew.source_error` | Feed reaches the consecutive-error threshold |
| System | `platform.sync.failed` | Connected-platform auto-refresh failure |
| Tapp | `tapp.message` | Tapp queued informational notification |
| Tapp | `tapp.warning` | Tapp queued warning |
| Tapp | `tapp.error` | Tapp error or scheduled task failure |
| Updater | `updater.submitted` | Update/rollback job accepted |
| Updater | `updater.running` | Update/rollback job running |
| Updater | `updater.succeeded` | Update/rollback completed |
| Updater | `updater.failed` | Update/rollback failed |
| Updater | `updater.needs_manual` | Recovery requires an operator |
| Updater | `updater.unknown` | Backend could not confirm the final state |
| Federation | `federation.channel_message` | Incoming direct message |
| Federation | `federation.room_message` | Incoming room message |
| Federation | `federation.new_follower` | Incoming follow |
| Federation | `federation.follow_accepted` | Outgoing follow accepted |
| Federation | `federation.channel_invite` | Incoming direct-channel request |
| Federation | `federation.room_invite` | Incoming room invitation |
| Federation | `federation.channel_accepted` | Direct channel accepted |
| System | `system.info` | Reserved for Myriad system information |

`system.info` remains in the catalog for compatibility and future system notices; there is no
current producer. In-page Tapp UI notifications and synchronous request responses remain local and
ephemeral to avoid duplicating the same event in the persistent notification center.
