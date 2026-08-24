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

test('stops contributing immediately when preview speech is disabled', () => {
  const speech = new AutoSpeechController(() => 0.5)
  speech.sample(0, true)
  const active = { ...speech.sample(0.3, true) }
  assert.ok(active.mouthOpen > 0)
  assert.deepEqual(
    { ...speech.sample(0.3, false) },
    {
      mouthOpen: 0,
      mouthForm: 0,
      phraseActivity: 0,
      browAccent: 0,
      headAccent: 0,
    },
  )
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
