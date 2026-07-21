/**
 * Unit tests for updater check freshness + pre-update plan helpers.
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/config/updaterCheckFreshness.test.ts
 */

/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  AGO_TICK_MS,
  checkAgeSecs,
  computeAgo,
  isCheckStale,
  planLatestUpdate,
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

describe('planLatestUpdate', () => {
  const base = {
    currentVersion: 'v0.3.9',
    channelMode: 'release' as const,
    downgradeAvailable: false,
  }

  it('aborts with no_target when tip is missing', () => {
    assert.deepEqual(
      planLatestUpdate({
        ...base,
        available: null,
        latestAvailable: null,
      }),
      { proceed: false, reason: 'no_target' },
    )
    assert.deepEqual(
      planLatestUpdate({
        ...base,
        available: { version: '  ' },
        latestAvailable: undefined,
      }),
      { proceed: false, reason: 'no_target' },
    )
  })

  it('aborts when relation is identical', () => {
    assert.deepEqual(
      planLatestUpdate({
        ...base,
        available: {
          version: 'v0.3.10',
          relation: 'identical',
          is_upgrade: false,
        },
        latestAvailable: null,
      }),
      { proceed: false, reason: 'identical' },
    )
  })

  it('aborts when tip version equals current (case-insensitive)', () => {
    assert.deepEqual(
      planLatestUpdate({
        ...base,
        available: { version: 'V0.3.9', relation: 'ahead' },
        latestAvailable: null,
      }),
      { proceed: false, reason: 'same_version' },
    )
  })

  it('prefers available over status.latest_available for target and relation', () => {
    const plan = planLatestUpdate({
      ...base,
      available: {
        version: 'v0.3.11',
        mode: 'release',
        relation: 'ahead',
        is_upgrade: true,
        is_downgrade: false,
      },
      latestAvailable: {
        version: 'v0.3.10',
        relation: 'diverged',
        is_upgrade: false,
        is_downgrade: false,
      },
    })
    assert.deepEqual(plan, {
      proceed: true,
      target: 'v0.3.11',
      mode: 'release',
      isDowngrade: false,
      needsRisk: false,
    })
  })

  it('falls back to status.latest_available when available is null', () => {
    const plan = planLatestUpdate({
      ...base,
      available: null,
      latestAvailable: {
        version: 'abc1234',
        mode: 'commit',
        relation: 'ahead',
        is_upgrade: true,
      },
      channelMode: 'commit',
    })
    assert.deepEqual(plan, {
      proceed: true,
      target: 'abc1234',
      mode: 'commit',
      isDowngrade: false,
      needsRisk: false,
    })
  })

  it('uses release mode for formal v-tags even when channel is commit', () => {
    const plan = planLatestUpdate({
      ...base,
      available: {
        version: 'v0.3.10',
        mode: 'commit',
        relation: 'ahead',
        is_upgrade: true,
      },
      latestAvailable: null,
      channelMode: 'commit',
    })
    assert.equal(plan.proceed, true)
    if (plan.proceed) {
      assert.equal(plan.mode, 'release')
    }
  })

  it('marks needsRisk for diverged or unknown-without-upgrade', () => {
    const diverged = planLatestUpdate({
      ...base,
      available: {
        version: 'deadbeef',
        mode: 'commit',
        relation: 'diverged',
        is_upgrade: false,
      },
      latestAvailable: null,
      channelMode: 'commit',
    })
    assert.equal(diverged.proceed, true)
    if (diverged.proceed) assert.equal(diverged.needsRisk, true)

    const unknownUpgrade = planLatestUpdate({
      ...base,
      available: {
        version: 'deadbeef',
        mode: 'commit',
        relation: 'unknown',
        is_upgrade: true,
      },
      latestAvailable: null,
      channelMode: 'commit',
    })
    assert.equal(unknownUpgrade.proceed, true)
    if (unknownUpgrade.proceed) assert.equal(unknownUpgrade.needsRisk, false)

    const unknownNoUpgrade = planLatestUpdate({
      ...base,
      available: {
        version: 'deadbeef',
        mode: 'commit',
        relation: 'unknown',
        is_upgrade: false,
      },
      latestAvailable: null,
      channelMode: 'commit',
    })
    assert.equal(unknownNoUpgrade.proceed, true)
    if (unknownNoUpgrade.proceed) {
      assert.equal(unknownNoUpgrade.needsRisk, true)
    }
  })

  it('treats status.downgrade_available as isDowngrade', () => {
    const plan = planLatestUpdate({
      ...base,
      available: {
        version: 'v0.3.8',
        relation: 'behind',
        is_downgrade: false,
      },
      latestAvailable: null,
      downgradeAvailable: true,
    })
    assert.equal(plan.proceed, true)
    if (plan.proceed) assert.equal(plan.isDowngrade, true)
  })
})
