import assert from 'node:assert/strict'
import test from 'node:test'
import {
  AutoSpeechController,
  speechPhraseAmplitudeScale,
  speechPhraseIntervalScale,
} from './speechMotion'

function seededRandom(initialSeed: number): () => number {
  let seed = initialSeed >>> 0
  return () => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
    return seed / 0x1_0000_0000
  }
}

test('shapes phrase onset, center, and ending without exceeding unity', () => {
  const onset = speechPhraseAmplitudeScale(0)
  const center = speechPhraseAmplitudeScale(0.5)
  const ending = speechPhraseAmplitudeScale(1)
  assert.ok(onset < center)
  assert.ok(ending < onset)
  assert.ok(center <= 1)
  assert.equal(speechPhraseAmplitudeScale(Number.NaN), onset)
})

test('lengthens only the phrase ending', () => {
  assert.equal(speechPhraseIntervalScale(0), 1)
  assert.equal(speechPhraseIntervalScale(0.5), 1)
  assert.ok(speechPhraseIntervalScale(0.9) > 1)
  assert.equal(speechPhraseIntervalScale(1), 1.18)
})

test('starts neutral and builds the first syllable without a hard jump', () => {
  const speech = new AutoSpeechController(() => 0.5)
  assert.deepEqual(
    { ...speech.sample(0, true) },
    {
      mouthOpen: 0,
      mouthWide: 0,
      mouthRound: 0,
      mouthNarrow: 0,
      mouthSeal: 0,
      mouthForm: 0,
      phraseActivity: 0,
      browAccent: 0,
      headAccent: 0,
    },
  )
  assert.deepEqual(
    { ...speech.sample(0.11, true) },
    {
      mouthOpen: 0,
      mouthWide: 0,
      mouthRound: 0,
      mouthNarrow: 0,
      mouthSeal: 0,
      mouthForm: 0,
      phraseActivity: 0,
      browAccent: 0,
      headAccent: 0,
    },
  )
  const transitionStart = { ...speech.sample(0.13, true) }
  assert.ok(transitionStart.mouthOpen < 0.01)
  const opening = { ...speech.sample(0.17, true) }
  assert.ok(opening.mouthOpen > transitionStart.mouthOpen)
  assert.ok(opening.mouthOpen < 0.34)
})

test('keeps preview speech bounded and frame-continuous', () => {
  const speech = new AutoSpeechController(seededRandom(0x1234_5678))
  let previous = { ...speech.sample(0, true) }
  let largestOpenStep = 0
  let peakOpen = 0
  let closedFrames = 0
  for (let frame = 1; frame <= 60 * 45; frame += 1) {
    const current = { ...speech.sample(frame / 60, true) }
    largestOpenStep = Math.max(
      largestOpenStep,
      Math.abs(current.mouthOpen - previous.mouthOpen),
    )
    peakOpen = Math.max(peakOpen, current.mouthOpen)
    if (current.mouthOpen < 0.001) closedFrames += 1
    assert.ok(current.mouthOpen >= 0)
    assert.ok(current.mouthOpen <= 0.8)
    assert.ok(Math.abs(current.mouthForm) <= 0.11)
    assert.ok(current.phraseActivity >= 0 && current.phraseActivity <= 1)
    assert.ok(current.browAccent >= 0 && current.browAccent <= 1)
    assert.ok(current.headAccent >= 0 && current.headAccent <= 1)
    previous = current
  }
  assert.ok(peakOpen > 0.6)
  assert.ok(closedFrames > 60)
  assert.ok(largestOpenStep < 0.16)
})

test('clearing speech drops queued visemes but keeps the current mouth for a rest release', () => {
  const speech = new AutoSpeechController(() => 0.5)
  speech.sample(0, true)
  const active = { ...speech.sample(0.3, true) }
  assert.ok(active.mouthOpen > 0)
  speech.clear(0.3)
  const onset = { ...speech.sample(0.3, false) }
  assert.ok(onset.mouthOpen > active.mouthOpen * 0.85)
  const rest = { ...speech.sample(0.52, false) }
  assert.ok(rest.mouthOpen < 1e-6)
})

