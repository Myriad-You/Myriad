import type { AnnotationItem } from '../../services/brewliaApi'
import type { BrewItem, SourceType } from '../../types/brew'
import type { ReadingQueue } from './logic/readingQueue'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useImmersiveChrome } from '../../contexts/NavigationContext'
import { usePageContentOptional } from '../../contexts/PageContentContext'
import {
  brewAnimationPresets,
  getBrewTransition,
  useBrewAnimationConfig,
} from '../../hooks/animation'
import { isExlight } from '../../hooks/useAnimationLevel'
import { authSubject } from '../../utils/authSubject'
import { userFacingError } from '../../utils/userFacingError'
import {
  AnnotationTooltip,
  CommentInputPopup,
  CommentsListPanel,
  CommentTooltip,
  Lightbox,
  MobileReaderBar,
  ReaderArticleBody,
  ReaderLeftPanel,
  ReaderProgressRail,
  ReaderRightPanel,
  STYLE_READER_CONTAINER,
  STYLE_SCROLL_SMOOTH,
  useAnnotations,
  useComments,
  useContentEvents,
  useContentPostprocess,
  useContentRender,
  usePodcast,
  useReaderChrome,
  useReaderControls,
  useReaderSettings,
} from './reader'
import { dismissReaderChrome, escapeWhileTyping } from './reader/readerPanels'
import './ui/brew.css'
import './skin/brew-reader.css'

interface BrewReaderProps {
  item: BrewItem
  onClose: () => void
  onToggleStar: () => void
  isAuthenticated?: boolean
  isAdmin?: boolean
  sourceType?: SourceType
  /** 自有内容用 `/brew/item/{id}`；缺省复制原文 link，勿把外站当本站 SEO 页分享。 */
  shareUrl?: string
  /** 上层只在「站长 + 手记」时传值，阅读器不自己判断。 */
  onEditNote?: () => void
  onNavigateToArticle?: (articleId: number) => void
  readingQueue?: ReadingQueue | null
}

/** Persistent settings stay here; article resources live in the keyed child. */
export default function BrewReader(props: BrewReaderProps) {
  const settings = useReaderSettings()
  const [displayed, setDisplayed] = useState(props)
  const columnRef = useRef<HTMLDivElement>(null)
  const animation = useRef<Animation | null>(null)
  const config = useBrewAnimationConfig()
  const enabled = !isExlight(config)
  const sameArticle = props.item.id === displayed.item.id
  if (sameArticle && props !== displayed) setDisplayed(props)
  const visible = sameArticle ? props : displayed
  const switched = useRef(false)

  useEffect(() => {
    animation.current?.cancel()
    animation.current = null
    if (sameArticle) return
    let live = true
    const commit = () => {
      if (!live) return
      live = false
      switched.current = true
      setDisplayed(props)
    }
    const column = columnRef.current
    if (!enabled || !column || typeof column.animate !== 'function') {
      commit()
      return
    }
    const fade = column.animate([
      { opacity: 1, transform: 'translateY(0)' },
      { opacity: 0, transform: 'translateY(-6px)' },
    ], { duration: 140, easing: 'ease-in', fill: 'forwards' })
    animation.current = fade
    fade.onfinish = commit
    const failSafe = window.setTimeout(commit, 220)
    return () => {
      live = false
      window.clearTimeout(failSafe)
      fade.onfinish = null
      fade.cancel()
    }
  }, [props, sameArticle, enabled])

  useLayoutEffect(() => {
    const column = columnRef.current
    if (!enabled || !column || typeof column.animate !== 'function') return
    const enter = column.animate([
      { opacity: 0, transform: 'translateY(8px)' },
      { opacity: 1, transform: 'translateY(0)' },
    ], { duration: 260, easing: 'cubic-bezier(0.16, 1, 0.3, 1)' })
    return () => enter.cancel()
  }, [displayed.item.id, enabled])

  return <ReaderArticleSession
    key={visible.item.id}
    {...visible}
    settings={settings}
    columnRef={columnRef}
    articleSwap={switched.current}
         />
}

