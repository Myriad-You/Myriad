import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  coerceReportVisuals,
  extractCardVisuals,
  hasRenderableCardVisuals,
  hasReportDetailContent,
  pickPlatformCardVisuals,
  resolveReportPlatformId,
} from './reportCardVisuals'

describe('resolveReportPlatformId', () => {
  it('prefers explicit config.platformId', () => {
    assert.equal(
      resolveReportPlatformId({
        type: 'report-steam',
        config: { platformId: 'github' },
      }),
      'github',
    )
  })

  it('falls back to report-* type suffix when config missing (home layout bug)', () => {
    // Saved dashboard layouts sometimes omit config; type still encodes platform.
    assert.equal(
      resolveReportPlatformId({ type: 'report-steam' }),
      'steam',
    )
    assert.equal(
      resolveReportPlatformId({ type: 'report-discord', config: {} }),
      'discord',
    )
  })

  it('defaults to bilibili only when nothing else is available', () => {
    assert.equal(resolveReportPlatformId({}), 'bilibili')
    assert.equal(resolveReportPlatformId({ type: 'welcome' }), 'bilibili')
  })

  it('normalizes aliases so API labels match widget branches', () => {
    assert.equal(
      resolveReportPlatformId({ config: { platformId: 'MyAnimeList' } }),
      'mal',
    )
    assert.equal(
      resolveReportPlatformId({ type: 'report-twitter' }),
      'x',
    )
    assert.equal(
      resolveReportPlatformId({ config: { platformId: 'PlayStation' } }),
      'psn',
    )
  })
})

describe('extractCardVisuals', () => {
  it('reads snake_case card_visuals from platform report', () => {
    const visuals = extractCardVisuals({
      platform: 'steam',
      summary: 'hi',
      insights: ['a'],
      card_visuals: { hardcore_score: 88, games_count: 12 },
    })
    assert.deepEqual(visuals, { hardcore_score: 88, games_count: 12 })
  })

  it('reads camelCase and nested content (catalog shape)', () => {
    assert.deepEqual(
      extractCardVisuals({
        platform: 'github',
        cardVisuals: { total_stars: 10 },
      }),
      { total_stars: 10 },
    )
    assert.deepEqual(
      extractCardVisuals({
        platform: 'x',
        content: { card_visuals: { vibe: 'lurker' } },
      }),
      { vibe: 'lurker' },
    )
  })

  it('parses double-encoded JSON string card_visuals', () => {
    const visuals = extractCardVisuals({
      platform: 'steam',
      card_visuals: JSON.stringify({ hardcore_score: 42 }),
    })
    assert.deepEqual(visuals, { hardcore_score: 42 })
  })

  it('accepts flat visuals payload (Reports page data= prop)', () => {
    const visuals = extractCardVisuals({
      hardcore_score: 70,
      player_type: 'hardcore',
    })
    assert.deepEqual(visuals, {
      hardcore_score: 70,
      player_type: 'hardcore',
    })
  })

  it('returns null for empty/missing visuals (empty-render guard)', () => {
    assert.equal(extractCardVisuals(null), null)
    assert.equal(
      extractCardVisuals({ platform: 'steam', summary: 'x', insights: [] }),
      null,
    )
    assert.equal(hasRenderableCardVisuals(null), false)
    assert.equal(hasRenderableCardVisuals({}), false)
    assert.equal(hasRenderableCardVisuals({ hardcore_score: 1 }), true)
  })
})

