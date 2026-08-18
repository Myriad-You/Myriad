import assert from 'node:assert/strict'
import test from 'node:test'
import { uniqPersonaTags } from './personaTags'

test('drops short tags and dedupes case-insensitively', () => {
  assert.deepEqual(
    uniqPersonaTags(['  夜战  ', '夜战', 'x', '喜欢独立游戏']),
    ['夜战', '喜欢独立游戏'],
  )
})

test('caps at 24 tags', () => {
  const tags = Array.from({ length: 30 }, (_, i) => `tag-${i}`)
  assert.equal(uniqPersonaTags(tags).length, 24)
})
