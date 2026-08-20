# Myriad Tapp CLI

Offline project tooling for Myriad Tapps. The backend installer remains the final
authority; this CLI catches common contract problems before upload.

## Requirements

- Node.js 20 or newer;
- access to a Myriad instance when you are ready to install the generated package.

The examples pin `@myriad/tapp-cli` to `0.1.0`. Keep the version explicit in
automation and upgrade it deliberately when a newer release is available.

## For agents

Use a pinned package version and the explicit binary name in automation. `--yes`
accepts npm's temporary-install prompt; `--package` makes the selected package and
binary unambiguous in CI.

```bash
npx --yes --package=@myriad/tapp-cli@0.1.0 myriad-tapp init ./my-tapp --type page
npx --yes --package=@myriad/tapp-cli@0.1.0 myriad-tapp check ./my-tapp --json
npx --yes --package=@myriad/tapp-cli@0.1.0 myriad-tapp pack ./my-tapp --json
```

Treat a non-zero status as failure. When `check --json` returns status `1`, repair
the reported diagnostics and repeat `check`; run `pack --json` only after it
succeeds. `pack` writes `dist/{manifest.id}.tapp` unless `--out` is supplied.

For a checked-in dependency, pin the package in the lockfile and invoke its binary
through `npm exec`:

```bash
npm install --save-dev --save-exact @myriad/tapp-cli@0.1.0
npm exec -- myriad-tapp check . --json
```

