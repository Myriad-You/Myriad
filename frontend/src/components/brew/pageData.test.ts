import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { requestCache } from '../../utils/requestCache.ts'
import {
  FEED_STORIES_CACHE_PREFIX,
  feedStoriesCacheKey,
  HOME_NOTES_CACHE_PREFIX,
  loadLatestStory,
  peekFeedStories,
  peekFeedStoriesLoose,
  peekLatestStory,
  putFeedStories,
} from './pageData.ts'

afterEach(() => {
  requestCache.deleteByPrefix(FEED_STORIES_CACHE_PREFIX)
  requestCache.deleteByPrefix(HOME_NOTES_CACHE_PREFIX)
})

describe('feedStoriesCacheKey', () => {
  it('一份源一把钥匙', () => {
    assert.equal(feedStoriesCacheKey(7), 'brew:feed-stories:7')
  })
})

describe('peek / put feed stories', () => {
  const items = [
    {
      id: 1,
      title: 'a',
      summary: null,
      image: null,
      published_at: 1,
      is_read: false,
    },
  ]

  it('写下就能读回，换戳当失效', async () => {
    putFeedStories(3, 9, items)
    assert.deepEqual(peekFeedStories(3, 9), items)
    assert.equal(peekFeedStories(3, 10), null)
    assert.deepEqual(peekFeedStoriesLoose(3), items)
    assert.equal(peekLatestStory({ id: 3, recent_items: [] })?.id, 1)
    assert.equal(
      (await loadLatestStory({ id: 3, last_success_at: 9, recent_items: [] }))
        ?.id,
      1,
    )
  })
})
