import type { AnnotationItem, PodcastDialogue } from '../../../services/phantasiaiApi'
import type { CommentItem } from '../../../services/phantasiApi'
import type { ArticleCacheResponse, TTSEngine, VoiceInfo } from '../../../services/speechApi'
import type { ReaderTooltipAnchor } from './tooltipPlacement'

const noop = () => undefined
const asyncNoop = async () => undefined

const emptyVoices: VoiceInfo[] = []

export const idleAnnotations = {
  annotations: [] as AnnotationItem[],
  annotationsLoading: false,
  annotationsError: null as string | null,
  showAnnotations: false,
  selectedAnnotation: null as AnnotationItem | null,
  showPhantasiaiPanel: false,
  hoveredAnnotation: null as AnnotationItem | null,
  tooltipPosition: { x: 0, top: 0, bottom: 0 } as ReaderTooltipAnchor,
  setSelectedAnnotation: noop as (annotation: AnnotationItem | null) => void,
  setShowPhantasiaiPanel: noop as (show: boolean) => void,
  setHoveredAnnotation: noop as (annotation: AnnotationItem | null) => void,
  setTooltipPosition: noop as (anchor: ReaderTooltipAnchor) => void,
  loadAnnotations: asyncNoop,
  regenerateAnnotations: asyncNoop,
  toggleAnnotations: noop,
  scrollToAnnotation: noop as (
    annotation: AnnotationItem,
    contentRef: React.RefObject<HTMLDivElement | null>,
    articleRef: React.RefObject<HTMLElement | null>,
  ) => void,
  hoverTimeoutRef: { current: null as ReturnType<typeof setTimeout> | null },
}

export const idlePodcast = {
  podcastDialogues: [] as PodcastDialogue[],
  podcastLoading: false,
  showPodcastPlayer: false,
  podcastState: 'stopped' as const,
  podcastCurrentIndex: 0,
  ttsEngine: 'local' as TTSEngine,
  cloudTtsAvailable: null as boolean | null,
  cloudTtsError: null as string | null,
  cloudTtsLoading: false,
  cloudTtsLoadProgress: { loaded: 0, total: 0 },
  voiceList: emptyVoices,
  showVoiceSettings: false,
  hostVoiceId: undefined as number | undefined,
  guestVoiceId: undefined as number | undefined,
  articleCache: null as ArticleCacheResponse | null,
  articleCacheLoading: false,
  clearingVoiceId: null as number | null,
  setShowPodcastPlayer: noop as (show: boolean) => void,
  setShowVoiceSettings: noop as (show: boolean) => void,
  loadPodcast: asyncNoop,
  handleTtsEngineChange: asyncNoop as (engine: TTSEngine) => Promise<void>,
  handleVoiceChange: noop as (role: 'host' | 'guest', voiceId: number) => void,
  handleOpenSettings: noop,
  handleSwitchToVoice: asyncNoop as (voiceId: number, role: string) => Promise<void>,
  handleClearVoiceCache: asyncNoop as (voiceId: number) => Promise<void>,
  reloadCloudTTS: asyncNoop,
  handlePodcastPlay: asyncNoop,
  handlePodcastPause: noop,
  handlePodcastStop: noop,
  handlePodcastPrev: asyncNoop,
  handlePodcastNext: asyncNoop,
  handlePodcastSeek: asyncNoop as (index: number) => Promise<void>,
}

export const idleComments = {
  comments: [] as CommentItem[],
  commentsLoading: false,
  hasComments: false,
  canWrite: false,
  showCommentPopup: false,
  commentPopupPosition: { x: 0, y: 0 },
  selectedText: '',
  commentInput: '',
  commentSubmitting: false,
  showCommentsPanel: false,
  replyingTo: null as CommentItem | null,
  replyInput: '',
  replySubmitting: false,
  expandedComments: new Set<number>(),
  commentReplies: {} as Record<number, CommentItem[]>,
  commentTooltip: null as (ReaderTooltipAnchor & { comment: CommentItem }) | null,
  setShowCommentPopup: noop as (show: boolean) => void,
  setCommentPopupPosition: noop as (position: { x: number; y: number }) => void,
  setSelectedText: noop as (text: string) => void,
  setSelectionRange: noop as (
    range: {
      start: number
      end: number
      contextBefore: string
      contextAfter: string
    } | null,
  ) => void,
  setCommentInput: noop as (input: string) => void,
  setShowCommentsPanel: noop as (show: boolean) => void,
  setReplyingTo: noop as (comment: CommentItem | null) => void,
  setReplyInput: noop as (input: string) => void,
  setCommentTooltip: noop as (
    tooltip: (ReaderTooltipAnchor & { comment: CommentItem }) | null,
  ) => void,
  loadComments: asyncNoop,
  submitComment: asyncNoop,
  deleteComment: asyncNoop as (commentId: number) => Promise<void>,
  toggleReplies: asyncNoop as (commentId: number) => Promise<void>,
  submitReply: asyncNoop,
}
