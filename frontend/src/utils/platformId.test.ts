/**
 * Run from frontend/:
 *   pnpm test:unit -- src/utils/platformId.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { resolvePlatformId } from './platformId.ts'

describe('resolvePlatformId', () => {
  it('maps display names and aliases to canonical ids', () => {
    assert.equal(resolvePlatformId('GitHub'), 'github')
    assert.equal(resolvePlatformId('MyAnimeList'), 'mal')
    assert.equal(resolvePlatformId('Netease Music'), 'netease')
    assert.equal(resolvePlatformId('PlayStation'), 'psn')
    assert.equal(resolvePlatformId('X (Twitter)'), 'x')
    assert.equal(resolvePlatformId('网易云音乐'), 'netease')
  })

  it('accepts already-canonical ids', () => {
    assert.equal(resolvePlatformId('mal'), 'mal')
    assert.equal(resolvePlatformId('psn'), 'psn')
    assert.equal(resolvePlatformId('netease'), 'netease')
  })

  it('returns null for unknown platforms', () => {
    assert.equal(resolvePlatformId(''), null)
    assert.equal(resolvePlatformId('unknown-platform'), null)
  })
})
