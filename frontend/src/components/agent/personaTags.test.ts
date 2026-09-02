import assert from 'node:assert/strict'
import test from 'node:test'
import { keepSelectedPersonaTags, uniqPersonaTags } from './personaTags'

test('drops short tags and dedupes case-insensitively', () => {
  assert.deepEqual(uniqPersonaTags(['  夜战  ', '夜战', 'x', '喜欢独立游戏']), [
    '夜战',
    '喜欢独立游戏',
  ])
})

test('drops selected tags that left the current deck', () => {
  assert.deepEqual(
    keepSelectedPersonaTags(
      ['慢热', 'Night owl', '边界感强'],
      ['Night owl', 'Slow to warm up'],
    ),
    ['Night owl'],
  )
})

test('caps at 28 tags', () => {
  const tags = Array.from({ length: 30 }, (_, i) => `tag-${i}`)
  assert.equal(uniqPersonaTags(tags).length, 28)
})
