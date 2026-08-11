# TAPP Existing Capability Expansion

Status: ready-for-agent

## Problem Statement

TAPP already exposes a broad sandbox SDK, but several useful, already-implemented capabilities are unavailable to ordinary authenticated users by default, while Widget registration remains administrator-only. As a result, a developer can write a TAPP that uses speech, shortcuts, themes, events, scheduling, and Widgets, yet an ordinary user cannot install and use that application without an administrator manually changing permission settings or installing it as an administrator application.

Guest users face a second usability gap. Session-safe interactions such as an in-app notification, a temporary theme, a temporary shortcut, and metered speech processing are implemented by the platform but withheld because their current host paths assume a durable authenticated subject. This prevents public TAPP experiences from using basic interactive features even when no durable user data needs to be written.

The problem is not a lack of SDK methods or a need for a new frontend framework. It is that the existing role-to-permission policy is more restrictive than the product scenarios the existing sandbox and host APIs can support.

## Solution

Expand access to the existing TAPP SDK without changing the package format, iframe sandbox, postMessage bridge, Manifest permission declarations, or SDK method names.

For ordinary authenticated users, the platform will enable the existing elevated permissions for theme registration, shortcut registration, event publication, scheduling, text-to-speech, and speech-to-text by default. Administrators retain the ability to disable each elevated capability. Widget registration will move from administrator-only privileged access to configurable elevated access for authenticated users and will be enabled by default.

For guests, the platform will provide session-safe forms of notifications, theme registration, shortcut registration, and metered speech. Guest UI contributions must be temporary and removed when the sandbox session ends. Guest notifications must be displayed without creating a durable user notification record. Guest speech must remain subject to server-authoritative anonymous rate limits and quotas. Guests will not receive Widget registration, scheduling, federation write/message/file, AI, arbitrary network, TAPP management, platform management, or trust-management capabilities.

Every TAPP must continue declaring every permission it uses. Expanding a role's eligibility does not implicitly grant undeclared capabilities. The backend Runtime Grant remains authoritative, and Page, Widget, and Headless capability profiles continue to remove actions that are invalid for the active surface.

A complete example TAPP, “Mood Radio,” will demonstrate every newly accessible existing SDK method from developer, ordinary-user, guest, and administrator perspectives. It will not introduce new builtin API identifiers.

## User Stories

