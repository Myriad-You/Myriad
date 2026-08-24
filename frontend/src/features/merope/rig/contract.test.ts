import assert from 'node:assert/strict'
import test from 'node:test'
import {
  MAX_RIG_BONES,
  MIN_SUPPORTED_RIG_IR_VERSION,
  RIG_IR_VERSION,
  RIG_MATRIX_CAPACITY,
} from './contract'

test('keeps one explicit legacy IR readable while new imports use current IR', () => {
  assert.equal(MIN_SUPPORTED_RIG_IR_VERSION, 2)
  assert.equal(RIG_IR_VERSION, 4)
})

test('GPU capacity covers every manifest bone accepted by the contract', () => {
  assert.equal(MAX_RIG_BONES, 48)
  assert.ok(RIG_MATRIX_CAPACITY >= MAX_RIG_BONES)
})
