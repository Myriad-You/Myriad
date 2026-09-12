import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  analyticsRangeDayCount,
  analyticsRangeQuery,
  defaultAnalyticsRange,
  defaultCustomRange,
} from './analyticsRangeLogic'

describe('analyticsRangeQuery', () => {
  it('uses days= for presets', () => {
    assert.equal(
      analyticsRangeQuery({ preset: '7', from: '', to: '' }),
      'days=7',
    )
    assert.equal(
      analyticsRangeQuery({ preset: '30', from: 'x', to: 'y' }),
      'days=30',
    )
  })

  it('uses from/to for custom', () => {
    assert.equal(
      analyticsRangeQuery({
        preset: 'custom',
        from: '2026-07-01',
        to: '2026-07-15',
      }),
      'from=2026-07-01&to=2026-07-15',
    )
  })

  it('counts inclusive custom days', () => {
    assert.equal(
      analyticsRangeDayCount({
        preset: 'custom',
        from: '2026-07-01',
        to: '2026-07-01',
      }),
      1,
    )
    assert.equal(
      analyticsRangeDayCount({
        preset: 'custom',
        from: '2026-07-01',
        to: '2026-07-10',
      }),
      10,
    )
    assert.equal(
      analyticsRangeDayCount({ preset: '14', from: '', to: '' }),
      14,
    )
  })

  it('default custom is 7-day window', () => {
    const { from, to } = defaultCustomRange('2026-08-01')
    assert.equal(to, '2026-08-01')
    assert.equal(from, '2026-07-26')
    const d = defaultAnalyticsRange()
    assert.equal(d.preset, '7')
  })
})
