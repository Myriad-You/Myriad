# 06 — Provide metered speech to guest TAPP sessions

**What to build:** Let a guest-facing TAPP use the existing TTS and ASR SDK methods when explicitly enabled and declared, with server-authoritative anonymous identity, validation, rate limits, quotas, and clear failure responses.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] Explicit guest policy controls exist for `speech:tts` and `speech:asr`, remain independently configurable, and expose effective values to administrators.
- [ ] When enabled and declared, guest Runtime Grants can include TTS and ASR without granting scheduler, AI, network, federation mutation, or other authenticated-only permissions.
- [ ] Guest speech calls are attributed to a server-derived anonymous subject and TAPP identity rather than a client-resettable identifier.
- [ ] TTS and ASR retain the existing input validation and host attribution paths.
- [ ] Anonymous usage is protected by server-authoritative request rate limits and finite usage quotas that cannot be reset by reloading the iframe.
- [ ] Responses distinguish permission denial, invalid input, exhausted rate/quota, and unavailable speech-service configuration.
- [ ] Existing authenticated-user and administrator speech behavior remains backward compatible.
- [ ] Speech output remains subject to the existing package/media and sandbox rules; this ticket does not relax CSP or direct network access.
- [ ] Runtime Grant, anonymous attribution, quota/rate-limit, input validation, error-contract, contract consistency, and CLI declaration tests pass.
- [x] If the existing quota ledger cannot safely represent anonymous speech without a new durable security boundary, implementation stops with evidence and guest speech remains denied.

## Progress

Guest speech remains denied. The speech host-attribution middleware requires authenticated Claims before it validates a Runtime Grant, so it has no anonymous subject derivation path. Speech writes have a server-side 45 requests/minute operation limit, but there is no finite daily speech quota ledger; the existing persistent quota ledger is specific to AI calls/tokens. Enabling the existing guest permission flags would therefore satisfy neither server-derived anonymous attribution nor the non-resettable finite usage quota required by this ticket.

A safe continuation needs a dedicated anonymous speech subject derivation and a persistent per-subject/per-TAPP speech usage ledger (or a generalized quota module) before the permission-service authenticated-subject exclusion can be removed. Until then `guest_perm_speech_tts` and `guest_perm_speech_asr` continue to be forced off and omitted from guest Runtime Grants.
