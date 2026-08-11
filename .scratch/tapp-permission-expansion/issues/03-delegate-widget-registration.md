# 03 — Delegate Widget registration to ordinary users

**What to build:** Allow an authenticated ordinary user to install and operate a TAPP's Manifest-declared Widgets by moving `widget:register` from administrator-only privileged access to configurable elevated access, enabled by default for new installations.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] `widget:register` is classified as elevated consistently across backend permission policy, frontend permission metadata, generated contracts, SDK declarations, and CLI output.
- [ ] A dedicated ordinary-user Widget delegation setting exists, is enabled by default for new installations, and is visible and editable in administrator settings.
- [ ] Existing installations retain an explicitly persisted setting and are not silently changed during upgrade.
- [ ] When enabled, an authenticated ordinary user's Runtime Grant includes declared `widget:register`; when disabled, it does not.
- [ ] Guest Runtime Grants never include `widget:register`.
- [ ] A TAPP without `widget:register` in its Manifest cannot register or manage Widgets even when the user role is eligible.
- [ ] A TAPP cannot register a Widget absent from its validated Manifest definition.
- [ ] An eligible ordinary-user TAPP can complete register, list, update, invalidate, instance-settings update, render, and unregister flows.
- [ ] Headless TAPPs remain unable to manage Widgets.
- [ ] Platform write/register, Agent registration, TAPP management, Brew management, federation trust management, and report writing remain privileged.
- [ ] Role-based Runtime Grant, Manifest validation, Widget lifecycle, Headless profile, contract consistency, and CLI permission tests pass.
