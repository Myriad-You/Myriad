/**
 *   pnpm exec tsx --test src/components/config/analytics/analyticsBackupFilenameDay.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { analyticsBackupFilenameDay } from './format.ts'

describe('analyticsBackupFilenameDay', () => {
  it('uses exported_at in IANA timezone (not browser UTC alone)', () => {
    // 2026-07-31T16:30:00Z → 2026-08-01 in Asia/Shanghai (UTC+8)
    const day = analyticsBackupFilenameDay({
      timezone: 'Asia/Shanghai',
      exported_at: '2026-07-31T16:30:00.000Z',
    })
    assert.equal(day, '2026-08-01')
  })

  it('uses UTC±N offset labels from analytics_tz_label', () => {
    const day = analyticsBackupFilenameDay({
      timezone: 'UTC+8',
      exported_at: '2026-07-31T16:30:00.000Z',
    })
    assert.equal(day, '2026-08-01')
  })

  it('uses bare UTC as UTC calendar day', () => {
    const day = analyticsBackupFilenameDay({
      timezone: 'UTC',
      exported_at: '2026-07-31T16:30:00.000Z',
    })
    assert.equal(day, '2026-07-31')
  })

  it('prefers bucket_today over payload max day and timezone formatting', () => {
    const day = analyticsBackupFilenameDay({
      bucket_today: '2026-08-01',
      timezone: 'Asia/Shanghai',
      exported_at: '2026-07-31T10:00:00.000Z',
      page_daily: [
        { day: '2026-07-30', views: 1 },
        { day: '2026-07-31', views: 2 },
      ],
    })
    assert.equal(day, '2026-08-01')
  })

  it('falls back to max day key in payload when timestamp invalid and no bucket_today', () => {
    const day = analyticsBackupFilenameDay({
      exported_at: 'not-a-date',
      page_daily: [
        { day: '2026-07-28', views: 1 },
        { day: '2026-07-30', views: 2 },
      ],
    })
    assert.equal(day, '2026-07-30')
  })

  it('does not equal naive UTC ISO slice when zone is ahead near midnight', () => {
    const exportedAt = '2026-07-31T20:00:00.000Z'
    const utcSlice = new Date(exportedAt).toISOString().slice(0, 10)
    const day = analyticsBackupFilenameDay({
      timezone: 'Asia/Shanghai',
      exported_at: exportedAt,
    })
    assert.equal(utcSlice, '2026-07-31')
    assert.equal(day, '2026-08-01')
    assert.notEqual(day, utcSlice)
  })

  it('treats timezone=local as UTC calendar of exported_at (not browser local)', () => {
    // BE analytics_tz_label returns "local" when process offset hours==0
    // (UTC docker, no TZ env). Browser may be UTC+9 — must not advance the day.
    const exportedAt = '2026-07-31T20:00:00.000Z'
    const day = analyticsBackupFilenameDay({
      timezone: 'local',
      exported_at: exportedAt,
    })
    assert.equal(day, '2026-07-31')
    assert.equal(day, new Date(exportedAt).toISOString().slice(0, 10))
  })

  it('uses timezone formatting even when payload max day is yesterday', () => {
    // Today may have zero rows yet — filename must not stick to max historical day
    const day = analyticsBackupFilenameDay({
      timezone: 'Asia/Shanghai',
      exported_at: '2026-07-31T20:00:00.000Z', // 08-01 in Shanghai
      page_daily: [
        { day: '2026-07-30', views: 1 },
        { day: '2026-07-31', views: 2 },
      ],
    })
    assert.equal(day, '2026-08-01')
  })
})
