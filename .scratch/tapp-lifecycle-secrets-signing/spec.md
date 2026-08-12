# Tapp Lifecycle Stash, Host Secrets, and Dynamic Request Signing — Specification

Status: draft (spec + tickets only; no implementation)
Created: 2026-08-12

## 1. Objective

Extend the TAPP runtime along three independent axes without changing the iframe/postMessage/Manifest architecture:

1. **W1 — Widget stash lifecycle**: widget sandboxes are *stashed* (kept alive off-screen) instead of destroyed on the many non-destructive teardown triggers; only a real *uninstall* (plus security boundaries) destroys them.
2. **W2 — Host secret variables**: a TAPP can reference secrets already configured in the Myriad host (e.g. a GitHub PAT) instead of forcing a per-TAPP duplicate configuration. Secrets remain write-only from the host's point of view and never enter the sandbox.
3. **W3 — Dynamic request signing**: write-only credentials can be used by the host to *sign* declared HTTP API requests (HMAC-SHA256 to start), instead of only static header injection.

## 2. Verified current state (evidence)

| Area | Current behavior | Evidence |
| --- | --- | --- |
| Widget lifecycle | Every teardown path destroys: viewport exit unmounts the sandbox (`inViewport ? <TappWidgetSandbox/> : <WidgetSkeleton hold/>`), React unmount/cleanup calls `bridge.destroy()` + iframe removal, dependency changes (codeFingerprint, stableWidgetProps, subjectEpoch, …) rebuild the iframe. `TappPageSandbox` already has `lifecycle:pause/resume` for minimize/hidden, but that pauses rather than detaches and is Page-only. | `frontend/src/components/widgets/TappWidget.tsx` (~line 1035), `frontend/src/tapp/runtime/TappWidgetSandbox.tsx` (cleanup at ~774), `frontend/src/tapp/runtime/TappBridge.ts` (`destroy()` at 433), `useSandboxSubscriptions.ts` |
| Uninstall signal | `TappRuntime.uninstallTapp()` emits `tapp:uninstalled`; backend `DELETE /api/tapps/{tapp_id}` transactionally deletes widgets, `tapp_storage`, tasks, and the install row; files quarantined. | `frontend/src/tapp/runtime/TappRuntime.ts` (413–455), `backend/src/api/tapp_store/uninstall.rs` |
| Credentials | Per-installation, write-only, encrypted (`tapp_storage` `_credentials.*`, `data_key`), manifest-declared, binding fingerprint forces re-authorization on manifest change; injected statically as `prefix + value` header. No host-shared secret concept; no signature modes. | `backend/src/services/tapp_credentials.rs`, `crates/tapp-contract/src/manifest.rs` (`TappApiCredentialBinding`: key/header/prefix) |
| Host config | Dynamic config lives in the `configurations` table; sensitive values are ciphertext, opened via `data_key::open_config_value`. Existing secrets are AI keys only (gemini/openai); GitHub is OAuth client id/secret for login, not a PAT store. | `backend/src/services/config_service.rs`, `backend/src/config.rs` (219–238) |
| Template variables | Declared APIs resolve `{{...}}` templates from an inject context (user.*, params.*, geo.* …) before sending; body is serialized *before* outbound send (comment explicitly keeps "payload hashing/signing aligned with the wire body"). | `backend/src/services/tapp_api_service.rs` (build_inject_context / resolve_template / ~510) |
| Sandbox visibility | `TappBridge.isActive()` is false when destroyed, host-paused, iframe gone, or CSS-hidden — the CSS-hidden branch already tolerates detached/hidden iframes. | `TappBridge.ts` (344–353) |

## 3. Design

### 3.1 W1 — Widget stash lifecycle

