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

### Frontend (`frontend/pnpm-workspace.yaml`)

1. Prefer **`overrides`** to pull patched transitive versions when safe.
2. Prefer **`auditConfig.ignoreGhsas`** only when an upgrade is blocked or the
   advisory does not apply (document the reason in the same PR / this file).

#### Current frontend exception

| GHSA | Package | Reason |
| --- | --- | --- |
| `GHSA-qwww-vcr4-c8h2` | `react-router` | RSC-mode CSRF only; app uses classic `react-router-dom` client routing, not unstable RSC APIs. Fix requires `react-router` ≥ 8.3 (major). Revisit when upgrading RR to v8. |
| `GHSA-vfj7-8cjw-p6xm` | `braces` | No patched release as of 2026-10-03. Only Stylelint's development dependency graph contains it (`pnpm why braces --prod` is empty). The vulnerable input is a deeply nested glob pattern, while `package.json` and `.stylelintrc.json` provide fixed repository-owned patterns; CSS contents and runtime user input are not supplied as glob patterns. A contributor who changes those patterns can already execute code through project scripts. Other uses with untrusted patterns remain vulnerable. Remove this exception when braces/Stylelint provides a fix, or if untrusted glob input is introduced. [Advisory](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm). |

## Local runs

```bash
# Rust (uses .cargo/audit.toml automatically)
cargo audit
(cd updater && cargo audit)

# rust_decimal 1.43 dropped the unused rkyv 0.7 dep (RUSTSEC-2026-0235).
# This must stay empty; CI fails if rkyv re-enters the resolved graph.
cargo tree -i rkyv --target all --locked

# Frontend
(cd frontend && pnpm audit --audit-level high)
```
