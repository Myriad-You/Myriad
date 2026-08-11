# 07 — Ship the Mood Radio capability example and role-matrix audit

**What to build:** Add and validate a complete “Mood Radio” TAPP example that demonstrates every newly accessible existing SDK action, documents what developers, ordinary users, guests, and administrators experience, and proves retained boundaries through one role-matrix audit.

**Blocked by:** 01 — Enable ordinary-user UI and event contributions by default; 02 — Enable ordinary-user speech and scheduling by default; 03 — Delegate Widget registration to ordinary users; 04 — Provide guest session notifications without durable records; 05 — Provide guest session-local themes and shortcuts; 06 — Provide metered speech to guest TAPP sessions.

**Status:** ready-for-agent

- [ ] The example uses the existing `main.js`, Page template, Widget template, Manifest, and `window.Tapp` SDK model; it introduces no framework build or new builtin API identifier.
- [ ] The Manifest declares every permission, event topic, Widget, background requirement, and resource used by the example.
- [ ] The developer flow demonstrates notification, theme register/list/unregister, shortcut register/list/unregister, event publish/subscribe, scheduler register/list/get/enable/disable/trigger/unregister and task callbacks, speech voices/status/TTS/ASR, Widget register/list/update/invalidate/settings/render/unregister, storage, and media integration.
- [ ] The authenticated ordinary-user scenario demonstrates all newly enabled user capabilities when defaults are enabled.
- [ ] The administrator scenario demonstrates independently disabling each elevated capability and observes corresponding Runtime Grant and behavior changes.
- [ ] The guest scenario demonstrates only session notification, session-local theme/shortcut, and metered speech when guest policy enables them.
- [ ] The guest scenario proves federation mutation/message/file, scheduling, Widget registration, AI, arbitrary network, platform management, TAPP management, and trust management remain denied.
- [ ] The Headless scenario proves visible UI, shortcut, component, file, TAPP-management, and Widget-management actions remain unavailable.
- [ ] `tapp check`, `tapp permissions`, and `tapp pack` succeed for the example, while a deliberately undeclared action fixture still fails with a missing-permission diagnostic.
- [ ] A role-matrix integration test verifies administrator, ordinary-user, guest, and Headless outcomes at the Runtime Grant plus executable-action seams.
- [ ] Shared permission catalogs, fixtures, generated contracts, generated SDK declarations, frontend tests, backend tests, and CLI tests all pass from a clean checkout.
- [ ] Documentation states the final supported matrix and explicitly lists AI, network, framework output, direct fetch/WebSocket/Worker, SSR, and arbitrary server code as out of scope.
