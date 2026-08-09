# Updater Security Baseline

**Status:** Complete enough for **single-tenant self-hosted Myriad** (current
`preview` topology). Further hardening is **event-driven only** — not a standing
roadmap item.

This document is the durable “done state” for updater security so operators and
maintainers are not relying on chat history.

Related:

- [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md) — topology and env
- [UPDATER_QUICKSTART.md](../UPDATER_QUICKSTART.md) — day-to-day update ops
- [updater-spec.md](../updater-spec.md) §15 — design-level trust boundary
- `bash scripts/docker/deploy.sh doctor` — read-only topology checks

---

## What is in the baseline

| Area | What shipped |
| --- | --- |
| **Topology** | Business / admin / guard nets; updater off business L2; frontend/postgres cannot reach updater |
| **docker-guard** | Only Guard mounts `docker.sock`; its operation/image/bind policy is compiled into the Guard TCB; updater-only on guard-net |
| **Guard identity** | Production Guard is selected by a host-owned `docker-guard.env` outside the deployment root and must use the independently verified `repo@sha256` identity |
| **Token hop** | `UPDATE_TOKEN` in updater + gateway only — **not** in backend or Guard env |
| **Gateway secret** | `UPDATER_GATEWAY_SECRET` (≥32) on backend ↔ updater-gateway; admin-net alone is not enough |
| **Cosign dual-key** | Default `COSIGN_VERIFY=strict`; `off` requires `UPDATER_ALLOW_INSECURE_COSIGN=true` (or alias) |
| **Audit** | `state/audit.log` (fsync, rotate); actor header from admin JWT when proxied |
| **Doctor** | `deploy.sh doctor` / `deploy.ps1 doctor` topology + secrets + cosign; optional `--host` scan |
| **TCB upgrade** | Guard cannot self-update and updater cannot select its image; a host operator must verify release identity/digest and recreate the TCB |
| **Hygiene** | Secret redaction, stricter rate limits on mutative admin routes, `confirm_risk` for high-risk flags |
| **Rescue path** | `PROXY_ALLOW_DIRECT_UPDATER=false` by default; direct `/_updater/*` is temporary only |
| **Backend** | Non-root image user; soft-doctor after `up` / `upgrade` |

---

## Operator red lines

Must follow on every production self-hosted install:

1. **Create the host-owned Guard policy before starting or upgrading.** Copy
   `docker-guard.env.example` to `/etc/myriad/docker-guard.env` (Windows:
   `%ProgramData%\Myriad\docker-guard.env`), replace the placeholder with the
   exact updater image `repo@sha256` from independently verified signed release
   metadata, and restrict it to the host administrator (`0600` or `0640` on Unix).
   To use another path, set `MYRIAD_GUARD_ENV_FILE` in the host shell, not `.env`.
2. **Existing installs / topology drift:** remove legacy Guard entries
   (`UPDATE_TOKEN`, `DOCKER_GUARD_ALLOWED_IMAGES`, self-update URL/env-file
   settings), then run one host-level `bash scripts/docker/deploy.sh upgrade`.
   The script supplies both `.env` and the host-owned Guard policy to Compose.
3. **After deploy:** run `bash scripts/docker/deploy.sh doctor` and fix every
   **FAIL** before trusting the stack.
4. **Protect secrets:** `UPDATE_TOKEN` and `UPDATER_GATEWAY_SECRET` stay out of the
   frontend, tickets, chat, and CI logs. Prefer deploy-generated values; rotate
   if leaked.
5. **Keep** `PROXY_ALLOW_DIRECT_UPDATER=false` except temporary rescue; turn it
   off again when done.
6. **Keep** `COSIGN_VERIFY=strict` unless you intentionally accept dual-key
   insecure mode (`off` + allow key).
7. **Do not publish** updater (`1101`), updater-gateway (`1104`), or docker-guard
   (`2375`) ports on the host. Only proxy’s `HTTP_PORT` is public.
8. **Optional:** `bash scripts/docker/deploy.sh doctor --host` for a non-fatal
   privileged / unexpected `docker.sock` scan after major host changes.

---

## What we deliberately accept

These are **by design** for single-tenant self-host, not open bugs:

- An **admin with update rights** can change the running stack (images, tags,
  maintenance). That is the product surface.
- A **compromised updater** can still change the explicitly writable deployment
  data (`.env`, `pgdata`, `state`), but the deployment root/Compose file and
  host-owned Guard policy are read-only to it. Those changes cannot select a new
  Guard image or expand the Guard operation/image policy.
- **docker-guard remains host-root equivalent TCB** while it holds
  `docker.sock`. Isolation is policy + network membership, not a second kernel.

### Guard upgrade and rollback

There is deliberately no updater/API/UI path that upgrades Guard. On the host:

1. Verify the release signature and the Guard/updater image digest using an
   independent trust source; do not obtain the expected digest from the running
   updater or its writable `.env`.
2. Update `DOCKER_GUARD_IMAGE` in the host-owned Guard policy and recreate
   `docker-guard`, `updater`, and `updater-gateway` with the deploy script.
3. Run `deploy.sh doctor`; it verifies the running Guard `Config.Image` exactly
   matches `DOCKER_GUARD_EXPECTED_IMAGE` and rejects legacy mutable policy/token.

Rollback uses the same host path with the previously verified digest. Keep the
previous policy file/digest as the recovery point; updater state is not a trust
source for this decision.

---

## Out of scope (unless a new threat appears)

Not planned as follow-up work without a concrete new threat model:

- Redesigning token / gateway auth again
- Removing `docker.sock` from the host TCB entirely
- Forced 2FA / step-up on every update action
- Multi-tenant hard isolation of the updater plane

If you believe a new threat invalidates this baseline, open an issue with the
threat and the failed control — do not treat “more isolation” as default debt.
