import assert from 'node:assert/strict'
import test from 'node:test'
import { activityKey, moodBand } from './meropeVitals'

test('moodBand matches Merope circumplex thresholds', () => {
  assert.equal(moodBand(0), 'floor')
  assert.equal(moodBand(10, 48), 'floor')
  assert.equal(moodBand(30, 40), 'sad')
  assert.equal(moodBand(30, 70), 'tense')
  assert.equal(moodBand(70, 48), 'calm')
  assert.equal(moodBand(90, 48), 'calm')
  assert.equal(moodBand(90, 70), 'excited')
  assert.equal(moodBand(undefined), 'calm')
})

test('activityKey treats unknown and stale labels as idle', () => {
  assert.equal(activityKey('working'), 'working')
  assert.equal(activityKey('thinking'), 'thinking')
  assert.equal(activityKey('talking'), 'talking')
  assert.equal(activityKey('idle'), 'idle')
  assert.equal(activityKey('napping'), 'idle')
  assert.equal(activityKey(undefined), 'idle')
})
