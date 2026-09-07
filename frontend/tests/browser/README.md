# Isolated browser regressions

Run from `frontend`:

```sh
pnpm exec playwright install chromium
pnpm test:browser
```

The test-only Vite entrypoint imports the production recorder hook, utterance
capture, ASR queue, speech pipeline, WebAudio player and speech motion source.
It never becomes an Astro production page and has no backend proxy. All speech
service requests are mocked. The microphone is a synthetic MediaStream, not a
hardware device; no credentials, paid models, or recorded personal speech are
used. Failures retain a local Playwright trace in the ignored `test-results`.

The suite covers ordered ASR completion, late results after stopping, microphone
permission arriving after unmount or a newer listening request, cancelled RTC
startup, and replacement TTS reaching actual WebAudio playback and the speech
behavior producer. Targeted cancellation also preserves the successor's voice
presence. Signal
duration waits are deliberate audio input, not substitutes for waiting for UI
or network conditions.

These tests do **not** measure a real provider's latency, speech recognition
accuracy, acoustic echo cancellation, Agora connectivity, or rendered avatar
appearance. Those need separate opt-in provider and asset scenarios. Passing
the speech producer check is not proof that a particular rig rendered the pose.

## Rig import

`pnpm test:browser rigImport.spec.ts` runs real PSD decoding and compilation in
a browser Worker with OffscreenCanvas. A shared synthetic PSD fixture checks
manifest and decoded PNG pixel parity with the page-side compiler, including
true high collars and independent necklaces. It also checks cancellation during
packing, successful retry, and the page's selected error language. It needs no
backend, login, existing assets, or UI manipulation. This verifies import parity,
not GPU motion quality or the accuracy of generated layer artwork.

The eye fixture also runs the production WebGL player through automatic blink,
deliberate one-/two-eye closure, crying and reopening. It checks the compiled
layer opacity choices and bounded rebound on actual player ticks. This is a
render-path regression using synthetic artwork, not a subjective appearance
review of a user's character.
