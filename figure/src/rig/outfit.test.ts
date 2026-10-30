import assert from 'node:assert/strict'
import test from 'node:test'
import { createOutfitProfile, inferOutfitProfileFromPartIds } from './outfit'
import { RIG_OUTFIT_TOPOLOGIES } from './types'

test('owns a stable canonical topology order', () => {
  assert.deepEqual(RIG_OUTFIT_TOPOLOGIES, [
    'fitted',
    'short-skirt',
    'long-skirt',
    'long-coat',
    'wide-sleeve',
    'cape',
    'armor',
  ])
})

test('composes mixed topology safety for torso and secondary layers', () => {
  assert.deepEqual(
    createOutfitProfile(['armor', 'long-skirt', 'wide-sleeve']),
    {
      topologies: ['long-skirt', 'wide-sleeve', 'armor'],
      secondaryPartIds: [],
      torsoTwistScale: 0.62,
      secondaryMotionScale: 0.45,
    },
  )
})

test('infers topology and secondary motion parts from semantic ids', () => {
  assert.deepEqual(
    inferOutfitProfileFromPartIds([
      'body',
      'long-skirt-front',
      'left-wide-sleeve',
      'armor-chest',
    ]),
    {
      topologies: ['long-skirt', 'wide-sleeve', 'armor'],
      secondaryPartIds: ['long-skirt-front', 'left-wide-sleeve'],
      torsoTwistScale: 0.62,
      secondaryMotionScale: 0.45,
    },
  )
})

test('uses fitted defaults and de-duplicates secondary parts', () => {
  assert.deepEqual(createOutfitProfile([], ['front-hair', 'front-hair']), {
    topologies: ['fitted'],
    secondaryPartIds: ['front-hair'],
    torsoTwistScale: 1,
    secondaryMotionScale: 1,
  })
})
