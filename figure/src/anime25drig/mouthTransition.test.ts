import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import { createAnime25DOpacityFrame, fadeOpacityFromFrame, writeAnime25DOpacityFrame } from './mouthRuntime'
import { MOUTH_HANDOFF_SECONDS, MouthTransitionController } from './mouthTransition'

const PROFILE = { version: 1, source: 'bounds-fallback', silhouettes: [], bridges: [] } as const
const SPEAKING = { mouthOpen: 0.5, mouthWide: 0, mouthRound: 0, mouthNarrow: 0, maniac: 0, mouthSeal: 0, mouthEase: 0 }

test('a passing consonant narrows the vowel art; a held narrow shape swaps its own in', () => {
  const mouth = new MouthTransitionController(PROFILE)
  assert.equal(mouth.sample(SPEAKING).material, 'mouthOpen')
  // A consonant between vowels leans toward narrow without owning the mouth.
  assert.equal(mouth.sample({ ...SPEAKING, mouthNarrow: 0.6 }).material, 'mouthOpen')
  assert.equal(mouth.sample(SPEAKING).material, 'mouthOpen')
  // Held clearly narrow, it does.
  assert.equal(mouth.sample({ ...SPEAKING, mouthNarrow: 0.95 }).material, 'mouthNarrow')
  // Leaving narrow for a vowel stays as quick as any other change.
  assert.equal(mouth.sample({ ...SPEAKING, mouthNarrow: 0.4 }).material, 'mouthOpen')
})

test('lips parting straight into a consonant use the narrow art', () => {
  const mouth = new MouthTransitionController(PROFILE)
  assert.equal(mouth.sample({ ...SPEAKING, mouthOpen: 0 }).material, 'mouthClose')
  assert.equal(mouth.sample({ ...SPEAKING, mouthNarrow: 1 }).material, 'mouthNarrow')
})

test('one drawing hands over to another: the new comes in over the old, then the old goes', () => {
  const mouth = new MouthTransitionController(PROFILE)
  mouth.sample(SPEAKING, 0)
  const switched = mouth.sample({ ...SPEAKING, mouthRound: 1 }, 1)
  assert.equal(switched.material, 'mouthRound')
  assert.equal(switched.previous, 'mouthOpen')
  assert.equal(switched.handoff, 0)
  const frame = createAnime25DOpacityFrame()
  const driver = { ...IDENTITY_DRIVER, mouthOpen: 0.8, mouthRound: 1 }
  for (let step = 0; step <= 10; step++) {
    const time = 1 + (step / 10) * MOUTH_HANDOFF_SECONDS
    const sample = mouth.sample({ ...SPEAKING, mouthRound: 1 }, time)
    writeAnime25DOpacityFrame(frame, driver, sample.material, 1, sample)
    const incoming = fadeOpacityFromFrame({ fade: 'mouthRound', side: null }, frame)
    const outgoing = fadeOpacityFromFrame({ fade: 'mouthOpen', side: null }, frame)
    // Never both see-through: one of them always covers the mouth.
    assert.ok(Math.max(incoming, outgoing) > 0.999, `${step}: ${incoming} ${outgoing}`)
    if (step === 0) assert.equal(incoming, 0)
    if (step === 10) assert.equal(outgoing, 0)
  }
  // Without a clock a switch is still a cut.
  const cut = new MouthTransitionController(PROFILE)
  cut.sample(SPEAKING)
  assert.equal(cut.sample({ ...SPEAKING, mouthRound: 1 }).handoff, 1)
})

test('a switch during a switch hands over from the drawing still fully shown', () => {
  const mouth = new MouthTransitionController(PROFILE)
  mouth.sample(SPEAKING, 0)
  mouth.sample({ ...SPEAKING, mouthRound: 1 }, 1)
  const again = mouth.sample({ ...SPEAKING, mouthWide: 1 }, 1 + MOUTH_HANDOFF_SECONDS * 0.2)
  assert.equal(again.material, 'mouthWide')
  assert.equal(again.previous, 'mouthOpen')
})
