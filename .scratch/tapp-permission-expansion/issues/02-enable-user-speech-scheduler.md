# 02 — Enable ordinary-user speech and scheduling by default

**What to build:** Make declared TTS, ASR, and scheduler capabilities work by default for an authenticated ordinary user, while retaining administrator controls, authenticated host attribution, scheduler ownership and interval rules, speech validation, and existing resource safeguards.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] A newly configured installation enables `speech:tts`, `speech:asr`, and `scheduler:register` for authenticated ordinary users by default while keeping them elevated permissions.
- [ ] An administrator can independently disable TTS, ASR, and scheduling, and the next Runtime Grant reflects each disabled capability.
- [ ] Existing installations retain explicitly persisted administrator choices rather than having them overwritten by new defaults.
- [ ] A TAPP receives each permission only when its Manifest declares it.
- [ ] TTS continues validating text, voice, codec, sample rate, speed, volume, and emotion through the existing host path.
- [ ] ASR continues validating audio data, format, engine, and word-information options through the existing host path.
- [ ] Scheduler task registration, ownership, minimum interval, enable/disable, trigger, completion, and teardown behavior remain enforced by the existing server-authoritative scheduler.
- [ ] Ordinary-user TAPPs can complete the existing speech and scheduler SDK flows, including voices/status, TTS, ASR, register/list/get/enable/disable/trigger/unregister, and task callbacks.
- [ ] Guests do not gain speech or scheduling in this ticket; administrators retain all declared capabilities.
- [ ] Shared backend, frontend, host-route fixture, generated contract, generated SDK declaration, and CLI permission-level data remain consistent.
- [ ] Role-based Runtime Grant tests, speech request tests, scheduler lifecycle tests, and CLI declaration tests pass.
