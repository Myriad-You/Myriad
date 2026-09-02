import assert from 'node:assert/strict'
import { afterEach, test } from 'node:test'
import {
  forgetComposerFavorite,
  invalidateComposerFavorites,
  subscribeComposerFavorites,
} from './composerFavorites'

afterEach(() => {
  invalidateComposerFavorites()
})

test('invalidate wakes subscribers so the composer can reload saved tags', () => {
  let ticks = 0
  const stop = subscribeComposerFavorites(() => {
    ticks += 1
  })
  invalidateComposerFavorites()
  assert.equal(ticks, 1)
  forgetComposerFavorite(1)
  assert.equal(ticks, 1)
  stop()
  invalidateComposerFavorites()
  assert.equal(ticks, 1)
})