1. As a TAPP developer, I want ordinary users to receive declared speech permissions by default, so that I can build voice-enabled applications without requiring administrator setup.
2. As a TAPP developer, I want ordinary users to receive declared shortcut permissions by default, so that I can provide efficient keyboard workflows.
3. As a TAPP developer, I want ordinary users to receive declared event-publication permission by default, so that my TAPP can publish its own declared topics to other TAPPs.
4. As a TAPP developer, I want ordinary users to receive declared theme-registration permission by default, so that my TAPP can contribute a visual theme.
5. As a TAPP developer, I want ordinary users to receive declared scheduler permission by default, so that my TAPP can perform useful authenticated background tasks.
6. As a TAPP developer, I want ordinary users to install declared Widgets, so that my TAPP can provide dashboard surfaces without administrator installation.
7. As a TAPP developer, I want the existing SDK method names and Manifest fields to remain stable, so that existing source code does not require migration.
8. As a TAPP developer, I want `tapp check` to continue identifying missing permission declarations, so that broader role eligibility does not hide Manifest mistakes.
9. As a TAPP developer, I want `tapp permissions` to report the updated permission levels, so that local tooling matches runtime behavior.
10. As a TAPP developer, I want event topics to remain Manifest-declared and namespace constrained, so that default event access does not permit topic impersonation.
11. As a TAPP developer, I want scheduling to preserve existing interval, lifecycle, and server authorization rules, so that broader access remains predictable.
12. As a TAPP developer, I want speech to preserve existing request validation and host attribution, so that broader access does not create a bypass.
13. As a TAPP developer, I want Widget registration to preserve Manifest Widget validation, so that only declared Widgets can be registered.
14. As an ordinary authenticated user, I want a voice-enabled TAPP to work immediately after I approve its Manifest permissions, so that I do not need an administrator to enable speech manually.
15. As an ordinary authenticated user, I want TAPP shortcuts to work immediately after installation, so that application commands feel native.
16. As an ordinary authenticated user, I want a TAPP to publish its declared events, so that installed TAPPs can coordinate through the existing event system.
17. As an ordinary authenticated user, I want a TAPP theme to be available after installation, so that I can use the appearance supplied by the application.
18. As an ordinary authenticated user, I want a TAPP's declared scheduled task to run, so that reminders, synchronization, and recurring jobs work without administrator intervention.
19. As an ordinary authenticated user, I want to install a TAPP with Widgets, so that I can personalize my dashboard.
20. As an ordinary authenticated user, I want AI and arbitrary outbound networking to remain disabled unless an administrator explicitly enables them, so that cost and data-exfiltration boundaries do not change silently.
21. As an ordinary authenticated user, I want system-management permissions to remain unavailable, so that an installed TAPP cannot manage platforms, trust, other TAPPs, or administrator components.
22. As a guest, I want an open TAPP to show an in-session notification, so that I receive feedback without creating an account.
23. As a guest, I want a TAPP to apply a temporary theme for its current session, so that public experiences can be visually complete without durable registration.
24. As a guest, I want a TAPP to register a temporary shortcut for its current session, so that I can use keyboard controls while the TAPP is open.
25. As a guest, I want text-to-speech and speech-to-text when anonymous quota is available, so that public voice experiences are possible without an account.
26. As a guest, I want temporary contributions removed when the TAPP session ends, so that one public session cannot affect later users.
27. As a guest, I want a clear quota or unavailable-service error when speech cannot run, so that failures are understandable and do not appear as permission bugs.
28. As a guest, I want federation access to remain public-read-only, so that an anonymous TAPP cannot publish, message, or transfer files on my behalf.
29. As a guest, I want scheduling and Widget registration to remain unavailable, so that anonymous sessions cannot create durable background work or dashboard state.
30. As an administrator, I want each ordinary-user elevated capability to remain individually configurable, so that I can tighten a deployment without changing code.
31. As an administrator, I want Widget registration to have its own ordinary-user delegation control, so that I can disable user-installed Widgets independently.
32. As an administrator, I want guest session-safe capabilities to have explicit policy controls where they consume host resources, so that public deployments can manage cost and abuse.
33. As an administrator, I want AI and network defaults to remain off, so that this release does not broaden high-cost or data-egress permissions.
34. As an administrator, I want Runtime Grants to reflect the actual role, configuration, and authenticated-subject requirements, so that displayed permissions match executable routes.
35. As an administrator, I want guest notifications and guest contributions to avoid durable user records, so that anonymous actions do not pollute account-owned data.
36. As an administrator, I want guest speech to use server-authoritative anonymous identity, rate limits, and quotas, so that clients cannot reset limits locally.
37. As an administrator, I want Headless TAPPs to retain their denied-action profile, so that permission expansion does not let background code open visible UI or manage Widgets.
38. As a maintainer, I want frontend, backend, host-route fixtures, CLI contracts, and generated SDK declarations to agree, so that a permission change cannot drift between layers.
39. As a maintainer, I want role-based Runtime Grant tests to cover administrator, authenticated user, guest, and Headless behavior, so that user-visible eligibility is verified at the highest existing seam.
40. As a maintainer, I want a complete Mood Radio fixture to exercise every newly accessible action, so that future regressions can be demonstrated and tested from a real application scenario.

## Implementation Decisions

- The existing TAPP architecture remains unchanged: `.tapp` package, Manifest, Page/Widget/Headless surfaces, iframe sandbox, CSP, postMessage bridge, host handlers, Runtime Grant, and CLI contract generation.
- No new permission strings are introduced. Existing permissions and SDK actions are reused.
- Permission declaration and role eligibility remain separate. A role may be eligible for a permission, but a TAPP receives it only when its Manifest requests it and installation/runtime policy grants it.
- For authenticated ordinary users, the existing elevated permissions `component:theme`, `shortcut:register`, `event:publish`, `scheduler:register`, `speech:tts`, and `speech:asr` remain elevated and become enabled by default through dynamic configuration. They do not become unconditional basic permissions.
- `widget:register` moves from privileged to elevated. A dedicated ordinary-user delegation setting is added and enabled by default. Guests remain ineligible.
- AI permissions and `network:fetch` remain elevated and disabled by default for ordinary users and guests.
- Platform write/register, Agent component registration, TAPP management, Brew management, federation trust management, and report writing remain privileged and administrator-only.
- Guest federation permissions remain public-read-only. Federation write, message, and file capabilities continue to be filtered from guest Runtime Grants.
- Guest scheduling and Widget registration remain denied because they create durable or background state.
- Guest `ui:notification` is session-safe: the host displays feedback to the active session without writing an account-owned notification record. Guest access to `ui:notification` does not make durable Dynamic Content mutations available; role/action enforcement must continue denying those actions to guests.
- Guest theme and shortcut contributions are session-local. They must not be persisted through authenticated host routes and must be automatically removed on sandbox teardown.
- Guest TTS and ASR are enabled only through host-attributed anonymous requests with server-authoritative quota/rate-limit enforcement. Failure modes distinguish permission denial, exhausted quota, and unavailable speech service.
- Existing event namespace rules remain: a TAPP publishes only topics under its own `tapp.{id}.` prefix and subscribes only to allowed TAPP/system topics declared in its Manifest.
- Existing scheduler validation, task ownership, minimum interval, completion, and runtime authorization remain authoritative.
- Existing Widget Manifest validation remains authoritative. A TAPP cannot register an undeclared Widget merely because the role is eligible for `widget:register`.
- Page/Widget/Headless capability profiles remain authoritative after Runtime Grant creation. In particular, Headless continues to deny visible UI, shortcuts, component registration, file download, TAPP management, and Widget management actions.
- Dynamic configuration and the administrator settings API/UI expose the updated defaults and the new ordinary-user Widget delegation setting. Existing configuration values must be respected on upgrade rather than overwritten.
- The shared permission catalog is regenerated after source-of-truth changes. Backend permission parsing, frontend levels, host-route/action fixtures, CLI contract, and generated SDK declarations must remain synchronized.
- The Mood Radio example uses only existing SDK surfaces: notifications, theme registration, shortcut register/list/unregister, event publish/subscribe, scheduler register/list/get/enable/disable/trigger/unregister and task callbacks, speech voices/status/TTS/ASR, Widget register/list/update/unregister and rendering, storage, and media. It does not rely on proposed builtin identifiers that do not exist today.

