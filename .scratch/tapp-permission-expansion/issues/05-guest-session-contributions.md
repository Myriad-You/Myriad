# 05 — Provide guest session-local themes and shortcuts

**What to build:** Let a guest-facing TAPP use the existing theme and shortcut SDK methods for the active sandbox session only, with automatic teardown and no durable authenticated contribution records.

**Blocked by:** None — can start immediately.

**Status:** resolved

- [x] Explicit guest policy controls exist for `component:theme` and `shortcut:register`, with secure defaults and effective values exposed to administrators.
- [x] When enabled and declared, a guest Runtime Grant permits only the session-safe theme and shortcut actions required by the existing SDK flows.
- [x] Guest theme registration/list/unregister works for the active session without writing an authenticated component record.
- [x] Guest shortcut register/list/unregister works for the active session without writing an authenticated shortcut record.
- [x] Registered guest contributions are scoped to the originating TAPP and sandbox session.
- [x] Destroying, replacing, logging out of, or closing the sandbox automatically removes every guest theme and shortcut contribution from that session.
- [x] A later guest or authenticated session cannot observe or activate a previous guest session's contributions.
- [x] Undeclared permissions remain denied, and Headless TAPPs remain unable to register themes or shortcuts.
- [x] Authenticated-user and administrator persistent contribution paths remain backward compatible.
- [x] Runtime Grant, bridge lifecycle, cross-session isolation, teardown, Headless profile, contract consistency, and CLI declaration tests pass.
- [x] Both contributions use explicit session registries; no durable identity boundary was bypassed.

## Progress

Guest themes and shortcuts use per-bridge in-memory registries. Theme presets are sanitized by the existing `useTappThemes` whitelist consumer, shortcut bindings remain host-local, and bridge teardown clears both registries and unbinds shortcuts. Authenticated and administrator paths continue using the existing persistent APIs.
