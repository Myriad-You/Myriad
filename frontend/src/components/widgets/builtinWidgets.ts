/**
 * Shared built-in widget catalog for Home and Control Panel.
 * Single source of truth for ids, components, sizes, and i18n name keys.
 */

import type { ComponentType } from 'react'
import type { TranslationKeys } from '../../i18n'
import type { WidgetComponentProps, WidgetSize, WidgetType } from '../WidgetGrid'
import { lazy } from 'react'
import {
  preloadReportCardsForTypes,
  ReportCardHost,
} from './reportCardHost'

// 非报告小组件：React.lazy + 与 preload 共用 Promise。
// 报告卡见 ReportCardHost——禁止渲染期 lazy（会破坏多卡 face 入场）。
function lazyWidget<K extends string>(
  factory: () => Promise<Record<K, ComponentType<WidgetComponentProps>>>,
  name: K,
) {
  let shared: Promise<{ default: ComponentType<WidgetComponentProps> }> | null =
    null
  const load = () => {
    shared ||= factory().then((m) => ({ default: m[name] }))
    return shared
  }
  const component = lazy(load)
  ;(component as unknown as { preload: () => Promise<unknown> }).preload = load
  return component
}

// Brew 磁贴：与 /brew 页共用同一组组件（`components/brew/tiles`），
// 首页与 Brew 的差异全靠 props / 注册配置，不复制第二份实现。
const BrewFeaturedWidget = lazyWidget(
  () => import('../brew/tiles/BrewFeaturedTile'),
  'BrewFeaturedWidget',
)
const BrewSourceWidget = lazyWidget(
  () => import('../brew/tiles/BrewSourceTile'),
  'BrewSourceWidget',
)
const BrewTopicWidget = lazyWidget(
  () => import('../brew/tiles/BrewTopicTile'),
  'BrewTopicWidget',
)
const FriendLinksWidget = lazyWidget(
  () => import('./FriendLinksWidget'),
  'FriendLinksWidget',
)
const GamePresenceWidget = lazyWidget(
  () => import('./GamePresenceWidget'),
  'GamePresenceWidget',
)
const MusicPlayerWidget = lazyWidget(
  () => import('./MusicPlayerWidget'),
  'MusicPlayerWidget',
)
const QuickStatsWidget = lazyWidget(
  () => import('./QuickStatsWidget'),
  'QuickStatsWidget',
)
const QuoteWidget = lazyWidget(() => import('./QuoteWidget'), 'QuoteWidget')
const RecentActivityWidget = lazyWidget(
  () => import('./RecentActivityWidget'),
  'RecentActivityWidget',
)
const SocialNetworkWidget = lazyWidget(
  () => import('./SocialNetworkWidget'),
  'SocialNetworkWidget',
)
const TappShortcutWidget = lazyWidget(
  () => import('./TappShortcutWidget'),
  'TappShortcutWidget',
)
const VisitorStatsWidget = lazyWidget(
  () => import('./VisitorStatsWidget'),
  'VisitorStatsWidget',
)
const WeatherWidget = lazyWidget(
  () => import('./WeatherWidget'),
  'WeatherWidget',
)
const WelcomeWidget = lazyWidget(
  () => import('./WelcomeWidget'),
  'WelcomeWidget',
)

type WidgetsI18n = TranslationKeys['widgets']

