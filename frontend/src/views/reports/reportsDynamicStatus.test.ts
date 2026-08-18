import type { ReportsStatusCopy } from './reportsDynamicStatus'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildReportsDynamicTips,

  resolveLifeActionLabel,
} from './reportsDynamicStatus'

const copy: ReportsStatusCopy = {
  heroStage: 'Stage',
  heroLife: 'Arael',
  platformReport: 'Platform Reports',
  clickToView: 'Click card to view',
  noEnabledPlatforms: 'No platforms',
  stagePlaying: 'Playing on stage',
  stagePaused: 'Stage paused',
  tipPlatformCount: '{count} data platforms',
  tipPlatformCountSub: 'Click a card',
  tipReportReady: '{count} reports ready',
  tipReportReadySub: 'Play all',
  tipNoReports: 'No reports yet',
  tipNoReportsSub: 'Generate below',
  lifeTitle: '设定',
  lifeLoading: 'Loading…',
  lifeDisabled: 'Disabled',
  lifeNeedLogin: 'Sign in',
  lifeCreateHint: 'Write persona',
  lifeReadyHint: '{activity} · mood {mood}',
  lifeIdle: 'Idle',
  lifeThinking: 'Thinking',
  lifeTalking: 'Talking',
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
    showLife: true,
    lifeKind: 'ready',
    lifeSnapshot: {
      name: 'Aiko',
      activity: 'idle',
      mood: 70,
    },
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
    showLife: false,
    lifeKind: 'loading',
    lifeSnapshot: null,
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
    stagePlatformHero: 'NetEase Music',
    enabledPlatformCount: 1,
    reportCount: 1,
    showLife: false,
    lifeKind: 'loading',
    lifeSnapshot: null,
  })

  assert.equal(tip.hero, 'NetEase Music')
  assert.equal(tip.main, '网易云')
})

test('platform count tip is dropped when it repeats the report count', () => {
  const allCovered = buildReportsDynamicTips({
    copy,
    isStageMode: false,
    stagePaused: false,
    enabledPlatformCount: 9,
    reportCount: 9,
    showLife: false,
    lifeKind: 'loading',
    lifeSnapshot: null,
  })
  assert.deepEqual(
    allCovered.map((t) => t.id),
    ['platform-ready'],
  )

  const partial = buildReportsDynamicTips({
    copy,
    isStageMode: false,
    stagePaused: false,
    enabledPlatformCount: 9,
    reportCount: 4,
    showLife: false,
    lifeKind: 'loading',
    lifeSnapshot: null,
  })
  assert.deepEqual(
    partial.map((t) => t.id),
    ['platform-ready', 'platform-count'],
  )
})

test('life tips keep the latin hero word, not the localized title', () => {
  const tips = buildReportsDynamicTips({
    copy,
    isStageMode: false,
    stagePaused: false,
    enabledPlatformCount: 2,
    reportCount: 1,
    showLife: true,
    lifeKind: 'guest',
    lifeSnapshot: null,
  })

  const life = tips.find((t) => t.kind === 'life-guest')
  assert.equal(life?.hero, 'Arael')
  assert.equal(life?.main, '设定')
})

test('merges platform and life tips when life is available', () => {
  const tips = buildReportsDynamicTips({
    copy,
    isStageMode: false,
    stagePaused: false,
    enabledPlatformCount: 4,
    reportCount: 2,
    showLife: true,
    lifeKind: 'create',
    lifeSnapshot: null,
  })

  assert.ok(tips.some((t) => t.kind === 'platform'))
  assert.ok(tips.some((t) => t.kind === 'life-create' && t.action === 'open-life'))
  assert.ok(tips.every((t) => t.hero.length > 0))
})

test('ready persona uses name as hero and open-life action', () => {
  const tips = buildReportsDynamicTips({
    copy,
    isStageMode: false,
    stagePaused: false,
    enabledPlatformCount: 2,
    reportCount: 1,
    showLife: true,
    lifeKind: 'ready',
    lifeSnapshot: {
      name: 'Nova',
      activity: 'thinking',
      mood: 88.4,
    },
  })

  const life = tips.find((t) => t.kind === 'life-ready')
  assert.ok(life)
  assert.equal(life?.hero, 'Nova')
  assert.equal(life?.action, 'open-life')
  assert.match(life?.sub || '', /Thinking/)
  assert.match(life?.sub || '', /88/)
})

test('life action labels resolve only for interactive kinds', () => {
  const labels = {
    create: 'Create',
    open: 'Open',
    login: 'Sign in',
  }
  assert.equal(resolveLifeActionLabel('create', labels), 'Create')
  assert.equal(resolveLifeActionLabel('ready', labels), 'Open')
  assert.equal(resolveLifeActionLabel('loading', labels), null)
  assert.equal(resolveLifeActionLabel('disabled', labels), null)
})
