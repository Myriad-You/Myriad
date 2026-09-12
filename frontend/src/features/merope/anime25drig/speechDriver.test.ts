import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import { bearingDriverPatch } from './performanceExpression'
import {
  speechArticulationDriverPatch,
  speechEnergyDriverPatch,
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
    speechArticulationDriverPatch({ energy: null, viseme: 'rest', amount: 1 }),
    {
      mouthOpen: 0,
      mouthWide: 0,
      mouthRound: 0,
      mouthNarrow: 0,
      mouthSeal: 0,
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
      talk: false,
    },
  )
})

test('speech, singing and musical rest preserve the current negative bearing', () => {
  const driver = { ...IDENTITY_DRIVER }
  for (const expression of ['subdued', 'withdrawn', 'tense'] as const) {
    const bearing = bearingDriverPatch({
      expression,
      posture: 'neutral',
      motionEnergy: 1,
      attention: 0.4,
    })
    Object.assign(driver, bearing)
    for (const viseme of [
      'open',
      'wide',
      'round',
      'narrow',
      'closed',
      'rest',
    ] as const) {
      const articulation = speechArticulationDriverPatch({
        energy: 0.7,
        viseme,
        amount: 0.8,
      })
      assert.equal(Object.hasOwn(articulation, 'mouthForm'), false)
      Object.assign(driver, articulation)
      assert.equal(driver.mouthForm, bearing.mouthForm)
      assert.equal(driver.browAngSym, bearing.browAngSym)
      if (viseme === 'open') assert.ok(driver.mouthOpen > 0.5)
      if (viseme === 'rest') assert.equal(driver.mouthOpen, 0)
    }
  }
  driver.mouthForm = 0.3
  Object.assign(
    driver,
    speechArticulationDriverPatch({ energy: 0.8, viseme: 'open', amount: 1 }),
  )
  assert.equal(driver.mouthForm, 0.3)
})
