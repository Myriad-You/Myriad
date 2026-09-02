import assert from 'node:assert/strict'
import test from 'node:test'
import {
  CHARACTER_ASSET_CONTRACT_VERSION,
  CHARACTER_ASSET_REQUIRED_CAPABILITIES,
  MAX_RIG_BONES,
  MIN_SUPPORTED_RIG_IR_VERSION,
  RIG_IR_VERSION,
  RIG_MATRIX_CAPACITY,
  RIG_PRESENTATION_SLOTS,
} from './contract'

test('keeps one explicit legacy IR readable while new imports use current IR', () => {
  assert.equal(MIN_SUPPORTED_RIG_IR_VERSION, 2)
  assert.equal(RIG_IR_VERSION, 4)
})

test('character imports require independent generated expression variants', () => {
  assert.equal(CHARACTER_ASSET_CONTRACT_VERSION, 13)
  assert.ok(CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('dizzy-eye-variant'))
  assert.ok(
    CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('squeeze-eye-variant'),
  )
  assert.ok(CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('cry-eye-variant'))
  assert.ok(CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('cry-mouth-variant'))
  assert.ok(
    CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('maniac-mouth-variant'),
  )
  assert.ok(CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('silly-eye-variant'))
  assert.ok(
    CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('lovestruck-heart-pupils'),
  )
  assert.ok(
    CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('lovestruck-face-effects'),
  )
  assert.ok(
    CHARACTER_ASSET_REQUIRED_CAPABILITIES.includes('silly-mouth-variant'),
  )
  assert.deepEqual(RIG_PRESENTATION_SLOTS['eye-left'].variants, [
    'open',
    'closed',
    'dizzy',
    'squeeze',
    'cry',
    'silly',
  ])
  assert.deepEqual(RIG_PRESENTATION_SLOTS['eye-right'].variants, [
    'open',
    'closed',
    'dizzy',
    'squeeze',
    'cry',
    'silly',
  ])
  assert.ok(RIG_PRESENTATION_SLOTS.mouth.variants.includes('cry'))
  assert.ok(RIG_PRESENTATION_SLOTS.mouth.variants.includes('wide'))
  assert.ok(RIG_PRESENTATION_SLOTS.mouth.variants.includes('round'))
  assert.ok(RIG_PRESENTATION_SLOTS.mouth.variants.includes('narrow'))
  assert.ok(RIG_PRESENTATION_SLOTS.mouth.variants.includes('maniac'))
  assert.ok(RIG_PRESENTATION_SLOTS.mouth.variants.includes('silly'))
})

test('GPU capacity covers every manifest bone accepted by the contract', () => {
  assert.equal(MAX_RIG_BONES, 48)
  assert.ok(RIG_MATRIX_CAPACITY >= MAX_RIG_BONES)
})