**Concept.** A widget instance moves through three states: `active` (mounted, live) → `stashed` (iframe detached into an off-screen pool, bridge alive, grant held, session token retained, `lifecycle:pause` emitted) → resumed back to `active`, or → `destroyed` (all resources released, semantics unchanged from today's `destroy()`).

**Trigger matrix.**

| Event | Today | Target |
| --- | --- | --- |
| Widget exits viewport (`inViewport=false`) | unmount + destroy | stash |
| React unmount (navigation, hidden layout, re-layout) | destroy | stash |
| `stableWidgetProps` change | rebuild (destroy) | P1: keep rebuild; P2: stash + props push via bridge |
| `codeFingerprint` change | rebuild | destroy (old code must not be reused) |
| `subjectEpoch` change (login/logout) | rebuild | destroy (security boundary) |
| `destroyAll()` | destroy all | destroy all, including stash pool |
| `uninstallTapp` | backend cleanup | stash pool purges that tapp + backend cleanup unchanged |
| locale change | `locale:change` emit, bridge kept | unchanged |

**Components.**

- New `frontend/src/tapp/runtime/WidgetStashPool.ts`: keyed by `tappId|widgetId|codeFingerprint`; capacity cap (default 8) with LRU eviction (destroyed on evict); idle timeout (default 5 min, configurable) destroys stashed entries; `purge(tappId)`, `clear()`, `stash(entry)`, `take(key)`.
- `TappBridge.ts`: add `stash()` (detach event routing, `surfaceActive=false`, keep grant/session/handlers) and `resume()` (re-`attachSource()`, restore routing, re-emit ready if needed, `surfaceActive=true`); `destroy()` semantics unchanged. Stash must not run through `destroy()`.
- `TappWidgetSandbox.tsx`: cleanup delegates to a stash callback instead of `bridge.destroy()`; remount path first tries `pool.take(key)` and resumes.
- `TappWidget.tsx`: viewport-exit stashes instead of unmounting the sandbox (keep the sandbox mounted but detach its iframe via the pool, or move the iframe node into the pool container).
- `TappRuntime.ts`: `uninstallTapp` → `pool.purge(tappId)`; `destroyAll` → `pool.clear()`.

**Safety and resources.**

- Stashed iframes live in a hidden container (`display:none`), which throttles timers/rAF; resume re-emits `lifecycle:resume`.
- Media: stash emits a pause signal so audio/video stops; resume restarts only if the sandbox requests it.
- Session token expiry across a long stash: reuse the existing `grantSeed` re-mint path on resume.
- Memory: pool cap + idle timeout bound worst-case footprint; stashed iframes are eligible for browser discarding only when detached (acceptable; resume path re-attaches and re-emits ready).

**Acceptance (W1).**

- Scroll away and back: no rebuild flash, widget state (including in-session data) survives; no `tapp.ready` duplicate side effects.
- Navigate away/back or hide/re-show: same behavior.
- Uninstall while stashed: pool entry destroyed, backend cleanup unchanged.
- `destroyAll()`: every stashed entry destroyed.
- `codeFingerprint` / `subjectEpoch` change: old entry destroyed, fresh instance built.
- Tests: pool unit tests (cap, LRU, purge, clear, timeout), bridge stash/resume unit tests, sandbox behavior test (stash→resume keeps session token and event routing; destroy after stash is terminal).

### 3.2 W2 — Host secret variables

**Concept.** A host-level shared secret store (admin-writable, encrypted) that installed TAPPs reference by declared name. Values are resolved only inside the backend outbound path, never in the template context, never in sandbox JS.

**Storage and administration.**

- Store under the `configurations` table with a new namespace `host_secrets.<name>` (value encrypted via existing `data_key`), consistent with today's sensitive dynamic config.
- Admin-only API: `GET /api/admin/host-secrets` (names, configured flag, updatedAt — never values), `PUT /api/admin/host-secrets/<name>`, `DELETE /api/admin/host-secrets/<name>`.
- Settings UI: new "Host secrets" section; write-only inputs (masked), list of names + timestamps, delete; mirrors the existing write-only credential UX.

**Manifest contract (two reference forms).**

- *Form A — credential binding references a host secret* (static header injection reuses the existing execution path):

```json
{
  "credentials": [
    { "key": "github", "label": "GitHub", "source": "host", "hostKey": "github_pat" }
  ],
  "apis": {
    "repos": {
      "type": "http",
      "endpoint": "https://api.github.com/user/repos",
      "credential": { "key": "github", "header": "Authorization", "prefix": "Bearer " }
    }
  }
}
```

- *Form B — template variable reference* (usable in header values, query, body, URL):

```json
{
  "hostSecrets": ["github_pat"],
  "apis": {
    "repos": {
      "type": "http",
      "endpoint": "https://api.github.com/user/repos",
      "headers": { "Authorization": "Bearer {{secrets.github_pat}}" }
    }
  }
}
```

Rules:

- Referencing a host secret that is not declared in the manifest is a validation error at install time.
- `credential_binding_fingerprint` extends to include the hostKey mapping (binding changes require re-authorization); a *value* change of the host secret does NOT require re-authorization.
- Resolution produces the same sealed `ResolvedApiCredential` shape (Debug redacted) used today; Form B resolution is scoped to template resolution inside `tapp_api_service` only.
- `{{secrets.*}}` is rejected anywhere outside declared API outbound templates (context endpoints, manifest echo, CLI output).

**Security boundaries.**

- Values never enter `/api/tapp/context/*` payloads, never enter the sandbox bridge, never appear in manifest status responses (only `configured`/`updatedAt`).
- Outbound responses keep the existing redaction path (text + parsed JSON).
- `outbound_security` header/URL validation unchanged.
- The "AI and arbitrary network default off" boundaries are untouched: host secrets are explicit user/administrator grants, not a relaxation of default permission policy.
- Audit log line per outbound use: tapp_id, secret name, api name, timestamp.

**Acceptance (W2).**

- Admin can put/get-status/delete host secrets; values never echo back (API or UI).
- A TAPP declaring `hostSecrets`/`source:"host"` resolves and injects the secret on outbound requests; undeclared references fail install validation; missing secret fails with a stable error code.
- Binding fingerprint: changing the manifest binding requires re-authorization; changing the secret value does not.
- Sandbox/context never expose values (regression tests on context payloads and bridge payloads).
- Response redaction covers the host secret.
- CLI schema/validation tests updated; frontend manifest types + permission config UI (host-source badge) updated.

### 3.3 W3 — Dynamic request signing

**Concept.** Extend `TappApiCredentialBinding` with an optional `signature` block so the host signs the request with the write-only credential. `source` (installation | host) is orthogonal: signing works for both per-installation credentials and W2 host secrets.

**Manifest extension.**

```json
"credential": {
  "key": "api_secret",
  "signature": {
    "algorithm": "hmac-sha256",
    "inputs": ["method", "path", "query", "body", "timestamp", "nonce"],
    "output": "header",
    "outputName": "X-Signature",
    "timestampName": "X-Timestamp",
    "nonceName": "X-Nonce"
  }
}
```

- `algorithm`: initially only `hmac-sha256`; the enum lives in `contract_rules` and is extensible.
- `inputs`: fixed enum set — `method`, `path`, `query`, `body`, `timestamp`, `nonce` (subset selection allowed).
- `output`: `header` (default) or `query` (signature goes into `outputName` query parameter; `timestampName`/`nonceName` are then query parameters too).
- `signature` and `prefix` are mutually exclusive (a signature replaces static injection).

**Execution flow (`tapp_api_service`).**

1. Resolve credential (installation encrypted store or W2 host secret) into the sealed `ResolvedApiCredential`.
2. Serialize the body first (existing ordering — the wire bytes are the signed bytes).
3. Build the canonical string: `METHOD\nURL_PATH\nQUERY(sort by key bytes)\nBODY_BYTES\nTIMESTAMP\nNONCE` (query sorted with the same canonicalization approach as `canonicalize_json`).
4. `HMAC-SHA256(secret, canonical)` hex lowercase; timestamp is UTC RFC3339 seconds; nonce is 16 random bytes hex (32 chars) generated host-side.
5. Inject `outputName` (+ timestamp/nonce names) into header or query.
6. Existing response redaction applies (secret never echoed).

**Security.**

- Secret material stays inside the backend execution path (unchanged write-only property).
- Replay protection is the peer's responsibility; the host provides standard timestamp/nonce semantics.
- New `contract_rules` caps: max signature algorithm name length, max inputs count (6), fixed nonce length 32 hex, max output/timestamp/nonce name lengths.
- Template context still never exposes the secret.

**Acceptance (W3).**

- Signing vector test against RFC 4231 test cases (fixed key/message → expected hex).
- Canonical string stability: query key order, body byte identity, method/path case.
- Both `header` and `query` output modes.
- Missing credential / re-authorization / invalid signature declaration error propagation (stable codes).
- Response redaction still hides the secret.
- `cargo check -p myriad-backend`; permission/config/CLI contract tests; frontend typecheck.

## 4. Shared contract synchronization (all three)

- `crates/tapp-contract`: manifest types + JSON schema + `contract_rules` caps + manifest version bump.
- `tools/tapp-cli`: schema generation, install validation, pack compatibility; tests updated.
- Frontend: manifest type mirror (`TappCodeStructure` etc.), permission/config UI (host-source badge, host secrets section), i18n (zh-CN/en-US/ja-JP).
- Docs: manifest reference updated; `docs/tapp/*` or equivalent.

## 5. Sequencing and dependencies

| Phase | Scope | Dependency |
| --- | --- | --- |
| P1 | W1 stash lifecycle (frontend only) | none |
| P2 | W3 dynamic signing (contract + backend execution path) | none (works on installation credentials) |
| P3 | W2 host secrets (storage + admin API + UI + resolution) | none; contract designed jointly with W3 so `source:host` composes with `signature` |

Contract changes for W2 and W3 must be designed together (one manifest bump) even if implemented separately; W1 touches no contract.

## 6. Out of scope

- Changing iframe/postMessage architecture, CSP, bridge protocol, or Entry/SDK method names.
- Relaxing the default-off AI and arbitrary-network boundaries.
- Browser-level manual QA remains outside automated tickets (known audit gap; recommend a manual pass before release).

## 7. Verification commands (per phase)

- Backend: `cargo check -p myriad-backend`, targeted `cargo test` for credentials/signing/context.
- Frontend: `pnpm exec tsc --noEmit`, targeted vitest suites for pool/bridge/sandbox.
- CLI: `npm test` in `tools/tapp-cli` (schema + pack + validation).
- Repo hygiene: `git diff --check`; `git status --short` empty at phase end.
