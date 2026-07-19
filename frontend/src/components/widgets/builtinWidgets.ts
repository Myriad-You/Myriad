/**
 * Shared built-in widget catalog for Home and Control Panel.
 * Single source of truth for ids, components, sizes, and i18n name keys.
 */

import type { TranslationKeys } from '../../i18n'
import type { WidgetSize, WidgetType } from '../WidgetGrid'
import { FriendLinksWidget } from './FriendLinksWidget'
import { GamePresenceWidget } from './GamePresenceWidget'
import { MusicPlayerWidget } from './MusicPlayerWidget'
import { QuickStatsWidget } from './QuickStatsWidget'
import { QuoteWidget } from './QuoteWidget'
import { RecentActivityWidget } from './RecentActivityWidget'
import { ReportCardWidget } from './ReportCardWidget'
import { SocialNetworkWidget } from './SocialNetworkWidget'
import { TappShortcutWidget } from './TappShortcutWidget'
import { WeatherWidget } from './WeatherWidget'
import { WelcomeWidget } from './WelcomeWidget'

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
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-steam': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-github': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-netease': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-bangumi': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-mal': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-x': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-discord': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-xbox': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
    supportedSizes: ['4x2'] as WidgetSize[],
  },
  'report-psn': {
    defaultSize: '4x2' as WidgetSize,
    component: ReportCardWidget,
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
