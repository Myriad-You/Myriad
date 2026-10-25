import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { commentHue, commentWhen } from './commentTime.ts'

const now = new Date(2026, 9, 25, 12, 0).getTime()

describe('commentWhen', () => {
  it('一分钟内是刚刚，一周内是相对时间', () => {
    assert.equal(commentWhen(now - 20_000, now, 'zh-CN', '刚刚'), '刚刚')
    assert.equal(commentWhen(now - 5 * 60_000, now, 'en-US', 'now'), '5 minutes ago')
    assert.equal(commentWhen(now - 3 * 3600_000, now, 'en-US', 'now'), '3 hours ago')
    assert.equal(commentWhen(now - 2 * 86400_000, now, 'en-US', 'now'), '2 days ago')
  })

  it('一周以前写日期，跨年才带年份', () => {
    assert.doesNotMatch(commentWhen(new Date(2026, 0, 3).getTime(), now, 'en-US', 'now'), /2026/)
    assert.match(commentWhen(new Date(2025, 0, 3).getTime(), now, 'en-US', 'now'), /2025/)
  })
})

it('commentHue 对同一个人稳定', () => {
  assert.equal(commentHue('alice'), commentHue('alice'))
  assert.ok(commentHue(42) >= 0 && commentHue(42) < 360)
})
