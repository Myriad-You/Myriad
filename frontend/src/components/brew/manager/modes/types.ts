/**
 * ControlIsland 模式组件共享类型
 */

import type { BrewSource, FeedType, SourceType } from '../../../../types/brew'

/** 排序模式 */
export type SortMode =
  /** 六因子分档评分；主题卡插到最前。默认。 */
  | 'smart'
  /** 主题卡全部在前，再接源 */
  | 'topic'
  | 'update'
  | 'custom'
  | 'category'
  | 'random'
  | 'pinyin'

/** 控制岛模式 */
export type ControlMode =
  | 'default'
  | 'search'
  | 'edit'
  | 'keyboard'
  | 'add'
  | 'feed'
  | 'category-feed'
  | 'topic-feed'
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
  /** 置灰不可选（如主题不足 3 个时的 `topic`） */
  disabled?: boolean
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

/** 主题 Feed 模式配置 */
export interface TopicFeedModeConfig {
  topicKey: string
  topicLabel: string
  total: number
  onBack: () => void
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

/** Brew 导出清单 */
export interface BrewExportManifest {
  version: string
  exported_at: string
  sources: Array<{
    url: string
    name: string
    category: string | null
    icon_file: string | null
    icon_url: string | null
    source_type: SourceType
    feed_type: FeedType
    theme_color: string | null
    update_interval: number
    card_size: string | null
    rsshub_route: string | null
    ai_style_tags: string[] | null
    admin_only: boolean
  }>
}
