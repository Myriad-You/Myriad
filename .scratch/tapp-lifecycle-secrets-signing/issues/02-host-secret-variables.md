# 02 — Host secret variables (TAPP reuses host-configured secrets)

**What to build:** A host-level encrypted secret store (admin-writable) that installed TAPPs reference by declared name, resolved only inside the backend outbound path. A TAPP needing e.g. a GitHub PAT can use the host's existing PAT instead of a duplicate per-TAPP configuration.

**Blocked by:** None (contract designed jointly with ticket 03; implement after or in parallel with the shared manifest bump).

**Status:** open

- [ ] Storage: `host_secrets.<name>` namespace in the `configurations` table, encrypted via `data_key` (consistent with sensitive dynamic config).
- [ ] Admin-only API: `GET /api/admin/host-secrets` (names/configured/updatedAt only, never values), `PUT`, `DELETE`; admin middleware enforced.
- [ ] Settings UI: "Host secrets" section with write-only masked inputs, name list + timestamps, delete; mirrors the existing credential UX; i18n zh-CN/en-US/ja-JP.
- [ ] Manifest Form A: `credentials[].source: "host"` + `hostKey`; resolves through the existing static header injection path.
- [ ] Manifest Form B: top-level `hostSecrets` declaration + `{{secrets.<name>}}` template variable usable only in declared API headers/query/body/URL; undeclared references fail install validation.
- [ ] `credential_binding_fingerprint` includes the hostKey mapping (binding change → re-authorization required); host secret *value* change does not force re-authorization.
- [ ] Resolution yields the sealed `ResolvedApiCredential` shape (Debug redacted); `{{secrets.*}}` rejected outside declared API outbound templates (context endpoints, manifest echo, CLI output).
- [ ] Security: values never enter `/api/tapp/context/*` payloads or bridge payloads; outbound response redaction (text + parsed JSON) covers host secrets; `outbound_security` validation unchanged; AI/network default-off boundaries untouched.
- [ ] Audit log per outbound use: tapp_id, secret name, api name, timestamp.
- [ ] Tests: resolve + inject; undeclared-reference install rejection; missing secret stable error code; fingerprint re-authorization vs value-change no-re-auth; context/bridge payloads never contain values; redaction; admin-only write enforcement; CLI schema/validation; frontend typecheck.

## Comments

Design context in `.scratch/tapp-lifecycle-secrets-signing/spec.md` §3.2. Form A (credential source) and Form B (template variable) share the same sealed resolution type so ticket 03's `signature` block composes with `source: "host"`.
