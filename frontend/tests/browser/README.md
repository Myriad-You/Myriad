# Isolated browser regressions

Real backend + Postgres smokes live in `tests/smoke` and are started with
`bash scripts/extra/smoke.sh` from the repo root (`pnpm test:smoke` needs
`MYRIAD_SMOKE_BASE_URL`). They are not this mock-provider Vite fixture.


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

The pixel eye replay uses diagnostic-colored synthetic artwork and the real
importer/player/WebGL shaders at fixed 30 and 60 fps, with autonomous motion
disabled. Every frame checks each iris against an independently rendered eye
white, including invisible whites during fades. Pixel counts distinguish ordinary
blink art, alternate deliberate closure and fallback on the other eye; reopening
must recover the original counts. The replay also covers special-expression
suppression, opposing head/gaze targets, finite vertices, monotonic eyelid motion
and ordinary-art coverage (within one 8-bit alpha level after expression release).
No screenshot baseline, business page, computer use or production debug API is
required. Diagnostic colors and isolated eyes intentionally do not claim coverage
of real artwork aesthetics, hair occlusion or collar/necklace deformation.

If another suite owns port 4179, run with `MYRIAD_BROWSER_TEST_PORT=4188` (or another
free port). The harness starts the installed Vite directly without asking the
package manager to reinstall dependencies.

## Rig body replay

The same suite replays ordinary clothing, a high collar and an open neck with an
independent necklace at 30/60 fps. Physics stays enabled; idle, automatic blinking
and random actions are disabled. Head targets reverse before settling, stop at
the current pose and return to neutral. Checks cover finite transformed vertices,
gross frame jumps/root stretch, immediate pose preservation on target replacement,
and persistent clothing/accessory pixels. The high-collar case must compile a
real aperture mesh and render without GPU errors, including the initial warmup.

Every replay frame also forces a second deformation pass at the same time/pose
with local geometry caches bypassed. Both geometry and rendered pixels must be
unchanged: duplicate deformation cannot hide behind a cache. This caught the
replaced high-collar neck being marked dirty and uploaded without a position
buffer. Its paint layer remains; only its redundant mesh update is skipped.

This is a player/renderer regression, not a director scheduling test. Gross jump
and stretch limits are fault detectors, not naturalness ratings. The colored
synthetic fixtures do not prove absence of tiny seams in every generated asset;
contour/contact and neckwear occlusion also retain their dedicated unit tests.

## Director-to-renderer replay

Two additional scenarios enter through `PerformanceMotionSource.handleForTest`
(the production publisher), `MotionRuntime`, its shared scheduler,
`applyMotionFrame`, the Anime2.5D realizer and the real WebGL player. A test-local
port mirrors the character's imperative bridge; the React mount and model API
are deliberately not involved. A scoped monotonic clock replaces real elapsed
time and is restored on failure as well as success.

The first replaces acknowledgement with delivery after 200ms, checks immediate
pose preservation, prompt expression onset, old-behavior recovery, duplicate
revision suppression, bounded frame changes, final release and changing rendered
pixels. The second supplies the real music source with synthetic audio features
and a manually driven frame callback while text speech and director expression
coexist. It verifies speech-owned mouth movement, music behavior admission,
music-owned head movement after speech, and channel release after stopping.
These are deterministic downstream director tests, not model latency,
provider/music analysis accuracy, React wiring or subjective animation acceptance.

The stream-race scenario uses the public sanitized performance/speech dispatchers
and their real event listeners, including lifecycle generation and cancellation
checks (not `handleForTest`). It appends text, replaces a generation, injects old
director/chunk/cancel events, repeats a plan with a new transport ID, and delivers
more data after cancellation. Assertions check exactly which text reaches the
player, successor ownership, expression output and continuing music. This caught
late chunks reopening a cancelled mouth. The speech lifecycle now remembers up
to 64 cancelled message/utterance keys; an explicit valid start permits replay,
and disposal clears the history. This is bounded in-memory protection, not a
persistent event log or protection against arbitrary historical replays. TTS
settings/UI toggles and provider-side cancellation are not covered here.

## TTS failure and opt-out

`speech.spec.ts` now checks a failed synthesis through the real host and speech
source: the failed segment emits text-mouth events, retains its queue position
for the existing text timing estimate, then yields to real WebAudio playback of
the successor. Cancelling that fallback invalidates its completion callback and
late synthesis results. It does not introduce a second mouth animation engine.

The opt-out test invokes the real status setter while audio is playing and a
successor is queued. Audio/presence stop and the successor does not play. This
does NOT yet preserve queued text on opt-out: `applyStatus` currently cancels it;
only subsequently arriving text can use the caller's disabled-TTS fallback.
Decoder/resume failures also still follow the player's existing end path rather
than this synthesis-failure fallback. These are remaining gaps, not acceptance
claims. Provider abort delivery and the settings UI are outside this harness.
