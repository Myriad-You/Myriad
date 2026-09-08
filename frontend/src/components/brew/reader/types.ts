/**
 * Brew 阅读器共享类型定义
 */

import type { CommentItem } from '../../../services/brewApi'
import type {
  AnnotationItem,
  PodcastDialogue,
} from '../../../services/brewliaApi'
import type {
  ArticleCacheResponse,
  TTSEngine,
  VoiceInfo,
} from '../../../services/speechApi'
import type { BrewItem, SourceType } from '../../../types/brew'

// 目录项类型
export interface TocItem {
  id: string
  text: string
  level: number
}

// 主题类型
export type ThemeKey = 'light' | 'sepia' | 'dark' | 'night'

// 布局类型
export type LayoutKey = 'narrow' | 'wide'

// 主题配置类型
export interface ThemeConfig {
  bg: string
  text: string
  secondary: string
  border: string
  surface: string
  surfaceSolid: string
  accent: string
  icon: string
}

// 字体选项类型
export interface FontOption {
  id: string
  labelKey: string
  family: string
}

// 布局选项类型
export interface LayoutOption {
  id: LayoutKey
  labelKey: string
  width: string
}

// 阅读器 Props
export interface BrewReaderProps {
  item: BrewItem
  onClose: () => void
  onToggleStar: () => void
  isAuthenticated?: boolean
  isAdmin?: boolean
  sourceType?: SourceType
}

// 移动端底栏 Props
export interface MobileReaderBarProps {
  // 基础信息
  item: BrewItem
  onClose: () => void
  onToggleStar: () => void
  isAuthenticated: boolean
  isAdmin: boolean
  isBrewlia: boolean

  // 主题
  theme: ThemeKey
  currentTheme: ThemeConfig
  isDark: boolean

  // 阅读进度
  readingProgress: number

  // 面板控制
  showPanels: boolean
  showMobileControls: boolean
  setShowMobileControls: (show: boolean) => void

  // 目录
  toc: TocItem[]
  showToc: boolean
  setShowToc: (show: boolean) => void
  activeHeadingId: string
  scrollToHeading: (id: string) => void

  // 收藏
  // (onToggleStar 已包含)

  // 评论
  comments: CommentItem[]
  hasComments: boolean
  showCommentsPanel: boolean
  setShowCommentsPanel: (show: boolean) => void

  // AI 注释
  annotations: AnnotationItem[]
  annotationsLoading: boolean
  showAnnotations: boolean
  showBrewliaPanel: boolean
  setShowBrewliaPanel: (show: boolean) => void
  toggleAnnotations: () => void
  loadAnnotations: () => void
  regenerateAnnotations: () => void
  annotationsError: string | null
  selectedAnnotation: AnnotationItem | null
  setSelectedAnnotation: (annotation: AnnotationItem | null) => void
  scrollToAnnotation: (annotation: AnnotationItem) => void

  // AI 播客
  podcastDialogues: PodcastDialogue[]
  podcastLoading: boolean
  cloudTtsLoading: boolean
  podcastState: 'stopped' | 'playing' | 'paused'
  showPodcastPlayer: boolean
  setShowPodcastPlayer: (show: boolean) => void
  loadPodcast: () => void
  podcastCurrentIndex: number

  // TTS 设置
  ttsEngine: TTSEngine
  handleTtsEngineChange: (engine: TTSEngine) => void
  cloudTtsAvailable: boolean | null
  cloudTtsError: string | null
  cloudTtsLoadProgress: { loaded: number; total: number }
  voiceList: VoiceInfo[]
  showVoiceSettings: boolean
  setShowVoiceSettings: (show: boolean) => void
  hostVoiceId: number | undefined
  guestVoiceId: number | undefined
  handleVoiceSelect: (role: 'host' | 'guest', voiceId: number) => void
  handleOpenSettings: () => void
  articleCache: ArticleCacheResponse | null
  articleCacheLoading: boolean
  clearingVoiceId: number | null
  handleClearVoiceCache: (voiceId: number) => void
  handleSwitchToCachedVoice: (voiceId: number, role: string) => void
  reloadCloudTts: () => void

  // 播客控制
  handlePlayPause: () => void
  handleStop: () => void
  handlePrevious: () => void
  handleNext: () => void
  handleDialogueClick: (index: number) => void

  // 阅读设置
  cycleTheme: () => void
  cycleFont: () => void
  fontSize: number
  adjustFontSize: (delta: number) => void
  lineHeight: number
  adjustLineHeight: (delta: number) => void
  currentFont: FontOption

