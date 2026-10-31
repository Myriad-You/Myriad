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
It never becomes an production page and has no backend proxy. All speech
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

## Rig body replay

`pnpm test:browser poseCorrectionEditor.spec.ts` mounts the real optional editing
panel and settings components without a backend or character asset. It checks
that the initially collapsed editor does not write a preview, closing restores
saved geometry without losing drafts, and changing outfit identity clears the
old draft. Only the character handle and save boundary are spies; this test does
not claim to verify rendered correction pixels.

`poseCorrectionStage.spec.ts` puts the same editor beside a real stage. With
`MEROPE_POSE_ASSET` (a bust) and/or `MEROPE_POSE_FULL_BODY_ASSET` (a full body)
set to a local `manifest.json` + `atlas.png` directory, it poses the head, adds
a spot, drags it and checks that the drawn point follows the pointer, picks a
second spot on the stage, holds the before view, and saves.

Rig import, player and real-artwork replays live with the figure in
`figure/tests/browser/` (`pnpm test:browser` in `figure/`); see the README there.

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
