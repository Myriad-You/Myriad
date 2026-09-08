/**
 * Brew 模块共享类型定义
 */

import type {
  AddSourceInput,
  BrewItem,
  BrewSource,
  CardSize,
} from '../../types/brew'

// 排序类型

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

// 卡片组件类型

/** 订阅源卡片 Props */
export interface SourceCardProps {
  source: BrewSource
  index: number
  onSourceClick: (source: BrewSource) => void
  onRefreshSource: (sourceId: number) => void
  onThemeColorExtracted?: (sourceId: number, color: string) => void
  // 编辑模式
  isEditMode?: boolean
  isSelected?: boolean
  isDeleting?: boolean
  onEdit?: () => void
  onDelete?: () => void
  // 尺寸调整
  onResizeStart?: (
    e: React.MouseEvent | React.TouchEvent,
    sourceId: number,
  ) => void
  previewSize?: CardSize
  // 拖拽排序
  isDragging?: boolean
  isDragOver?: boolean
  onDragStart?: (
    e: React.MouseEvent | React.TouchEvent,
    sourceId: number,
  ) => void
  sortMode?: SortMode
  /**
   * 已登录才有真实已读态。游客侧 `list_sources` 用 `user_id = -1` 做 LEFT JOIN，
   * `recent_items[].is_read` 恒为 false —— 未读圆点必须按角色隐藏，不是改后端 SQL。
   */
  isAuthenticated?: boolean
}

/** 文章条目卡片 Props */
export interface ItemCardProps {
  item: BrewItem
  index: number
  isSelected: boolean
  isLast: boolean
  themeColor: string
  onItemSelect: (item: BrewItem) => void
  onToggleStar: (item: BrewItem) => void
  lastItemRef?: (node: HTMLDivElement | null) => void
  // 编辑模式
  editMode?: boolean
  isChecked?: boolean
  onToggleCheck?: (id: number) => void
  // 权限
  isAuthenticated?: boolean
  // 国际化
  timeTranslations: TimeTranslations
  brewTranslations: BrewItemTranslations
  locale: string
}

/** 时间格式化翻译 */
export interface TimeTranslations {
  justNow: string
  minutesAgo: string
  hoursAgo: string
  daysAgo: string
}

/** 文章条目翻译 */
export interface BrewItemTranslations {
  hasAnnotations: string
  annotationsLabel: string
  hasPodcast: string
  podcastLabel: string
  unstarArticle: string
  starArticle: string
  openInNewTab: string
}

// 管理组件类型

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
  icon: string
  iconUrl?: string
  main: string
  sub: string
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

/** 控制岛 Props */
export interface ControlIslandProps {
  sources: BrewSource[]
  filteredSources: BrewSource[]
  categories: string[]
  // 搜索
  searchQuery?: string
  setSearchQuery?: (query: string) => void
  // 编辑模式
  selectedIds?: Set<number>
  onSelectAll?: () => void
  onBatchDelete?: () => void
  onBatchRefresh?: () => void
  onMarkAllSourcesRead?: () => void
  onEnterEditMode?: () => void
  onExitEditMode?: () => void
  isEditMode?: boolean
  isDeleting?: boolean
  isRefreshing?: boolean
  // 添加
  onAddSource?: (input: AddSourceInput) => Promise<void>
  onSourcesChange?: () => void
  // 排序
  sortMode?: SortMode
  onSortModeChange?: (mode: SortMode) => void
  isSubCategory?: boolean
  // 模式配置
  feedMode?: FeedModeConfig
  categoryFeedMode?: CategoryFeedModeConfig
  topicFeedMode?: TopicFeedModeConfig
  starredMode?: StarredModeConfig
  // 权限
  isAdmin?: boolean
  isAuthenticated?: boolean
  /** 当期成卡的主题数；< 3 时 `topic` 排序置灰 */
  topicCount?: number
}

// 导入/导出类型

export type { BrewExportManifest } from '../../types/brew'

/** 导入进度 */
export interface ImportProgress {
  step: string
  current: number
  total: number
}
