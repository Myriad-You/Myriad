import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  FEEDS_ARTICLE_MAX,
  latestStoryPreview,
  paintReadyStories,
  storiesAreFresh,
  storiesForSource,
  toFeedStory,
} from './feedStories.ts'
import { makeItem, makePreview } from './fixtures.ts'

describe('toFeedStory', () => {
  it('只收轨上文章卡要的字段', () => {
    const story = toFeedStory(
      makeItem({
        id: 9,
        title: 's9',
        source_name: '源',
        source_icon: '/i.png',
        author: 'a',
      }),
    )
    assert.deepEqual(story, {
      id: 9,
      title: 's9',
      summary: story.summary,
      image: story.image,
      published_at: story.published_at,
      is_read: false,
      is_starred: false,
      topic: null,
      author: 'a',
      source_name: '源',
      source_icon: '/i.png',
    })
    assert.equal(FEEDS_ARTICLE_MAX, 20)
  })
})

describe('storiesForSource', () => {
  it('已拉到的全文盖过源上的预览，并写上当前源名', () => {
    const preview = makePreview({ id: 1, title: '旧' })
    const fetched = {
      id: 10,
      items: [toFeedStory(makeItem({ id: 2, title: '新', source_id: 10 }))],
    }
    const stories = storiesForSource(fetched, {
      id: 10,
      name: '当前源',
      icon: '/now.png',
      recent_items: [preview],
    })
    assert.deepEqual(
      stories.map((story) => ({
        id: story.id,
        title: story.title,
        source_name: story.source_name,
        source_icon: story.source_icon,
      })),
      [{ id: 2, title: '新', source_name: '当前源', source_icon: '/now.png' }],
    )
  })

  it('还没拉到或拉空时用预览', () => {
    const preview = makePreview({ id: 3, title: '预览' })
    const stories = storiesForSource(null, {
      id: 10,
      name: '当前源',
      icon: null,
      recent_items: [preview],
    })
    assert.equal(stories[0]?.id, 3)
    assert.equal(stories[0]?.source_name, '当前源')
  })

  it('没有焦点源不收', () => {
    assert.deepEqual(
      storiesForSource({ id: 1, items: [toFeedStory(makeItem())] }, null),
      [],
    )
  })
})

describe('latestStoryPreview', () => {
  it('宽松缓存盖过源上预览', () => {
    const loose = [makePreview({ id: 2, title: 'cached' })]
    const recent = [makePreview({ id: 1, title: 'recent' })]
    assert.equal(latestStoryPreview(loose, recent)?.id, 2)
    assert.equal(latestStoryPreview(null, recent)?.id, 1)
    assert.equal(latestStoryPreview(null, null), undefined)
  })
})

describe('paintReadyStories', () => {
  it('精确戳优先，没有则用宽松缓存', () => {
    const exact = [toFeedStory(makeItem({ id: 1, title: 'exact' }))]
    const loose = [toFeedStory(makeItem({ id: 2, title: 'loose' }))]
    assert.equal(paintReadyStories(1, exact, loose)?.[0]?.title, 'exact')
    assert.equal(paintReadyStories(1, null, loose)?.[0]?.title, 'loose')
    assert.equal(storiesAreFresh(1, exact), true)
    assert.equal(storiesAreFresh(1, null), false)
    assert.equal(
      storiesAreFresh(1, null, { stamp: 1, items: loose }),
      true,
    )
  })
})
