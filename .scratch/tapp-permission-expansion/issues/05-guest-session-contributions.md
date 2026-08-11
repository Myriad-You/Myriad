# 05 — Provide guest session-local themes and shortcuts

**What to build:** Let a guest-facing TAPP use the existing theme and shortcut SDK methods for the active sandbox session only, with automatic teardown and no durable authenticated contribution records.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] Explicit guest policy controls exist for `component:theme` and `shortcut:register`, with secure defaults and effective values exposed to administrators.
- [ ] When enabled and declared, a guest Runtime Grant permits only the session-safe theme and shortcut actions required by the existing SDK flows.
- [ ] Guest theme registration/list/unregister works for the active session without writing an authenticated component record.
- [ ] Guest shortcut register/list/unregister works for the active session without writing an authenticated shortcut record.
- [ ] Registered guest contributions are scoped to the originating TAPP and sandbox session.
- [ ] Destroying, replacing, logging out of, or closing the sandbox automatically removes every guest theme and shortcut contribution from that session.
- [ ] A later guest or authenticated session cannot observe or activate a previous guest session's contributions.
- [ ] Undeclared permissions remain denied, and Headless TAPPs remain unable to register themes or shortcuts.
- [ ] Authenticated-user and administrator persistent contribution paths remain backward compatible.
- [ ] Runtime Grant, bridge lifecycle, cross-session isolation, teardown, Headless profile, contract consistency, and CLI declaration tests pass.
- [ ] If either contribution cannot be made session-local without weakening durable identity checks, that contribution remains denied and the blocker is recorded with evidence.