## Testing Decisions

- The primary test seam is role-based Runtime Grant filtering. Tests supply a role, dynamic configuration, and requested Manifest permissions, then assert the resulting grant. This verifies user-visible eligibility rather than individual constants.
- Runtime Grant tests cover administrator, authenticated ordinary user, guest, and Headless scenarios, including negative assertions for undeclared permissions and retained system-level restrictions.
- Guest session-safe behavior is tested through the host bridge at the highest existing handler seam: actions succeed during the session, produce no durable user record, and are cleaned up on teardown.
- Guest notification tests verify that notification feedback is visible but durable Dynamic Content actions remain denied.
- Guest speech tests use the existing host-attribution and rate-limit/quota seams. Tests verify success within limits and distinct errors for exhausted quota, missing service configuration, and denied permissions.
- Widget tests verify the complete path from Manifest declaration and role eligibility through Runtime Grant to register/list/update/unregister behavior. Undeclared Widgets and guest registration remain rejected.
- Scheduler tests verify the complete path from default ordinary-user eligibility to task registration and execution while preserving minimum intervals, ownership, disablement, and Headless behavior.
- Shared contract tests verify that backend permissions, frontend levels, action mappings, host-route fixtures, generated contract data, and generated SDK declarations agree.
- CLI tests verify that existing source analysis still requires Manifest declarations and reports the updated levels without changing `.tapp` package layout.
- The Mood Radio example is validated with the normal CLI check path and a role-matrix integration test. Tests assert externally observable capability results, not implementation-specific helper calls.
- Prior art includes the existing permission-service role tests, host-attribution fixture tests, frontend permission-map consistency tests, capability-profile tests, Manifest validation tests, and CLI project permission tests.

## Out of Scope

- Astro, React, Vue, Svelte, or other framework support.
- Accepting arbitrary framework `dist/` output or changing the TAPP package layout.
- A new ESM Entry lifecycle or VS Code-style extension host.
- New builtin API identifiers such as `storage:app.read`, `federation:post`, `media:current`, or `ui:notify`.
- Direct sandbox `fetch`, WebSocket, Worker, Service Worker, nested iframe, or CSP relaxation.
- Arbitrary server-side code, SSR, Node.js backends, or developer-provided server processes.
- Making AI or `network:fetch` available by default.
- Granting guests federation mutations, messages, file transfer, scheduling, Widget registration, or durable component/shortcut state.
- Replacing the existing permission taxonomy or splitting all SDK actions into new permission strings.
- Changing existing storage quotas, archive limits, asset limits, or API declaration limits.

## Further Notes

- The current permission model has four levels and already supports configurable elevated delegation; this feature expands policy rather than replacing the model. Source: https://github.com/Myriad-You/Myriad/blob/preview/backend/src/services/permission_service.rs
- Existing SDK actions and action-to-permission mappings remain the implementation surface. Source: https://github.com/Myriad-You/Myriad/blob/preview/frontend/src/tapp/runtime/permissionConfig.ts
- Headless denied actions remain unchanged. Source: https://github.com/Myriad-You/Myriad/blob/preview/frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts
- CLI source analysis and generated contract validation remain the developer-facing compatibility boundary. Source: https://github.com/Myriad-You/Myriad/blob/preview/tools/tapp-cli/src/project.mjs
- Guest behavior that cannot be made session-safe without weakening durable identity boundaries must stop with evidence and remain denied rather than silently bypassing host authorization.
