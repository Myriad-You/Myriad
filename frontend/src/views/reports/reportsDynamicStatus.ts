export type ReportsTipKind = 'stage' | 'platform' | 'empty'

export interface ReportsDynamicTip {
  id: string
  /** Decorative Latin. Idle = Stage; stage follows the platform. */
  hero: string
  main: string
  /** Omit for status-only tips. */
  sub?: string
  kind: ReportsTipKind
  platformId?: string
  /** Scroll a long subtitle before advancing. */
  scrollSub?: boolean
}

export interface ReportsStatusCopy {
  /** Decorative Latin; never localized. */
  heroStage: string
  platformReport: string
  noEnabledPlatforms: string
  stagePlaying: string
  stagePaused: string
  tipNoReports: string
  tipNoReportsSub: string
}

export interface ReportHighlight {
  platformId: string
  platformName: string
  hook: string
}

export interface BuildReportsTipsInput {
  copy: ReportsStatusCopy
  isStageMode: boolean
  stagePaused: boolean
  stagePlatformId?: string | null
  stagePlatformName?: string | null
  /** Latin stage hero; display names localize to CJK. */
  stagePlatformHero?: string | null
  enabledPlatformCount: number
  reportCount: number
  highlights?: ReportHighlight[]
}

function oneLine(value: unknown): string {
  if (typeof value !== 'string') return ''
  return value.replaceAll(/\s+/g, ' ').trim()
}

function moodLine(value: unknown): string {
  if (!Array.isArray(value)) return ''
  const words = value
    .filter((item): item is string => typeof item === 'string' && item.trim() !== '')
    .map((item) => item.trim())
    .slice(0, 3)
  return words.join(' · ')
}

/** Card portrait, else first insight/summary. */
export function pickReportHook(report: {
  summary?: string | null
  insights?: string[] | null
  card_visuals?: {
    vibe?: unknown
    taste_profile?: unknown
    mood_keywords?: unknown
  } | null
}): string {
  const visuals = report.card_visuals
  const candidates = [
    oneLine(visuals?.vibe),
    oneLine(visuals?.taste_profile),
    moodLine(visuals?.mood_keywords),
    oneLine(report.insights?.[0]),
    oneLine(report.summary),
  ]
  return candidates.find((item) => item.length > 0) ?? ''
}

const MARQUEE_PX_PER_SEC = 36
const MARQUEE_MIN_MS = 1800
const MARQUEE_MAX_MS = 14_000

export function marqueeDurationMs(overflowPx: number): number {
  if (overflowPx <= 0) return 0
  return Math.min(
    MARQUEE_MAX_MS,
    Math.max(
      MARQUEE_MIN_MS,
      Math.round((overflowPx / MARQUEE_PX_PER_SEC) * 1000),
    ),
  )
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
    highlights = [],
  } = input

  // Stage: one tip only.
  if (isStageMode && stagePlatformName) {
    return [
      {
        id: `stage-${stagePlatformId || 'unknown'}`,
        hero: stagePlatformHero || stagePlatformName,
        main: stagePlatformName,
        sub: stagePaused ? copy.stagePaused : copy.stagePlaying,
        kind: 'stage',
        platformId: stagePlatformId || undefined,
      },
    ]
  }

  if (enabledPlatformCount === 0) {
    return [
      {
        id: 'empty-platforms',
        hero: copy.heroStage,
        main: copy.platformReport,
        sub: copy.noEnabledPlatforms,
        kind: 'empty',
      },
    ]
  }

  if (reportCount === 0) {
    return [
      {
        id: 'no-reports',
        hero: copy.heroStage,
        main: copy.tipNoReports,
        sub: copy.tipNoReportsSub,
        kind: 'empty',
      },
    ]
  }

  if (highlights.length > 0) {
    return highlights.map((item) => ({
      id: `highlight-${item.platformId}`,
      hero: copy.heroStage,
      main: item.platformName,
      sub: item.hook,
      kind: 'platform',
      platformId: item.platformId,
      scrollSub: true,
    }))
  }

  return [
    {
      id: 'idle',
      hero: copy.heroStage,
      main: copy.platformReport,
      kind: 'platform',
    },
  ]
}