describe('pickPlatformCardVisuals', () => {
  it('matches platform and returns non-empty card_visuals from latest payload', () => {
    const picked = pickPlatformCardVisuals(
      {
        success: true,
        platform_reports: [
          {
            platform: 'bilibili',
            card_visuals: { danmaku: ['hi'] },
          },
          {
            platform: 'steam',
            card_visuals: { hardcore_score: 91, games_count: 50 },
          },
        ],
      },
      'steam',
    )
    assert.deepEqual(picked, { hardcore_score: 91, games_count: 50 })
  })

  it('fails closed on success:false / missing platform (would blank home card)', () => {
    assert.equal(
      pickPlatformCardVisuals(
        { success: false, message: 'No valid report found' },
        'steam',
      ),
      null,
    )
    assert.equal(
      pickPlatformCardVisuals(
        {
          success: true,
          platform_reports: [
            { platform: 'github', card_visuals: { total_stars: 1 } },
          ],
        },
        'steam',
      ),
      null,
    )
  })

  it('does not treat empty card_visuals as renderable (empty shell bug)', () => {
    // Regression: setReportData({}) is truthy → platform widget mounts with no stats
    const picked = pickPlatformCardVisuals(
      {
        success: true,
        platform_reports: [{ platform: 'steam', card_visuals: {} }],
      },
      'steam',
    )
    assert.equal(picked, null)
  })

  it('matches content.platform for catalog-shaped rows', () => {
    const picked = pickPlatformCardVisuals(
      {
        platform_reports: [
          {
            content: {
              platform: 'discord',
              card_visuals: { vibe: 'builder', stats: { guilds: 3 } },
            },
          },
        ],
      },
      'discord',
    )
    assert.deepEqual(picked, { vibe: 'builder', stats: { guilds: 3 } })
  })

  it('matches platform case-insensitively (home type report-steam vs Steam)', () => {
    const picked = pickPlatformCardVisuals(
      {
        success: true,
        platform_reports: [
          {
            platform: 'Steam',
            card_visuals: { hardcore_score: 77, games_count: 9 },
          },
        ],
      },
      'steam',
    )
    assert.deepEqual(picked, { hardcore_score: 77, games_count: 9 })
  })

  it('recovers visuals nested under report.report (older stored shape)', () => {
    const picked = pickPlatformCardVisuals(
      {
        success: true,
        platform_reports: [
          {
            platform: 'github',
            report: {
              card_visuals: {
                contribution_level: 'senior',
                total_stars: 42,
              },
            },
          },
        ],
      },
      'github',
    )
    assert.deepEqual(picked, {
      contribution_level: 'senior',
      total_stars: 42,
    })
  })

  it('skips empty card_visuals {} and uses content.card_visuals instead', () => {
    const picked = pickPlatformCardVisuals(
      {
        success: true,
        platform_reports: [
          {
            platform: 'steam',
            card_visuals: {},
            content: {
              card_visuals: { hardcore_score: 88, games_count: 40 },
            },
          },
        ],
      },
      'steam',
    )
    assert.deepEqual(picked, { hardcore_score: 88, games_count: 40 })
  })

  it('accepts flat stats fields (games_count / stats) without hardcore_score', () => {
    assert.deepEqual(
      extractCardVisuals({ games_count: 12, player_type: 'casual' }),
      { games_count: 12, player_type: 'casual' },
    )
    assert.deepEqual(extractCardVisuals({ stats: { guilds: 4 } }), {
      stats: { guilds: 4 },
    })
  })
})

describe('coerceReportVisuals (report JSON exists but nested)', () => {
  it('unwraps PlatformReport envelope to flat card_visuals', () => {
    const visuals = coerceReportVisuals({
      platform: 'steam',
      summary: 'A summary',
      insights: ['a'],
      card_visuals: {
        hardcore_score: 91,
        games_count: 50,
        player_type: 'hardcore',
      },
    })
    assert.equal(visuals?.hardcore_score, 91)
    assert.equal(visuals?.games_count, 50)
    assert.equal(visuals?.summary, undefined)
  })

  it('unwraps double-nested card_visuals', () => {
    const visuals = coerceReportVisuals({
      card_visuals: {
        card_visuals: { hardcore_score: 70, library_items: [{ title: 'A' }] },
      },
    })
    assert.equal(visuals?.hardcore_score, 70)
    assert.equal((visuals?.library_items as unknown[])?.length, 1)
  })

  it('merges sibling library_items onto visuals when missing inside', () => {
    const visuals = coerceReportVisuals({
      card_visuals: { hardcore_score: 60 },
      library_items: [{ title: 'Outer' }],
    })
    assert.equal(visuals?.hardcore_score, 60)
    assert.deepEqual(visuals?.library_items, [{ title: 'Outer' }])
  })

  it('merges sibling X following fields onto visuals when missing inside', () => {
    const visuals = coerceReportVisuals({
      card_visuals: { vibe: '沉浸观察者' },
      following_sample: [{ username: 'a', name: 'A' }],
      following_highlights: [{ username: 'a', tag: '品味' }],
    })
    assert.equal(visuals?.vibe, '沉浸观察者')
    assert.equal((visuals?.following_sample as unknown[])?.length, 1)
    assert.equal((visuals?.following_highlights as unknown[])?.length, 1)
  })
})

describe('hasReportDetailContent', () => {
  it('is true for library_items (covers / guilds / posts)', () => {
    assert.equal(
      hasReportDetailContent({ library_items: [{ title: 'A' }] }),
      true,
    )
  })

  it('is true for X following highlights/sample even when library_items empty', () => {
    // Regression: #155 Discord empty-face gate only checked library_items,
    // so low-post X cards never auto-flipped to the following carousel.
    assert.equal(
      hasReportDetailContent({
        library_items: [],
        following_highlights: [{ username: 'foo', tag: '创作者' }],
      }),
      true,
    )
    assert.equal(
      hasReportDetailContent({
        following_sample: [{ username: 'bar' }],
      }),
      true,
    )
  })

  it('is true for YouTube recent_videos when library_items empty', () => {
    assert.equal(
      hasReportDetailContent({
        subscriber_count: 100,
        library_items: [],
        recent_videos: [{ title: 'v1' }],
      }),
      true,
    )
  })

  it('is true for Discord guilds_preview fallback list', () => {
    assert.equal(
      hasReportDetailContent({
        guilds_preview: [{ name: 'Server' }],
        library_items: [],
      }),
      true,
    )
  })

  it('is false when only overview stats exist', () => {
    assert.equal(
      hasReportDetailContent({
        vibe: '沉浸观察者',
        stats: { followers: 10 },
        library_items: [],
        following_sample: [],
      }),
      false,
    )
    assert.equal(hasReportDetailContent(null), false)
  })
})
