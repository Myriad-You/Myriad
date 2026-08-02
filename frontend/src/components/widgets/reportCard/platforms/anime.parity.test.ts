import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { zhCN } from '../../../../i18n/zh-CN'
import { buildReportCardPreviewData } from '../previewData'
import { ANIME_THEMES } from './anime'

/**
 * Bangumi and MyAnimeList render the same face component, so layout parity is
 * structural. What can still drift is each platform's *vocabulary*: a subject
 * type with no entry in `typeColors` falls back to a flat brand color, and one
 * with no entry in `typeLabels` renders its raw English key in the legend.
 *
 * These tests pin that vocabulary against the data each card actually receives.
 */

type Locale = Parameters<typeof buildReportCardPreviewData>[1]
const t = zhCN as unknown as Locale

/** Subject types the backend can emit per platform (smart_filter.rs). */
const BACKEND_SUBJECT_TYPES = {
  // Self::bangumi_subject_type_label
  bangumi: ['book', 'anime', 'game', 'music', 'real'],
  // filter_mal pushes only these two media kinds
  mal: ['anime', 'manga'],
} as const

describe('anime report cards — Bangumi / MAL parity', () => {
  it('both platforms declare every theme slot', () => {
    const slots = Object.keys(ANIME_THEMES.bangumi).sort()
    assert.deepEqual(Object.keys(ANIME_THEMES.mal).sort(), slots)
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
