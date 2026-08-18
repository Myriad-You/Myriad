/**
 * Reports page dynamic status: rotating tips + hero title for the unified bar.
 */
export type LifeStatusKind =
  | 'loading'
  | 'disabled'
  | 'guest'
  | 'create'
  | 'ready'

export interface AgentLifeSnapshot {
  name: string
  mood?: number
  activity?: string
}

export type ReportsTipKind =
  | 'stage'
  | 'platform'
  | 'empty'
  | 'life-ready'
  | 'life-create'
  | 'life-guest'
  | 'life-loading'
  | 'life-disabled'

export type ReportsTipAction = 'none' | 'open-life' | 'login'

export interface ReportsDynamicTip {
  id: string
  /** Large background hero title — decorative Latin word, never localized */
  hero: string
  main: string
  sub: string
  kind: ReportsTipKind
  /** Tip body click target */
  action: ReportsTipAction
  /** Platform id for stage tip icon lookup */
  platformId?: string
}

export interface ReportsStatusCopy {
  /** Hero word for the report tips — decorative Latin, never localized */
  heroStage: string
  /** Hero word for life tips — Latin like heroStage, not the localized title */
  heroLife: string
  platformReport: string
  clickToView: string
  noEnabledPlatforms: string
  stagePlaying: string
  stagePaused: string
  tipPlatformCount: string
  tipPlatformCountSub: string
  tipReportReady: string
  tipReportReadySub: string
  tipNoReports: string
  tipNoReportsSub: string
  lifeTitle: string
  lifeLoading: string
  lifeDisabled: string
  lifeNeedLogin: string
  lifeCreateHint: string
  lifeReadyHint: string
  lifeIdle: string
  lifeThinking: string
  lifeTalking: string
}

export interface BuildReportsTipsInput {
  copy: ReportsStatusCopy
  isStageMode: boolean
  stagePaused: boolean
  stagePlatformId?: string | null
  stagePlatformName?: string | null
  /** Latin platform name for the hero — display names get localized to CJK */
  stagePlatformHero?: string | null
  enabledPlatformCount: number
  reportCount: number
  showLife: boolean
  lifeKind: LifeStatusKind
  lifeSnapshot: AgentLifeSnapshot | null
}

function lifeActivityLabel(
  activity: string | undefined,
  copy: ReportsStatusCopy,
): string {
  if (activity === 'thinking') return copy.lifeThinking
  if (activity === 'talking') return copy.lifeTalking
  return copy.lifeIdle
}

export function buildReportsDynamicTips(
  input: BuildReportsTipsInput,
): ReportsDynamicTip[] {
  const {
    copy,
    isStageMode,
    stagePaused,
    stagePlatformId,
    stagePlatformName,
    stagePlatformHero,
    enabledPlatformCount,
    reportCount,
    showLife,
    lifeKind,
    lifeSnapshot,
  } = input

  // Stage locks the tip carousel — one focused status.
  if (isStageMode && stagePlatformName) {
    return [
      {
        id: `stage-${stagePlatformId || 'unknown'}`,
        hero: stagePlatformHero || stagePlatformName,
        main: stagePlatformName,
        sub: stagePaused ? copy.stagePaused : copy.stagePlaying,
        kind: 'stage',
        action: 'none',
        platformId: stagePlatformId || undefined,
      },
    ]
  }

  const tips: ReportsDynamicTip[] = []

  if (enabledPlatformCount === 0) {
    tips.push({
      id: 'empty-platforms',
      hero: copy.heroStage,
      main: copy.platformReport,
      sub: copy.noEnabledPlatforms,
      kind: 'empty',
      action: 'none',
    })
  } else if (reportCount === 0) {
    tips.push({
      id: 'no-reports',
      hero: copy.heroStage,
      main: copy.tipNoReports,
      sub: copy.tipNoReportsSub,
      kind: 'empty',
      action: 'none',
    })
  } else {
    tips.push({
      id: 'platform-ready',
      hero: copy.heroStage,
      main: copy.tipReportReady.replace('{count}', String(reportCount)),
      sub: copy.tipReportReadySub,
      kind: 'platform',
      action: 'none',
    })

    // Every platform already has a report → "N reports ready" and
    // "N data platforms" are the same sentence twice. Only carry the
    // platform count when it actually says something new.
    if (enabledPlatformCount !== reportCount) {
      tips.push({
        id: 'platform-count',
        hero: copy.heroStage,
        main: copy.tipPlatformCount.replace(
          '{count}',
          String(enabledPlatformCount),
        ),
        sub: copy.tipPlatformCountSub || copy.clickToView,
        kind: 'platform',
        action: 'none',
      })
    }
  }

  if (!showLife) return tips

  switch (lifeKind) {
    case 'loading':
      tips.push({
        id: 'life-loading',
        hero: copy.heroLife,
        main: copy.lifeTitle,
        sub: copy.lifeLoading,
        kind: 'life-loading',
        action: 'none',
      })
      break
    case 'disabled':
      tips.push({
        id: 'life-disabled',
        hero: copy.heroLife,
        main: copy.lifeTitle,
        sub: copy.lifeDisabled,
        kind: 'life-disabled',
        action: 'none',
      })
      break
    case 'guest':
      tips.push({
        id: 'life-guest',
        hero: copy.heroLife,
        main: copy.lifeTitle,
        sub: copy.lifeNeedLogin,
        kind: 'life-guest',
        action: 'login',
      })
      break
    case 'create':
      tips.push({
        id: 'life-create',
        hero: copy.heroLife,
        main: copy.lifeTitle,
        sub: copy.lifeCreateHint,
        kind: 'life-create',
        action: 'open-life',
      })
      break
    case 'ready': {
      const name = lifeSnapshot?.name || copy.lifeTitle
      const activity = lifeActivityLabel(lifeSnapshot?.activity, copy)
      const mood = Math.round(lifeSnapshot?.mood ?? 70)
      tips.push({
        id: `life-ready-${name}`,
        hero: name,
        main: name,
        sub: copy.lifeReadyHint
          .replace('{activity}', activity)
          .replace('{mood}', String(mood)),
        kind: 'life-ready',
        action: 'open-life',
      })
      break
    }
  }

  return tips
}

export function resolveLifeActionLabel(
  kind: LifeStatusKind,
  labels: {
    create: string
    open: string
    login: string
  },
): string | null {
  switch (kind) {
    case 'create':
      return labels.create
    case 'ready':
      return labels.open
    case 'guest':
      return labels.login
    default:
      return null
  }
}
