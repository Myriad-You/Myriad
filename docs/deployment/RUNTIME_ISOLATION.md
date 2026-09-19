# Runtime roles and deployment migration

The bundled and external-PostgreSQL Compose files run federation HTTP, WebSocket
connections and outbound delivery in `federation-worker`. The proxy routes this
domain directly to that process; `MYRIAD_PROCESS_ROLE=web` does not register its
handlers. Persona HTTP, live state, channel bots, background execution and the MCP client run in
`persona-worker`. These three services use the same backend image/version, with
separate processes and resource budgets. They can restart independently but are
not independent release artifacts. **Production refuses local MCP subprocesses.
The fixed-container gateway provides OS/resource isolation and bounded lifecycle
cleanup; see [MCP gateway](MCP_GATEWAY.md) and its optional
[network/persistence controls](MCP_CAPABILITIES.md).**

| Entry | Starts | Intended use |
| --- | --- | --- |
| `MYRIAD_PROCESS_ROLE=web` | Web bootstrap, platform/Phantasi/TAPP schedulers; no persona or federation domain | Production web |
| `/app/myriad-federation-worker` or `MYRIAD_PROCESS_ROLE=federation-worker` | Existing-schema check, configuration refresh, federation HTTP/WS, delivery and health | Trusted first-party federation process |
| `/app/myriad-persona-worker` or `MYRIAD_PROCESS_ROLE=persona-worker` | Existing-schema check, persona HTTP/state, notifications, supervised drivers and MCP | Trusted first-party persona process |
| `MYRIAD_PROCESS_ROLE=all` | Combined runtime | Development only; rejected with `ENVIRONMENT=production` |
| Role unset | Startup error before web bootstrap in every environment | Migrate host topology; local dev explicitly selects `all` |

The dedicated worker executable path takes precedence over role environment values.
It is an alias in new images and absent from old images. Older backend images must
never be launched as workers just by changing an environment variable: they do not
understand the role and would start the entire application.

For external PostgreSQL in another container, backend and both workers retain
`myriad-net` and join `MYRIAD_BACKEND_EXTRA_NETWORK` (default `myriad-backend-ext`)
with the database. Network membership is
per container. Updater/Guard permit this network only for these three services;
upgrade both from older builds before online updates. Workers still cannot join
admin or Guard networks. See [external PostgreSQL](EXTERNAL_POSTGRES.md).

## Federation worker boundary

The official worker has a read-only root filesystem, UID/GID 1000, all capabilities
dropped, `no-new-privileges`, and a 32 MiB `/tmp`. `backend_data` is read-only at
`/app/data`; only these existing volume subdirectories are writable:

| Volume subpath | Container path | Purpose |
| --- | --- | --- |
| `backend_data/federation` | `/app/data/federation` | File transfers |
| `backend_data/federation_media` | `/app/data/federation_media` | Published Note media |
| `backend_cache/images` | `/tmp/cache/images` | Shared avatar cache |

