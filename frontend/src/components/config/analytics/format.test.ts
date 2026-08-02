/**
 * Run from frontend/:
 *   pnpm test:unit -- src/components/config/analytics/format.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { formatCount, formatDuration, niceAxis, shortDay } from './format.ts'

describe('formatCount', () => {
  it('returns em dash for non-finite', () => {
    assert.equal(formatCount(Number.NaN, 'en-US'), '—')
  })

  it('formats integers', () => {
    assert.equal(formatCount(121, 'en-US'), '121')
  })
})

describe('formatDuration', () => {
  it('returns em dash for zero or negative', () => {
    assert.equal(formatDuration(0, 'zh-CN'), '—')
    assert.equal(formatDuration(-1, 'en-US'), '—')
  })

  it('formats seconds under a minute', () => {
    assert.equal(formatDuration(4500, 'zh-CN'), '5 秒')
    assert.equal(formatDuration(4500, 'en-US'), '5s')
  })

  it('formats minutes with remainder', () => {
    assert.equal(formatDuration(125_000, 'zh-CN'), '2 分 5 秒')
    assert.equal(formatDuration(120_000, 'en-US'), '2m')
  })
})

describe('shortDay', () => {
  it('strips year prefix', () => {
    assert.equal(shortDay('2026-07-30'), '07-30')
  })

  it('passes short values through', () => {
    assert.equal(shortDay('07-30'), '07-30')
  })
})

describe('niceAxis', () => {
  it('covers zero as a single unit max', () => {
    const axis = niceAxis(0)
    assert.ok(axis.max >= 1)
    assert.equal(axis.ticks[0], 0)
    assert.equal(axis.ticks[axis.ticks.length - 1], axis.max)
  })

  it('produces integer ticks that cover the data', () => {
    const axis = niceAxis(47)
    assert.ok(axis.max >= 47)
    assert.ok(axis.ticks.every(t => Number.isInteger(t)))
    assert.ok(axis.ticks.length >= 3)
  })

  it('stays close for already-nice maxima', () => {
    const axis = niceAxis(100)
    assert.equal(axis.max, 100)
  })
})
