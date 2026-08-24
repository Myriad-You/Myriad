import assert from 'node:assert/strict'
import test from 'node:test'
import {
  speechArticulationDriverPatch,
  speechEnergyDriverPatch,
} from './speechDriver'

test('keeps authored energy separate from preview speech', () => {
  assert.deepEqual(speechEnergyDriverPatch(null), {
    mouthOpen: 0,
    talk: false,
  })
  assert.deepEqual(speechEnergyDriverPatch(1.4), {
    mouthOpen: 1,
    talk: false,
  })
  assert.deepEqual(speechEnergyDriverPatch(Number.NaN), {
    mouthOpen: 0,
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
    { mouthOpen: 0, mouthForm: 0.12, talk: false },
  )
  assert.deepEqual(
    speechArticulationDriverPatch({
      energy: null,
      viseme: 'wide',
      amount: 0.8,
    }),
    { mouthOpen: 0.8, mouthForm: 0.25, talk: false },
  )
  assert.deepEqual(
    speechArticulationDriverPatch({
      energy: null,
      viseme: 'round',
      amount: 0.5,
    }),
    { mouthOpen: 0.34, mouthForm: -0.2, talk: false },
  )
})
