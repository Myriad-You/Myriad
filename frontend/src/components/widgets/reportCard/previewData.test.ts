import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import zhCN from '../../../i18n/zh-CN.json' with { type: 'json' }
import { buildReportCardPreviewData } from './previewData'

// 预览 fixture 不要把 Bangumi/MAL 条目类型并成一份，多余 key 会以未翻译原文画出。
const t = zhCN as unknown as Parameters<typeof buildReportCardPreviewData>[1]

function typeKeys(platformId: string): string[] {
  return Object.keys(
    (buildReportCardPreviewData(platformId, t)
      .subject_type_distribution as Record<string, number>) ?? {},
  ).toSorted()
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
