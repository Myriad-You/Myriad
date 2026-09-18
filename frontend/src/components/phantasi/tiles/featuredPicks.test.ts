import type { PhantasiItemPreview, PhantasiSource } from '../../../types/phantasi'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  collectFeaturedPicks,
  feedSourcesForFeatured,
} from './featuredPicks'

const NOW = Date.UTC(2026, 4, 1)

function item(
  over: Partial<PhantasiItemPreview> & Pick<PhantasiItemPreview, 'id' | 'title'>,
): PhantasiItemPreview {
  return {
    summary: null,
    image: null,
    published_at: NOW,
    is_read: false,
    ...over,
  }
}

function source(
  over: Partial<PhantasiSource> & Pick<PhantasiSource, 'id' | 'name'>,
): PhantasiSource {
  return {
    url: `https://example.com/${over.id}.xml`,
    feed_type: 'rss',
    source_type: 'rss',
    category: null,
    icon: null,
    description: null,
    site_url: `https://example.com/${over.id}`,
    update_interval: 3600,
    last_fetched_at: NOW,
    last_success_at: NOW,
    last_error: null,
    error_count: 0,
    enabled: true,
    item_count: 3,
    unread_count: 0,
    card_size: null,
    theme_color: '#f97316',
    sort_order: null,
    ai_style_tags: null,
    rsshub_route: null,
    admin_only: false,
    created_at: NOW,
    recent_items: [],
    ...over,
  }
}

describe('feedSourcesForFeatured', () => {
  it('drops link, note, and friend-link feeds', () => {
    const kept = source({ id: 1, name: 'kept' })
    const link = source({ id: 2, name: 'link', source_type: 'link' })
    const note = source({ id: 3, name: 'note', source_type: 'note' })
    const friend = source({
      id: 4,
      name: 'friend rss',
      category: '友情链接',
    })
    assert.deepEqual(
      feedSourcesForFeatured([kept, link, note, friend]).map((row) => row.id),
      [1],
    )
  })
})

describe('collectFeaturedPicks', () => {
  it('returns empty when nothing is readable', () => {
    assert.deepEqual(collectFeaturedPicks([]), [])
    assert.deepEqual(
      collectFeaturedPicks([
        source({ id: 1, name: 'empty' }),
        source({
          id: 2,
          name: 'blank title',
          recent_items: [item({ id: 9, title: '   ' })],
        }),
      ]),
      [],
    )
  })

  it('orders by published_at then id, and caps each source at two seats', () => {
    const picks = collectFeaturedPicks(
      [
        source({
          id: 1,
          name: 'alpha',
          recent_items: [
            item({ id: 11, title: 'a1', published_at: NOW - 1000 }),
            item({ id: 12, title: 'a2', published_at: NOW - 2000 }),
            item({ id: 13, title: 'a3', published_at: NOW - 3000 }),
          ],
        }),
        source({
          id: 2,
          name: 'beta',
          theme_color: '#0ea5e9',
          recent_items: [
            item({ id: 21, title: 'b1', published_at: NOW - 1500 }),
            item({ id: 22, title: 'b2', published_at: NOW - 2500 }),
          ],
        }),
      ],
      4,
    )
    assert.deepEqual(
      picks.map((row) => row.id),
      [11, 21, 12, 22],
    )
    assert.equal(picks.filter((row) => row.source_id === 1).length, 2)
    assert.equal(picks[1]?.source_color, '#0ea5e9')
  })

  it('fills leftover seats from overflow after the per-source cap', () => {
    const picks = collectFeaturedPicks(
      [
        source({
          id: 1,
          name: 'only',
          recent_items: [
            item({ id: 1, title: 'one', published_at: NOW - 1 }),
            item({ id: 2, title: 'two', published_at: NOW - 2 }),
            item({ id: 3, title: 'three', published_at: NOW - 3 }),
          ],
        }),
      ],
      3,
    )
    assert.deepEqual(
      picks.map((row) => row.id),
      [1, 2, 3],
    )
  })

  it('promotes a cover only inside the already picked set', () => {
    const picks = collectFeaturedPicks(
      [
        source({
          id: 1,
          name: 'fresh text',
          recent_items: [
            item({ id: 1, title: 'newest', published_at: NOW }),
            item({
              id: 2,
              title: 'with cover',
              image: 'https://example.com/c.jpg',
              published_at: NOW - 10,
            }),
          ],
        }),
        source({
          id: 2,
          name: 'old cover',
          recent_items: [
            item({
              id: 99,
              title: 'ancient',
              image: 'https://example.com/old.jpg',
              published_at: NOW - 86_400_000,
            }),
          ],
        }),
      ],
      2,
    )
    assert.equal(picks[0]?.id, 2)
    assert.equal(picks[1]?.id, 1)
    assert.equal(
      picks.some((row) => row.id === 99),
      false,
    )
  })

  it('dedupes the same item id across sources', () => {
    const shared = item({ id: 7, title: 'shared', published_at: NOW })
    const picks = collectFeaturedPicks([
      source({ id: 1, name: 'a', recent_items: [shared] }),
      source({ id: 2, name: 'b', recent_items: [shared] }),
    ])
    assert.equal(picks.length, 1)
    assert.equal(picks[0]?.source_id, 1)
  })
})
