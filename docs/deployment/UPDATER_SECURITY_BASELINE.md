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
| **docker-guard** | Only guard mounts `docker.sock`; method/project/image/bind policy; updater-only on guard-net |
| **Token hop** | `UPDATE_TOKEN` in updater + gateway + guard only — **not** in backend env |
| **Gateway secret** | `UPDATER_GATEWAY_SECRET` (≥32) on backend ↔ updater-gateway; admin-net alone is not enough |
| **Cosign dual-key** | Default `COSIGN_VERIFY=strict`; `off` requires `UPDATER_ALLOW_INSECURE_COSIGN=true` (or alias) |
| **Audit** | `state/audit.log` (fsync, rotate); actor header from admin JWT when proxied |
| **Doctor** | `deploy.sh doctor` / `deploy.ps1 doctor` topology + secrets + cosign; optional `--host` scan |
| **Self-update** | TCB rebuild (updater + docker-guard); `UPDATER_TAG` restore on helper failure; last-status visibility |
| **Hygiene** | Secret redaction, stricter rate limits on mutative admin routes, `confirm_risk` for high-risk flags |
| **Rescue path** | `PROXY_ALLOW_DIRECT_UPDATER=false` by default; direct `/_updater/*` is temporary only |
| **Backend** | Non-root image user; soft-doctor after `up` / `upgrade` |

---

## Operator red lines

Must follow on every production self-hosted install:

1. **Existing installs / topology drift:** run one host-level
   `bash scripts/docker/deploy.sh upgrade` (or `docker compose up -d` from the
   deploy root) so networks and services match current compose (admin-net,
   updater-gateway, docker-guard). UI alone cannot switch topology.
2. **After deploy:** run `bash scripts/docker/deploy.sh doctor` and fix every
   **FAIL** before trusting the stack.
3. **Protect secrets:** `UPDATE_TOKEN` and `UPDATER_GATEWAY_SECRET` stay out of the
   frontend, tickets, chat, and CI logs. Prefer deploy-generated values; rotate
   if leaked.
4. **Keep** `PROXY_ALLOW_DIRECT_UPDATER=false` except temporary rescue; turn it
   off again when done.
5. **Keep** `COSIGN_VERIFY=strict` unless you intentionally accept dual-key
   insecure mode (`off` + allow key).
6. **Do not publish** updater (`1101`), updater-gateway (`1104`), or docker-guard
   (`2375`) ports on the host. Only proxy’s `HTTP_PORT` is public.
7. **Optional:** `bash scripts/docker/deploy.sh doctor --host` for a non-fatal
   privileged / unexpected `docker.sock` scan after major host changes.

---

## What we deliberately accept

These are **by design** for single-tenant self-host, not open bugs:

- An **admin with update rights** can change the running stack (images, tags,
  maintenance). That is the product surface.
- A **compromised updater** can still affect deploy data under its bind
  (`.env`, `pgdata`, `state`) — that is the intended control plane.
- **docker-guard remains host-root equivalent TCB** while it holds
  `docker.sock`. Isolation is policy + network membership, not a second kernel.

---

## Out of scope (unless a new threat appears)

Not planned as follow-up work without a concrete new threat model:

- Redesigning token / gateway auth again
- Removing `docker.sock` from the host TCB entirely
- Forced 2FA / step-up on every update action
- Multi-tenant hard isolation of the updater plane

If you believe a new threat invalidates this baseline, open an issue with the
threat and the failed control — do not treat “more isolation” as default debt.