Do not infer success from human-readable output. Read the JSON object and process
exit status described in [Automation contract](#automation-contract).

## For users

Install the pinned CLI globally when you want a short interactive command:

```bash
npm install --global @myriad/tapp-cli@0.1.0
```

Create a starter, edit its `manifest.json` and source files, then validate and
package it:

```bash
myriad-tapp init ./my-tapp --type page
cd ./my-tapp
myriad-tapp check .
myriad-tapp permissions .
myriad-tapp pack .
```

`pack` writes `dist/{manifest.id}.tapp` by default. In Myriad, open the Tapp
management page, choose the install action, and upload that file. The CLI does not
currently log in to a Myriad server or upload packages itself.

A Playground export uses the same default layer paths (`core.js`, `page/index.js`,
`widget/index.js`, `templates/{widgetId}-{size}.html`). Unzip the `.tapp` and run
`myriad-tapp check` before editing extra files.

For Page 3D, prefer `runtimeModules: ["three"]` (category `game` or `developer`)
so the host injects pinned Three r170 + `GLTFLoader`. You can still bundle a guest
IIFE under `page/` and `require` it from the page entry. Put textures and `.glb`
in `manifest.assets` and load them through `Tapp.assets`. The sandbox cannot fetch
a CDN copy of the engine; `check` warns when page HTML or JS points at `unpkg` /
`jsdelivr` / `cdnjs` / `esm.sh`. See
[GRAPHICS.md](../../docs/development/tapp/GRAPHICS.md).

Online games use `Tapp.game` plus `game:session` and federation permissions.
`create()` is private by default; preview cannot exercise these APIs.

## Commands

Run `myriad-tapp <command> --help` for the command-specific interface and exit
status. Each command accepts at most one optional directory; it defaults to the
current directory.

| Command | Purpose | Allowed options |
| --- | --- | --- |
| `init [directory]` | Create a project. | `--type <page\|widget\|both>`, `--id <id>`, `--name <name>`, `--author <name>`, `--force`, `--json` |
| `check [directory]` | Validate a project. | `--json` |
| `permissions [directory]` | List declared and inferred permissions. | `--json` |
| `pack [directory]` | Validate and write a `.tapp` archive. | `-o, --out <path>`, `--json` |

`init --force` allows initialization in a non-empty directory. It overwrites only
the starter files managed by the CLI; it does not delete unrelated files or clean
existing package resource directories.

Unsupported options are usage errors. `--version` returns the installed CLI
version; `--help` returns global help when supplied without a command.

## Automation contract

| Exit status | Meaning | JSON-mode output |
| --- | --- | --- |
| `0` | Requested operation succeeded. | Command result. |
| `1` | Validation, packaging, or execution failed. | Inspection report for validation failures; error envelope for execution failures. |
| `2` | Command-line usage is invalid. | Error envelope. |

When executing a subcommand with `--json`, stdout is exactly one JSON object and
stderr is empty. Successful `init` returns `{ result, report }`; `check` and
`permissions` return an inspection report; successful `pack` returns
`{ outputPath, sizeBytes, entries, diagnostics }`. Validation failures return their
inspection report so callers can read diagnostics.
Command-line errors return this envelope:

```json
{
  "error": { "code": "usage-error", "message": "..." },
  "exitCode": 2
}
```

Execution errors use the same shape with `code: "execution-error"` and
`exitCode: 1`. The package also exposes `tapp` and `tapp-cli`; use `myriad-tapp`
in automation to keep the selected binary explicit.

## Checks

- strict Manifest fields including Widget settings/refresh, AI, events, Agent and Data Exchange;
- declared paths, extensions, missing files, Agent schemas, i18n and asset quotas;
- symlink containment: declared resources and auto-included `i18n/`, `page/`,
  `schemas/` directories must be regular files inside the project root;
- permission names and permissions inferred from static `Tapp.*` calls;
- `Tapp.api("name")` declarations, the fixed HTTP method allow-list, and
  HTTP/builtin API permissions;
- write-only `credentials` declarations and bindings: limits, declared/bound keys,
  fixed absolute HTTPS origins, fixed non-routing headers, and duplicate headers;
- literal `Tapp.assets.*("path")` references;
- runtime surface consistency (`page` layer resources, widgets ↔ `widget:register`);
- headless capability profile: actions denied in background core;
- `manifest.json` size, per-resource byte limits, and `.tapp` entry count and
  package size limits.

Dynamic property access and computed API names cannot be proven statically. They are
reported as warnings or left to the backend/runtime permission checks.

Generated editor assets under `src/generated/`:

- `contract.json` — full offline contract (schema, limits, permissions, capabilities)
- `manifest.schema.json` — JSON Schema for `manifest.json`
- `capability-profiles.json` — Page/Widget/headless profile data
- `tapp-sdk.d.ts` — sandbox `window.Tapp` types for editor tooling

`init` copies `tapp-sdk.d.ts` into `types/` and adds a `jsconfig.json` + triple-slash
reference so editors understand `Tapp.*` without a full npm SDK package. Those local
editor files are not packed into `.tapp`.

## Generated contract

The committed contract combines the Rust Manifest schema and semantic rules
from `crates/tapp-contract` (shared with backend install validation) with the
runtime permission map and sandbox capability profiles:

```bash
cd tools/tapp-cli
npm run sync-contract
```

Run this command after changing `crates/tapp-contract` (Manifest types or
contract rules), `frontend/src/tapp/runtime/permissionConfig.ts`, or
`frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts`. Templates and ZIP
packaging remain handwritten; validation consumes `src/generated/contract.json`.

`sync-contract` is a repository-maintainer command. It is run by
`prepublishOnly` before publishing and is not included in the published tarball;
end users only consume the committed generated contract.

The generated contract has two layers:

1. **Structure layer**: `crates/tapp-contract/src/manifest.rs` derives the JSON
   Schema used for Manifest fields, nested objects, required fields and Rust
   enum values.
2. **Semantic layer**: `crates/tapp-contract/src/contract_rules.rs` exports
   limits, path/extensions, conditional field rules, API/event/Data Exchange
   relationships and permission requirements. Backend install validation reuses
   the same constants, so `check` results cannot drift from the installer.

`init` starter files and `pack` ZIP mechanics are intentionally handwritten;
everything else in the CLI reads the generated contract rather than copying
backend values.

## Publishing

From this directory, a release check runs the contract exporter and the complete
test suite before npm accepts the publish:

```bash
npm run pack:check
npm publish
```

Publishing requires an authenticated npm account with access to the `@myriad`
scope. The package declares public scoped access in `publishConfig`.
