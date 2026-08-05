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

#### Current Cargo exceptions

| RUSTSEC | Package | Reason |
| --- | --- | --- |
| `RUSTSEC-2023-0071` | `rsa` | Medium timing side channel with no fixed release; track `rsa` / `jsonwebtoken`. |
| `RUSTSEC-2026-0235` | `rkyv` | Inactive optional dependency recorded through SeaORM's `rust_decimal` defaults. CI proves `rkyv` is absent from the resolved feature graph; revisit when SeaORM / `rust_decimal` can use `rkyv` >= 0.8.17 or remove it. |

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

# This must print no reverse-dependency tree. CI enforces the same boundary
# before accepting the RUSTSEC-2026-0235 exception.
cargo tree -i rkyv --target all --locked

# Frontend
(cd frontend && pnpm audit --audit-level high)
```
