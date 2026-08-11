# TAPP Capability Expansion — Completion Audit

Audit time: 2026-08-12 02:19

Status: partial — safe completed slices are committed; anonymous guest speech and the final complete example remain explicitly open.

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
| Guest metered TTS/ASR | Ticket 06 Progress records blockers: speech middleware requires authenticated Claims; only 45/min rate limit exists; no persistent finite speech quota ledger | Not achieved; safely blocked |
| Mood Radio complete example covering every newly accessible SDK action | Ticket 07 remains blocked by 06 | Not achieved |
| Shared permission/catalog consistency | Frontend permission-map tests and CLI generated-contract tests pass | Verified |
| CLI check/permissions/pack compatibility | `npm test` in `tools/tapp-cli`: 47/47 pass | Verified |
| Headless boundaries | capabilityProfiles tests: 3/3 pass; CLI headless checks included in 47/47 | Verified |
| Backend role matrix | permission-service tests: 14/14 pass | Verified |
| Frontend behavior and type integrity | final targeted tests: 12/12 pass; Astro typecheck: 0 errors (19 existing hints) | Verified |
| Repository state | `git status --short` empty at audit time | Verified |
| Reviewable commits | setup/spec/tickets plus one commit per completed or blocked unit; no push | Verified |

## Final verification evidence

- Backend role/config tests: 15 tests passed (14 permission-service + 1 config payload).
- Frontend policy/contract tests: 12 tests passed.
- Frontend typecheck: 0 errors, 0 warnings, 19 existing hints.
- CLI: 47 tests passed.
- Working tree: clean.

## Missing or weakly covered requirements

1. Guest speech is intentionally not implemented. The current authenticated middleware and absence of a finite persistent speech quota make enabling it non-compliant with the agreed safety contract.
2. The complete Mood Radio example and final four-role executable matrix remain blocked by guest speech.
3. No browser-level manual QA was performed; current evidence is backend, frontend policy/contract, typecheck, and CLI test coverage.

## Stop condition

Do not mark the whole feature complete. Safe slices 01–05 are resolved. Ticket 06 remains open with an anonymous attribution/quota blocker. Ticket 07 remains blocked by Ticket 06. The next defensible implementation input is approval and design for a generalized persistent anonymous speech quota ledger.