The volume initializer creates these directories before container creation and
rejects symlinks. No data relocation is needed. This requires Docker/Compose support
for [volume subpath mounts](https://docs.docker.com/reference/compose-file/services/#long-syntax-5).
Guard allows only these exact source/subpath/destination tuples, with `nocopy`; it
rejects writable access to the data root, agent files or other caches. The process
has no updater secret, Docker socket or management-network attachment. Container
limits remain 0.5 CPU, 512 MiB memory and 64 PIDs. Its DB pool has at most four
connections, with connection/acquisition and SQL statement/lock deadlines. These
are first-party credentials; this container is **not** a third-party MCP sandbox.

Workers use separate database logins with server-enforced connection limits,
transaction deadlines, and bounded temporary query files; see
[worker database budgets](WORKER_DATABASE.md) for migration and external-DB setup.
PostgreSQL and underlying storage remain shared: these controls do not establish
CPU quotas or provide database high availability.

The worker admits eight HTTP requests immediately and holds capacity through the
response body, with a 60-second handler deadline and 120-second response deadline.
Proxy federation forwarding has a separate 32-request budget and a 180-second
response deadline. Neither queues unbounded waiters. WebSockets keep their existing
authentication, Origin checks and one-time TAPP tickets; the worker and proxy each
cap them at 64 connections separately from HTTP. Worker WS frames/messages are
limited to 1 MiB. The broadcaster registry and its HTTP/WS producers now share the
federation process. Health is outside the worker's HTTP admission budget.

Web owns migrations and installation-key creation. Both workers refuse schema drift,
a missing/invalid installation key and JWT-derived fallback. Deployments overriding
`MYRIAD_DATA_KEY` must supply the same existing key to all trusted processes. The
worker does not generate a separate key. It refreshes durable site origin and DB
configuration every 15 seconds; refresh failure stops the process, allowing its
supervisor to restart it rather than continue indefinitely with stale settings.

Runtime TAPP grant revalidation reads committed permission policy directly from
PostgreSQL on each request, using the existing permission parser/defaults. The
15-second general-config refresh is not a revocation grace period. Failed reads or
invalid policy values reject the grant; host credentials are not read by this query.

`GET /health` reports the worker role, DB readiness, version/commit and federation
gate. Geographic disablement exits this process with status 0; Compose
`restart: on-failure` keeps it down instead of looping an idle worker. Web and
persona keep serving. The delivery task and HTTP/config refresh tasks are
supervised together while the gate is open. SIGTERM cancels an in-flight delivery
and bounds health-server draining to five seconds. Existing token-checked leases
remain recoverable after interruption; remote HTTP delivery is still at-least-once.

## Update and rollback

Migrate the host-owned Compose file and upgrade the proxy/updater/Guard TCB **before**
upgrading the business images. The new proxy can still route to an old backend, so
it can be deployed first. Apply `PROXY_FEDERATION_UPSTREAM=http://federation-worker:1103`
and `PROXY_PERSONA_UPSTREAM=http://persona-worker:1103` to the running proxy,
add both worker services and set backend `MYRIAD_PROCESS_ROLE=web`. The new Guard
must understand both fixed worker commands and their storage/resource contracts. Pulling an image does not
migrate Compose. Current binaries reject an unset role in every environment; they no longer
continue combined execution after a warning.

Updater preflight checks the explicit web role, worker topology and storage mounts,
and inspects the running proxy's image capability label
`io.myriad.proxy.federation-routing=1` and `io.myriad.proxy.persona-routing=1`,
plus their routing environment. Failure blocks the
update before maintenance, service stop or snapshot. Existing installations must
complete this migration instead of discovering missing services after downtime.

The updated updater manages both workers alongside web for stop/recreate and rescue
rollback. Before database restore, it proves that both workers have stopped, including
an orphaned worker no longer mentioned by Compose. Inspection failures block the
restore. Image capability labels `io.myriad.runtime.federation-worker=1` and
`io.myriad.runtime.persona-worker=1` determine which workers may start after a tag swap. Rolling back to a pre-role image leaves the
unsupported workers stopped and restores that image's combined backend. Health checks compare
actual container image identity and Docker health, without attaching updater to
the worker network.

Proxy refreshes the backend's `/health` routing capability every two seconds. A
recognized full backend reporting `federation_http_isolated=true` or
`persona_http_isolated=true` uses the corresponding worker; a recognized legacy
full backend without that field uses its original routes. The two flags are independent.
Failed/malformed probes retain the last routing choice (startup defaults to worker).
Worker failure never triggers fallback into a current web process. A tag transition
can briefly return 404/502 until the next capability refresh; end-to-end container
rollback verification remains necessary.

Guard permits only the fixed worker executables, health commands, UID, runtime paths,
environment keys, bounded resources, per-role volume contracts and business network. This does not permit arbitrary command overrides or mounts.

## Notifications across processes

Web and federation persona observations use a separate bounded PostgreSQL `NOTIFY` channel,
independent of notification preferences. Only the process owning persona live state
consumes them; federation does not start persona inference or speech ticks. These
observations carry a user ID, whitelisted event kind and capped summary within the
trusted backend/DB boundary. They are ephemeral: a disconnected listener can miss
an observation, with no replay on reconnect (matching their best-effort nature).

Notification INSERT/UPSERT and PostgreSQL `NOTIFY` commit in one SQL statement.
The wakeup carries process/row/user identity, never a notification body. Persona reads
the committed row and applies the existing owner filter before SSE delivery. A
stable ID cannot be reassigned to a different owner by UPSERT. If a row was deleted
before the wakeup is consumed, it is not resurrected. Persistent producers reload
notification preferences from the database, avoiding an indefinite process-local
opt-in after the user changes settings.

`NOTIFY` is a wakeup, not a durable event queue. Reconnection emits a resync; active
SSE clients also receive a history resync every 30 seconds. Missed wakeups therefore
recover the persisted list while DB connectivity is available. This does not replay
every intermediate toast or ephemeral persona speech. Notification persistence
retains the previous best-effort failure behavior; this bridge does not make a
failed database write durable.

## Persona worker boundary and lifecycle

`persona-worker` owns `/api/agent`, `/api/speech`, `/api/merope/rig` and
`/api/tapp/agent/v2/interactions/*` as one state domain, including run hubs,
cancellation, interaction callbacks, voice sessions and notification SSE. QQ,
Telegram, Discord and Feishu connections and their recovery loop also run here:
they create Agent runs and must share the same owner as pairing/status APIs. Web
registers none of these handlers. Web and federation publish persistent notifications;
only persona consumes their wakeups and persona observations. Live speech remains
local and is not replayed. Lightweight diary writes may still originate in web.

The worker uses UID/GID 1000, a read-only root, all capabilities dropped,
`no-new-privileges`, a 32 MiB `/tmp`, and limits of 1 CPU, 1 GiB memory and 64 PIDs.
Its DB pool has at most eight connections, acquisition/connection deadlines of
5/10 seconds and SQL statement/lock deadlines of 30/3 seconds. Unlike federation,
first-party Agent capabilities require writable `backend_data` and `backend_cache`
volumes. No updater secret, Docker socket or management network is attached.
These volumes and DB/JWT credentials make it a trusted first-party process;
development-only stdio children are **not** a separate OS security boundary.

Persona HTTP admits eight ordinary requests, 32 passive GET streams and eight
control requests independently. Cancellation/presence and conversation stop/interrupt
remain admissible when the work or stream budgets are full. Capacity is held through
the response body; handler and response deadlines are 600/3600 seconds. Proxy uses
separate persona budgets of 32 requests, 64 passive streams and 16 controls.
Health is outside these budgets; the edge rejects `/internal/*` and persona WS upgrades.

`backend/src/persona/` owns initialization, five supervised continuous channel
drivers (four bot connections and recovery), and six periodic drivers: autonomy,
speech intents, TAPP interaction expiry, confirmation cleanup, heartbeat, and
memory/skill pruning. Autonomy and speech each await their own tick with missed
wakes skipped. Heartbeat processes one reserved batch with two active executions,
retaining each claim's scheduled minute. Missed cron minutes do not accumulate.

Shutdown cancels channel connection/recovery futures and their configuration
watchers, stops periodic admission, drains HTTP for up to five seconds and active
driver work for up to 30 seconds, then cancels remaining futures before flushing
memory/skill state and reaping MCP children. Official Compose allows 45 seconds.
An unexpected driver exit or configuration-refresh failure stops the worker;
web remains a separate process. Persona health requires DB readiness, current
configuration and running drivers. This status does not supervise every detached
Agent task or certify availability of a configured external model/MCP server.

## Calls to web-owned schedulers

Persona forwards only seven first-party capabilities to the web state owner:
`scheduler.create`, `scheduler.trigger`, `scheduler.list`, `phantasi.schedule`,
`task.submit`, `platform.refresh` and `system.metrics`. This avoids starting
second copies of web's schedulers and task processor. Other first-party Agent
capabilities continue in persona.

The private POST `/internal/persona/web-capability` uses an HMAC binding the entire
body, method, path, timestamp and nonce to the existing trusted JWT secret. The
acceptance window is 30 seconds; the bounded replay cache refuses new calls when
full instead of evicting unexpired entries. Admission is eight calls, the body limit
256 KiB, response limit 4 MiB and deadline 120 seconds. Production upstream is fixed
to `http://backend:1103`; no model-supplied URL, executable or capability category
is accepted. The proxy does not expose this route.

Web verifies current account existence, Agent access and fresh granted permissions,
intersecting any autonomy cap before dispatch. A signed caller is not a persistent
user grant. Non-admin Agent permission checks read committed policy directly;
the general 15-second configuration refresh is not a permission-revocation delay.
Channel admission and delivery additionally read committed bot enablement and
credential scope, refusing stale bindings during disablement/credential rotation
even before connection supervisors refresh their cached configuration.
Transport failures after dispatch have an unknown outcome. The executor marks
these errors non-retryable to avoid automatically repeating a possibly completed
mutation; this is not durable exactly-once execution.

## MCP transport and migration

Production (`ENVIRONMENT=production`) does not start local stdio MCP processes.
Existing definitions remain on disk and visible for editing; blocked definitions
are excluded from the active registry. Administrators can disable old entries one
by one without activating the remaining ones. A changed/enabled definition that
violates host policy is rejected before persistence. Development may explicitly
use local stdio, which still shares its host UID/filesystem/network.

Gateway definitions select `transport: "gateway"` and contain no command, arguments
or environment. One definition represents the endpoint's aggregate tool collection;
IDs do not select individual remote servers. Use one enabled gateway entry. See
[MCP gateway deployment](MCP_GATEWAY.md) for the pinned static gateway and fixed
tool containers. The tested deployment uses bubblewrap, cgroup limits and an
external watchdog to bound untrusted guests; it does not mount the Docker socket.
The upstream dynamic sibling-container mode failed cleanup tests and is excluded.
The operator sets `MYRIAD_MCP_GATEWAY_URL` and
`MYRIAD_MCP_GATEWAY_TOKEN` in the persona deployment; the token must contain at least
32 printable non-space ASCII bytes. Neither is accepted from tool definitions or
returned by MCP configuration APIs. The UI receives only availability flags.
The gateway endpoint must be HTTP(S), without embedded credentials, query or
fragment. Its reachability is the host operator's decision. Redirects, ambient
proxy discovery and automatic HTTP retries are disabled.

The client implements Streamable HTTP JSON and SSE responses, session/protocol
headers, 4 MiB messages, a 16 MiB stream total, 64 SSE events, and a 30-second request
deadline inside the actor's existing 35-second queue-inclusive budget. Invalid
response IDs, unsupported protocol versions, oversized data and interrupted
exchanges poison the session. Tool-call failures/ambiguous deadlines are marked
non-retryable by the Agent executor. Cancellation and DELETE are best effort with
a one-second cleanup budget and bounded detached cleanup concurrency.

**HTTP disconnect/cancellation is not container destruction.** This transport does
not inspect or attest the remote gateway's sandbox policy. Before migrating an
installation with enabled MCP servers, the gateway/server deployment must supply
per-server filesystem/network/resource isolation and bounded cleanup of detached
descendants. The bundled fixed-container example supplies and tests those controls;
merely setting another endpoint does not establish equivalent isolation. Do not re-enable
production stdio as a gateway-outage fallback.

## Validation and remaining work

A disposable PostgreSQL integration test exercises committed wakeups across separate
connections, row updates, owner filtering, stable-ID ownership and deletion races:

```sh
MYRIAD_NOTIFICATION_BRIDGE_TEST_DB='<disposable PostgreSQL URL>' \
  cargo test -p myriad-backend committed_changes_cross_connections_without_reassigning_owners \
  -- --ignored --nocapture
```

Do not point it at a production database. It creates and drops a uniquely named
schema. Guard and topology unit tests run with `cargo test --manifest-path
updater/Cargo.toml --lib`. The unit tests establish policy decisions; the separate container run below
checks their deployment behavior.

Production local stdio is blocked. The optional fixed-container MCP deployment
provides OS/resource isolation and bounded guest destruction; development stdio
and independently operated remote endpoints do not inherit those guarantees.
Shared PostgreSQL and storage remain common failure domains: process separation
cannot guarantee homepage latency for arbitrary database locks or host failure.


### Container acceptance (2026-09-12)

A disposable deployment ran on Docker Desktop Engine 29.6.2 / arm64, using the
actual published v0.4.8 backend/frontend images and locally built current images
under the test tag `v0.4.9` (dev profile, official runtime stages). Proxy, updater
and Guard were real containers; Guard used its exact image digest and normal
policy. No production data, credentials, registry pushes or live deployment were
involved. Test healthcheck intervals were shortened; service resource limits and
filesystem/network restrictions came from official Compose.

| Fault / transition | Observed result |
| --- | --- |
| Persona stopped / CPU saturated | Homepage HTML, public config and installed TAPP resources all HTTP 200 (27 / 33 requests); maximum 129 / 57 ms |
| Federation stopped / CPU saturated | Same paths all HTTP 200 (27 / 33 requests); maximum 48 / 117 ms |
| Persona / federation memory exhaustion | Each cgroup recorded an OOM kill; 75 requests per case all HTTP 200; maximum 73 / 187 ms |
| Shared PostgreSQL pressure | 12 concurrent CPU-bound SQL queries, each limited to 15 seconds; 39 application requests all HTTP 200, maximum 296 ms |
| Both worker domains stopped | TAPP runtime grant issued; authorized private storage write/read succeeded |
| v0.4.8 → split current topology | Real old database migrated; web, persona and federation independently healthy |
| Current → real v0.4.8 rollback | Actual updater API / Guard restored the database canary and old images; Docker events showed both workers stopped before PostgreSQL; worker containers stayed stopped; homepage/API recovered |
| Current → current rollback | Actual updater / Guard restored the database canary and recreated web plus both workers; all three became healthy |
| Snapshot missing during rollback | Actual restoration error left backend, frontend, both workers and PostgreSQL stopped, with maintenance active; no automatic restart on uncertain data |

The run exposed and fixed two rollback defects: an EROFS rename under the
read-only deployment root now uses the existing staged in-place restore; failed
restoration no longer restarts writers. Guard also accepts official `myriad-*`
service aliases when the Compose project name is customized, without allowing
another service's identity. The real backend image build required copying
`backend/i18n` for compile-time embedded locales.

The upgrade above uses local Compose with prebuilt images; release discovery,
registry/signature verification and TCB self-update are outside this acceptance
run. The traffic checks exercise HTTP and TAPP grants/storage, not browser rendering.
Finite resource-pressure results do not establish an availability guarantee for
arbitrary database locks, shared disk exhaustion, kernel failure or other hosts.

## Release verification

Use an immutable source snapshot when other work is changing the checkout. Build
backend and proxy with their Dockerfiles and `CARGO_PROFILE=ci-release`; the backend
accepts `CARGO_BUILD_JOBS=1` to reduce concurrent compiler memory on small builders.
Build frontend with `docker/Dockerfile.frontend`. Tag the three local images
`myriad-closeout/backend:current`, `myriad-closeout/frontend:current` and
`myriad-closeout/proxy:current`, or pass explicit image arguments to the test.

```sh
cargo clippy -p myriad-backend -- -D warnings
docker build -f docker/mcp-tool/Dockerfile -t myriad-closeout/mcp-base:current .
docker build --build-arg BASE_IMAGE=myriad-closeout/mcp-base:current \
  -t myriad-closeout/mcp-fixture:current docker/mcp-tool/tests
python3 scripts/extra/test-worker-database.py
python3 scripts/extra/test-runtime-upgrade.py
```

The upgrade test starts actual v0.4.8 business images, creates fixture data, then
recreates web/frontend and the two workers with the current images. It verifies
preserved data, worker health and separate database logins, real gateway discovery,
production stdio refusal, TAPP grants/storage with both workers stopped, and MCP
configuration reload/revocation after worker recreation. It creates a unique
Compose project with fake credentials and disposable volumes, removes containers
and volumes in `finally`, and prints the location of redacted logs/results.

This is a host-driven business-image rollout test; it does not exercise the
updater API, Guard replacement, external PostgreSQL failover, or production data.
Those deployment operations retain their separate acceptance requirements.

On 2026-09-12, this release check passed on Docker Engine 29.7.2 / Linux arm64
with an 8 GiB builder: the complete backend/frontend/proxy images, strict backend
Clippy, 47 Agent-rule tests, PostgreSQL budget tests and the old-to-current rollout
above. The rollout exposed an IPv4-only frontend listener paired with a localhost
probe resolving to IPv6. The image and both official Compose files now probe
`127.0.0.1:1102`; the complete rollout passed after that correction. Test volumes
and containers were removed. This validates the recorded source snapshot and
explicit probes, not unrelated edits made concurrently after the snapshot.
