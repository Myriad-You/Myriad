import type { ReportsStatusCopy } from './reportsDynamicStatus'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildReportsDynamicTips,
  marqueeDurationMs,
  pickReportHook,
} from './reportsDynamicStatus'

const copy: ReportsStatusCopy = {
  heroStage: 'Stage',
  platformReport: 'Platform Reports',
  noEnabledPlatforms: 'No platforms',
  stagePlaying: 'Playing on stage',
  stagePaused: 'Stage paused',
  tipNoReports: 'No reports yet',
  tipNoReportsSub: 'Generate below',
}

test('stage mode locks tip carousel to platform', () => {
  const tips = buildReportsDynamicTips({
    copy,
    isStageMode: true,
    stagePaused: false,
    stagePlatformId: 'steam',
    stagePlatformName: 'Steam',
    enabledPlatformCount: 3,
    reportCount: 2,
  })

  assert.equal(tips.length, 1)
  assert.equal(tips[0].kind, 'stage')
  assert.equal(tips[0].hero, 'Steam')
  assert.equal(tips[0].sub, 'Playing on stage')
})

test('stage paused updates subtitle', () => {
  const tips = buildReportsDynamicTips({
    copy,
    isStageMode: true,
    stagePaused: true,
    stagePlatformId: 'github',
    stagePlatformName: 'GitHub',
    enabledPlatformCount: 1,
    reportCount: 1,
  })
  assert.equal(tips[0].sub, 'Stage paused')
})

test('stage hero uses the latin platform name, main keeps the localized one', () => {
  const [tip] = buildReportsDynamicTips({
    copy,
    isStageMode: true,
    stagePaused: false,
    stagePlatformId: 'netease',
    stagePlatformName: '网易云',
    stagePlatformHero: 'NetEase',
    enabledPlatformCount: 1,
    reportCount: 1,
  })

  assert.equal(tip.hero, 'NetEase')
  assert.equal(tip.main, '网易云')
})

test('idle bar puts the platform first and the hook in the subtitle', () => {
  const tips = buildReportsDynamicTips({
    copy,
    isStageMode: false,
    stagePaused: false,
    enabledPlatformCount: 2,
    reportCount: 2,
    highlights: [
      {
        platformId: 'youtube',
        platformName: 'YouTube',
        hook: '技术日志型创作者，上传稳均播不虚',
      },
      {
        platformId: 'bangumi',
        platformName: 'Bangumi',
        hook: '偏爱深夜动画与硬核科幻',
      },
    ],
  })

  assert.deepEqual(
    tips.map((t) => t.id),
    ['highlight-youtube', 'highlight-bangumi'],
  )
  assert.equal(tips[0].main, 'YouTube')
  assert.equal(tips[0].sub, '技术日志型创作者，上传稳均播不虚')
  assert.equal(tips[0].scrollSub, true)
  assert.equal(tips[1].hero, 'Stage')
})

test('pickReportHook prefers vibe, then taste, then insight', () => {
  assert.equal(
    pickReportHook({
      summary: 'long summary',
      insights: ['first insight'],
      card_visuals: { vibe: '  short vibe  ', taste_profile: 'taste' },
    }),
    'short vibe',
  )
  assert.equal(
    pickReportHook({
      insights: ['first insight'],
      card_visuals: { taste_profile: '深夜向', mood_keywords: ['欢快', '夜'] },
    }),
    '深夜向',
  )
  assert.equal(
    pickReportHook({
      insights: ['  keep going  '],
      card_visuals: { mood_keywords: ['欢快', '夜', '燃', 'extra'] },
    }),
    '欢快 · 夜 · 燃',
  )
  assert.equal(pickReportHook({ summary: 'only summary' }), 'only summary')
  assert.equal(pickReportHook({}), '')
})

test('marqueeDurationMs scales with overflow and stays bounded', () => {
  assert.equal(marqueeDurationMs(0), 0)
  assert.equal(marqueeDurationMs(-4), 0)
  assert.ok(marqueeDurationMs(20) >= 1800)
  assert.ok(marqueeDurationMs(20_000) <= 14_000)
  assert.equal(marqueeDurationMs(360), 10_000)
})
