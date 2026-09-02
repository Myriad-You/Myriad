import assert from 'node:assert/strict'
import test from 'node:test'
import { RELATIVE_TIME_MAX_DAYS, relativeTimeBucket } from './agentRelativeTime'

const NOW = Date.parse('2026-08-28T12:00:00.000Z')
const ago = (ms: number) => new Date(NOW - ms).toISOString()

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

test('按分、时、天分档', () => {
  assert.deepEqual(relativeTimeBucket(ago(30_000), NOW), { kind: 'justNow' })
  assert.deepEqual(relativeTimeBucket(ago(5 * MINUTE), NOW), {
    kind: 'minutes',
    value: 5,
  })
  assert.deepEqual(relativeTimeBucket(ago(3 * HOUR), NOW), {
    kind: 'hours',
    value: 3,
  })
  assert.deepEqual(relativeTimeBucket(ago(2 * DAY), NOW), {
    kind: 'days',
    value: 2,
  })
})

test('边界落在下一档的第一格，不出现「60 分钟前」', () => {
  assert.deepEqual(relativeTimeBucket(ago(MINUTE - 1), NOW), {
    kind: 'justNow',
  })
  assert.deepEqual(relativeTimeBucket(ago(MINUTE), NOW), {
    kind: 'minutes',
    value: 1,
  })
  assert.deepEqual(relativeTimeBucket(ago(HOUR), NOW), {
    kind: 'hours',
    value: 1,
  })
  assert.deepEqual(relativeTimeBucket(ago(DAY), NOW), {
    kind: 'days',
    value: 1,
  })
})

test('太久远改报日期 —— 别让人算「43 天前」是哪天', () => {
  const bucket = relativeTimeBucket(
    ago((RELATIVE_TIME_MAX_DAYS + 1) * DAY),
    NOW,
  )
  assert.equal(bucket?.kind, 'date')
  assert.deepEqual(relativeTimeBucket(ago(RELATIVE_TIME_MAX_DAYS * DAY), NOW), {
    kind: 'days',
    value: RELATIVE_TIME_MAX_DAYS,
  })
})

test('服务端时钟快一点也不显示负数', () => {
  assert.deepEqual(relativeTimeBucket(ago(-5 * MINUTE), NOW), {
    kind: 'justNow',
  })
})

test('没有时间或者时间不合法就不显示，而不是显示 NaN', () => {
  assert.equal(relativeTimeBucket(null, NOW), null)
  assert.equal(relativeTimeBucket(undefined, NOW), null)
  assert.equal(relativeTimeBucket('', NOW), null)
  assert.equal(relativeTimeBucket('不是时间', NOW), null)
})
