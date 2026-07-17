# Migrating to the docker-guard topology

This guide covers moving an existing single-node Myriad deployment from the
legacy layout (**updater mounts `docker.sock` directly**) to the current
production layout (**`docker-guard` holds the socket; updater uses a policy
proxy on an internal network**).

You do **not** reinstall Myriad. Keep the same deployment directory, `pgdata`,
`.env`, `state/`, and `backups/`.

> UI-only update **cannot** switch topology. Compose file shape, networks, and
> volume mounts must be applied on the host with Docker Compose.

## What changes

| Before | After |
| --- | --- |
| `updater` mounts `/var/run/docker.sock` | Only **`docker-guard`** mounts the socket |
| No internal guard network | `myriad-docker-guard-net` (internal) + dual-home `updater` |
| Updater image runs the daemon only | Same image also provides `myriad-docker-guard` entrypoint |
| Self-update helper / compose up `updater` | Guard schedules **`docker-guard` + `updater`** recreate |

Unchanged:

- Deployment root (compose project directory)
- `./pgdata` bind mount and PostgreSQL data
- `.env` secrets and tags (`POSTGRES_PASSWORD`, `JWT_SECRET`, `UPDATE_TOKEN`, …)
- `./state` and `./backups`
- Business network membership of backend / frontend / postgres / proxy
- Updater remains on **`myriad-net`** (dual-homed with the guard network); this
  migration does **not** require removing updater from the business network

## Prerequisites

- Host Docker Engine with `docker compose` v2
- Ability to pull the release that includes `myriad-docker-guard` in the
  updater image (or build from a tree that has `updater/src/bin/myriad-docker-guard.rs`)
- Back up once (optional but recommended):

```bash
cd /path/to/myriad   # directory that contains docker-compose.yml and .env
docker compose exec -T postgres pg_dump -U myriad -d myriad \
  > "backups/pre-guard-migration_$(date +%Y%m%d_%H%M%S).sql"
```

## Steps (same deploy directory)

1. **Stop the stack** (volumes and bind data stay on disk):

   ```bash
   docker compose down
   # or: bash scripts/docker/deploy.sh down
   ```

2. **Update the deployment files** to a revision that includes:
   - `docker-guard` service in `docker-compose.yml`
   - updater without the raw socket mount
   - `DOCKER_HOST=tcp://docker-guard:2375`
   - `myriad-docker-guard-net` (internal)

   Typical approaches:
   - `git pull` (or unpack a release tarball) **into the same directory**, or
   - copy only `docker-compose.yml` / deploy scripts from the new release while
     leaving `.env`, `pgdata/`, `state/`, `backups/` untouched.

3. **Keep `.env`**. Confirm at least:

   ```bash
   grep -E '^(MYRIAD_TAG|UPDATER_TAG|PROXY_TAG|UPDATE_TOKEN|COMPOSE_PROJECT_NAME)=' .env
   ```

   Optional new keys (defaults work if omitted):

   ```bash
   # MYRIAD_DOCKER_GUARD_NETWORK=myriad-docker-guard-net
   ```

4. **Ensure backend volume ownership** (backend runs as uid 1000, not root):

   ```bash
   bash scripts/docker/deploy.sh up
   # deploy.sh chowns ${COMPOSE_PROJECT_NAME}_backend_{cache,data} to 1000:1000
   ```

   Or manually:

   ```bash
   PROJECT=${COMPOSE_PROJECT_NAME:-myriad}
   docker volume create "${PROJECT}_backend_cache"
   docker volume create "${PROJECT}_backend_data"
   docker run --rm \
     -v "${PROJECT}_backend_cache:/app/cache" \
     -v "${PROJECT}_backend_data:/app/data" \
     alpine:3.20 chown -R 1000:1000 /app/cache /app/data
   docker compose pull
   docker compose up -d
   ```

5. **Verify**:

   ```bash
   docker compose ps
   # docker-guard healthy; updater has no docker.sock mount
   docker inspect myriad-updater --format '{{json .Mounts}}' | head
   docker inspect myriad-docker-guard --format '{{json .Mounts}}' | head
   curl -fsS "http://localhost:${HTTP_PORT:-80}/api/health"   # or site root
   ```

6. **Day-to-day updates** remain Admin UI → Settings → About → Update Management.
   Self-update recreates **both** `docker-guard` and `updater` so the TCB binary
   tracks `UPDATER_TAG`.

## Rollback of the migration

If the new compose file fails to start:

1. `docker compose down`
2. Restore the previous `docker-compose.yml` (and image tags in `.env` if needed)
3. `docker compose up -d`

`pgdata` and `.env` were not rewritten by the topology switch.

## Trust notes (unchanged by this migration)

- Same-network trust on `myriad-net` still applies: other containers on that
  bridge can still try to reach updater HTTP. The guard network is **internal**
  and only updater may join it (compose + guard policy). See
  [updater-spec.md §15.1](../updater-spec.md).
- Example host-side audit filters for privileged containers / socket binds:
  [scripts/security/docker-audit-example.sh](../../scripts/security/docker-audit-example.sh).

## Related docs

- [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md) — current topology
- [UPDATER_QUICKSTART.md](../UPDATER_QUICKSTART.md) — operator update / rescue
- [updater-spec.md §14](../updater-spec.md) — self-update (guard + direct sock)

---

## Follow-on: admin-net + updater-gateway (P0 token / network isolation)

If you already run the docker-guard layout above and are upgrading to a release that
adds **`myriad-admin-net`** and **`updater-gateway`**:

### What changes

| Before | After |
| --- | --- |
| `updater` dual-homes `myriad-net` + guard-net | `updater` on **admin-net + guard-net** only (leaves business net) |
| Backend holds `UPDATE_TOKEN` | Backend has **no** `UPDATE_TOKEN`; talks to `updater-gateway:1104` |
| No admin plane network | `myriad-admin-net` for backend / gateway / updater / proxy rescue |
| Self-update recreates `docker-guard` + `updater` | Also recreates **`updater-gateway`** (same `UPDATER_TAG` image) |

### Steps (same deploy directory — one recreate)

1. Pull or unpack the release that includes `myriad-updater-gateway` in the updater
   image and the new `docker-compose.yml` shape.
2. Keep `.env` as-is (`UPDATE_TOKEN` still required for gateway/updater/guard).
   Ensure `UPDATER_GATEWAY_SECRET` is set (deploy scripts fill it if empty) for
   backend↔gateway caller auth. Optional: `MYRIAD_ADMIN_NETWORK=myriad-admin-net`.
3. From the deployment root:

   ```bash
   docker compose pull
   docker compose up -d
   # or: bash scripts/docker/deploy.sh upgrade
   ```

4. Verify:

   ```bash
   bash scripts/docker/deploy.sh doctor
   # backend Config.Env has no UPDATE_TOKEN
   # updater not on myriad-net; gateway on admin-net; no sock on gateway
   ```

UI-only update **cannot** create the new network or gateway service definition;
host compose is required once, same as the original guard migration.
