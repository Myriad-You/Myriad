/* eslint-disable test/no-import-node-test -- node:test is the repository test runner */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  extractCardVisuals,
  findPlatformReport,
  hasRenderableCardVisuals,
  resolveReportPlatformId,
} from './reportCardVisuals'

describe('resolveReportPlatformId', () => {
  it('prefers config.platformId', () => {
    assert.equal(
      resolveReportPlatformId({
        type: 'report-steam',
        config: { platformId: 'github' },
      }),
      'github',
    )
  })

  it('falls back to report-* type suffix when config missing', () => {
    assert.equal(
      resolveReportPlatformId({ type: 'report-steam' }),
      'steam',
    )
    assert.equal(
      resolveReportPlatformId({ type: 'report-discord' }),
      'discord',
    )
  })

  it('defaults to bilibili', () => {
    assert.equal(resolveReportPlatformId({}), 'bilibili')
  })
})

describe('extractCardVisuals', () => {
  it('reads snake_case card_visuals', () => {
    const v = extractCardVisuals({
      platform: 'steam',
      card_visuals: { hardcore_score: 90, games_count: 12 },
    })
    assert.deepEqual(v, { hardcore_score: 90, games_count: 12 })
  })

  it('reads camelCase cardVisuals', () => {
    const v = extractCardVisuals({
      cardVisuals: { vibe: 'builder' },
    })
    assert.equal(v?.vibe, 'builder')
  })

  it('reads catalog content nesting', () => {
    const v = extractCardVisuals({
      content: {
        card_visuals: { profile: { username: 'alice' } },
      },
    })
    assert.deepEqual(v, { profile: { username: 'alice' } })
  })

  it('parses double-encoded JSON string', () => {
    const v = extractCardVisuals({
      card_visuals: JSON.stringify({ danmaku: ['AWSL'] }),
    })
    assert.deepEqual(v, { danmaku: ['AWSL'] })
  })

  it('accepts flat visuals payload', () => {
    const v = extractCardVisuals({
      hardcore_score: 70,
      player_type: 'hardcore',
    })
    assert.equal(v?.hardcore_score, 70)
  })
})

describe('hasRenderableCardVisuals', () => {
  it('rejects empty / null', () => {
    assert.equal(hasRenderableCardVisuals(null), false)
    assert.equal(hasRenderableCardVisuals({}), false)
  })

  it('accepts non-empty', () => {
    assert.equal(hasRenderableCardVisuals({ games_count: 1 }), true)
  })
})

describe('findPlatformReport', () => {
  it('matches top-level platform', () => {
    const row = findPlatformReport(
      [
        { platform: 'steam', card_visuals: { games_count: 3 } },
        { platform: 'github', card_visuals: { repos_count: 2 } },
      ],
      'github',
    )
    assert.equal(row?.platform, 'github')
  })

  it('matches nested content.platform', () => {
    const row = findPlatformReport(
      [{ content: { platform: 'discord', card_visuals: { vibe: 'x' } } }],
      'discord',
    )
    assert.ok(row)
  })
})