/** Base config without localized name — useful for static references / tests */
export const BUILTIN_WIDGET_BASE_CONFIG = {
  welcome: {
    defaultSize: '4x2' as WidgetSize,
    component: WelcomeWidget,
    supportedSizes: ['2x2', '4x2'] as WidgetSize[],
  },
  'quick-stats': {
    defaultSize: '4x2' as WidgetSize,
    component: QuickStatsWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'recent-activity': {
    defaultSize: '4x2' as WidgetSize,
    component: RecentActivityWidget,
    supportedSizes: ['2x2', '4x2', '4x4'] as WidgetSize[],
  },
  'friend-links': {
    defaultSize: '4x2' as WidgetSize,
    component: FriendLinksWidget,
    supportedSizes: ['4x1', '2x2', '4x2'] as WidgetSize[],
  },
  weather: {
    defaultSize: '2x2' as WidgetSize,
    component: WeatherWidget,
    supportedSizes: ['2x2', '4x2', '4x1'] as WidgetSize[],
  },
  quote: {
    defaultSize: '2x2' as WidgetSize,
    component: QuoteWidget,
    supportedSizes: ['2x2', '4x2', '4x1'] as WidgetSize[],
  },
  'music-player': {
    defaultSize: '2x2' as WidgetSize,
    component: MusicPlayerWidget,
    supportedSizes: ['2x2', '4x2'] as WidgetSize[],
  },
  'report-bilibili': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-steam': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-github': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-youtube': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-netease': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-bangumi': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-mal': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-x': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-discord': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-xbox': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-psn': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardHost,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'social-network': {
    defaultSize: '1x1' as WidgetSize,
    component: SocialNetworkWidget,
    supportedSizes: ['1x1', '2x1', '2x2'] as WidgetSize[],
  },
  'tapp-shortcut': {
    defaultSize: '1x1' as WidgetSize,
    component: TappShortcutWidget,
    supportedSizes: ['1x1', '2x1', '2x2'] as WidgetSize[],
  },
  'game-presence': {
    defaultSize: '4x2' as WidgetSize,
    component: GamePresenceWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'visitor-stats': {
    defaultSize: '4x2' as WidgetSize,
    component: VisitorStatsWidget,
    supportedSizes: ['2x2', '4x2'] as WidgetSize[],
  },
  // 三档尺寸与 Brew 页一致（logic/layout 的 BREW_TILE_SIZES）；
  // 首页由用户手摆，Brew 页由 tileSize() 派生，物理格子是同一套。
  'brew-source': {
    defaultSize: '4x2' as WidgetSize,
    component: BrewSourceWidget,
    supportedSizes: ['2x2', '4x2', '4x4'] as WidgetSize[],
  },
  'brew-topic': {
    defaultSize: '4x2' as WidgetSize,
    component: BrewTopicWidget,
    supportedSizes: ['2x2', '4x2', '4x4'] as WidgetSize[],
  },
  'brew-featured': {
    defaultSize: '4x4' as WidgetSize,
    component: BrewFeaturedWidget,
    supportedSizes: ['4x2', '4x4'] as WidgetSize[],
  },
} as const

export type BuiltinWidgetId = keyof typeof BUILTIN_WIDGET_BASE_CONFIG

/** Stable catalog order (library UI) */
const BUILTIN_WIDGET_ORDER: BuiltinWidgetId[] = [
  'welcome',
  'quick-stats',
  'recent-activity',
  'friend-links',
  'weather',
  'quote',
  'music-player',
  'report-bilibili',
  'report-steam',
  'report-github',
  'report-youtube',
  'report-netease',
  'report-bangumi',
  'report-mal',
  'report-x',
  'report-discord',
  'report-xbox',
  'report-psn',
  'social-network',
  'tapp-shortcut',
  'game-presence',
  'visitor-stats',
  'brew-featured',
  'brew-topic',
  'brew-source',
]

/** Map widget id → t.widgets key */
const WIDGET_NAME_KEY: Record<BuiltinWidgetId, keyof WidgetsI18n> = {
  welcome: 'welcome',
  'quick-stats': 'quickStats',
  'recent-activity': 'recentActivity',
  'friend-links': 'friendLinks',
  weather: 'weather',
  quote: 'quote',
  'music-player': 'musicPlayer',
  'report-bilibili': 'reportBilibili',
  'report-steam': 'reportSteam',
  'report-github': 'reportGithub',
  'report-youtube': 'reportYoutube',
  'report-netease': 'reportNetease',
  'report-bangumi': 'reportBangumi',
  'report-mal': 'reportMal',
  'report-x': 'reportX',
  'report-discord': 'reportDiscord',
  'report-xbox': 'reportXbox',
  'report-psn': 'reportPsn',
  'social-network': 'socialNetwork',
  'tapp-shortcut': 'tappShortcut',
  'game-presence': 'gamePresence',
  'visitor-stats': 'visitorStats',
  // id 是 kebab、i18n key 是 camel；getWidgetTranslationKey 也做这个转换，
  // 但这张表仍要显式写，别指望隐式推导。
  'brew-source': 'brewSource',
  'brew-topic': 'brewTopic',
  'brew-featured': 'brewFeatured',
}

/**
 * 预加载布局中用到的小组件实现（须在 setWidgets / 入场前 await）。
 * - report-*：壳 + **仅布局出现的平台 face**（非全平台）
 * - 其它：lazyWidget shared Promise
 */
export function preloadBuiltinWidgets(types: Iterable<string>): Promise<void> {
  const typeList = Array.from(types)
  const jobs: Promise<unknown>[] = []
  const seen = new Set<unknown>()
  let hasReport = false

  for (const type of typeList) {
    if (type.startsWith('report-')) {
      hasReport = true
      continue // 报告走下面专用预热，避免只 preload 壳漏 face
    }
    const base = BUILTIN_WIDGET_BASE_CONFIG[type as BuiltinWidgetId]
    if (!base || seen.has(base.component)) continue
    seen.add(base.component)
    const preload = (
      base.component as unknown as { preload?: () => Promise<unknown> }
    ).preload
    if (preload) jobs.push(preload().catch(() => {}))
  }

  if (hasReport) {
    jobs.push(preloadReportCardsForTypes(typeList).catch(() => {}))
  }

  return Promise.all(jobs).then(() => undefined)
}

/**
 * Built-in WidgetType[] with localized names.
 * Used by Home and ControlPanelWidgets (plus Tapp widgets merged by callers).
 */
export function getBuiltinWidgets(widgetsI18n: WidgetsI18n): WidgetType[] {
  return BUILTIN_WIDGET_ORDER.map((id) => {
    const base = BUILTIN_WIDGET_BASE_CONFIG[id]
    return {
      id,
      name: widgetsI18n[WIDGET_NAME_KEY[id]],
      defaultSize: base.defaultSize,
      component: base.component,
      supportedSizes: [...base.supportedSizes],
    }
  })
}
