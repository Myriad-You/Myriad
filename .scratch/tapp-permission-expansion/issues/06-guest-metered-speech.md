# 06 — Provide metered speech to guest TAPP sessions

**What to build:** Let a guest-facing TAPP use the existing TTS and ASR SDK methods when explicitly enabled and declared, with server-authoritative anonymous identity, validation, rate limits, quotas, and clear failure responses.

**Blocked by:** None — can start immediately.

**Status:** resolved

- [x] Explicit guest policy controls exist for `speech:tts` and `speech:asr`, remain independently configurable, and expose effective values to administrators.
- [x] When enabled and declared, guest Runtime Grants can include TTS and ASR without granting scheduler, AI, network, federation mutation, or other authenticated-only permissions.
- [x] Guest speech calls are attributed to the signed guest subject, validated Runtime Grant owner/TAPP identity, and HMAC-IP/site aggregate buckets rather than a client-resettable iframe identifier.
- [x] TTS and ASR retain the existing input validation and host attribution paths; guest ASR URL input is rejected to avoid anonymous provider-side URL fetching.
- [x] Anonymous usage is protected by server-authoritative per-subject/TAPP rate limits, HMAC-IP short-window limits, and finite daily quota ledger buckets that cannot be reset by reloading the iframe or replacing a guest cookie alone.
- [x] Responses distinguish permission denial, invalid input, exhausted rate/quota, and unavailable speech-service configuration with stable error codes.
- [x] Existing authenticated-user and administrator speech behavior remains backward compatible; authenticated requests bypass guest quota accounting.
- [x] Speech output remains subject to the existing package/media and sandbox rules; this ticket does not relax CSP or direct network access.
- [x] Runtime Grant, anonymous attribution, quota/rate-limit, input validation, error-contract, contract consistency, and CLI declaration tests pass.
- [x] If the existing quota ledger cannot safely represent anonymous speech without a new durable security boundary, implementation stops with evidence and guest speech remains denied. The existing durable `tapp_quota_usage` table safely represents the required speech buckets, so implementation proceeded.

## Progress

Implemented in commit `29485b2f` (`feat(tapp): enable metered guest speech`). Guest speech now uses the existing optional-auth middleware and signed guest session Claims, then validates the Runtime Grant before entering the handler. The permission service re-evaluates the current role/config and Manifest-approved permissions on every grant validation, so enabling `guest_perm_speech_tts` or `guest_perm_speech_asr` remains explicit and independently configurable.

Daily usage is server-authoritative in the existing `tapp_quota_usage` ledger. Each guest call reserves session/TAPP, HMAC-IP/TAPP, and site-owner aggregate buckets transactionally; the original IP is never stored. The existing `speech.tts`/`speech.asr` 45-per-60-second host limiter remains active, and guest calls additionally use an HMAC-IP limiter. Batch TTS is denied to guests because it is not part of the guest SDK surface and would otherwise bypass the single-call quota path. Guest ASR accepts only base64 audio; authenticated URL-based ASR remains unchanged.

Validation evidence: `cargo check -p myriad-backend`; permission-service 15/15; host-attribution 19/19; speech quota 3/3; rate-limit 7/7; configuration payload test 1/1; frontend `pnpm exec tsc --noEmit`; CLI `npm test` 47/47; `git diff --check`. Browser-level manual QA was not performed.
