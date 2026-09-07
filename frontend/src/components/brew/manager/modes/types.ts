/**
 * ControlIsland 模式组件共享类型
 */

import type { BrewSource } from '../../../../types/brew'

/** 排序模式 */
export type SortMode = 'update' | 'custom' | 'category' | 'random' | 'pinyin'

/** 控制岛模式 */
export type ControlMode =
  | 'default'
  | 'search'
  | 'edit'
  | 'keyboard'
  | 'add'
  | 'feed'
  | 'category-feed'
  | 'starred'
  | 'starred-edit'

/** 动态提示信息 */
export interface DynamicTip {
  icon: React.ReactNode
  iconUrl?: string
  main: string
  sub: string
}

/** 排序选项 */
export interface SortOption {
  value: SortMode
  labelKey: string
  icon: React.ReactNode
}

/** 导入进度 */
export interface ImportProgress {
  step: string
  current: number
  total: number
}

/** Feed 模式配置 */
export interface FeedModeConfig {
  source: BrewSource
  total: number
  onBack: () => void
  onRefresh: () => void
  onMarkAllRead: () => void
  isRefreshing?: boolean
}

/** 分类 Feed 模式配置 */
export interface CategoryFeedModeConfig {
  categoryName: string
  categoryLabel: string
  total: number
  unreadCount: number
  onBack: () => void
  onMarkAllRead: () => void
}

/** 收藏模式配置 */
export interface StarredModeConfig {
  total: number
  selectedIds: Set<number>
  isEditMode: boolean
  onBack: () => void
  onEnterEditMode: () => void
  onExitEditMode: () => void
  onSelectAll: () => void
  onBatchUnstar: () => void
  isProcessing?: boolean
}

export type { BrewExportManifest } from '../../../../types/brew'
