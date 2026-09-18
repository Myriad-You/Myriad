# Worker database budgets

Web owns migrations. Persona and federation use different PostgreSQL login roles,
with server-side connection limits of 8 and 4 respectively. Workers do not receive
the migration login's password in official Compose. This protects web's connection
capacity from extra pools or reconnect loops in a worker; it does not make one
PostgreSQL server into independent databases.

## Bundled PostgreSQL

Run the host deployment script when migrating the Compose topology. It generates
independent `PERSONA_DB_PASSWORD` and `FEDERATION_DB_PASSWORD` values in the protected
`.env`. Manual deployments must set both to distinct 32..128-character URL-safe
ASCII secrets (letters, digits, `-`, `_`). Never copy the administrator password.

The web process provisions the reserved `myriad_persona` and `myriad_federation`
roles after schema migrations and before readiness. It grants runtime DML and
sequence access, including default privileges for later migrations. It removes
PUBLIC schema CREATE and database TEMP privileges: temporary tables bypass
`temp_file_limit`, and workers have no schema-management responsibility. Use a
dedicated Myriad database; these changes affect PUBLIC privileges in that database.

| Control | Persona | Federation |
| --- | --- | --- |
| Database connections | 8 | 4 |
| Statement timeout | 30 s | 10 s |
| Total transaction timeout | 35 s | 15 s |
| Lock wait timeout | 3 s | 3 s |
| Idle in transaction timeout | 10 s | 10 s |
| work_mem | 4 MiB per operation | 4 MiB per operation |
| Temporary query files | 64 MiB per PostgreSQL process | 64 MiB per PostgreSQL process |
| Parallel query workers | 0 | 0 |
| Superuser / role creation / replication / schema CREATE / TEMP | Denied | Denied |

Worker startup verifies the login's privilege and budget policy in production.
An administrator login, unbounded connection/temp budget or missing transaction
limit fails closed. PostgreSQL 17+ is required for transaction_timeout; stock
Compose uses PostgreSQL 18. Web has a one-second lock wait limit so a contested
operation does not occupy a web connection indefinitely. Migrations also fail
promptly on a conflicting lock; stop old writers before upgrading.

## External or high-availability PostgreSQL

Use the [external DB Compose](examples/docker-compose.external-db.example.yml).
Set `DATABASE_URL` for the migration/web login, and separate `PERSONA_DATABASE_URL`
and `FEDERATION_DATABASE_URL` values for the bounded worker logins. All three must
point to the same Myriad database/schema. Route them through the provider's stable
writer/failover endpoint; database read replicas cannot serve worker writes or
arbitrary TAPP storage requests.

For a separate DB container, backend and **both** workers must share
`MYRIAD_BACKEND_EXTRA_NETWORK` (default `myriad-backend-ext`) with it, while
retaining `myriad-net`. The external example
declares this as an existing external network. Workers do not inherit backend
network attachments or host mappings. Apply any required `extra_hosts` to all
three services for host PostgreSQL; ensure routed/cloud addresses are reachable
from all three. Verify each worker, not only web. See [network setup and checks](EXTERNAL_POSTGRES.md).

Updater/Guard allow this fixed network only for backend and both workers. Upgrade
both before online updates; older builds reject it. Arbitrary extra networks and
worker access to admin/Guard networks remain denied. `MYRIAD_DB_MODE=external`
changes pgdata handling, not general network authorization.

If the migration login has role-administration rights, the two worker password
variables can provision the reserved roles as in the bundled deployment. Otherwise
ask the database administrator to provision distinct worker accounts with the
budgets and runtime grants above, and omit both password variables from web. Custom
role names are supported through the worker URLs. The database owner must retain
migration/DDL rights and grant access to tables and sequences created later.

Server-enforced connection/TEMP/privilege restrictions supplement client-pool
limits. Statement and memory settings protect against first-party workload bugs;
they are not a claim that a malicious database administrator can be contained.
`work_mem` is per operation and temp_file_limit is per database process, not a
cluster-wide disk quota.

A shared server crash, disk failure, or a lock on data directly required by a web
request remains a common failure. Provider-managed failover/replication and tested
backups are necessary for that layer. Authentication/TAPP writes must fail safely
while the writer is unavailable; serving stale authorization or silently replaying
an ambiguous write is not a valid availability workaround. External mode keeps
physical database backup/restore and failover with its operator; updater performs
image/tag rollback rather than replacing external pgdata.

PostgreSQL documents [role limits](https://www.postgresql.org/docs/18/role-attributes.html),
[resource settings](https://www.postgresql.org/docs/18/runtime-config-resource.html)
and [transaction/lock timeouts](https://www.postgresql.org/docs/18/runtime-config-client.html).

## Verification

Run `python3 scripts/extra/test-worker-database.py` against Docker. It creates and
removes its own PostgreSQL 18 container and executes the provisioning/inspection
SQL from the Rust source with random test credentials. Acceptance on 2026-09-12
confirmed both connection ceilings, denied DDL/TEMP/privilege escalation, the
10-second statement and 15-second transaction deadlines, the 64 MiB temporary
file ceiling, and a one-second web lock deadline while an unrelated read succeeds.
This tests database controls, not a production failover or HTTP load benchmark.
