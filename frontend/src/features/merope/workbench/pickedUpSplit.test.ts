import type { TurnKeyformsAsked } from '../turnKeyformsApi'
import assert from 'node:assert/strict'
import test from 'node:test'
import { besideOfPickedUp, boxesOfPickedUp } from './pickedUpSplit'

function asked(patch: Partial<TurnKeyformsAsked>): TurnKeyformsAsked {
  return { head: true, body: false, plain: false, fromArchive: null, expressions: null, arms: null, ...patch }
}

test("a picked-up split's result goes in with what its own page chose, not what this page's boxes say", () => {
  // After a reload every box is ticked again; the job was asked without gesture arms.
  const boxes = { expressions: true, arms: true }
  assert.deepEqual(besideOfPickedUp(asked({ expressions: true, arms: false }), boxes), { expressions: true, arms: false })
  assert.deepEqual(besideOfPickedUp(asked({ expressions: false, arms: true }), { expressions: true, arms: false }), { expressions: false, arms: true })
  // A job that kept no choice, or a status without what it was asked: the boxes.
  assert.deepEqual(besideOfPickedUp(asked({}), boxes), boxes)
  assert.deepEqual(besideOfPickedUp(undefined, { expressions: false, arms: true }), { expressions: false, arms: true })
})

test('the boxes show what the picked-up job was asked for', () => {
  // A figure keyed by its body alone.
  assert.deepEqual(
    boxesOfPickedUp(asked({ head: false, body: true, expressions: false, arms: true })),
    { turn: false, body: true, expressions: false, arms: true },
  )
  // The bust split once for its expressions.
  assert.deepEqual(
    boxesOfPickedUp(asked({ head: false, plain: true, expressions: true, arms: false })),
    { turn: false, body: false, expressions: true, arms: false },
  )
  // A kept job fitted again says nothing of the parts; a choice not kept leaves its box.
  assert.deepEqual(
    boxesOfPickedUp(asked({ head: false, fromArchive: { id: 'kept-1', refit: true }, arms: true })),
    { turn: null, body: null, expressions: null, arms: true },
  )
})
