/**
 * Brew 模块统一导出入口
 * 组件化架构，便于按需引入
 */

export { default as BrewFeedList } from './BrewFeedList'
export { default as BrewReader } from './BrewReader'
export { default as BrewSidebar } from './BrewSidebar'
// 主要容器组件
export { default as BrewSourceGrid } from './BrewSourceGrid'

// 卡片组件
export { CardSkeleton, EmptyState, ItemCard, SourceCard } from './cards'
export type { SourceCardProps } from './cards'

// 常量导出（排除与 reader 冲突的常量）
export {
  API_URL,
  DEFAULT_THEME_COLOR,
  getBase64Info,
  getFullPlainText,
  getIconUrl,
  getImageUrl,
  getPlainText,
  getSourceColor,
  isBase64Image,
  PRESET_CATEGORY_DB_VALUES,
  RESIZE_THRESHOLD,
  SHORT_CONTENT_THRESHOLD,
  SIZE_ORDER,
  SIZE_TO_ROWS,
  SPRING_SMOOTH,
  SPRING_SNAPPY,
  stripHtml,
  TRANSITION_QUICK,
} from './constants'
// 管理组件
export { ControlIsland, EditModal, RSSHubConfig } from './manager'

export type { SortMode } from './manager'

// 阅读器子组件
export * from './reader'

// 类型导出（排除与 cards 冲突的类型）
export type {
  BrewExportManifest,
  BrewItemTranslations,
  CardSkeletonProps,
  CategoryFeedModeConfig,
  ControlIslandProps,
  ControlMode,
  DynamicTip,
  EmptyStateProps,
  FeedModeConfig,
  ImportProgress,
  ItemCardProps,
  StarredModeConfig,
  TimeTranslations,
} from './types'
