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
  game_cover?: string | null
  avatar?: string | null
  personaname?: string | null
  recent_2weeks_minutes?: number | null
}

export interface ReportCardWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
  onConfigChange?: (newConfig: any) => void
  /** 打开这张卡的舞台弹窗。 */
  openPopup?: (params?: unknown) => void
}
