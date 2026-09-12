import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import zhCN from '../../../../i18n/zh-CN.json' with { type: 'json' }
import { buildReportCardPreviewData } from '../previewData'
import { ANIME_THEMES } from './anime'

type Locale = Parameters<typeof buildReportCardPreviewData>[1]
const t = zhCN as unknown as Locale

const BACKEND_SUBJECT_TYPES = {
  bangumi: ['book', 'anime', 'game', 'music', 'real'],
  mal: ['anime', 'manga'],
} as const

describe('anime report cards — Bangumi / MAL parity', () => {
  it('both platforms declare every theme slot', () => {
    const slots = Object.keys(ANIME_THEMES.bangumi).toSorted()
    assert.deepEqual(Object.keys(ANIME_THEMES.mal).toSorted(), slots)
    for (const [platform, theme] of Object.entries(ANIME_THEMES)) {
      for (const slot of slots) {
        assert.ok(
          theme[slot as keyof typeof theme] != null,
          `${platform} theme is missing "${slot}"`,
        )
      }
    }
  })

  it('every backend subject type has a color and a label', () => {
    for (const [platform, types] of Object.entries(BACKEND_SUBJECT_TYPES)) {
      const theme = ANIME_THEMES[platform as keyof typeof ANIME_THEMES]
      const labels = theme.labels(t)
      for (const type of types) {
        assert.ok(
          theme.typeColors[type],
          `${platform} has no bar color for subject type "${type}"`,
        )
        assert.ok(
          labels.typeLabels[type],
          `${platform} has no legend label for subject type "${type}"`,
        )
      }
    }
  })

  it('preview fixtures stay inside each platform’s own vocabulary', () => {
    for (const platform of ['bangumi', 'mal'] as const) {
      const dist = buildReportCardPreviewData(platform, t)
        .subject_type_distribution as Record<string, number>
      const labels = ANIME_THEMES[platform].labels(t)
      for (const type of Object.keys(dist)) {
        assert.ok(
          labels.typeLabels[type],
          `${platform} preview contains foreign subject type "${type}"`,
        )
      }
    }
  })

  it('each platform names its own stats and taste fallback', () => {
    const bangumi = ANIME_THEMES.bangumi.labels(t)
    const mal = ANIME_THEMES.mal.labels(t)
    assert.notEqual(bangumi.fallbackTaste, mal.fallbackTaste)
    for (const key of ['done', 'doing', 'wish'] as const) {
      assert.ok(bangumi[key], `bangumi label "${key}" is empty`)
      assert.ok(mal[key], `mal label "${key}" is empty`)
    }
  })
})
