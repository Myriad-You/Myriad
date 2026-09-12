import type { TranslationKeys } from '../../../i18n'
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
import type { ReadingProgress } from './progressStore'

export type ReaderCopy = Pick<TranslationKeys, 'brew' | 'errors'>

export interface TocItem {
  id: string
  text: string
  level: number
}

export type ThemeKey = 'light' | 'sepia' | 'dark' | 'night'

export type LayoutKey = 'narrow' | 'wide'

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

export type FontLabelKey = 'fontSerif' | 'fontSans' | 'fontSystem'

export type LayoutLabelKey = 'layoutNarrow' | 'layoutWide'

export interface FontOption {
  id: string
  labelKey: FontLabelKey
  family: string
}

export interface LayoutOption {
  id: LayoutKey
  labelKey: LayoutLabelKey
  width: string
}

export interface BrewReaderProps {
  item: BrewItem
  onClose: () => void
  onToggleStar: () => void
  isAuthenticated?: boolean
  isAdmin?: boolean
  sourceType?: SourceType
}

export interface MobileReaderBarProps {
  item: BrewItem
  onClose: () => void
  onToggleStar: () => void
  isAuthenticated: boolean
  isAdmin: boolean
  isBrewlia: boolean

  theme: ThemeKey
  currentTheme: ThemeConfig
  isDark: boolean

  readingProgress: ReadingProgress

  showPanels: boolean
  showMobileControls: boolean
  setShowMobileControls: (show: boolean) => void

  toc: TocItem[]
  showToc: boolean
  setShowToc: (show: boolean) => void
  activeHeadingId: string
  scrollToHeading: (id: string) => void

  comments: CommentItem[]
  hasComments: boolean
  showCommentsPanel: boolean
  setShowCommentsPanel: (show: boolean) => void

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

  podcastDialogues: PodcastDialogue[]
  podcastLoading: boolean
  cloudTtsLoading: boolean
  podcastState: 'stopped' | 'playing' | 'paused'
  showPodcastPlayer: boolean
  setShowPodcastPlayer: (show: boolean) => void
  loadPodcast: () => void
  podcastCurrentIndex: number

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

  handlePlayPause: () => void
  handleStop: () => void
  handlePrevious: () => void
  handleNext: () => void
  handleDialogueClick: (index: number) => void

  cycleTheme: () => void
  cycleFont: () => void
  fontSize: number
  adjustFontSize: (delta: number) => void
  lineHeight: number
  adjustLineHeight: (delta: number) => void
  currentFont: FontOption

  handleShare: () => void

  /** 有值才能改：只有站长打开自己的手记时才传入。 */
  onEditNote?: () => void

  enableAnimations: boolean

  onTouchStart?: () => void
  onTouchEnd?: () => void

  t: ReaderCopy
}

export interface ReaderLeftPanelProps {
  item: BrewItem
  onClose: () => void
  isAuthenticated: boolean
  isAdmin: boolean
  isBrewlia: boolean
  /** 有值才能改：只有站长打开自己的手记时才传入。 */
  onEditNote?: () => void

  currentTheme: ThemeConfig
  isDark: boolean

  readingProgress: ReadingProgress

  showPanels: boolean

  toc: TocItem[]
  showToc: boolean
  setShowToc: (show: boolean) => void
  activeHeadingId: string
  scrollToHeading: (id: string) => void

  onToggleStar: () => void

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

  podcastDialogues: PodcastDialogue[]
  podcastLoading: boolean
  cloudTtsLoading: boolean
  podcastState: 'stopped' | 'playing' | 'paused'
  showPodcastPlayer: boolean
  setShowPodcastPlayer: (show: boolean) => void
  loadPodcast: () => void
  podcastCurrentIndex: number

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

  handlePlayPause: () => void
  handleStop: () => void
  handlePrevious: () => void
  handleNext: () => void
  handleDialogueClick: (index: number) => void

  handleProgressPointerDown: () => void
  handleProgressPointerUp: () => void
  handleProgressPointerLeave: () => void

  enableAnimations: boolean
  sideButtonClass: string

  onMouseEnter?: () => void
  onMouseLeave?: () => void

  t: ReaderCopy
}

export interface ReaderRightPanelProps {
  theme: ThemeKey
  currentTheme: ThemeConfig
  isDark: boolean

  showPanels: boolean

  cycleTheme: () => void
  cycleFont: () => void
  cycleLayout: () => void
  fontSize: number
  adjustFontSize: (delta: number) => void
  lineHeight: number
  adjustLineHeight: (delta: number) => void
  currentFont: FontOption
  currentLayout: LayoutOption

  handleShare: () => void

  enableAnimations: boolean
  sideButtonClass: string

  onMouseEnter?: () => void
  onMouseLeave?: () => void

  t: ReaderCopy
}
