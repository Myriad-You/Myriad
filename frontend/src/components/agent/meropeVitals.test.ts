import assert from 'node:assert/strict'
import test from 'node:test'
import {
  activityKey,
  formatVitalsLine,
  moodBand,
} from './meropeVitals'

const copy = {
  mood: {
    floor: '很低',
    low: '偏低',
    normal: '平常',
    high: '轻松',
  },
  moodLine: '心情{band}',
  activity: {
    idle: '空闲',
    working: '在办事',
    thinking: '在想',
    talking: '在聊',
  },
}

test('moodBand matches Merope tone thresholds', () => {
  assert.equal(moodBand(0), 'floor')
  assert.equal(moodBand(10), 'floor')
  assert.equal(moodBand(39.9), 'low')
  assert.equal(moodBand(40), 'normal')
  assert.equal(moodBand(70), 'normal')
  assert.equal(moodBand(84.9), 'normal')
  assert.equal(moodBand(85), 'high')
  assert.equal(moodBand(undefined), 'normal')
})

test('activityKey treats unknown and stale labels as idle', () => {
  assert.equal(activityKey('working'), 'working')
  assert.equal(activityKey('thinking'), 'thinking')
  assert.equal(activityKey('talking'), 'talking')
  assert.equal(activityKey('idle'), 'idle')
  assert.equal(activityKey('napping'), 'idle')
  assert.equal(activityKey(undefined), 'idle')
})

test('formatVitalsLine joins band and activity', () => {
  assert.equal(formatVitalsLine(copy, 72, 'idle'), '心情平常 · 空闲')
  assert.equal(formatVitalsLine(copy, 8, 'working'), '心情很低 · 在办事')
})
