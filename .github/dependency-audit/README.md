# Dependency audit policy (MYR-027)

CI job **Dependency vulnerability scan** enforces a hard gate with an explicit
allowlist, instead of `continue-on-error` on every audit step.

## Hard fail (blocks merge)

| Ecosystem | Command | Threshold |
| --- | --- | --- |
| Rust (root + updater) | `cargo audit` | Any **unignored vulnerability** |
| Frontend (pnpm) | `pnpm audit --audit-level high` | **High / critical** after overrides + `ignoreGhsas` |

## Soft warn (visibility only)

| Ecosystem | Command | Behavior |
| --- | --- | --- |
| Rust | `cargo audit --deny warnings` | Unmaintained / unsound / yanked — `continue-on-error` |
| Frontend | `pnpm audit` (no level filter) | Full report including moderate/low — `continue-on-error` |

## Allowlists / exceptions

### Cargo (`.cargo/audit.toml`)

Add `RUSTSEC-…` IDs under `[advisories].ignore` **with a comment** explaining:

- severity / why not high-priority
- no fixed upgrade path (or upgrade blocked)
- residual risk and revisit trigger

`cargo-audit` auto-loads `.cargo/audit.toml` from the workspace (and parent
trees when auditing nested packages like `updater/`).

### Frontend (`frontend/package.json`)

1. Prefer **`pnpm.overrides`** to pull patched transitive versions when safe.
2. Prefer **`pnpm.auditConfig.ignoreGhsas`** only when an upgrade is blocked or the
   advisory does not apply (document the reason in the same PR / this file).

#### Current frontend exception

| GHSA | Package | Reason |
| --- | --- | --- |
| `GHSA-qwww-vcr4-c8h2` | `react-router` | RSC-mode CSRF only; app uses classic `react-router-dom` client routing, not unstable RSC APIs. Fix requires `react-router` ≥ 8.3 (major). Revisit when upgrading RR to v8. |

## Local runs

```bash
# Rust (uses .cargo/audit.toml automatically)
cargo audit
(cd updater && cargo audit)

# Frontend
(cd frontend && pnpm audit --audit-level high)
```
