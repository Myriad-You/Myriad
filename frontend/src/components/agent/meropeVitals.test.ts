import assert from 'node:assert/strict'
import test from 'node:test'
import { activityKey } from './meropeVitals'

test('activityKey treats unknown and stale labels as idle', () => {
  assert.equal(activityKey('working'), 'working')
  assert.equal(activityKey('thinking'), 'thinking')
  assert.equal(activityKey('talking'), 'talking')
  assert.equal(activityKey('idle'), 'idle')
  assert.equal(activityKey('napping'), 'idle')
  assert.equal(activityKey(undefined), 'idle')
})
