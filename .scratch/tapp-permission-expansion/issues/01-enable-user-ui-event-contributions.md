# 01 — Enable ordinary-user UI and event contributions by default

**What to build:** Make declared theme registration, shortcut registration, and event publication work by default for an authenticated ordinary user, while preserving administrator controls, event namespace enforcement, Manifest declaration requirements, and Headless surface restrictions.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] A newly configured installation enables `component:theme`, `shortcut:register`, and `event:publish` for authenticated ordinary users by default while keeping them elevated permissions.
- [ ] An administrator can independently disable each capability, and the next Runtime Grant excludes the disabled permission.
- [ ] Existing installations retain explicitly persisted administrator choices rather than having them overwritten by new defaults.
- [ ] A TAPP receives only permissions declared in its Manifest; default role eligibility never grants undeclared permissions.
- [ ] Event publication remains limited to Manifest-declared topics under the publishing TAPP's own namespace, and impersonated or undeclared topics are rejected.
- [ ] Headless TAPPs remain unable to use theme and shortcut contribution actions even when the Runtime Grant contains eligible permissions.
- [ ] Administrator and guest behavior remains unchanged: administrators retain full access, and guests do not gain these durable contribution paths in this ticket.
- [ ] Administrator settings expose the effective defaults and allow all three controls to be changed without requiring a restart or code change.
- [ ] Shared backend, frontend, host-route fixture, generated contract, generated SDK declaration, and CLI permission-level data remain consistent.
- [ ] Role-based Runtime Grant tests, event-boundary tests, Headless capability-profile tests, and CLI missing-declaration tests pass.
