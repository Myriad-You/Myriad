# Build Instructions

This document provides detailed build and compilation instructions for the Myriad project.

## Prerequisites

### Required Tools
- **Rust**: 1.94 or later ([install](https://rustup.rs/))
- **Node.js**: 24.x LTS ([install](https://nodejs.org/))
- **PostgreSQL**: 18 recommended (Compose default); see release `min_pg_version` for the compatibility floor ([install](https://www.postgresql.org/download/))
- **Git**: Latest version

### Optional Tools
- **Docker**: For containerized development
- **sea-orm-cli**: For database migrations
  ```bash
  cargo install sea-orm-cli
  ```

## Backend Build

Myriad uses a **Cargo workspace** at the repo root for packages that share path
dependencies (see [Cargo Workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html)):

| Member | Crate name |
|--------|------------|
| `backend/` | `myriad-backend` |
| `backend/migrations/` | `migration` |
| `crates/myriad-error/` | `myriad-error` (shared `AppError` + redact) |
| `crates/myriad-data-key/` | `myriad-data-key` (config/federation AES-GCM key) |
| `crates/myriad-merope/` | `myriad-merope` (Merope domain rules and contracts) |
| `crates/myriad-outbound/` | `myriad-outbound` (SSRF-safe HTTP egress) |
| `crates/myriad-image-proxy/` | `myriad-image-proxy` (hotlink image proxy helpers) |
| `crates/myriad-json-schema/` | `myriad-json-schema` (JSON schema helpers) |
| `crates/myriad-platform-utils/` | `myriad-platform-utils` (platform sync helpers) |
| `crates/myriad-module-visibility/` | `myriad-module-visibility` (page visibility prefs) |
| `crates/myriad-process-info/` | `myriad-process-info` (memory/uptime/version) |
| `crates/myriad-prompt-security/` | `myriad-prompt-security` (prompt injection heuristics) |
| `crates/myriad-psn-auth/` | `myriad-psn-auth` (PSN NPSSO→token) |
| `crates/myriad-tapp-registry/` | `myriad-tapp-registry` (runtime registry/mailbox) |
| `crates/tapp-contract/` | `myriad-tapp-contract` |
| `crates/myriad-agent-rules/` | `myriad-agent-rules` (pure Agent caps and projections) |
| `crates/myriad-tapp-rules/` | `myriad-tapp-rules` (HMAC, transform, package plan, feed) |
| `tools/tapp-contract-export/` | `myriad-tapp-contract-export` (prints `export_tapp_contract()`) |

Shared artifacts: root `Cargo.lock` and root `target/`.  
`proxy/` and `updater/` are listed in `workspace.exclude` — independent
packages with their own lock + `target`. `tools/tapp-contract-export/` is a
workspace member.

From any directory under the workspace, Cargo still finds the root. Preferred
commands from the repo root:

```bash
cargo build -p myriad-backend
cargo run -p myriad-backend
cargo test -p myriad-backend
cargo check -p myriad-backend --all-targets
```

`cd backend && cargo run` continues to work (package in cwd is selected).

### Development Build

```powershell
# From repo root (preferred)
cargo fetch
cargo build -p myriad-backend
cargo run -p myriad-backend

# Or from backend/ (same workspace)
cd backend
cargo run

# Binary path is always the workspace target/
#   Unix:    ./target/debug/myriad-backend
#   Windows: .\target\debug\myriad-backend.exe
```

### Release Build

```powershell
# From repo root
cargo build -p myriad-backend --release

# Binary: target/release/myriad-backend[.exe]
```

### Build Options

**Debug Build:**
- Fast compilation
- Includes debug symbols
- No optimizations
- Larger binary size
- Good for development

**Release Build:**
- Slower compilation
- Optimized for speed
- Smaller binary (with strip)
- Link-time optimization (LTO)
- Best for production

### Common Build Issues

**Issue: OpenSSL not found**
```powershell
# Windows: Install OpenSSL via vcpkg or use rustls feature
cargo build --no-default-features --features rustls
```

**Issue: Out of memory during compilation**
```powershell
# Reduce parallel compilation
cargo build -j 2
```

**Issue: Linker errors**
```powershell
# Ensure Visual Studio C++ Build Tools are installed
# Or install via: https://visualstudio.microsoft.com/downloads/
```

## Frontend Build

### Development Build

```powershell
cd frontend

# Install dependencies
pnpm install

# Start development server (with hot reload)
pnpm run dev

# The server will be available at http://localhost:1102
```

### Production Build

```powershell
cd frontend

# Build static site
pnpm run build

# Preview the build
pnpm run preview

# Output will be in: frontend/dist/
```

### Build Configuration

Edit `astro.config.mjs` to customize:

```javascript
export default defineConfig({
  output: 'static',  // or 'server' for SSR
  build: {
    assets: 'assets',
    format: 'directory',
  },
  // ... more options
});
```

### Common Build Issues

**Issue: Node Sass errors**
```powershell
# Clear node_modules and reinstall
rm -r node_modules
pnpm install
```

**Issue: TypeScript errors**
```powershell
# Check types without building
pnpm run typecheck
```

**Issue: Memory issues during build**
```powershell
# Increase Node.js memory limit
$env:NODE_OPTIONS="--max-old-space-size=4096"
pnpm run build
```

## Full Stack Build

Build the frontend and backend separately:

```bash
(cd frontend && pnpm install --frozen-lockfile && pnpm run build)
(cd backend && cargo build --release)
```

This creates:

1. `frontend/dist/` for the frontend static build.
2. `target/release/myriad-backend` for the backend binary (workspace root `target/`).

The default production deployment does not run these artifacts directly. It uses
versioned Docker images through `docker-compose.yml` and `scripts/extra/deploy.sh`.

## Database Setup

Schema is owned by SeaORM migrations under `backend/migrations/` (no standalone `database/schema.sql`).

```bash
# Dev DB via Compose
docker compose -f docker-compose.dev.yml up -d postgres

# Migrations run on backend startup. Manual:
cargo run -p migration
# or: cd backend && sea-orm-cli migrate up
```

## Docker Build

### Build Images

Local verification of a Dockerfile:

```bash
docker build -f docker/Dockerfile.backend -t myriad-backend .
docker build -f docker/Dockerfile.frontend -t myriad-frontend .
docker build -f proxy/Dockerfile -t myriad-proxy ./proxy
docker build -f updater/Dockerfile -t myriad-updater ./updater
```

Production releases should normally be built by GitHub Actions `release.yml`
after pushing a `v*` tag. That workflow builds backend/frontend/proxy/updater
images, generates `release.json`, signs it, and publishes a GitHub Release.

Dev images (`docker-publish.yml`, commit title contains `-p`) and releases use
**native multi-arch** builders — `ubuntu-latest` for `linux/amd64` and
`ubuntu-24.04-arm` for `linux/arm64` — then merge digests into a manifest list.
QEMU is not used. Frontend static assets are built once on amd64 and reused by
both runtime images. Rust image builds pass `CARGO_PROFILE=ci-release` (thin
LTO); local `cargo build --release` / default Docker builds keep full LTO.

`proxy` / `updater` ship on an **independent cadence**: only built when their
trees changed, or when forced via commit title `-full` / dispatch `force_infra`.
Unchanged infra is omitted from a release (not retagged to the app version).

Dev packaging commit-title flags: `-p` (package) · `-full` (package + force infra).

### Multi-stage Build Details

**Backend Dockerfile:**
- Stage 1: Build Rust binary (BuildKit cargo registry/git/target cache mounts)
- Stage 2: Minimal runtime image with Debian slim

**Frontend Dockerfile:**
- Stage 1: Build static site with Node.js (`builder` / `assets` / `export`)
- Stage 2: Serve with lightweight Node.js (`runtime`; CI may inject prebuilt `assets`)

### Docker Build Options

```bash
# Build without cache
docker build --no-cache -f docker/Dockerfile.backend -t myriad-backend .

# Push a locally tagged image
docker push your-registry/myriad-backend:tag
```

## CI/CD Considerations

| Workflow | Trigger | What it builds |
|----------|---------|----------------|
| `ci.yml` | PR / push | Tests & lint (amd64 only, no images) |
| `docker-publish.yml` | push with `-p` in title, or `workflow_dispatch` | Dev multi-arch images |
| `release.yml` | `v*` tag | Release multi-arch images + `release.json` |

Image packaging matrix: **component × platform**, each on a native runner, then
`docker buildx imagetools create` publishes the shared tags / version tag.

```bash
# Faster image profile (used by CI Docker builds)
docker build -f docker/Dockerfile.backend \
  --build-arg CARGO_PROFILE=ci-release \
  -t myriad-backend .

# Default full-LTO release profile
docker build -f docker/Dockerfile.backend -t myriad-backend .
```

## Troubleshooting

```bash
cargo clean
(cd frontend && rm -rf node_modules dist .astro)
```

部署见 [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md)。
