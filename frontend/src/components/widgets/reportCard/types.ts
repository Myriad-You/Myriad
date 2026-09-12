import type { WidgetConfig } from '../../widgetGridTypes'

export interface LangSegment {
  name: string
  pct: number
  delay: number
  duration: number
}

export type ReportCardClickAction = 'report' | 'social'

export interface SteamPresence {
  personastate?: number
  personastate_label?: string
  is_online?: boolean
  is_in_game?: boolean
  gameextrainfo?: string | null
  gameid?: string | null
  avatar?: string | null
  personaname?: string | null
  recent_2weeks_minutes?: number | null
}

export interface ReportCardWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
  // 外部已给 card_visuals 时不再自行请求。
  data?: any
  bare?: boolean
  // 外部控制翻转时禁用内部 10s 轮播，与外部状态完全同步。
  showOverview?: boolean
  onConfigChange?: (newConfig: any) => void
}
