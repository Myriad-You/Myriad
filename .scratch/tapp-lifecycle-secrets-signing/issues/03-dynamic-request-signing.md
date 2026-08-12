# 03 — Dynamic request signing with write-only credentials

**What to build:** Extend `TappApiCredentialBinding` with an optional `signature` block so the host signs declared HTTP API requests (HMAC-SHA256 to start) instead of only static header injection. Orthogonal to `source`: works for per-installation credentials and host secrets (ticket 02).

**Blocked by:** None (contract designed jointly with ticket 02; implement after or in parallel with the shared manifest bump).

**Status:** open

- [ ] Manifest: `credential.signature { algorithm, inputs, output, outputName, timestampName, nonceName }`; `algorithm` enum limited to `hmac-sha256` in `contract_rules`; `inputs` fixed enum (`method/path/query/body/timestamp/nonce`); `output` `header` (default) or `query`; `signature` and `prefix` mutually exclusive.
- [ ] `contract_rules` caps: max signature algorithm name length, max inputs (6), fixed nonce length 32 hex, max output/timestamp/nonce name lengths.
- [ ] Execution in `tapp_api_service`: body serialized before signing (wire bytes are signed bytes); canonical string `METHOD\nURL_PATH\nQUERY(sort by key bytes)\nBODY_BYTES\nTIMESTAMP\nNONCE`; UTC RFC3339 seconds timestamp; 16-byte random nonce hex; HMAC-SHA256 hex lowercase.
- [ ] Injection: `output=header` adds `outputName`/`timestampName`/`nonceName` headers; `output=query` adds them as query parameters.
- [ ] Error contract: missing credential, re-authorization required, invalid signature declaration → stable codes (reuse `TAPP_CREDENTIAL_*` family where applicable).
- [ ] Response redaction unchanged (secret never echoed; signing does not weaken it).
- [ ] Signing vector test against RFC 4231 test cases; canonical-string stability tests (query order, body byte identity, method/path case); both output modes; CLI schema/validation tests; frontend manifest types; `cargo check -p myriad-backend`.

## Comments

Design context in `.scratch/tapp-lifecycle-secrets-signing/spec.md` §3.3. The existing pre-send body serialization (`tapp_api_service.rs` ~line 510) already guarantees "payload hashing/signing aligned with the wire body" — the signing hook plugs into that ordering. Replay protection remains the peer's responsibility; the host provides standard timestamp/nonce semantics only.
