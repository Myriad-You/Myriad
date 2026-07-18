/**
 * Unit tests for updater check freshness helpers.
 *
 * Run from frontend/:
 *   node --experimental-strip-types --test src/components/config/updaterCheckFreshness.test.ts
 */

/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  AGO_TICK_MS,
  checkAgeSecs,
  computeAgo,
  isCheckStale,
  STALE_WHEN_OFF_SECS,
} from './updaterCheckFreshness.ts'

const NOW = Date.parse('2026-07-18T12:00:00.000Z')

function isoAgo(secs: number): string {
  return new Date(NOW - secs * 1000).toISOString()
}

describe('STALE_WHEN_OFF_SECS / AGO_TICK_MS', () => {
  it('uses 1h stale threshold when auto-check is off', () => {
    assert.equal(STALE_WHEN_OFF_SECS, 3600)
  })
  it('ticks relative time every 30s', () => {
    assert.equal(AGO_TICK_MS, 30_000)
  })
})

describe('checkAgeSecs', () => {
  it('returns null when never checked', () => {
    assert.equal(checkAgeSecs(null, NOW), null)
    assert.equal(checkAgeSecs(undefined, NOW), null)
    assert.equal(checkAgeSecs('', NOW), null)
  })
  it('returns null for invalid timestamps', () => {
    assert.equal(checkAgeSecs('not-a-date', NOW), null)
  })
  it('returns age in seconds', () => {
    assert.equal(checkAgeSecs(isoAgo(90), NOW), 90)
  })
  it('clamps future timestamps to 0', () => {
    assert.equal(checkAgeSecs(new Date(NOW + 5000).toISOString(), NOW), 0)
  })
})

describe('isCheckStale', () => {
  it('treats missing last_checked_at as stale', () => {
    assert.equal(isCheckStale(null, 3600, NOW), true)
    assert.equal(isCheckStale(undefined, 0, NOW), true)
  })

  it('with interval > 0: fresh when age < interval', () => {
    assert.equal(isCheckStale(isoAgo(100), 3600, NOW), false)
    assert.equal(isCheckStale(isoAgo(3599), 3600, NOW), false)
  })

  it('with interval > 0: stale when age >= interval', () => {
    assert.equal(isCheckStale(isoAgo(3600), 3600, NOW), true)
    assert.equal(isCheckStale(isoAgo(7200), 3600, NOW), true)
    assert.equal(isCheckStale(isoAgo(86400), 86400, NOW), true)
  })

  it('with interval === 0 (off): uses STALE_WHEN_OFF_SECS', () => {
    assert.equal(isCheckStale(isoAgo(STALE_WHEN_OFF_SECS - 1), 0, NOW), false)
    assert.equal(isCheckStale(isoAgo(STALE_WHEN_OFF_SECS), 0, NOW), true)
    assert.equal(isCheckStale(isoAgo(STALE_WHEN_OFF_SECS * 24), 0, NOW), true)
  })

  it('treats missing / non-finite interval as off (1h stale rule)', () => {
    assert.equal(isCheckStale(isoAgo(100), undefined, NOW), false)
    assert.equal(isCheckStale(isoAgo(STALE_WHEN_OFF_SECS), null, NOW), true)
    assert.equal(isCheckStale(isoAgo(100), Number.NaN, NOW), false)
  })
})

describe('computeAgo', () => {
  it('returns null for invalid input', () => {
    assert.equal(computeAgo('bad', NOW), null)
  })
  it('just now under 45s', () => {
    assert.deepEqual(computeAgo(isoAgo(0), NOW), { unit: 'justNow' })
    assert.deepEqual(computeAgo(isoAgo(44), NOW), { unit: 'justNow' })
  })
  it('minutes under 60m', () => {
    assert.deepEqual(computeAgo(isoAgo(45), NOW), { unit: 'min', n: 1 })
    assert.deepEqual(computeAgo(isoAgo(90), NOW), { unit: 'min', n: 2 })
    assert.deepEqual(computeAgo(isoAgo(59 * 60), NOW), { unit: 'min', n: 59 })
  })
  it('hours under 24h', () => {
    assert.deepEqual(computeAgo(isoAgo(60 * 60), NOW), { unit: 'hour', n: 1 })
    assert.deepEqual(computeAgo(isoAgo(90 * 60), NOW), { unit: 'hour', n: 2 })
    assert.deepEqual(computeAgo(isoAgo(23 * 3600), NOW), {
      unit: 'hour',
      n: 23,
    })
  })
  it('days at 24h+', () => {
    assert.deepEqual(computeAgo(isoAgo(24 * 3600), NOW), { unit: 'day', n: 1 })
    assert.deepEqual(computeAgo(isoAgo(48 * 3600), NOW), { unit: 'day', n: 2 })
  })
})
