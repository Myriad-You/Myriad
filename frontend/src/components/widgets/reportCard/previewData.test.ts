import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { zhCN } from '../../../i18n/zh-CN'
import { buildReportCardPreviewData } from './previewData'

/**
 * Widget-library previews feed the same fixture to every report card, so the
 * anime pair used to share one union of subject types. That made the MAL card
 * draw Bangumi-only segments (book/game/music/real) and the Bangumi card draw
 * `manga` — in both cases as untranslated raw keys in the fallback color.
 */
const t = zhCN as unknown as Parameters<typeof buildReportCardPreviewData>[1]

function typeKeys(platformId: string): string[] {
  return Object.keys(
    (buildReportCardPreviewData(platformId, t)
      .subject_type_distribution as Record<string, number>) ?? {},
  ).sort()
}

describe('buildReportCardPreviewData — anime subject types', () => {
  it('gives Bangumi its five subject types and no manga', () => {
    assert.deepEqual(typeKeys('bangumi'), [
      'anime',
      'book',
      'game',
      'music',
      'real',
    ])
  })

  it('limits MAL to anime/manga', () => {
    assert.deepEqual(typeKeys('mal'), ['anime', 'manga'])
  })

  it('labels every preview segment the card can render', () => {
    // Mirrors the typeLabels maps in platforms/anime.tsx: a preview key with no
    // label falls through to the raw key, which is what this guards against.
    const labelled: Record<string, string[]> = {
      bangumi: ['book', 'anime', 'game', 'music', 'real'],
      mal: ['anime', 'manga'],
    }
    for (const [platformId, known] of Object.entries(labelled)) {
      for (const key of typeKeys(platformId)) {
        assert.ok(
          known.includes(key),
          `${platformId} preview has unlabelled subject type "${key}"`,
        )
      }
    }
  })

  it('uses a distinct taste badge per platform', () => {
    const bangumi = buildReportCardPreviewData('bangumi', t).taste_profile
    const mal = buildReportCardPreviewData('mal', t).taste_profile
    assert.equal(bangumi, zhCN.reportCardWidget.bangumiTasteDefault)
    assert.equal(mal, zhCN.reportCardWidget.malTasteDefault)
    assert.notEqual(bangumi, mal)
  })
})
