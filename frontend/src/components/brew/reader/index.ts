/**
 * Brew 阅读器子组件导出
 */

export { default as CommentsListPanel } from './CommentsListPanel'

// 常量导出
export {
  DATE_FORMAT_FULL,
  DATE_FORMAT_SHORT,
  FONT_OPTIONS,
  LAYOUT_OPTIONS,
  STYLE_MAX_HEIGHT_60VH,
  STYLE_MAX_HEIGHT_320,
  STYLE_READER_CONTAINER,
  STYLE_SCROLL_SMOOTH,
  STYLE_TRANSFORM_ORIGIN,
  THEME_ORDER,
  THEMES,
  TRANSITION_FAST,
  TRANSITION_NORMAL,
  TRANSITION_PANEL,
  TRANSITION_SLOW,
} from './constants'

// Hooks 导出
export {
  useAnnotations,
  useComments,
  usePodcast,
  useReaderSettings,
} from './hooks'
export type {
  SelectionRange,
  UseAnnotationsOptions,
  UseAnnotationsReturn,
  UseCommentsOptions,
  UseCommentsReturn,
  UsePodcastOptions,
  UsePodcastReturn,
  UseReaderSettingsReturn,
} from './hooks'

export { Lightbox } from './Lightbox'
// 组件导出
export { MobileReaderBar } from './MobileReaderBar'
export { default as ReaderLeftPanel } from './ReaderLeftPanel'
export { default as ReaderRightPanel } from './ReaderRightPanel'
export { AnnotationTooltip, CommentInputPopup, CommentTooltip } from './ReaderTooltips'
// 类型导出
export type {
  FontOption,
  LayoutKey,
  LayoutOption,
  MobileReaderBarProps,
  ReaderLeftPanelProps,
  ReaderRightPanelProps,
  ThemeConfig,
  ThemeKey,
  TocItem,
} from './types'
