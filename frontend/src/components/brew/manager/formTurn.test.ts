import assert from 'node:assert/strict'
import { it } from 'node:test'
import { FormTurn } from './formTurn'

it('rejects results after the form is abandoned', () => {
  const turn = new FormTurn()
  const first = turn.begin()
  assert.equal(turn.isCurrent(first), true)
  turn.abandon()
  assert.equal(turn.isCurrent(first), false)
  const second = turn.begin()
  assert.equal(turn.isCurrent(second), true)
  assert.equal(turn.isCurrent(first), false)
})
