#!/bin/bash
# =============================================================================
# Myriad Docker Unified Deployment Script
# =============================================================================
# Brings up the full stack (proxy + frontend + backend + postgres + updater)
# defined in docker-compose.yml.
#
# This script wraps docker compose with a few conveniences:
#   - On first run, copies .env.production.example -> .env
#   - Ensures current production directories and updater env keys exist
#   - Provides quick subcommands: up / down / restart / logs / status / pull
#
# After this script bootstraps the stack, normal day-to-day operation is the
# admin UI -> 设置/关于 -> Update Management. See docs/UPDATER_QUICKSTART.md.
# =============================================================================

set -e

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
ok()   { echo -e "${GREEN}$1${NC}"; }
info() { echo -e "${CYAN}$1${NC}"; }
warn() { echo -e "${YELLOW}$1${NC}"; }
err()  { echo -e "${RED}$1${NC}"; }

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

COMMAND="${1:-up}"

show_usage() {
    cat <<EOF
Usage: $0 [command]

Commands:
  up        (default) Initialise .env / pgdata if needed, then \`docker compose up -d\`
  down      Stop and remove containers (volumes preserved)
  restart   docker compose restart
  pull      Pull images pinned by .env tags
  logs      docker compose logs -f
  status    docker compose ps + image versions
  doctor [--host]  Read-only topology / security checks (nets, gateway, secrets, cosign)
                   --host also runs a non-fatal privileged/docker.sock scan
  upgrade          Pull images pinned by .env tags + recreate (manual upgrade path)
  help             Show this help

Notes:
  - This script only handles bootstrap. Normal updates run through the admin UI.
  - To switch versions, edit MYRIAD_TAG / UPDATER_TAG / PROXY_TAG in .env then
    run \`$0 upgrade\`.
  - Optional host audit (privileged / docker.sock binds):
      bash scripts/security/docker-audit-example.sh scan
      $0 doctor --host

Examples:
  $0                 # Bootstrap + start
  $0 down            # Stop
  $0 status          # See running versions
  $0 doctor          # Topology security checks
  $0 doctor --host   # Topology + non-fatal host privilege scan
  $0 upgrade         # After editing .env, recreate with new tags
EOF
}

detect_compose() {
    if docker compose version >/dev/null 2>&1; then
        echo "docker compose"
    elif command -v docker-compose >/dev/null 2>&1; then
        echo "docker-compose"
    else
        err "✗ neither \`docker compose\` v2 nor \`docker-compose\` v1 is available"
        exit 2
    fi
}

generate_secret() {
    if command -v openssl >/dev/null 2>&1; then
        openssl rand -base64 36 | tr -d '\n=' | tr '+/' '-_'
    else
        head -c 48 /dev/urandom | base64 | tr -d '\n=' | tr '+/' '-_'
    fi
}

ensure_key() {
    local key="$1"
    local default="$2"
    if ! grep -qE "^${key}=" .env 2>/dev/null; then
        echo "${key}=${default}" >> .env
        info "  + appended ${key}"
    fi
}

# Fill empty or missing KEY with a generated secret (same pattern as UPDATE_TOKEN).
ensure_secret_key() {
    local key="$1"
    if grep -qE "^${key}=.+" .env 2>/dev/null; then
        return 0
    fi

    local token
    token="$(generate_secret)"
    if grep -qE "^${key}=" .env 2>/dev/null; then
        local tmp=".env.tmp.$$"
        awk -v key="$key" -v token="$token" '
          BEGIN { replaced = 0; re = "^" key "=" }
          $0 ~ re && replaced == 0 {
            print key "=" token
            replaced = 1
            next
          }
          { print }
        ' .env > "$tmp"
        mv "$tmp" .env
        info "  + filled empty ${key}"
    else
        echo "${key}=${token}" >> .env
        info "  + appended ${key}"
    fi
}

ensure_update_token() {
    ensure_secret_key UPDATE_TOKEN
}

ensure_updater_gateway_secret() {
    ensure_secret_key UPDATER_GATEWAY_SECRET
}

ensure_env() {
    if [ ! -f .env ]; then
        if [ ! -f .env.production.example ]; then
            err "✗ Missing both .env and .env.production.example; cannot bootstrap"
            exit 2
        fi
        warn ".env not found — copying from .env.production.example"
        cp .env.production.example .env
        warn ""
        warn "Edit .env now and set at minimum:"
        warn "  - POSTGRES_PASSWORD (openssl rand -base64 32)"
        warn "  - JWT_SECRET        (openssl rand -base64 32)"
        warn "  - CORS_ORIGINS      (your domain)"
        warn ""
        warn "This script will create pgdata/state/backups and fill empty UPDATE_TOKEN / UPDATER_GATEWAY_SECRET."
        warn ""
        read -r -p "Open .env in \$EDITOR now? (y/N): " r
        if [[ "$r" =~ ^[Yy]$ ]]; then
            ${EDITOR:-nano} .env
        fi
    fi
}

ensure_current_layout() {
    info "==> Ensuring current proxy + updater layout"
    mkdir -p pgdata state state/snapshots state/cache backups
    ensure_key MYRIAD_TAG v0.2.5
    ensure_key PROXY_TAG v0.2.5
    ensure_key UPDATER_TAG v0.2.5
    ensure_key BACKEND_IMAGE docker.io/somekawahitomi/myriad-backend
    ensure_key FRONTEND_IMAGE docker.io/somekawahitomi/myriad-frontend
    ensure_key COMPOSE_PROJECT_NAME myriad
    ensure_key CHANNEL stable
    ensure_key UPDATE_MODE release
    ensure_key MYRIAD_GITHUB_REPO Myriad-You/Myriad
    ensure_key CHECK_INTERVAL_SECS 3600
    ensure_key PROXY_ALLOW_DIRECT_UPDATER false
    ensure_update_token
    ensure_updater_gateway_secret
}

# Backend runs as uid 1000 (USER myriad). Named volumes are root-owned on first
# create; chown so /app/cache and /app/data stay writable without forcing root.
ensure_backend_volume_perms() {
    local project
    project="$(grep -E '^COMPOSE_PROJECT_NAME=' .env 2>/dev/null | head -1 | cut -d= -f2-)"
    project="${project:-myriad}"
    # Strip optional quotes
    project="${project%\"}"
    project="${project#\"}"
    project="${project%\'}"
    project="${project#\'}"

    local cache_vol="${project}_backend_cache"
    local data_vol="${project}_backend_data"

    info "==> Ensuring backend named volumes writable by uid 1000 (myriad)"
    docker volume create "$cache_vol" >/dev/null
    docker volume create "$data_vol" >/dev/null
    if ! docker run --rm \
        -v "${cache_vol}:/app/cache" \
        -v "${data_vol}:/app/data" \
        alpine:3.20 \
        chown -R 1000:1000 /app/cache /app/data
    then
        warn "Could not chown backend volumes (docker run alpine failed)."
        warn "If backend cannot write /app/cache or /app/data, run as host admin:"
        warn "  docker run --rm -v ${cache_vol}:/app/cache -v ${data_vol}:/app/data alpine:3.20 chown -R 1000:1000 /app/cache /app/data"
    fi
}

# Soft (warn-only) topology check after successful up/upgrade. Never fails deploy.
cmd_soft_doctor() {
    info "==> Post-deploy topology soft-check (warn-only)"
    if cmd_doctor; then
        :
    else
        warn "Topology soft-check reported issues; run: $0 doctor  for details"
    fi
    return 0
}

cmd_up() {
    ensure_env
    ensure_current_layout
    ensure_backend_volume_perms
    info "==> docker compose up -d"
    $COMPOSE up -d
    echo ""
    ok "Stack started. Access via http://localhost:${HTTP_PORT:-80}/"
    ok "Admin UI: http://localhost:${HTTP_PORT:-80}/  → 设置 → 关于 → 更新管理"
    cmd_soft_doctor
}

cmd_down() {
    info "==> docker compose down (volumes preserved)"
    $COMPOSE down
}

cmd_restart() {
    info "==> docker compose restart"
    $COMPOSE restart
}

cmd_pull() {
    info "==> docker compose pull"
    $COMPOSE pull
}

cmd_logs() {
    $COMPOSE logs -f --tail=200
}

cmd_status() {
    $COMPOSE ps
    echo ""
    info "Image versions in use:"
    $COMPOSE images 2>/dev/null || $COMPOSE ps --format "table {{.Service}}\t{{.Image}}"
}

# Read-only topology checks. Does not migrate or restart services.
# Expected: docker-guard holds docker.sock; updater on admin+guard nets (not business net);
# updater-gateway injects token; backend has UPDATER_GATEWAY_SECRET but no UPDATE_TOKEN.
# Optional: pass --host for a non-fatal privileged/docker.sock scan.
cmd_doctor() {
    local fail=0
    local skip=0
    local host_scan=0
    local admin_net guard_net business_net
    admin_net="${MYRIAD_ADMIN_NETWORK:-myriad-admin-net}"
    guard_net="${MYRIAD_DOCKER_GUARD_NETWORK:-myriad-docker-guard-net}"
    business_net="${MYRIAD_DOCKER_NETWORK:-myriad-net}"
    for arg in "$@"; do
        case "$arg" in
            --host) host_scan=1 ;;
        esac
    done
    if [ -f .env ]; then
        local v
        v="$(grep -E '^MYRIAD_ADMIN_NETWORK=' .env 2>/dev/null | head -1 | cut -d= -f2- | tr -d "\"'" || true)"
        [ -n "$v" ] && admin_net="$v"
        v="$(grep -E '^MYRIAD_DOCKER_GUARD_NETWORK=' .env 2>/dev/null | head -1 | cut -d= -f2- | tr -d "\"'" || true)"
        [ -n "$v" ] && guard_net="$v"
        v="$(grep -E '^MYRIAD_DOCKER_NETWORK=' .env 2>/dev/null | head -1 | cut -d= -f2- | tr -d "\"'" || true)"
        [ -n "$v" ] && business_net="$v"
    fi

    info "==> Deploy topology doctor (read-only)"

    container_exists() {
        docker inspect "$1" >/dev/null 2>&1
    }

    container_mounts_sock() {
        # Match Source or Destination containing docker.sock
        docker inspect -f '{{range .Mounts}}{{.Source}}|{{.Destination}}{{"\n"}}{{end}}' "$1" 2>/dev/null \
            | grep -q 'docker\.sock'
    }

    container_health() {
        docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$1" 2>/dev/null \
            || echo "unknown"
    }

    # Space-separated network names the container is attached to.
    container_networks() {
        docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' "$1" 2>/dev/null \
            || true
    }

    container_on_network() {
        # $1=container $2=network name
        printf '%s' "$(container_networks "$1")" | tr ' ' '\n' | grep -qx "$2"
    }

    container_env_has() {
        # $1=container $2=env key prefix (e.g. UPDATE_TOKEN=)
        docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$1" 2>/dev/null \
            | grep -qE "^${2}"
    }

    container_env_value() {
        # $1=container $2=KEY
        docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$1" 2>/dev/null \
            | grep -E "^${2}=" | head -1 | cut -d= -f2- || true
    }

    # --- docker-guard ---
    if container_exists myriad-docker-guard; then
        ok "PASS  myriad-docker-guard container exists"
        local gh
        gh="$(container_health myriad-docker-guard)"
        case "$gh" in
            healthy|running)
                ok "PASS  docker-guard status=$gh"
                ;;
            *)
                err "FAIL  docker-guard status=$gh (expected healthy or running)"
                fail=$((fail + 1))
                ;;
        esac
        if container_mounts_sock myriad-docker-guard; then
            ok "PASS  docker-guard mounts docker.sock"
        else
            err "FAIL  docker-guard does not mount docker.sock"
            fail=$((fail + 1))
        fi
        if container_on_network myriad-docker-guard "$guard_net"; then
            ok "PASS  docker-guard is on $guard_net"
        else
            err "FAIL  docker-guard is not on $guard_net (networks: $(container_networks myriad-docker-guard))"
            fail=$((fail + 1))
        fi
        if container_on_network myriad-docker-guard "$business_net"; then
            err "FAIL  docker-guard must not be on business net $business_net"
            fail=$((fail + 1))
        else
            ok "PASS  docker-guard is not on business net $business_net"
        fi
    else
        err "FAIL  myriad-docker-guard not found (stack down or legacy pre-guard topology)"
        fail=$((fail + 1))
    fi

    # --- updater: must NOT mount docker.sock; DOCKER_HOST → docker-guard; admin+guard only ---
    if container_exists myriad-updater; then
        ok "PASS  myriad-updater container exists"
        if container_mounts_sock myriad-updater; then
            err "FAIL  myriad-updater mounts docker.sock (legacy layout — sock should only be on docker-guard)"
            fail=$((fail + 1))
        else
            ok "PASS  myriad-updater does not mount docker.sock"
        fi
        local dhost
        dhost="$(container_env_value myriad-updater DOCKER_HOST)"
        case "$dhost" in
            *docker-guard*)
                ok "PASS  updater DOCKER_HOST points at docker-guard ($dhost)"
                ;;
            "")
                err "FAIL  updater DOCKER_HOST is unset (expected tcp://docker-guard:2375)"
                fail=$((fail + 1))
                ;;
            *)
                err "FAIL  updater DOCKER_HOST=$dhost (expected to contain docker-guard)"
                fail=$((fail + 1))
                ;;
        esac
        if container_on_network myriad-updater "$admin_net"; then
            ok "PASS  updater is on admin-net $admin_net"
        else
            err "FAIL  updater is not on admin-net $admin_net (networks: $(container_networks myriad-updater))"
            fail=$((fail + 1))
        fi
        if container_on_network myriad-updater "$guard_net"; then
            ok "PASS  updater is on guard-net $guard_net"
        else
            err "FAIL  updater is not on guard-net $guard_net"
            fail=$((fail + 1))
        fi
        if container_on_network myriad-updater "$business_net"; then
            err "FAIL  updater must not be on business net $business_net (frontend/postgres isolation)"
            fail=$((fail + 1))
        else
            ok "PASS  updater is not on business net $business_net"
        fi
    else
        warn "SKIP  myriad-updater not running"
        skip=$((skip + 1))
    fi

    # --- updater-gateway: exists, no docker.sock, admin-net only preferred ---
    if container_exists myriad-updater-gateway; then
        ok "PASS  myriad-updater-gateway container exists"
        if container_mounts_sock myriad-updater-gateway; then
            err "FAIL  updater-gateway mounts docker.sock (must not)"
            fail=$((fail + 1))
        else
            ok "PASS  updater-gateway does not mount docker.sock"
        fi
        if container_on_network myriad-updater-gateway "$admin_net"; then
            ok "PASS  updater-gateway is on admin-net $admin_net"
        else
            err "FAIL  updater-gateway is not on admin-net $admin_net"
            fail=$((fail + 1))
        fi
        if container_on_network myriad-updater-gateway "$guard_net"; then
            err "FAIL  updater-gateway must not be on guard-net $guard_net"
            fail=$((fail + 1))
        else
            ok "PASS  updater-gateway is not on guard-net"
        fi
        if container_on_network myriad-updater-gateway "$business_net"; then
            err "FAIL  updater-gateway must not be on business net $business_net"
            fail=$((fail + 1))
        else
            ok "PASS  updater-gateway is not on business net"
        fi
    else
        err "FAIL  myriad-updater-gateway not found (P0 topology requires gateway; token off backend)"
        fail=$((fail + 1))
    fi

    # --- backend: non-root when inspectable; no UPDATE_TOKEN in Config.Env ---
    if container_exists myriad-backend; then
        ok "PASS  myriad-backend container exists"
        local buser
        buser="$(docker inspect -f '{{.Config.User}}' myriad-backend 2>/dev/null || true)"
        case "$buser" in
            ""|0|0:0|root)
                # Empty often means image default; try runtime if possible
                local uid
                uid="$(docker exec myriad-backend id -u 2>/dev/null || echo "")"
                if [ "$uid" = "0" ]; then
                    warn "WARN  backend appears to run as uid 0 (prefer non-root / USER myriad)"
                elif [ -n "$uid" ]; then
                    ok "PASS  backend runtime uid=$uid (Config.User=${buser:-empty})"
                else
                    warn "SKIP  backend user not inspectable (Config.User=${buser:-empty})"
                    skip=$((skip + 1))
                fi
                ;;
            *)
                ok "PASS  backend Config.User=$buser"
                ;;
        esac
        if container_env_has myriad-backend 'UPDATE_TOKEN='; then
            err "FAIL  backend Config.Env contains UPDATE_TOKEN (should use updater-gateway only)"
            fail=$((fail + 1))
        else
            ok "PASS  backend Config.Env has no UPDATE_TOKEN"
        fi
        if container_env_has myriad-backend 'UPDATER_GATEWAY_SECRET='; then
            local bsec
            bsec="$(container_env_value myriad-backend UPDATER_GATEWAY_SECRET)"
            if [ "${#bsec}" -ge 32 ]; then
                ok "PASS  backend has UPDATER_GATEWAY_SECRET (≥32 chars)"
            else
                err "FAIL  backend UPDATER_GATEWAY_SECRET is set but shorter than 32 chars"
                fail=$((fail + 1))
            fi
        else
            err "FAIL  backend Config.Env missing UPDATER_GATEWAY_SECRET (required for gateway hop)"
            fail=$((fail + 1))
        fi
        local up_url
        up_url="$(container_env_value myriad-backend MYRIAD_UPDATER_URL)"
        case "$up_url" in
            *updater-gateway*|*updater*)
                ok "PASS  backend MYRIAD_UPDATER_URL=$up_url"
                ;;
            "")
                warn "WARN  backend MYRIAD_UPDATER_URL unset (defaults may still apply)"
                ;;
            *)
                warn "WARN  backend MYRIAD_UPDATER_URL=$up_url (expected updater-gateway)"
                ;;
        esac
        # Soft reachability: gateway /healthz when exec works
        if docker exec myriad-backend wget --spider -q "http://updater-gateway:1104/healthz" 2>/dev/null \
            || docker exec myriad-backend wget --spider -q "${up_url:-http://updater-gateway:1104}/healthz" 2>/dev/null; then
            ok "PASS  backend can reach updater-gateway /healthz (soft)"
        else
            warn "WARN  backend cannot probe updater-gateway /healthz (soft; stack may still be starting)"
        fi
    else
        warn "SKIP  myriad-backend not running"
        skip=$((skip + 1))
    fi

    # --- updater-gateway secret presence ---
    if container_exists myriad-updater-gateway; then
        if container_env_has myriad-updater-gateway 'UPDATER_GATEWAY_SECRET='; then
            local gsec
            gsec="$(container_env_value myriad-updater-gateway UPDATER_GATEWAY_SECRET)"
            if [ "${#gsec}" -ge 32 ]; then
                ok "PASS  updater-gateway has UPDATER_GATEWAY_SECRET (≥32 chars)"
            else
                err "FAIL  updater-gateway UPDATER_GATEWAY_SECRET shorter than 32 chars"
                fail=$((fail + 1))
            fi
        else
            err "FAIL  updater-gateway missing UPDATER_GATEWAY_SECRET"
            fail=$((fail + 1))
        fi
    fi

    # --- .env cosign dual-key (optional, when file present) ---
    if [ -f .env ]; then
        local cosign_raw cosign_lc direct direct_lc
        cosign_raw="$(grep -E '^COSIGN_VERIFY=' .env 2>/dev/null | head -1 | cut -d= -f2- | tr -d "\"'" || true)"
        cosign_raw="${cosign_raw:-strict}"
        cosign_lc="$(printf '%s' "$cosign_raw" | tr '[:upper:]' '[:lower:]')"
        case "$cosign_lc" in
            off|false|0)
                if grep -qiE '^(UPDATER_ALLOW_INSECURE_COSIGN|COSIGN_INSECURE_OK)=(true|1|yes|on)[[:space:]]*$' .env 2>/dev/null; then
                    warn "WARN  COSIGN_VERIFY=$cosign_raw with insecure allow key set (supply-chain risk)"
                else
                    err "FAIL  COSIGN_VERIFY=$cosign_raw without UPDATER_ALLOW_INSECURE_COSIGN=true (updater refuses to start)"
                    fail=$((fail + 1))
                fi
                ;;
            soft|warn)
                warn "WARN  COSIGN_VERIFY=$cosign_raw (prefer strict for production)"
                ;;
            *)
                ok "PASS  COSIGN_VERIFY=$cosign_raw"
                ;;
        esac

        direct="$(grep -E '^PROXY_ALLOW_DIRECT_UPDATER=' .env 2>/dev/null | head -1 | cut -d= -f2- | tr -d "\"'" || true)"
        direct_lc="$(printf '%s' "${direct:-false}" | tr '[:upper:]' '[:lower:]')"
        case "$direct_lc" in
            true|1|yes|on)
                warn "WARN  PROXY_ALLOW_DIRECT_UPDATER=$direct (rescue path; keep false for normal ops)"
                ;;
            *)
                ok "PASS  PROXY_ALLOW_DIRECT_UPDATER=${direct:-false}"
                ;;
        esac
    else
        warn "SKIP  .env not found (cosign / direct-updater checks)"
        skip=$((skip + 1))
    fi

    echo ""
    info "Optional host audit tip:"
    if [ -f scripts/security/docker-audit-example.sh ]; then
        info "  path: scripts/security/docker-audit-example.sh"
        info "  run:  bash scripts/security/docker-audit-example.sh scan"
        info "  or:   $0 doctor --host   (non-fatal privileged / docker.sock scan)"
    else
        info "  scripts/security/docker-audit-example.sh not present in this tree"
    fi

    if [ "$host_scan" = "1" ]; then
        echo ""
        info "==> Optional host privilege scan (non-fatal; warn only)"
        local found_priv=0 found_sock=0
        if command -v docker >/dev/null 2>&1; then
            while read -r id; do
                [ -z "$id" ] && continue
                local priv name
                priv="$(docker inspect -f '{{.HostConfig.Privileged}}' "$id" 2>/dev/null || echo false)"
                name="$(docker inspect -f '{{.Name}}' "$id" 2>/dev/null | sed 's#^/##')"
                if [ "$priv" = "true" ]; then
                    warn "WARN  privileged container: ${name:-$id}"
                    found_priv=$((found_priv + 1))
                fi
                if docker inspect -f '{{range .Mounts}}{{.Source}}|{{.Destination}}{{"\n"}}{{end}}' "$id" 2>/dev/null \
                    | grep -q 'docker\.sock'; then
                    # Expected on docker-guard only in the recommended topology.
                    case "$name" in
                        myriad-docker-guard|myriad-docker-guard-dev)
                            ok "PASS  docker.sock bind expected on $name"
                            ;;
                        *)
                            warn "WARN  docker.sock bind on unexpected container: ${name:-$id}"
                            found_sock=$((found_sock + 1))
                            ;;
                    esac
                fi
            done < <(docker ps -aq 2>/dev/null || true)
            if [ "$found_priv" -eq 0 ]; then
                ok "PASS  no Privileged=true containers found (scan)"
            fi
            if [ "$found_sock" -eq 0 ]; then
                ok "PASS  no unexpected docker.sock binds (scan)"
            fi
        else
            warn "SKIP  docker CLI unavailable for host scan"
            skip=$((skip + 1))
        fi
    fi

    echo ""
    if [ "$fail" -gt 0 ]; then
        err "Doctor: $fail check(s) failed (skip=$skip). Fix topology; this command does not auto-migrate."
        return 1
    fi
    ok "Doctor: all checks passed (skip=$skip)."
    # Soft operator red lines (never fail doctor). Full list:
    # docs/deployment/UPDATER_SECURITY_BASELINE.md
    echo ""
    info "Operator red lines (reminders — see docs/deployment/UPDATER_SECURITY_BASELINE.md):"
    info "  1. Existing installs: host deploy upgrade / compose up -d so topology matches"
    info "  2. Protect UPDATE_TOKEN and UPDATER_GATEWAY_SECRET (not frontend/tickets)"
    info "  3. Keep PROXY_ALLOW_DIRECT_UPDATER=false except temporary rescue"
    info "  4. Keep COSIGN_VERIFY=strict unless intentional dual-key off"
    info "  5. Do not publish updater/gateway/guard ports on the host"
    return 0
}

cmd_upgrade() {
    ensure_env
    ensure_backend_volume_perms
    info "==> docker compose pull"
    $COMPOSE pull
    info "==> docker compose up -d (recreate with new tags)"
    $COMPOSE up -d
    ok "Upgrade complete. Verify with: $0 status"
    cmd_soft_doctor
}

COMPOSE=$(detect_compose)

case "$COMMAND" in
    up)       cmd_up ;;
    down)     cmd_down ;;
    restart)  cmd_restart ;;
    pull)     cmd_pull ;;
    logs)     cmd_logs ;;
    status)   cmd_status ;;
    doctor)   shift || true; cmd_doctor "$@" ;;
    upgrade)  cmd_upgrade ;;
    help|-h|--help) show_usage ;;
    *) err "Unknown command: $COMMAND"; show_usage; exit 1 ;;
esac
