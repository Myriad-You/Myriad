import type { BrewSource } from '../../../types/brew'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  BREWPACK_LEGACY_VERSION,
  BREWPACK_VERSION,
  buildBrewpackManifest,
  categoryToPackEntry,
  dataImageInfo,
  isDataImageUrl,
  normalizeBrewpackUrl,
  parseBrewpackManifest,
  resolvePackIcon,
  rsshubInstanceToPackEntry,
  sourceAddPayload,
  sourceToPackEntry,
  sourceUpdatePayload,
} from './brewpack'

const source: BrewSource = {
  id: 1,
  user_id: 1,
  name: 'Example',
  url: 'https://example.com/feed.xml',
  feed_type: 'rsshub',
  source_type: 'rss',
  category: '科技',
  icon: 'data:image/png;base64,aaa',
  description: 'desc',
  site_url: 'https://example.com',
  update_interval: 15,
  last_fetched_at: null,
  last_success_at: null,
  last_error: null,
  error_count: 0,
  enabled: false,
  item_count: 0,
  unread_count: 0,
  card_size: 'mini',
  theme_color: '#111111',
  sort_order: 3,
  ai_style_tags: ['技术'],
  rsshub_route: '/github/issue/foo',
  admin_only: true,
  created_at: 0,
}

describe('brewpack manifest', () => {
  it('round-trips portable source fields through v2 manifest', () => {
    const entry = sourceToPackEntry(source, 'icon_0.png', null)
    const manifest = buildBrewpackManifest({
      sources: [entry],
      categories: [
        categoryToPackEntry({
          id: 2,
          user_id: 1,
          name: '科技',
          icon: '📁',
          color: '#abc',
          sort_order: 4,
          created_at: 0,
        }),
      ],
      rsshubInstances: [
        rsshubInstanceToPackEntry({
          name: 'Private',
          url: 'https://rsshub.example/',
          priority: 10,
          enabled: false,
        }),
      ],
    })

    assert.equal(manifest.version, BREWPACK_VERSION)
    const parsed = parseBrewpackManifest(manifest)
    assert.equal(parsed.sources[0].description, 'desc')
    assert.equal(parsed.sources[0].site_url, 'https://example.com')
    assert.equal(parsed.sources[0].enabled, false)
    assert.equal(parsed.sources[0].sort_order, 3)
    assert.equal(parsed.sources[0].rsshub_route, '/github/issue/foo')
    assert.equal(parsed.categories?.[0].sort_order, 4)
    assert.equal(parsed.rsshub_instances?.[0].enabled, false)
    assert.equal(
      normalizeBrewpackUrl(parsed.rsshub_instances?.[0].url ?? ''),
      'https://rsshub.example',
    )

    const add = sourceAddPayload(parsed.sources[0], 'data:image/png;base64,aaa')
    assert.equal(add.icon, 'data:image/png;base64,aaa')
    assert.equal(add.enabled, false)
    assert.equal(add.sort_order, 3)
    assert.equal(add.feed_type, 'rsshub')

    const update = sourceUpdatePayload(parsed.sources[0], add.icon)
    assert.equal(update.icon, add.icon)
    assert.equal(update.theme_color, '#111111')
    assert.equal(update.card_size, 'mini')
    assert.equal(update.feed_type, 'rsshub')

    const rssOnly = sourceToPackEntry(
      { ...source, feed_type: 'rss' },
      null,
      null,
    )
    assert.equal(sourceUpdatePayload(rssOnly).feed_type, undefined)
  })

  it('accepts v1 packs and fills missing fields', () => {
    const parsed = parseBrewpackManifest({
      version: BREWPACK_LEGACY_VERSION,
      exported_at: '2026-01-01T00:00:00Z',
      sources: [
        {
          url: 'https://old.example/rss',
          name: 'Old',
          category: null,
          icon_file: null,
          icon_url: 'https://old.example/icon.png',
          source_type: 'rss',
          feed_type: 'rss',
          theme_color: null,
          update_interval: 30,
          card_size: null,
          rsshub_route: null,
          ai_style_tags: null,
          admin_only: false,
        },
      ],
    })
    assert.equal(parsed.sources[0].enabled, true)
    assert.equal(parsed.sources[0].description, null)
    assert.deepEqual(parsed.categories, [])
    assert.deepEqual(parsed.rsshub_instances, [])
    assert.equal(
      resolvePackIcon(parsed.sources[0]),
      'https://old.example/icon.png',
    )
  })

  it('rejects unsupported versions', () => {
    assert.throws(() =>
      parseBrewpackManifest({
        version: '9.0',
        sources: [],
      }),
    )
  })

  it('reads svg+xml and jpeg data URIs for pack icon files', () => {
    assert.equal(isDataImageUrl('data:image/svg+xml;base64,PHN2Zy'), true)
    assert.equal(dataImageInfo('data:image/svg+xml;base64,PHN2Zy').ext, 'svg')
    assert.equal(
      dataImageInfo('data:image/svg+xml;charset=utf-8;base64,PHN2Zy').mime,
      'image/svg+xml',
    )
    assert.equal(dataImageInfo('data:image/jpeg;base64,aaa').ext, 'jpg')
    assert.equal(dataImageInfo('data:image/png;base64,aaa').ext, 'png')
  })
})
