import assert from 'node:assert/strict'
import test from 'node:test'
import {
  speechArticulationDriverPatch,
  speechEnergyDriverPatch,
  updatedSpeechMouthFormBaseline,
} from './speechDriver'

test('keeps authored energy separate from preview speech', () => {
  assert.deepEqual(speechEnergyDriverPatch(null), {
    mouthOpen: 0,
    mouthWide: 0,
    mouthRound: 0,
    mouthNarrow: 0,
    mouthSeal: 0,
    talk: false,
  })
  assert.deepEqual(speechEnergyDriverPatch(1.4), {
    mouthOpen: 1,
    mouthWide: 0,
    mouthRound: 0,
    mouthNarrow: 0,
    mouthSeal: 0,
    talk: false,
  })
  assert.deepEqual(speechEnergyDriverPatch(Number.NaN), {
    mouthOpen: 0,
    mouthWide: 0,
    mouthRound: 0,
    mouthNarrow: 0,
    mouthSeal: 0,
    talk: false,
  })
})

test('maps authored visemes without enabling random speech', () => {
  assert.deepEqual(
    speechArticulationDriverPatch(
      {
        energy: null,
        viseme: 'rest',
        amount: 1,
      },
      0.12,
    ),
    {
      mouthOpen: 0,
      mouthWide: 0,
      mouthRound: 0,
      mouthNarrow: 0,
      mouthSeal: 0,
      mouthForm: 0.12,
      talk: false,
    },
  )
  assert.deepEqual(
    speechArticulationDriverPatch({
      energy: null,
      viseme: 'wide',
      amount: 0.8,
    }),
    {
      mouthOpen: 0.54 * 0.8,
      mouthWide: 0.8,
      mouthRound: 0,
      mouthNarrow: 0,
      mouthSeal: 0,
      mouthForm: 0,
      talk: false,
    },
  )
  assert.deepEqual(
    speechArticulationDriverPatch({
      energy: null,
      viseme: 'round',
      amount: 0.5,
    }),
    {
      mouthOpen: 0.64 * 0.5,
      mouthWide: 0,
      mouthRound: 0.5,
      mouthNarrow: 0,
      mouthSeal: 0,
      mouthForm: 0,
      talk: false,
    },
  )
})

test('tracks a manual mouth-form edit during authored speech', () => {
  assert.equal(updatedSpeechMouthFormBaseline(0.1, true, -0.35), -0.35)
  assert.equal(updatedSpeechMouthFormBaseline(0.1, false, -0.35), 0.1)
  assert.equal(updatedSpeechMouthFormBaseline(0.1, true, Number.NaN), 0.1)
  assert.equal(updatedSpeechMouthFormBaseline(0.1, true, undefined), 0.1)
})
