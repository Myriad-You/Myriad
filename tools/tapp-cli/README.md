# Myriad Tapp CLI

Offline project tooling for Myriad Tapps. The backend installer remains the final
authority; this CLI catches common contract problems before upload.

## Commands

```bash
npx --yes --package=@myriad/tapp-cli myriad-tapp init ./my-tapp --type page
npx --yes --package=@myriad/tapp-cli myriad-tapp check ./my-tapp
npx --yes --package=@myriad/tapp-cli myriad-tapp permissions ./my-tapp
npx --yes --package=@myriad/tapp-cli myriad-tapp pack ./my-tapp
```

`init` supports `page`, `widget`, and `both`. `check --json` emits diagnostics for
editor and CI integration. `pack` refuses projects with errors and writes
`dist/{manifest.id}.tapp` by default.

For a checked-in dependency or CI job, pin the package version:

```bash
npm exec --yes --package=@myriad/tapp-cli@0.1.0 -- myriad-tapp check . --json
```

The package exposes `myriad-tapp`, `tapp`, and `tapp-cli` binaries. The last name
matches the unscoped part of `@myriad/tapp-cli`, so npm can infer the executable
for the short form `npx @myriad/tapp-cli` according to its bin resolution rules.
The explicit `--package ... myriad-tapp` form above is preferred in CI because it
also makes the selected command and version obvious.

## Checks

- strict Manifest fields including Widget settings/refresh, AI, events, Agent and Data Exchange;
- declared paths, extensions, missing files, Agent schemas, i18n and asset quotas;
- permission names and permissions inferred from static `Tapp.*` calls;
- `Tapp.api("name")` declarations and HTTP/builtin API permissions;
- literal `Tapp.assets.*("path")` references;
- runtime surface consistency (`hasPage` resources, widgets ↔ `widget:register`);
- headless capability profile: actions denied in background core;
- `.tapp` entry count and package size limits.

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

The committed contract combines the backend Rust Manifest schema and semantic
rules with the runtime permission map and sandbox capability profiles:

```bash
cd tools/tapp-cli
npm run sync-contract
```

Run this command after changing the backend Manifest types, Tapp contract rules,
`frontend/src/tapp/runtime/permissionConfig.ts`, or
`frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts`. Templates and ZIP
packaging remain handwritten; validation consumes `src/generated/contract.json`.

`sync-contract` is a repository-maintainer command. It is run by
`prepublishOnly` before publishing and is not included in the published tarball;
end users only consume the committed generated contract.

The generated contract has two layers:

1. **Structure layer**: `manifest.rs` derives the JSON Schema used for Manifest
   fields, nested objects, required fields and Rust enum values.
2. **Semantic layer**: `contract_rules.rs` exports limits, path/extensions,
   conditional field rules, API/event/Data Exchange relationships and permission
   requirements.

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
