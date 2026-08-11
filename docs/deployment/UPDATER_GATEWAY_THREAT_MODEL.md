# Updater gateway capability threat model

## Asset and trust boundary

`UPDATE_TOKEN` authorizes the updater API to mutate the deployed Myriad stack.
Only `updater-gateway` and `updater` hold it. The backend holds the distinct
`UPDATER_GATEWAY_SECRET`; an authenticated Myriad administrator can ask the
backend to perform the product's update operations, but cannot supply an
updater token or an arbitrary upstream request.

The gateway trusts the backend secret for caller identity. It does **not** trust
the caller's HTTP method, path, query, headers, or JSON body. Admin-network
membership and source-address rate limiting are defense in depth, not
authorization.

## Exposed capabilities

The gateway exposes only the methods and paths used by
`backend/src/api/updater_admin.rs`:

- Read: status, available versions/builds/releases/commits, comparison, jobs,
  snapshots, diagnostics, and the last self-update result.
- Mutate: trigger update, change updater preferences, rollback, delete a named
  snapshot, the three fixed rescue actions, and the two fixed TCB/proxy update
  actions.

Each route is registered explicitly. Dynamic job and snapshot identifiers are
single bounded `[A-Za-z0-9_-]+` segments. Queries use per-route field lists and
bounded values. Mutative JSON bodies use per-route field and type schemas.
Only the gateway secret and the specific audit/idempotency/risk headers needed
by a capability are accepted; only a smaller fixed set is forwarded.

There is no arbitrary-method or fallback proxy. Unknown routes, encoded path
traversal, method changes, duplicate/unknown query fields, unexpected headers,
unknown JSON fields, wrong types, and bodies on bodyless operations are rejected
before `X-Update-Token` is attached.

## Failure and compromise consequences

- A leaked backend gateway secret permits only the explicit capabilities above;
  it cannot turn the gateway into a token-bearing general HTTP client.
- A compromised backend administrator can still perform the update actions the
  product intentionally grants to administrators. This gateway does not narrow
  authorization within that product role.
- A compromised gateway or updater has the updater token and remains inside the
  updater TCB. Docker Guard policy, network isolation, signed image checks, and
  host recovery remain the next boundaries.
- Availability failures fail closed: validation errors never contact upstream;
  upstream transport failures return `502` without exposing credentials.

When adding an updater UI/backend operation, maintainers must add its exact
route and schema to the gateway in the same change. A generic fallback is not a
compatibility mechanism.
