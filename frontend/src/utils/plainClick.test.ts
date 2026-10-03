import assert from 'node:assert/strict'
import { it } from 'node:test'
import { isPlainClick } from './plainClick'

const click = { button: 0, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, defaultPrevented: false }

it('only an unmodified primary click is plain', () => {
  assert.equal(isPlainClick(click), true)
  assert.equal(isPlainClick({ ...click, button: 1 }), false)
  assert.equal(isPlainClick({ ...click, metaKey: true }), false)
  assert.equal(isPlainClick({ ...click, ctrlKey: true }), false)
  assert.equal(isPlainClick({ ...click, shiftKey: true }), false)
  assert.equal(isPlainClick({ ...click, altKey: true }), false)
  assert.equal(isPlainClick({ ...click, defaultPrevented: true }), false)
})