  // 分享
  handleShare: () => void

  /**
   * 编辑这篇手记。只有站长打开自己写的那篇时才有值 —— 抓来的文章不能改，
   * 所以这个回调的存在与否本身就是「能不能改」的答案。
   */
  onEditNote?: () => void

  // 动画
  enableAnimations: boolean

  // 触摸悬停回调（用于阻止自动隐藏）
  onTouchStart?: () => void
  onTouchEnd?: () => void

  // 翻译
  t: Record<string, any>
}

// 左侧控制栏 Props
export interface ReaderLeftPanelProps {
  // 基础信息
  item: BrewItem
  onClose: () => void
  isAuthenticated: boolean
  isAdmin: boolean
  isBrewlia: boolean
  /**
   * 编辑这篇手记。只有站长打开自己写的那篇时才有值 —— 抓来的文章不能改，
   * 所以这个回调的存在与否本身就是「能不能改」的答案。
   */
  onEditNote?: () => void

  // 主题
  currentTheme: ThemeConfig
  isDark: boolean

  // 阅读进度
  readingProgress: number

  // 面板控制
  showPanels: boolean

  // 目录
  toc: TocItem[]
  showToc: boolean
  setShowToc: (show: boolean) => void
  activeHeadingId: string
  scrollToHeading: (id: string) => void

  // 收藏
  onToggleStar: () => void

  // AI 注释
  annotations: AnnotationItem[]
  annotationsLoading: boolean
  showAnnotations: boolean
  showBrewliaPanel: boolean
  setShowBrewliaPanel: (show: boolean) => void
  toggleAnnotations: () => void
  loadAnnotations: () => void
  regenerateAnnotations: () => void
  annotationsError: string | null
  selectedAnnotation: AnnotationItem | null
  setSelectedAnnotation: (annotation: AnnotationItem | null) => void
  scrollToAnnotation: (annotation: AnnotationItem) => void

  // AI 播客
  podcastDialogues: PodcastDialogue[]
  podcastLoading: boolean
  cloudTtsLoading: boolean
  podcastState: 'stopped' | 'playing' | 'paused'
  showPodcastPlayer: boolean
  setShowPodcastPlayer: (show: boolean) => void
  loadPodcast: () => void
  podcastCurrentIndex: number

  // TTS 设置
  ttsEngine: TTSEngine
  handleTtsEngineChange: (engine: TTSEngine) => void
  cloudTtsAvailable: boolean | null
  cloudTtsError: string | null
  cloudTtsLoadProgress: { loaded: number; total: number }
  voiceList: VoiceInfo[]
  showVoiceSettings: boolean
  setShowVoiceSettings: (show: boolean) => void
  hostVoiceId: number | undefined
  guestVoiceId: number | undefined
  handleVoiceSelect: (role: 'host' | 'guest', voiceId: number) => void
  handleOpenSettings: () => void
  articleCache: ArticleCacheResponse | null
  articleCacheLoading: boolean
  clearingVoiceId: number | null
  handleClearVoiceCache: (voiceId: number) => void
  handleSwitchToCachedVoice: (voiceId: number, role: string) => void
  reloadCloudTts: () => void

  // 播客控制
  handlePlayPause: () => void
  handleStop: () => void
  handlePrevious: () => void
  handleNext: () => void
  handleDialogueClick: (index: number) => void

  // 进度按钮长按
  handleProgressPointerDown: () => void
  handleProgressPointerUp: () => void
  handleProgressPointerLeave: () => void

  // 动画
  enableAnimations: boolean
  sideButtonClass: string

  // 鼠标悬停回调（用于阻止自动隐藏）
  onMouseEnter?: () => void
  onMouseLeave?: () => void

  // 翻译
  t: Record<string, any>
}

// 右侧控制栏 Props
export interface ReaderRightPanelProps {
  // 主题
  theme: ThemeKey
  currentTheme: ThemeConfig
  isDark: boolean

  // 面板控制
  showPanels: boolean

  // 阅读设置
  cycleTheme: () => void
  cycleFont: () => void
  cycleLayout: () => void
  fontSize: number
  adjustFontSize: (delta: number) => void
  lineHeight: number
  adjustLineHeight: (delta: number) => void
  currentFont: FontOption
  currentLayout: LayoutOption

  // 分享
  handleShare: () => void

  // 动画
  enableAnimations: boolean
  sideButtonClass: string

  // 鼠标悬停回调（用于阻止自动隐藏）
  onMouseEnter?: () => void
  onMouseLeave?: () => void

  // 翻译
  t: Record<string, any>
}