function ReaderArticleSession({
  item,
  onClose,
  onToggleStar,
  isAuthenticated = false,
  isAdmin = false,
  sourceType,
  shareUrl,
  onEditNote,
  onNavigateToArticle,
  readingQueue,
  settings,
  columnRef,
  articleSwap,
}: BrewReaderProps & {
  settings: ReturnType<typeof useReaderSettings>
  columnRef: React.RefObject<HTMLDivElement | null>
  articleSwap: boolean
}) {
  const { t } = useI18n()
  const contentRef = useRef<HTMLDivElement>(null)
  const articleRef = useRef<HTMLElement>(null)
  const contentInnerRef = useRef<HTMLDivElement>(null)

  useImmersiveChrome('brew-reader', true)

  const { setPageContent, clearPageContent } = usePageContentOptional() ?? {}
  // This article session is keyed by item id; retained old content is not a new observation.
  const [contentSubject] = useState(() => authSubject.signal)

  useEffect(() => {
    if (contentSubject.aborted) return
    if (setPageContent && item) {
      const articleContent = item.content || item.summary || ''

      setPageContent({
        type: 'brew_article',
        title: item.title,
        content: articleContent,
        sourceUrl: item.link,
        author: item.author || undefined,
        publishedAt: item.published_at
          ? new Date(item.published_at).toISOString()
          : undefined,
        metadata: {
          sourceId: item.source_id,
          sourceName: item.source_name,
          sourceType,
          isBrewlia: sourceType === 'brewlia',
        },
      })

      console.log('[BrewReader] Page content set:', {
        title: item.title,
        hasContent: !!articleContent,
        contentLength: articleContent?.length || 0,
      })
    }

    return () => {
      if (clearPageContent) {
        clearPageContent()
      }
    }
  }, [item, sourceType, setPageContent, clearPageContent, contentSubject])

  const animConfig = useBrewAnimationConfig()
  const readerTransition = useMemo(
    () => getBrewTransition(animConfig, 'reader'),
    [animConfig],
  )
  const enableAnimations = !isExlight(animConfig)

  // WebKit：入场动画完成后再灌正文。
  const [contentReady, setContentReady] = useState(articleSwap || !enableAnimations)

  const isBrewlia = sourceType === 'brewlia'

  const {
    fontSize,
    lineHeight,
    theme,
    layout,
    currentTheme,
    currentFont,
    currentLayout,
    isDark,
    adjustFontSize,
    adjustLineHeight,
    cycleTheme,
    cycleFont,
    cycleLayout,
  } = settings

  const [showToast, setShowToast] = useState<string | null>(null)
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const [lightboxImage, setLightboxImage] = useState<string | null>(null)

  const showToastMessage = useCallback((message: string, duration = 2000) => {
    if (toastTimerRef.current) {
      clearTimeout(toastTimerRef.current)
    }
    setShowToast(message)
    toastTimerRef.current = setTimeout(() => {
      setShowToast(null)
      toastTimerRef.current = null
    }, duration)
  }, [])

  useEffect(() => {
    return () => {
      if (toastTimerRef.current) {
        clearTimeout(toastTimerRef.current)
      }
    }
  }, [])

  const {
    annotations,
    annotationsLoading,
    annotationsError,
    showAnnotations,
    selectedAnnotation,
    showBrewliaPanel,
    hoveredAnnotation,
    tooltipPosition,
    setSelectedAnnotation,
    setShowBrewliaPanel,
    setHoveredAnnotation,
    setTooltipPosition,
    loadAnnotations,
    regenerateAnnotations,
    toggleAnnotations,
    scrollToAnnotation,
    hoverTimeoutRef,
  } = useAnnotations({
    itemId: item.id,
    isBrewlia,
    showToastMessage,
    t,
  })

  const {
    comments,
    commentsLoading,
    hasComments,
    showCommentPopup,
    commentPopupPosition,
    selectedText,
    commentInput,
    commentSubmitting,
    showCommentsPanel,
    replyingTo,
    replyInput,
    replySubmitting,
    expandedComments,
    commentReplies,
    commentTooltip,
    setShowCommentPopup,
    setCommentPopupPosition,
    setSelectedText,
    setSelectionRange,
    setCommentInput,
    setShowCommentsPanel,
    setReplyingTo,
    setReplyInput,
    setCommentTooltip,
    loadComments,
    submitComment,
    deleteComment,
    toggleReplies,
    submitReply,
    highlightComments,
  } = useComments({
    itemId: item.id,
    isAuthenticated,
    showToastMessage,
    t,
  })

  const {
    podcastDialogues,
    podcastLoading,
    showPodcastPlayer,
    podcastState,
    podcastCurrentIndex,
    ttsEngine,
    cloudTtsAvailable,
    cloudTtsError,
    cloudTtsLoading,
    cloudTtsLoadProgress,
    voiceList,
    showVoiceSettings,
    hostVoiceId,
    guestVoiceId,
    articleCache,
    articleCacheLoading,
    clearingVoiceId,
    setShowPodcastPlayer,
    setShowVoiceSettings,
    loadPodcast,
    handleTtsEngineChange,
    handleVoiceChange,
    handleOpenSettings,
    handleSwitchToVoice,
    handleClearVoiceCache,
    reloadCloudTTS,
    handlePodcastPlay,
    handlePodcastPause,
    handlePodcastStop,
    handlePodcastPrev,
    handlePodcastNext,
    handlePodcastSeek,
  } = usePodcast({
    itemId: item.id,
    sourceId: item.source_id,
    isBrewlia,
    showToastMessage,
    t,
  })

  const sideButtonClass = 'brew-reader__btn'

  const {
    readingProgress,
    toc,
    setToc,
    showToc,
    setShowToc,
    activeHeadingId,
    scrollToHeading,
    handleProgressPointerDown,
    handleProgressPointerUp,
    handleProgressPointerLeave,
    syncPaused,
    recoverLocalProgress,
    recoverRemoteProgress,
  } = useReaderControls({
    articleRef,
    contentRef,
    itemId: item.id,
    readProgress: item.read_progress,
    stateRevision: item.state_revision,
    contentReady,
    isAuthenticated: isAuthenticated && !item.fromWebSearch,
    adjustFontSize,
    showToastMessage,
    t,
  })

  const [focusedCommentIds, setFocusedCommentIds] = useState<number[]>([])
  const [unresolvedCommentIds, setUnresolvedCommentIds] = useState<Set<number>>(new Set())
  useEffect(() => {
    if (!showCommentsPanel) setFocusedCommentIds([])
  }, [showCommentsPanel])

  const baseContent = useContentRender({
    contentInnerRef,
    contentReady,
    item,
    t,
    showAnnotations,
    annotations,
    comments,
    highlightComments,
    theme,
  })

  useEffect(() => {
    if (!baseContent || !contentInnerRef.current) return
    const marked = new Set(
      Iterator.from(contentInnerRef.current.querySelectorAll('[data-comment-id]'))
        .map((node) => Number(node.getAttribute('data-comment-id'))),
    )
    setUnresolvedCommentIds(new Set(comments.filter(comment => comment.selected_text && !comment.parent_id && !marked.has(comment.id)).map(comment => comment.id)))
  }, [baseContent, comments, annotations, showAnnotations, theme, item.content_revision])

  useContentPostprocess({
    contentRef,
    baseContent,
    showAnnotations,
    annotations,
    comments,
    theme,
    copyCodeLabel: t.brew.copyCode,
    setToc,
  })

  // WebKit：入场动画完成后再灌正文。
  useEffect(() => {
    if (!enableAnimations) {
      setContentReady(true)
      return
    }

    // 延迟略长于动画时长。
    const delay = readerTransition.duration * 1000 + 50
    const timer = setTimeout(() => {
      requestAnimationFrame(() => {
        setContentReady(true)
      })
    }, delay)

    return () => clearTimeout(timer)
  }, [enableAnimations, readerTransition.duration])

  const { handleTooltipMouseEnter, handleTooltipMouseLeave } = useContentEvents({
    contentRef: contentInnerRef,
    setFocusedCommentIds,
    comments,
    isAuthenticated,
    showCommentPopup,
    showAnnotations,
    hoverTimeoutRef,
    setLightboxImage,
    setCommentTooltip,
    setShowCommentsPanel,
    setHoveredAnnotation,
    setTooltipPosition,
    setSelectedText,
    setCommentPopupPosition,
    setSelectionRange,
    setShowCommentPopup,
    setCommentInput,
    showToastMessage,
    t,
  })

  useEffect(() => {
    if (isBrewlia && annotations.length === 0 && !annotationsLoading) {
      const timer = setTimeout(() => {
        loadAnnotations()
      }, 500)
      return () => clearTimeout(timer)
    }
  }, [isBrewlia])

  useEffect(() => {
    if (isAuthenticated && comments.length === 0 && !commentsLoading) {
      const timer = setTimeout(() => {
        loadComments()
      }, 300)
      return () => clearTimeout(timer)
    }
  }, [isAuthenticated])

  const handleScrollToAnnotation = useCallback(
    (annotation: AnnotationItem) => {
      scrollToAnnotation(annotation, contentRef, articleRef)
    },
    [scrollToAnnotation],
  )

  const closeAllTooltips = useCallback(() => {
    setShowToc(false)
    setShowBrewliaPanel(false)
    setShowPodcastPlayer(false)
    setShowVoiceSettings(false)
    setHoveredAnnotation(null)
    setCommentTooltip(null)
  }, [])

  const {
    showPanels,
    showMobileControls,
    setShowMobileControls,
    isHoveringControlsRef,
    resetHideTimer,
  } = useReaderChrome({
    articleRef,
    layout,
    closeAllTooltips,
  })

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      const typing = Boolean(
        event.target instanceof HTMLElement &&
          event.target.closest(
            'input, textarea, select, [contenteditable="true"]',
          ),
      )
      const layer = dismissReaderChrome({
        lightbox: !!lightboxImage,
        popup: showCommentPopup,
        comments: showCommentsPanel,
        toc: showToc,
        brewlia: showBrewliaPanel,
        voice: showVoiceSettings,
        podcast: showPodcastPlayer,
        controls: showMobileControls,
      })
      const next = typing ? escapeWhileTyping(layer) : layer
      if (!next) {
        if (!typing) onClose()
        return
      }
      if (next === 'lightbox') setLightboxImage(null)
      else if (next === 'popup') setShowCommentPopup(false)
      else if (next === 'comments') setShowCommentsPanel(false)
      else if (next === 'toc') setShowToc(false)
      else if (next === 'annotations') setShowBrewliaPanel(false)
      else if (next === 'voice') setShowVoiceSettings(false)
      else if (next === 'podcast') setShowPodcastPlayer(false)
      else if (next === 'controls') setShowMobileControls(false)
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [
    lightboxImage,
    showCommentPopup,
    showCommentsPanel,
    showToc,
    showBrewliaPanel,
    showVoiceSettings,
    showPodcastPlayer,
    showMobileControls,
    onClose,
    setShowCommentPopup,
    setShowCommentsPanel,
    setShowToc,
    setShowBrewliaPanel,
    setShowVoiceSettings,
    setShowPodcastPlayer,
    setShowMobileControls,
  ])

  const handleShare = async () => {
    try {
      const url = (shareUrl && shareUrl.trim()) || item.link
      await navigator.clipboard.writeText(url)
      showToastMessage(t.brew.linkCopied)
    } catch (err) {
      console.error('Failed to copy:', err)
      showToastMessage(userFacingError(err, t.errors.clipboardFailed))
    }
  }

  const readerAnimProps = useMemo(() => {
    if (!enableAnimations) {
      return {
        initial: false as const,
        animate: undefined,
        exit: undefined,
        transition: undefined,
      }
    }

    return {
      initial: articleSwap ? false : brewAnimationPresets.readerEnter.initial,
      animate: brewAnimationPresets.readerEnter.animate,
      exit: brewAnimationPresets.readerEnter.exit,
      transition: {
        duration: readerTransition.duration,
        ease: readerTransition.ease,
      },
    }
  }, [enableAnimations, readerTransition, articleSwap])

  return (
    <motion.div
      {...readerAnimProps}
      style={STYLE_READER_CONTAINER}
      className={`brew-skin brew-reader fixed inset-0 z-50 ${currentTheme.bg}`}
      data-brew-reader="true"
      data-brew-theme={theme}
    >
      {/* 进度条用 scaleX，避免 width 触发布局。 */}
      <ReaderProgressRail progress={readingProgress} />

      {/* translateZ(0) 独立合成层，避免 sticky 子元素回流。 */}
      <article
        ref={articleRef}
        className="h-full overflow-y-auto overflow-x-hidden"
        style={{
          ...STYLE_SCROLL_SMOOTH,
          transform: 'translateZ(0)',
          overflowAnchor: 'none',
        }}
      >
        <div className="flex justify-center">
          <ReaderLeftPanel
            item={item}
            onClose={onClose}
            isAuthenticated={isAuthenticated || false}
            isAdmin={isAdmin || false}
            isBrewlia={isBrewlia}
            currentTheme={currentTheme}
            isDark={isDark}
            readingProgress={readingProgress}
            showPanels={showPanels}
            toc={toc}
            showToc={showToc}
            setShowToc={setShowToc}
            activeHeadingId={activeHeadingId}
            scrollToHeading={scrollToHeading}
            onToggleStar={onToggleStar}
            onEditNote={onEditNote}
            annotations={annotations}
            annotationsLoading={annotationsLoading}
            showAnnotations={showAnnotations}
            showBrewliaPanel={showBrewliaPanel}
            setShowBrewliaPanel={setShowBrewliaPanel}
            toggleAnnotations={toggleAnnotations}
            loadAnnotations={loadAnnotations}
            regenerateAnnotations={regenerateAnnotations}
            annotationsError={annotationsError}
            selectedAnnotation={selectedAnnotation}
            setSelectedAnnotation={setSelectedAnnotation}
            scrollToAnnotation={handleScrollToAnnotation}
            podcastDialogues={podcastDialogues}
            podcastLoading={podcastLoading}
            cloudTtsLoading={cloudTtsLoading}
            podcastState={podcastState}
            showPodcastPlayer={showPodcastPlayer}
            setShowPodcastPlayer={setShowPodcastPlayer}
            loadPodcast={loadPodcast}
            podcastCurrentIndex={podcastCurrentIndex}
            ttsEngine={ttsEngine}
            handleTtsEngineChange={handleTtsEngineChange}
            cloudTtsAvailable={cloudTtsAvailable}
            cloudTtsError={cloudTtsError}
            cloudTtsLoadProgress={cloudTtsLoadProgress}
            voiceList={voiceList}
            showVoiceSettings={showVoiceSettings}
            setShowVoiceSettings={setShowVoiceSettings}
            hostVoiceId={hostVoiceId}
            guestVoiceId={guestVoiceId}
            handleVoiceSelect={handleVoiceChange}
            handleOpenSettings={handleOpenSettings}
            articleCache={articleCache}
            articleCacheLoading={articleCacheLoading}
            clearingVoiceId={clearingVoiceId}
            handleClearVoiceCache={handleClearVoiceCache}
            handleSwitchToCachedVoice={handleSwitchToVoice}
            reloadCloudTts={reloadCloudTTS}
            handlePlayPause={
              podcastState === 'playing'
                ? handlePodcastPause
                : handlePodcastPlay
            }
            handleStop={handlePodcastStop}
            handlePrevious={handlePodcastPrev}
            handleNext={handlePodcastNext}
            handleDialogueClick={handlePodcastSeek}
            handleProgressPointerDown={handleProgressPointerDown}
            handleProgressPointerUp={handleProgressPointerUp}
            handleProgressPointerLeave={handleProgressPointerLeave}
            enableAnimations={enableAnimations}
            sideButtonClass={sideButtonClass}
            onMouseEnter={() => {
              isHoveringControlsRef.current = true
            }}
            onMouseLeave={() => {
              isHoveringControlsRef.current = false
              resetHideTimer(2000)
            }}
            t={t}
          />

          <ReaderArticleBody
            item={item}
            columnRef={columnRef}
            currentTheme={currentTheme}
            currentFont={currentFont}
            currentLayout={currentLayout}
            isDark={isDark}
            fontSize={fontSize}
            lineHeight={lineHeight}
            t={t}
            contentRef={contentRef}
            contentInnerRef={contentInnerRef}
            contentReady={contentReady}
            onNavigateToArticle={onNavigateToArticle}
            readingQueue={readingQueue}
          />
          <ReaderRightPanel
            theme={theme}
            currentTheme={currentTheme}
            isDark={isDark}
            showPanels={showPanels}
            cycleTheme={cycleTheme}
            cycleFont={cycleFont}
            cycleLayout={cycleLayout}
            fontSize={fontSize}
            adjustFontSize={adjustFontSize}
            lineHeight={lineHeight}
            adjustLineHeight={adjustLineHeight}
            currentFont={currentFont}
            currentLayout={currentLayout}
            handleShare={() => {}}
            enableAnimations={enableAnimations}
            sideButtonClass={sideButtonClass}
            onMouseEnter={() => {
              isHoveringControlsRef.current = true
            }}
            onMouseLeave={() => {
              isHoveringControlsRef.current = false
              resetHideTimer(2000)
            }}
            t={t}
            isAuthenticated={isAuthenticated || false}
            hasComments={hasComments}
            comments={comments}
            showCommentsPanel={showCommentsPanel}
            setShowCommentsPanel={setShowCommentsPanel}
          />
        </div>
      </article>

      {syncPaused ? (
        <div className="brew-reader__conflict" role="status">
          <p>{t.brew.readingSyncConflict}</p>
          <div className="brew-reader__conflict-actions">
            <button type="button" className="brew-reader__btn" onClick={() => void recoverLocalProgress()}>
              {t.brew.readingSyncKeepLocal}
            </button>
            <button type="button" className="brew-reader__btn" onClick={() => void recoverRemoteProgress()}>
              {t.brew.readingSyncUseLatest}
            </button>
          </div>
        </div>
      ) : null}

      <AnimatePresence>
        {showToast && (
          <motion.div
            initial={
              enableAnimations ? { opacity: 0, y: 50, scale: 0.95 } : false
            }
            animate={
              enableAnimations ? { opacity: 1, y: 0, scale: 1 } : undefined
            }
            exit={
              enableAnimations ? { opacity: 0, y: 50, scale: 0.95 } : undefined
            }
            transition={
              enableAnimations
                ? { duration: 0.25, ease: [0.16, 1, 0.3, 1] }
                : undefined
            }
            className="brew-reader__toast"
          >
            {showToast}
          </motion.div>
        )}
      </AnimatePresence>

      <CommentTooltip
        commentTooltip={commentTooltip}
        onMouseEnter={handleTooltipMouseEnter}
        onMouseLeave={handleTooltipMouseLeave}
        currentTheme={currentTheme}
        isDark={isDark}
        enableAnimations={enableAnimations}
        t={t}
      />

      <CommentInputPopup
        showCommentPopup={showCommentPopup && !!isAuthenticated}
        setShowCommentPopup={setShowCommentPopup}
        commentPopupPosition={commentPopupPosition}
        selectedText={selectedText}
        setSelectedText={setSelectedText}
        commentInput={commentInput}
        setCommentInput={setCommentInput}
        commentSubmitting={commentSubmitting}
        submitComment={submitComment}
        currentTheme={currentTheme}
        isDark={isDark}
        enableAnimations={enableAnimations}
        t={t}
      />

      <CommentsListPanel
        focusedCommentIds={focusedCommentIds}
        clearCommentFocus={() => setFocusedCommentIds([])}
        unresolvedCommentIds={unresolvedCommentIds}
        currentTheme={currentTheme}
        isDark={isDark}
        showCommentsPanel={showCommentsPanel}
        setShowCommentsPanel={setShowCommentsPanel}
        comments={comments}
        commentsLoading={commentsLoading}
        replyingTo={replyingTo}
        setReplyingTo={setReplyingTo}
        replyInput={replyInput}
        setReplyInput={setReplyInput}
        replySubmitting={replySubmitting}
        submitReply={submitReply}
        expandedComments={expandedComments}
        toggleReplies={toggleReplies}
        commentReplies={commentReplies}
        deleteComment={deleteComment}
        enableAnimations={enableAnimations}
        t={t}
      />

      <AnnotationTooltip
        hoveredAnnotation={hoveredAnnotation}
        tooltipPosition={tooltipPosition}
        currentTheme={currentTheme}
        isDark={isDark}
        enableAnimations={enableAnimations}
        t={t}
      />

      <MobileReaderBar
        item={item}
        onClose={onClose}
        onToggleStar={onToggleStar}
        isAuthenticated={isAuthenticated || false}
        isAdmin={isAdmin || false}
        isBrewlia={isBrewlia}
        theme={theme}
        currentTheme={currentTheme}
        isDark={isDark}
        readingProgress={readingProgress}
        showPanels={showPanels}
        showMobileControls={showMobileControls}
        setShowMobileControls={setShowMobileControls}
        toc={toc}
        showToc={showToc}
        setShowToc={setShowToc}
        activeHeadingId={activeHeadingId}
        scrollToHeading={scrollToHeading}
        comments={comments}
        hasComments={hasComments}
        showCommentsPanel={showCommentsPanel}
        setShowCommentsPanel={setShowCommentsPanel}
        annotations={annotations}
        annotationsLoading={annotationsLoading}
        showAnnotations={showAnnotations}
        showBrewliaPanel={showBrewliaPanel}
        setShowBrewliaPanel={setShowBrewliaPanel}
        toggleAnnotations={toggleAnnotations}
        loadAnnotations={loadAnnotations}
        regenerateAnnotations={regenerateAnnotations}
        annotationsError={annotationsError}
        selectedAnnotation={selectedAnnotation}
        setSelectedAnnotation={setSelectedAnnotation}
        scrollToAnnotation={handleScrollToAnnotation}
        podcastDialogues={podcastDialogues}
        podcastLoading={podcastLoading}
        cloudTtsLoading={cloudTtsLoading}
        podcastState={podcastState}
        showPodcastPlayer={showPodcastPlayer}
        setShowPodcastPlayer={setShowPodcastPlayer}
        loadPodcast={loadPodcast}
        podcastCurrentIndex={podcastCurrentIndex}
        ttsEngine={ttsEngine}
        handleTtsEngineChange={handleTtsEngineChange}
        cloudTtsAvailable={cloudTtsAvailable}
        cloudTtsError={cloudTtsError}
        cloudTtsLoadProgress={cloudTtsLoadProgress}
        voiceList={voiceList}
        showVoiceSettings={showVoiceSettings}
        setShowVoiceSettings={setShowVoiceSettings}
        hostVoiceId={hostVoiceId}
        guestVoiceId={guestVoiceId}
        handleVoiceSelect={handleVoiceChange}
        handleOpenSettings={handleOpenSettings}
        articleCache={articleCache}
        articleCacheLoading={articleCacheLoading}
        clearingVoiceId={clearingVoiceId}
        handleClearVoiceCache={handleClearVoiceCache}
        handleSwitchToCachedVoice={handleSwitchToVoice}
        reloadCloudTts={reloadCloudTTS}
        handlePlayPause={
          podcastState === 'playing' ? handlePodcastPause : handlePodcastPlay
        }
        handleStop={handlePodcastStop}
        handlePrevious={handlePodcastPrev}
        handleNext={handlePodcastNext}
        handleDialogueClick={handlePodcastSeek}
        cycleTheme={cycleTheme}
        cycleFont={cycleFont}
        fontSize={fontSize}
        adjustFontSize={adjustFontSize}
        lineHeight={lineHeight}
        adjustLineHeight={adjustLineHeight}
        currentFont={currentFont}
        handleShare={handleShare}
        onEditNote={onEditNote}
        enableAnimations={enableAnimations}
        onTouchStart={() => {
          isHoveringControlsRef.current = true
        }}
        onTouchEnd={() => {
          isHoveringControlsRef.current = false
          resetHideTimer(2000)
        }}
        t={t}
      />

      <Lightbox
        src={lightboxImage}
        isDark={isDark}
        onClose={() => setLightboxImage(null)}
        t={t}
      />
    </motion.div>
  )
}
