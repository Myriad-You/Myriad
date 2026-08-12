# 01 — Widget stash lifecycle (stash instead of destroy; uninstall destroys)

**What to build:** Introduce a third widget state — `stashed` — between `active` and `destroyed`. Widget sandboxes are stashed (iframe detached into an off-screen pool, bridge alive, grant and session retained, `lifecycle:pause` emitted) on non-destructive teardown triggers; only uninstall and security boundaries destroy.

**Blocked by:** None — can start immediately.

**Status:** open

- [ ] `WidgetStashPool` exists (`frontend/src/tapp/runtime/WidgetStashPool.ts`): keyed by `tappId|widgetId|codeFingerprint`, capacity cap (default 8) with LRU eviction, idle timeout (default 5 min) destroying entries, `stash/take/purge/clear`.
- [ ] `TappBridge.stash()` / `resume()` exist and are distinct from `destroy()`: stash detaches event routing and sets `surfaceActive=false` while keeping grant/session/handlers; resume re-`attachSource()`, restores routing, re-emits ready as needed; `destroy()` semantics unchanged (still terminal).
- [ ] `TappWidgetSandbox` cleanup delegates to stash instead of `bridge.destroy()`; remount path tries `pool.take(key)` and resumes before building a fresh iframe.
- [ ] `TappWidget.tsx` viewport-exit stashes instead of unmounting the sandbox (no skeleton-swap rebuild; iframe moves into the pool container).
- [ ] `TappRuntime.uninstallTapp` purges the pool for that tapp; `destroyAll` clears the whole pool.
- [ ] `codeFingerprint` change and `subjectEpoch` change destroy the old entry (never reuse stale code or cross-subject state) and build fresh.
- [ ] Stashed media pauses; resume re-emits `lifecycle:resume`; long-stash token expiry reuses the `grantSeed` re-mint path.
- [ ] Unit tests: pool cap/LRU/purge/clear/timeout; bridge stash→resume keeps session token and event routing; destroy-after-stash is terminal; subjectEpoch change forces destroy.
- [ ] Behavior test: scroll away/back and navigate away/back preserve widget in-session state without rebuild flash; uninstall while stashed destroys the entry.
- [ ] Frontend typecheck and targeted vitest suites pass; `git diff --check` clean.

## Comments

Design context in `.scratch/tapp-lifecycle-secrets-signing/spec.md` §3.1. The existing `lifecycle:pause/resume` (Page sandbox, multi-window minimize) stays as-is; stash is a stronger detachment that keeps the bridge object alive. P2 option (props push instead of rebuild on `stableWidgetProps` change) is explicitly out of scope for this ticket.
