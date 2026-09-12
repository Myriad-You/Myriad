# Touch semantic evaluation

Run from the repository root:

```sh
node scripts/test-merope-semantics.mjs --kind touch --export
node scripts/test-merope-semantics.mjs --kind touch --live
node scripts/test-merope-semantics.mjs --kind touch --replay /absolute/path/to/reviewed-report.json
```

Live reuses the configured Lite model through the existing read-only acceptance
loader, from `backend/.env` and the site configuration. No separate API key is
needed. Credentials stay in the backend test process, never fixtures or reports.
The loader verifies PostgreSQL read-only mode and refuses to create a host data key.
It does not alter persona, mood, memory, playback or chats.
Twelve synthetic appraisal cases mean at most twelve logical model calls per round.

Completed-contact replies and their generated director requests:

```sh
node scripts/test-merope-semantics.mjs --kind touch-response --live --repeat 3
```

Six scenarios per round: three eligible reactions and three suppressed cases
(talking, expired, do-not-disturb). Speak/ask outputs feed the production motion
contract as the actual response text; ignore never launches a director. At most
six model calls per round, 18 for three rounds. These are synthetic stage tests,
not an authenticated UI-to-render or network delivery latency measurement.
Replay the same kind and repeat count; dependent director requests are hashed too.

The event schema is narrowed by the production event kind: touch offers only
ignore/speak/ask, with null memory and work proposal. The dependent motion stage
uses Delivery, matching asynchronous refinement after live speech is published.
Do not sum reply and director model times as time-to-first-speech: the live path
no longer waits for the director. Conversely, a valid director response does not
prove it affected the face: the client accepts it only while the matching message
is still playing. Short replies may finish before the director returns; late
results are intentionally discarded rather than replaying or reviving the line.

If calls fail at the production deadline, one bounded diagnostic can use
`MEROPE_SEMANTIC_DIAGNOSTIC_SECONDS=15`. This changes only the test transport
deadline. `withinRequestBudget=false` must still prevent a complete pass even
if the late response is semantically correct. Do not report this as a latency fix.

For configured OpenRouter latency diagnosis only, `MEROPE_SEMANTIC_PROBE=default`
or `disabled` reuses the existing single-request probe (2048 output tokens, no
temperature override, no retry). Both groups use the same output budget; only
the reasoning parameter differs. This is not the unbounded production request.
The policy participates in replay hashes; replay must use the same environment.
`probeObservation` records headers/body timing, HTTP status, finish reason and
usage when available, never credentials or reasoning text. Non-streaming headers
may arrive after generation: these timings cannot separate network transit from
provider queueing or inference. Missing observations remain unknown, not zero.

Requests use the production touch contract and parser. Allowed reaction sets
are engineering rubrics, not scientifically established human ground truth.
Passing the allowed set still requires review: add `review` containing
`verdict` (`pass` or `fail`), a literal output `evidence`, and a case-specific
`reason`, then replay. Hashes reject reports made against older requests or
rubrics. Export, transport success and valid JSON are not semantic passes.

Touch calls have a 2-second model deadline. Report latency excludes production
authentication, state lookup, network-to-app and rendering. `remainingMs` is a
synthetic contact-lifetime assumption, never exposed to the model. Timely rate
is measured only where latency exists; export rates stay null. Replay preserves
the original reported latency rather than measuring local JSON parsing.

Repeated samples test independent-call stability, not remembered dialogue:
`displayedReaction` supplies the last rendered response where observed, not an
unapplied model decision. `differsFromLocal` is
only a semantic disagreement, not evidence of better visible acting. Real
renderer integration is covered separately in `frontend/tests/browser/rigImport.spec.ts`;
the semantic report deliberately leaves `visibleImprovement` null.

## Touch-to-speech lifecycle regression

From `frontend`, run the existing suites together:

```sh
pnpm exec tsx --test src/features/merope/interaction/*.test.ts src/features/merope/motion/touchSource.test.ts src/features/merope/body/*.test.ts src/features/merope/faceSpeechArbitration.test.ts src/features/merope/anime25drig/touchCoordination.test.ts src/features/merope/speechLifecycle.test.ts
```

The sequence tests sample accept/hesitate/withdraw at 30/60/120 FPS through
release, completed-contact evidence, delayed speech, finish and recovery.
They check single-use evidence, new-contact invalidation, no repeated orienting
stroke, no virtual caress after release, and speech-only cleanup notification.
The adapter tests check that late direction cannot restart speech or target a
different/completed utterance. Contradictory playful refinements are rejected
only during the matching hesitant/withdrawn touch continuation; ordinary speech
and accepted contact remain unaffected.

These tests establish lifecycle and bounded semantic invariants, not subjective
naturalness, exhaustive semantic correctness or measured user-visible latency.
They do not replace the model-result review above or claim a visual acceptance.
