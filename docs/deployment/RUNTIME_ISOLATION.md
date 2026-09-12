# Runtime roles and deployment migration

The bundled and external-PostgreSQL Compose files run federation HTTP, WebSocket
connections and outbound delivery in `federation-worker`. The proxy routes this
domain directly to that process; `MYRIAD_PROCESS_ROLE=web` does not register its
handlers. Both services still use the same backend image/version, with separate
processes and resource budgets. **Persona execution and MCP stdio still run in web;
this is not yet full runtime isolation.**

| Entry | Starts | Intended use |
| --- | --- | --- |
| `MYRIAD_PROCESS_ROLE=web` | Web bootstrap without federation HTTP or delivery | Production web |
| `/app/myriad-federation-worker` or `MYRIAD_PROCESS_ROLE=federation-worker` | Existing-schema check, configuration refresh, federation HTTP/WS, delivery and health | Trusted first-party federation process |
| `MYRIAD_PROCESS_ROLE=all` | Combined runtime | Development only; rejected with `ENVIRONMENT=production` |
| Role unset | Startup error before web bootstrap in every environment | Migrate host topology; local dev explicitly selects `all` |

The dedicated worker executable path takes precedence over role environment values.
It is an alias in new images and absent from old images. Older backend images must
never be launched as workers just by changing an environment variable: they do not
understand the role and would start the entire application.

## Worker boundary

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

PostgreSQL and the underlying storage are still shared with web. Pool and SQL
deadlines limit worker concurrency and waiting; they do not establish database CPU
or disk quotas. Shared-resource pressure and homepage latency need fault/load tests.

The worker admits eight HTTP requests immediately and holds capacity through the
response body, with a 60-second handler deadline and 120-second response deadline.
Proxy federation forwarding has a separate 32-request budget and a 180-second
response deadline. Neither queues unbounded waiters. WebSockets keep their existing
authentication, Origin checks and one-time TAPP tickets; the worker and proxy each
cap them at 64 connections separately from HTTP. Worker WS frames/messages are
limited to 1 MiB. The broadcaster registry and its HTTP/WS producers now share the
federation process. Health is outside the worker's HTTP admission budget.

Web owns migrations and installation-key creation. The worker refuses schema drift,
a missing/invalid installation key and JWT-derived fallback. Deployments overriding
`MYRIAD_DATA_KEY` must supply the same existing key to both trusted processes. The
worker does not generate a separate key. It refreshes durable site origin and DB
configuration every 15 seconds; refresh failure stops the process, allowing its
supervisor to restart it rather than continue indefinitely with stale settings.

Runtime TAPP grant revalidation reads committed permission policy directly from
PostgreSQL on each request, using the existing permission parser/defaults. The
15-second general-config refresh is not a revocation grace period. Failed reads or
invalid policy values reject the grant; host credentials are not read by this query.

`GET /health` reports the worker role, DB readiness, version/commit and federation
gate. Geographic disablement is healthy idle. The delivery task and HTTP/config
refresh tasks are supervised together. SIGTERM cancels an in-flight delivery and
bounds health-server draining to five seconds. Existing token-checked leases remain
recoverable after interruption; remote HTTP delivery is still at-least-once.

## Update and rollback

Migrate the host-owned Compose file and upgrade the proxy/updater/Guard TCB **before**
upgrading the business images. The new proxy can still route to an old backend, so
it can be deployed first. Apply `PROXY_FEDERATION_UPSTREAM=http://federation-worker:1103`
to the running proxy and set backend `MYRIAD_PROCESS_ROLE=web`. The new Guard must
understand the worker command and fixed subpath mounts. Pulling an image does not
migrate Compose. Current binaries reject an unset role in every environment; they no longer
continue combined execution after a warning.

Updater preflight checks the explicit web role, worker topology and storage mounts,
and inspects the running proxy's image capability label
`io.myriad.proxy.federation-routing=1` and routing environment. Failure blocks the
update before maintenance, service stop or snapshot. Existing installations must
complete this migration instead of discovering missing services after downtime.

The updated updater manages the worker alongside web for stop/recreate and rescue
rollback. Before database restore, it proves that the worker has stopped, including
an orphaned worker no longer mentioned by Compose. Inspection failures block the
restore. Image capability label `io.myriad.runtime.federation-worker=1` determines
whether to start it after a tag swap. Rolling back to a pre-role image leaves the
worker stopped and restores that image's combined backend. Health checks compare
actual container image identity and Docker health, without attaching updater to
the worker network.

Proxy refreshes the backend's `/health` routing capability every two seconds. A
recognized full backend reporting `federation_http_isolated=true` uses the worker;
a recognized legacy full backend without the field uses its original routes.
Failed/malformed probes retain the last routing choice (startup defaults to worker).
Worker failure never triggers fallback into a current web process. A tag transition
can briefly return 404/502 until the next capability refresh; end-to-end container
rollback verification remains necessary.

Guard permits only the fixed worker executable, health command, UID, runtime paths,
environment keys, bounded resources, read-only data root, fixed writable subpaths
and business network. This does not permit arbitrary command overrides or mounts.

## Notifications across processes

Federation persona observations use a separate bounded PostgreSQL `NOTIFY` channel,
independent of notification preferences. Only the process owning persona live state
consumes them; federation does not start persona inference or speech ticks. These
observations carry a user ID, whitelisted event kind and capped summary within the
trusted backend/DB boundary. They are ephemeral: a disconnected listener can miss
an observation, with no replay on reconnect (matching their best-effort nature).

Notification INSERT/UPSERT and PostgreSQL `NOTIFY` commit in one SQL statement.
The wakeup carries process/row/user identity, never a notification body. Web reads
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
updater/Cargo.toml --lib`. These tests do not establish container OOM behavior,
end-to-end rollback recovery or homepage latency under federation load. Those need
an actual container-engine fault-injection run.

MCP OS isolation and persona process/state separation remain unfinished. Process
groups and cancellation protect MCP lifecycle but do not prevent filesystem/network
access by third-party children. Persona ticks cannot be moved alone: live state,
API ownership, cancellation and SSE reconnect behavior must move together.