test('predicts a quiet mouth response while text visemes are still compiling', async () => {
  let resolveCompilation:
    | ((
        cues: Array<{ viseme: 'round'; duration: number; emphasis: boolean }>,
      ) => void)
    | undefined
  const compilation = new Promise<
    Array<{ viseme: 'round'; duration: number; emphasis: boolean }>
  >((resolve) => {
    resolveCompilation = resolve
  })
  const speech = new AutoSpeechController(
    () => 0.5,
    () => compilation,
  )
  speech.sample(0, true)
  speech.enqueueText('你好', 'zh-CN')

  const requestFrame = { ...speech.sample(0.001, true) }
  const predicted = { ...speech.sample(0.045, true) }
  assert.ok(requestFrame.mouthOpen < 0.01)
  assert.ok(predicted.mouthOpen > 0.02)
  assert.ok(predicted.mouthOpen < 0.46)
  assert.equal(predicted.browAccent, 0)
  assert.equal(predicted.headAccent, 0)

  resolveCompilation?.([{ viseme: 'round', duration: 0.2, emphasis: false }])
  await new Promise<void>((resolve) => setImmediate(resolve))
  const handoff = { ...speech.sample(0.08, true) }
  const authoritative = { ...speech.sample(0.13, true) }
  assert.ok(Math.abs(handoff.mouthOpen - predicted.mouthOpen) < 0.55)
  assert.ok(authoritative.mouthRound > 0.5)
})

test('rests at sentence boundaries and eases into the next phrase', async () => {
  const speech = new AutoSpeechController(
    () => 0.5,
    async () => [
      { viseme: 'open', duration: 0.2, emphasis: true },
      { viseme: 'rest', duration: 0.32, emphasis: false },
      { viseme: 'wide', duration: 0.2, emphasis: true },
    ],
  )
  speech.sample(0, true)
  speech.enqueueText('一句。下一句', 'zh-CN')
  await new Promise<void>((resolve) => setImmediate(resolve))

  speech.sample(0.001, true)
  const firstPhrase = { ...speech.sample(0.12, true) }
  const sentencePause = { ...speech.sample(0.36, true) }
  const restart = { ...speech.sample(0.521, true) }
  const resumed = { ...speech.sample(0.61, true) }

  assert.ok(firstPhrase.phraseActivity > 0.8)
  assert.equal(sentencePause.phraseActivity, 0)
  assert.ok(sentencePause.mouthOpen < 0.01)
  assert.ok(restart.phraseActivity < 0.01)
  assert.ok(resumed.phraseActivity > restart.phraseActivity)
  assert.ok(resumed.phraseActivity < 1)
  assert.ok(resumed.mouthWide > 0.5)
})

test('varies text-only phrase pace instead of replaying a fixed metronome', async () => {
  const cues = [
    { viseme: 'open' as const, duration: 0.2, emphasis: false },
    { viseme: 'wide' as const, duration: 0.2, emphasis: false },
    { viseme: 'round' as const, duration: 0.2, emphasis: false },
    { viseme: 'rest' as const, duration: 0.32, emphasis: false },
  ]
  const fast = new AutoSpeechController(
    () => 0,
    async () => cues,
  )
  const slow = new AutoSpeechController(
    () => 1,
    async () => cues,
  )
  for (const speech of [fast, slow]) {
    speech.sample(0, true)
    speech.enqueueText('同一句话。', 'zh-CN')
  }
  await new Promise<void>((resolve) => setImmediate(resolve))
  fast.sample(0.001, true)
  slow.sample(0.001, true)

  const fastSecondCue = { ...fast.sample(0.18, true) }
  const slowFirstCue = { ...slow.sample(0.18, true) }
  assert.ok(fastSecondCue.mouthWide > 0.8)
  assert.ok(slowFirstCue.mouthWide < 0.1)

  const fastFinished = { ...fast.sample(0.64, true) }
  const slowStillSpeaking = { ...slow.sample(0.64, true) }
  assert.ok(fastFinished.mouthRound < 0.01)
  assert.ok(slowStillSpeaking.mouthRound > 0.8)
})

