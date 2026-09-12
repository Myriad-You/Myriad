import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  AGENT_SCAN_MAX_PAGES,
  AGENT_SCAN_PER_PAGE,
  findAgentArticle,
  itemMatchesAgentHint,
  numericArticleId,
  takeFreshPending,
  wantsLatestOnly,
} from './agentOpen.ts'

describe('itemMatchesAgentHint', () => {
  const item = { id: 9, link: 'https://a.test/p', guid: 'g-9' }

  it('link 优先，然后 id，然后 guid', () => {
    assert.equal(
      itemMatchesAgentHint(item, { articleLink: 'https://a.test/p' }),
      true,
    )
    assert.equal(itemMatchesAgentHint(item, { articleId: '9' }), true)
    assert.equal(itemMatchesAgentHint(item, { articleId: 'g-9' }), true)
    assert.equal(itemMatchesAgentHint(item, { articleId: '8' }), false)
  })
})

describe('numericArticleId / wantsLatestOnly', () => {
  it('数字 id 才走单篇接口；指定了目标就不取第一篇', () => {
    assert.equal(numericArticleId('22'), 22)
    assert.equal(numericArticleId('abc'), null)
    assert.equal(numericArticleId(undefined), null)
    assert.equal(wantsLatestOnly({ openLatest: true }), true)
    assert.equal(
      wantsLatestOnly({ openLatest: true, articleId: '1' }),
      false,
    )
  })
})

describe('findAgentArticle', () => {
  it('数字 id 先单篇取，命中就不翻页', async () => {
    const pages: number[] = []
    const hit = await findAgentArticle(
      { articleId: '22' },
      [],
      {
        getById: async (id) => ({ id, title: `item-${id}` }),
        getPage: async (page) => {
          pages.push(page)
          return { items: [], total: 0 }
        },
      },
    )
    assert.equal(hit?.id, 22)
    assert.equal(pages.length, 0)
  })

  it('本地没有才扫页，找到就停，不带回整页', async () => {
    const fetched: number[] = []
    const hit = await findAgentArticle(
      { articleId: '9' },
      [{ id: 1, title: 'other' }],
      {
        getById: async () => {
          throw new Error('miss')
        },
        getPage: async (page, perPage) => {
          fetched.push(page)
          assert.equal(perPage, AGENT_SCAN_PER_PAGE)
          assert.ok(page <= AGENT_SCAN_MAX_PAGES)
          return {
            items: [
              { id: 8, title: 'no' },
              { id: 9, title: 'yes' },
              { id: 10, title: 'later' },
            ],
            total: 60,
          }
        },
      },
    )
    assert.equal(hit?.id, 9)
    assert.deepEqual(fetched, [1])
  })
})

describe('takeFreshPending', () => {
  it('缺、坏、过期、无时间戳都不当成待办', () => {
    const now = 1_000_000
    assert.equal(takeFreshPending(null, now).ok, false)
    assert.equal(takeFreshPending('{', now).ok, false)
    assert.equal(
      takeFreshPending(JSON.stringify({ timestamp: now - 20_000 }), now).ok,
      false,
    )
    assert.equal(takeFreshPending(JSON.stringify({}), now).ok, false)
  })

  it('10 秒内的才执行', () => {
    const now = 1_000_000
    const taken = takeFreshPending(
      JSON.stringify({ timestamp: now - 1_000, articleId: '3' }),
      now,
    )
    assert.equal(taken.ok, true)
    if (taken.ok) assert.equal(taken.value.articleId, '3')
  })
})
