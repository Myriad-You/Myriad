import type { ComponentType } from 'react'
import type { TranslationKeys } from '../../i18n'
import type {
  WidgetComponentProps,
  WidgetPopupDef,
  WidgetSize,
  WidgetType,
} from '../widgetGridTypes'
import type { BuiltinWidgetHost, BuiltinWidgetId } from './builtinWidgetMeta'
import { lazyWithPreload } from '../../utils/codeSplitting'
import {
  BUILTIN_WIDGET_META,

  builtinWidgetVisibleOn,
} from './builtinWidgetMeta'
import {
  preloadReportCardsForTypes,
  ReportCardHost,
} from './reportCardHost'

// 报告卡走 ReportCardHost，禁止渲染期 lazy。
function lazyWidget<K extends string>(
  factory: () => Promise<Record<K, ComponentType<WidgetComponentProps>>>,
  name: K,
) {
  return lazyWithPreload(() => factory().then((m) => ({ default: m[name] })))
}

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
const MeropeWidget = lazyWidget(() => import('./MeropeWidget'), 'MeropeWidget')
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
const GithubReposWidget = lazyWidget(
  () => import('./GithubReposWidget'),
  'GithubReposWidget',
)

const COMPONENT_LONG_PRESS_WIDGETS = new Set<unknown>([
  ReportCardHost,
  SocialNetworkWidget,
  TappShortcutWidget,
  GamePresenceWidget,
])

type WidgetsI18n = TranslationKeys['widgets']

const WIDGET_COMPONENTS: Record<BuiltinWidgetId, ComponentType<WidgetComponentProps>> = {
  welcome: WelcomeWidget,
  'agent-persona': MeropeWidget,
  'quick-stats': QuickStatsWidget,
  'recent-activity': RecentActivityWidget,
  'friend-links': FriendLinksWidget,
  weather: WeatherWidget,
  quote: QuoteWidget,
  'music-player': MusicPlayerWidget,
  'report-bilibili': ReportCardHost,
  'report-steam': ReportCardHost,
  'report-github': ReportCardHost,
  'report-youtube': ReportCardHost,
  'report-netease': ReportCardHost,
  'report-bangumi': ReportCardHost,
  'report-mal': ReportCardHost,
  'report-x': ReportCardHost,
  'report-discord': ReportCardHost,
  'report-xbox': ReportCardHost,
  'report-psn': ReportCardHost,
  'social-network': SocialNetworkWidget,
  'tapp-shortcut': TappShortcutWidget,
  'game-presence': GamePresenceWidget,
  'visitor-stats': VisitorStatsWidget,
  'github-repos': GithubReposWidget,
}

export const BUILTIN_WIDGET_BASE_CONFIG = Object.fromEntries(
  BUILTIN_WIDGET_META.map((meta) => [
    meta.id,
    {
      defaultSize: meta.defaultSize,
      component: WIDGET_COMPONENTS[meta.id],
      supportedSizes: [...meta.supportedSizes],
      ...(meta.hosts ? { hosts: [...meta.hosts] } : {}),
    },
  ]),
) as Record<
  BuiltinWidgetId,
  {
    defaultSize: WidgetSize
    component: ComponentType<WidgetComponentProps>
    supportedSizes: WidgetSize[]
    hosts?: BuiltinWidgetHost[]
  }
>

export type { BuiltinWidgetHost, BuiltinWidgetId }

export function preloadBuiltinWidgets(types: Iterable<string>): Promise<void> {
  const typeList = Iterator.from(types).toArray()
  const jobs: Promise<unknown>[] = []
  const seen = new Set<unknown>()
  let hasReport = false

  for (const type of typeList) {
    if (type.startsWith('report-')) {
      hasReport = true
      continue
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

/** 报告卡的弹窗就是舞台。舞台自带关闭键，不要宿主角上那颗。 */
const ReportStagePopup = lazyWithPreload(() => import('./reportStage/ReportStagePopup'))
const REPORT_STAGE_POPUP: WidgetPopupDef = {
  component: ReportStagePopup,
  size: 'stage',
  chrome: 'none',
  preload: ReportStagePopup.preload,
  // 舞台能切到任何平台、能连播：所有报告卡共用一份，打开它的卡带上自己的平台。
  shareKey: 'report-stage',
}

/** 人设卡的弹窗：竖向名片，上面是会动的她，下面是她的人设。关闭键用宿主的。 */
const MeropePersonaPopup = lazyWithPreload(() => import('./MeropePersonaPopup'))
const PERSONA_POPUP: WidgetPopupDef = {
  component: MeropePersonaPopup,
  size: 'card',
  preload: MeropePersonaPopup.preload,
}

function builtinPopup(id: BuiltinWidgetId): WidgetPopupDef | undefined {
  if (id.startsWith('report-')) return REPORT_STAGE_POPUP
  if (id === 'agent-persona') return PERSONA_POPUP
  return undefined
}

export function getBuiltinWidgets(
  widgetsI18n: WidgetsI18n,
  host: BuiltinWidgetHost,
): WidgetType[] {
  return BUILTIN_WIDGET_META.filter((meta) => builtinWidgetVisibleOn(meta, host)).map((meta) => {
    const base = BUILTIN_WIDGET_BASE_CONFIG[meta.id]
    return {
      id: meta.id,
      name: widgetsI18n[meta.nameKey],
      defaultSize: base.defaultSize,
      component: base.component,
      preload: () => preloadBuiltinWidgets([meta.id]),
      supportedSizes: [...base.supportedSizes],
      componentLongPress:
        COMPONENT_LONG_PRESS_WIDGETS.has(base.component) || undefined,
      popup: builtinPopup(meta.id),
      settings:
        meta.id === 'github-repos'
          ? [
              {
                key: 'repo',
                type: 'input',
                label: widgetsI18n.githubReposField,
                description: widgetsI18n.githubReposFieldHint,
                placeholder: 'owner/repo',
                defaultValue: '',
              },
            ]
          : undefined,
    }
  })
}