test('keeps one rhythm across streaming chunks without inventing a phrase break', async () => {
  let randomCalls = 0
  const speech = new AutoSpeechController(
    () => (randomCalls++ < 4 ? 1 : 0),
    async (text) => [
      {
        viseme: text === '前' ? ('open' as const) : ('wide' as const),
        duration: 0.2,
        emphasis: false,
      },
    ],
  )
  speech.sample(0, true)
  speech.enqueueText('前', 'zh-CN')
  await new Promise<void>((resolve) => setImmediate(resolve))
  speech.sample(0.001, true)
  speech.sample(0.3, true)

  // The first chunk has drained, but no punctuation ended its phrase. The
  // second chunk must inherit its slow phrase curve rather than sample a new,
  // fast one from token arrival timing.
  speech.enqueueText('后', 'zh-CN')
  await new Promise<void>((resolve) => setImmediate(resolve))
  speech.sample(0.301, true)
  const continuation = { ...speech.sample(0.48, true) }
  assert.ok(continuation.mouthWide > 0.5)
})

test('does not revive a pending text prediction after speech is cleared', async () => {
  let resolveCompilation:
    | ((
        cues: Array<{ viseme: 'open'; duration: number; emphasis: boolean }>,
      ) => void)
    | undefined
  const compilation = new Promise<
    Array<{ viseme: 'open'; duration: number; emphasis: boolean }>
  >((resolve) => {
    resolveCompilation = resolve
  })
  const speech = new AutoSpeechController(
    () => 0.5,
    () => compilation,
  )
  speech.sample(0, true)
  speech.enqueueText('稍后到达', 'zh-CN')
  speech.sample(0.06, true)
  speech.clear(0.06)
  resolveCompilation?.([{ viseme: 'open', duration: 0.2, emphasis: false }])
  await new Promise<void>((resolve) => setImmediate(resolve))

  const rest = { ...speech.sample(0.3, false) }
  assert.ok(rest.mouthOpen < 1e-6)
  assert.equal(rest.phraseActivity, 0)
})

test('releases to rest without a hard cut when preview speech is disabled', () => {
  const speech = new AutoSpeechController(() => 0.5)
  speech.sample(0, true)
  const active = { ...speech.sample(0.3, true) }
  assert.ok(active.mouthOpen > 0)
  const onset = { ...speech.sample(0.3, false) }
  assert.ok(onset.mouthOpen > active.mouthOpen * 0.85)
  assert.ok(onset.phraseActivity > active.phraseActivity * 0.85)
  const mid = { ...speech.sample(0.4, false) }
  assert.ok(mid.mouthOpen < onset.mouthOpen)
  assert.ok(mid.phraseActivity < onset.phraseActivity)
  const rest = { ...speech.sample(0.52, false) }
  assert.ok(rest.mouthOpen < 1e-6)
  assert.ok(rest.phraseActivity < 1e-6)
  assert.ok(rest.browAccent < 1e-6)
  assert.ok(rest.headAccent < 1e-6)
})

test('leads an emphasized syllable with the brow before the head nod', () => {
  const speech = new AutoSpeechController(() => 0)
  speech.sample(0, true)
  speech.sample(0.08, true)
  const anticipation = { ...speech.sample(0.12, true) }
  const followingNod = { ...speech.sample(0.16, true) }

  assert.ok(anticipation.phraseActivity > 0)
  assert.ok(anticipation.browAccent > 0.4)
  assert.equal(anticipation.headAccent, 0)
  assert.ok(followingNod.headAccent > 0)
})

test('reuses its result object and remains time-based at 30 and 60 fps', () => {
  const reused = new AutoSpeechController(() => 0.5)
  assert.equal(reused.sample(0, true), reused.sample(0.01, true))

  const at30 = new AutoSpeechController(() => 0.5)
  const at60 = new AutoSpeechController(() => 0.5)
  let pose30 = { ...at30.sample(0, true) }
  let pose60 = { ...at60.sample(0, true) }
  for (let frame = 1; frame <= 30 * 12; frame += 1) {
    pose30 = { ...at30.sample(frame / 30, true) }
  }
  for (let frame = 1; frame <= 60 * 12; frame += 1) {
    pose60 = { ...at60.sample(frame / 60, true) }
  }
  assert.ok(Math.abs(pose30.mouthOpen - pose60.mouthOpen) < 0.01)
  assert.ok(Math.abs(pose30.mouthForm - pose60.mouthForm) < 0.01)
  assert.ok(Math.abs(pose30.phraseActivity - pose60.phraseActivity) < 0.01)
  assert.ok(Math.abs(pose30.browAccent - pose60.browAccent) < 0.01)
  assert.ok(Math.abs(pose30.headAccent - pose60.headAccent) < 0.01)
})
