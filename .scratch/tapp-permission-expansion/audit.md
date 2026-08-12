# TAPP Capability Expansion — Completion Audit

Audit time: 2026-08-12 14:29

Status: partial — Tickets 01–06 are resolved; the final Mood Radio example and complete role matrix remain open in Ticket 07.

## Objective restatement

Deliver a local specification and agent-ready vertical tickets for expanding the existing TAPP permission surface without adopting frontend frameworks or changing the iframe/postMessage/Manifest architecture. Implement as many independently reviewable slices as can be safely completed and verified, while retaining AI, arbitrary network, system-management, guest durability, and Headless boundaries.

## Prompt-to-artifact checklist

| Requirement | Artifact / evidence | Result |
| --- | --- | --- |
| Keep the existing TAPP architecture | Spec `Out of Scope`; no package, CSP, iframe, bridge protocol, Entry, or SDK method-name change | Verified |
| Use local Markdown tracker | `docs/agents/issue-tracker.md`; `.scratch/tapp-permission-expansion/` | Verified |
| Produce a PRD/spec | `.scratch/tapp-permission-expansion/spec.md`, 7 required sections and 40 user stories | Verified |
| Produce vertical tickets with blockers | `issues/01` through `issues/07`; Ticket 07 blocks on 01–06 | Verified |
| Ordinary users: theme, shortcuts, event publish default on | Ticket 01 resolved; commit `4b22b0d9`; Runtime Grant role test | Verified |
| Ordinary users: TTS, ASR, scheduler default on | Ticket 02 resolved; commit `4f2b2990`; Runtime Grant, speech-route, scheduler tests | Verified |
| Keep AI and network default off | Permission-service `test_user_high_cost_elevated_permissions_remain_disabled_by_default` | Verified |
| Align administrator form defaults | Commit `78eccfd4`; frontend typecheck 0 errors | Verified |
| Delegate Widget registration to users, not guests | Ticket 03 resolved; commit `c682dc36`; role, Manifest, lifecycle, frontend, CLI tests | Verified |
| Keep Widget elevated, Manifest-declared, Headless-denied | Backend/frontend/generated contract + Widget lifecycle and capability-profile tests | Verified |
| Guest session-only notification, no durable record | Ticket 04 resolved; commit `1be7ca21`; policy test proves session delivery; authenticated roles remain durable | Verified |
| Deny guest Dynamic Content mutation sharing notification permission | `canMutateDynamicContent` policy + set/update/remove handler guards and tests | Verified |
| Guest session shortcut register/list/unregister + teardown | Commit `585626ce`; per-bridge `GuestShortcutSession`, teardown clear, host unbind, behavior test | Verified |
| Guest temporary theme | Commit `29d78c41`; per-bridge session theme registry, whitelist consumption through `useTappThemes`, teardown cleanup, behavior test | Verified |
| Guest metered TTS/ASR | Ticket 06 resolved; optional-auth + signed guest subject, Runtime Grant attribution, session/IP/site persistent daily ledger buckets, guest HMAC-IP rate limit, input/error contract | Verified in commit `29485b2f`; browser QA not performed |
| Mood Radio complete example covering every newly accessible SDK action | Ticket 07 remains blocked by 06 | Not achieved |
| Shared permission/catalog consistency | Frontend permission-map tests and CLI generated-contract tests pass | Verified |
| CLI check/permissions/pack compatibility | `npm test` in `tools/tapp-cli`: 47/47 pass | Verified |
| Headless boundaries | capabilityProfiles tests: 3/3 pass; CLI headless checks included in 47/47 | Verified |
| Backend role matrix | permission-service tests: 15/15 pass, including independent guest TTS/ASR and no AI/network spillover | Verified |
| Frontend behavior and type integrity | final targeted tests: 12/12 pass; Astro typecheck: 0 errors (19 existing hints) | Verified |
| Repository state | `git status --short` empty at audit time | Verified |
| Reviewable commits | setup/spec/tickets plus one commit per completed or blocked unit; no push | Verified |

## Final verification evidence

- Backend role/config tests: permission-service 15/15; host-attribution 19/19; speech quota 3/3; rate-limit 7/7; config payload 1/1.
- Frontend typecheck: 0 errors.
- CLI: 47 tests passed.
- `cargo check -p myriad-backend` and `git diff --check` passed.
- Browser-level manual QA: not performed.
- Ticket 06 implementation commit: `29485b2f`.
- Working tree: documentation update pending; code commit is clean.

## Missing or weakly covered requirements

1. The complete Mood Radio example and final four-role executable matrix remain open in Ticket 07.
2. No browser-level manual QA was performed; current evidence is backend, frontend typecheck, quota/rate-limit, permission, attribution, config, and CLI test coverage.

## Stop condition

Do not mark the whole feature complete. Tickets 01–06 are resolved. Ticket 07 remains open because the Mood Radio example and final complete role matrix have not been built or browser-verified. No push was performed.
