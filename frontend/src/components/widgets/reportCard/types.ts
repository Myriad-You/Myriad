/** Shared types for report card platform widgets. */
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
  /** 外部直接提供 card_visuals，提供时不再自行请求（用于报告页复用） */
  data?: any
  /** 去掉自带 glass 外壳与背景光效，供已有外壳的容器内嵌 */
  bare?: boolean
  /**
   * 外部控制概览/详情切换（如舞台模式按篇章驱动）。
   * 传入后禁用内部 10s 自动轮播，与外部状态完全同步。
   */
  showOverview?: boolean
  /** 小组件配置变更回调（用于持久化长按设置） */
  onConfigChange?: (newConfig: any) => void
}
