# 04 — Provide guest session notifications without durable records

**What to build:** Let a guest-facing TAPP show feedback through the existing `Tapp.ui.showNotification` SDK method during the active sandbox session without writing an account-owned notification or broadening guest access to durable Dynamic Content actions.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] A guest Runtime Grant can authorize the declared `ui:notification` capability for `ui.showNotification` on supported visible surfaces.
- [ ] The notification is observably displayed to the active guest session and does not create an account-owned or durable notification record.
- [ ] Guest notification data is bounded and validated using the existing notification option contract.
- [ ] `dynamicContent.set`, `dynamicContent.update`, and `dynamicContent.remove` remain denied to guests even though they share the broad `ui:notification` permission mapping.
- [ ] A TAPP that does not declare `ui:notification` cannot show a guest notification.
- [ ] Headless notification behavior remains governed by the existing surface and background policy rather than being implicitly widened.
- [ ] Authenticated-user and administrator notification behavior remains backward compatible.
- [ ] Session cleanup leaves no guest notification state that can affect a later session.
- [ ] Runtime Grant, bridge-handler, role/action denial, durable-record absence, cleanup, and CLI declaration tests pass.
- [ ] If no session-only host notification seam can be proven safe, implementation stops with concrete evidence and leaves guest notification denied rather than persisting anonymous state.
