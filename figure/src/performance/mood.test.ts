import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { coreFile } from '../testing/coreFile.test-support'
import { moodBand } from './mood'

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

test('moodBand thresholds stay aligned with backend mood_band source', () => {
  const backend = readFileSync(
    coreFile('crates/myriad-merope/src/affect.rs'),
    'utf8',
  )
  const frontend = readFileSync(new URL('./mood.ts', import.meta.url), 'utf8')
  const backendBody = backend.match(/pub fn mood_band\([\s\S]*?\n\}/)?.[0]
  const frontendBody = frontend.match(/export function moodBand\([\s\S]*?\n\}/)?.[0]
  const floor = backend.match(/pub const MOOD_FLOOR:\s*f64\s*=\s*(\d+(?:\.\d+)?);/)
  assert.ok(backendBody, 'backend mood_band must be found')
  assert.ok(frontendBody, 'frontend moodBand must be found')
  assert.ok(floor, 'backend MOOD_FLOOR must be found')

  const backendThresholds = Iterator.from(
    backendBody.matchAll(
      /\b(mood|arousal)\s*(<=|<)\s*(MOOD_FLOOR|\d+(?:\.\d+)?)/g,
    ),
  )
    .map(([, axis, operator, value]) => ({
      axis,
      operator,
      value: Number(value === 'MOOD_FLOOR' ? floor[1] : value),
    }))
    .toArray()
  const frontendThresholds = Iterator.from(
    frontendBody.matchAll(/\b(v|a)\s*(<=|<)\s*(\d+(?:\.\d+)?)/g),
  )
    .map(([, axis, operator, value]) => ({
      axis: axis === 'v' ? 'mood' : 'arousal',
      operator,
      value: Number(value),
    }))
    .toArray()

  assert.equal(backendThresholds.length, 5, 'expected all backend band comparisons')
  assert.equal(frontendThresholds.length, 5, 'expected all frontend band comparisons')
  assert.deepEqual(frontendThresholds, backendThresholds)
})
