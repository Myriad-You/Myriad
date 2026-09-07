/**
 * Brew 文章阅读器组件
 * 全屏沉浸式阅读体验，两侧悬浮控制栏
 *
 *
 * 性能优化：
 * - useMemo 缓存主题配置和样式计算
 * - useCallback 缓存所有回调函数
 * - 动画统一接入调度器，根据设备性能自适应
 */

import type { AnnotationItem } from '../../services/brewliaApi'
import type { BrewItem, SourceType } from '../../types/brew'
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

interface BrewReaderProps {
  item: BrewItem
  onClose: () => void
  onToggleStar: () => void
  isAuthenticated?: boolean // 是否已登录（游客隐藏收藏按钮）
  isAdmin?: boolean // 是否为管理员（游客/普通用户隐藏重新生成按钮）
  sourceType?: SourceType // 来源类型（brewlia 时显示 AI 功能）
  /**
   * 分享用 URL。自有内容传入站内 `/brew/item/{id}`；
   * 缺省则复制原文 `item.link`（外部订阅，避免把别人的文章当本站 SEO 页分享）。
   */
  shareUrl?: string
  /**
   * 编辑这篇手记。上层只在「站长 + 这篇是手记」时传值，阅读器不自己判断。
   */
  onEditNote?: () => void
  // 阅读列表导航回调（从 Brew.tsx 传入）
  onNavigateToArticle?: (articleId: number) => void
  // 全局文章列表导航（非阅读列表时使用）
  articleList?: BrewItem[]
  currentArticleIndex?: number
}

export default function BrewReader({
  item: incomingItem,
  onClose,
  onToggleStar,
  isAuthenticated = false,
  isAdmin = false,
  sourceType,
  shareUrl,
  onEditNote,
  onNavigateToArticle,
  articleList,
  currentArticleIndex,
}: BrewReaderProps) {
  const { t } = useI18n()
  const contentRef = useRef<HTMLDivElement>(null)
  const articleRef = useRef<HTMLElement>(null)
  const contentInnerRef = useRef<HTMLDivElement>(null)

  /**
   * 换文章 = 淡出 → 换 → 淡入，而不是换完 DOM 再补一个淡入。
   *
   * 后者会先把新文章按满不透明画一帧，再跳到 0 开始淡入 —— 就是读者看到的
   * 「闪一下」。这里把 prop 延迟一步：组件内部所有逻辑（标题、目录、进度、
   * 批注、滚动复位）都只看 `item`，它在旧正文淡到 0 之后才切成新的一篇，
   * 于是这些切换全都发生在正文不可见的那一刻。
   *
   * 同一篇的字段更新（已读 / 收藏 / 进度）直接透传，不走动画。
   */
  const [item, setItem] = useState(incomingItem)
  if (incomingItem !== item && incomingItem.id === item.id)
    setItem(incomingItem)
  /** 中栏：标题 + 元信息 + 正文 + 上下篇。淡入淡出只作用在这一层。 */
  const columnRef = useRef<HTMLDivElement>(null)
  const fadeOutRef = useRef<Animation | null>(null)

  // 沉浸模式 - 进入阅读器时隐藏导航栏和控制面板
  useImmersiveChrome('brew-reader', true)

  // 页面内容上下文 - 用于 Agent 访问当前阅读的文章
  const { setPageContent, clearPageContent } = usePageContentOptional() || {}

  // 设置当前阅读的文章内容到全局上下文
  useEffect(() => {
    if (setPageContent && item) {
      // 获取文章内容：优先使用 content，其次 summary
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

    // 清理：离开阅读器时清除内容
    return () => {
      if (clearPageContent) {
        clearPageContent()
      }
    }
  }, [item, sourceType, setPageContent, clearPageContent])

  // 动画配置 - 根据设备性能自适应
  const animConfig = useBrewAnimationConfig()
  const readerTransition = useMemo(
    () => getBrewTransition(animConfig, 'reader'),
    [animConfig],
  )
  const enableAnimations = !isExlight(animConfig)

  /**
   * 第一步：淡出。父组件换了 incomingItem 时，先把中栏淡到 0（140ms），
   * 结束时才真正 setItem —— 目录、进度、滚动复位都在这一刻发生，读者看不见。
   * 连续快速切换（狂按 j/k）时淡出已经在跑，只改它的目标，不从头再来。
   *
   * exlight 一律不做 —— `prefers-reduced-motion` 在本仓库就会解析成 exlight。
   */
  useEffect(() => {
    if (incomingItem.id === item.id) return
    const col = columnRef.current
    if (!enableAnimations || !col || typeof col.animate !== 'function') {
      setItem(incomingItem)
      return
    }
    const running = fadeOutRef.current
    if (running && running.playState === 'running') {
      running.onfinish = () => setItem(incomingItem)
      return
    }
    const out = col.animate(
      [
        { opacity: 1, transform: 'translateY(0)' },
        { opacity: 0, transform: 'translateY(-6px)' },
      ],
      { duration: 140, easing: 'ease-in', fill: 'forwards' },
    )
    fadeOutRef.current = out
    out.onfinish = () => setItem(incomingItem)
  }, [incomingItem, item.id, enableAnimations])

  /**
   * 第二步：淡入。用 useLayoutEffect 在新正文首帧绘制之前起动画，
   * 第一帧就是 opacity 0 —— useEffect 会晚一帧，那一帧就是「闪」。
   * 淡出留下的 fill: forwards 必须先取消，否则新正文永远透明。
   */
  useLayoutEffect(() => {
    fadeOutRef.current?.cancel()
    fadeOutRef.current = null
    if (!enableAnimations) return
    const col = columnRef.current
    if (!col || typeof col.animate !== 'function') return
    const swap = col.animate(
      [
        { opacity: 0, transform: 'translateY(8px)' },
        { opacity: 1, transform: 'translateY(0)' },
      ],
      { duration: 260, easing: 'cubic-bezier(0.16, 1, 0.3, 1)', fill: 'none' },
    )
    return () => swap.cancel()
  }, [item.id, enableAnimations])

  // WebKit 优化：延迟渲染内容，让入场动画先完成
  const [contentReady, setContentReady] = useState(!enableAnimations)

  // Brewlia AI 功能标识
  const isBrewlia = sourceType === 'brewlia'

  // 阅读设置 - 使用自定义 hook
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
  } = useReaderSettings()

  const [showToast, setShowToast] = useState<string | null>(null)
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null) // Toast 定时器，防止泄漏
  const [lightboxImage, setLightboxImage] = useState<string | null>(null) // 灯箱图片

  // 统一的 Toast 显示函数，自动管理定时器防止泄漏
  const showToastMessage = useCallback((message: string, duration = 2000) => {
    // 清除之前的定时器
    if (toastTimerRef.current) {
      clearTimeout(toastTimerRef.current)
    }
    setShowToast(message)
    toastTimerRef.current = setTimeout(() => {
      setShowToast(null)
      toastTimerRef.current = null
    }, duration)
  }, [])

  // 清理 toast 定时器
  useEffect(() => {
    return () => {
      if (toastTimerRef.current) {
        clearTimeout(toastTimerRef.current)
      }
    }
  }, [])

  // Brewlia AI 注释 - 使用自定义 hook
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

  // 用户评论 - 使用自定义 hook
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

  // Brewlia AI 播客 - 使用自定义 hook
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

  // 侧边栏按钮样式 - useMemo 缓存
  const sideButtonClass = useMemo(
    () =>
      `p-2.5 rounded-xl transition-all duration-200 ${currentTheme.secondary} hover:${currentTheme.text} ${
        isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'
      }`,
    [currentTheme.secondary, currentTheme.text, isDark],
  )

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
  } = useReaderControls({
    articleRef,
    contentRef,
    itemId: item.id,
    readProgress: item.read_progress,
    contentReady,
    isAuthenticated,
    onClose,
    adjustFontSize,
    showToastMessage,
    t,
  })

  const baseContent = useContentRender({
    contentInnerRef,
    contentReady,
    item,
    isDark,
    t,
    showAnnotations,
    annotations,
    comments,
    highlightComments,
    theme,
  })

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

  // WebKit 优化：延迟渲染内容，让入场动画先完成
  // 这避免了同时执行动画 + 大量 DOM 渲染导致的卡顿
  useEffect(() => {
    if (!enableAnimations) {
      setContentReady(true)
      return
    }

    // 使用 requestAnimationFrame 确保在下一帧开始前设置
    // 延迟时间略长于动画时长，确保动画完成
    const delay = readerTransition.duration * 1000 + 50
    const timer = setTimeout(() => {
      requestAnimationFrame(() => {
        setContentReady(true)
      })
    }, delay)

    return () => clearTimeout(timer)
  }, [enableAnimations, readerTransition.duration])

  const { handleTooltipMouseEnter, handleTooltipMouseLeave } = useContentEvents({
    contentRef,
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

  // Brewlia 订阅自动加载注释
  useEffect(() => {
    if (isBrewlia && annotations.length === 0 && !annotationsLoading) {
      // 延迟加载，等页面渲染完成
      const timer = setTimeout(() => {
        loadAnnotations()
      }, 500)
      return () => clearTimeout(timer)
    }
  }, [isBrewlia]) // 只在初始化时触发一次

  // 登录用户自动加载评论
  useEffect(() => {
    if (isAuthenticated && comments.length === 0 && !commentsLoading) {
      const timer = setTimeout(() => {
        loadComments()
      }, 300)
      return () => clearTimeout(timer)
    }
  }, [isAuthenticated]) // 只在初始化时触发一次

  // 包装 scrollToAnnotation 以传入 refs
  const handleScrollToAnnotation = useCallback(
    (annotation: AnnotationItem) => {
      scrollToAnnotation(annotation, contentRef, articleRef)
    },
    [scrollToAnnotation],
  )

  // 关闭所有附属的 tooltip 和面板
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

  // 复制链接：自有内容用站内规范 URL，外部订阅仍用原文
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

  // 阅读器动画配置 - 根据性能等级动态调整
  const readerAnimProps = useMemo(() => {
    if (!enableAnimations) {
      // 禁用动画时直接显示
      return {
        initial: false as const,
        animate: undefined,
        exit: undefined,
        transition: undefined,
      }
    }

    // 完整动画
    return {
      initial: brewAnimationPresets.readerEnter.initial,
      animate: brewAnimationPresets.readerEnter.animate,
      exit: brewAnimationPresets.readerEnter.exit,
      transition: {
        duration: readerTransition.duration,
        ease: readerTransition.ease,
      },
    }
  }, [enableAnimations, readerTransition])

  return (
    <motion.div
      {...readerAnimProps}
      style={STYLE_READER_CONTAINER}
      className={`fixed inset-0 z-50 ${currentTheme.bg}`}
      data-brew-reader="true"
      data-brew-theme={theme}
    >
      {/* 顶部进度条 - 用 scaleX 替代 width 动画，避免触发 layout recalculation */}
      <div className="absolute top-0 left-0 right-0 h-0.5 z-10 overflow-hidden">
        <div
          className="h-full w-full bg-linear-to-r from-amber-500 to-orange-500 origin-left"
          style={{ transform: `scaleX(${readingProgress / 100})` }}
        />
      </div>

      {/* 主内容区 - 三栏布局 */}
      {/* transform: translateZ(0) 将滚动容器提升为独立合成层，避免 sticky 子元素回流影响主线程 */}
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
          {/* 左侧控制栏 - 导航与进度 */}
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
            articleList={articleList}
            currentArticleIndex={currentArticleIndex}
          />
          {/* 右侧控制栏 - 设置与操作 */}
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

      {/* Toast 提示 */}
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
            className={`fixed bottom-8 inset-x-0 mx-auto w-fit px-4 py-2 rounded-xl shadow-lg z-60 ${
              isDark ? 'bg-neutral-800/95 text-white' : 'bg-black/90 text-white'
            }`}
          >
            {showToast}
          </motion.div>
        )}
      </AnimatePresence>

      {/* 评论 Tooltip */}
      <CommentTooltip
        commentTooltip={commentTooltip}
        onMouseEnter={handleTooltipMouseEnter}
        onMouseLeave={handleTooltipMouseLeave}
        currentTheme={currentTheme}
        isDark={isDark}
        enableAnimations={enableAnimations}
        t={t}
      />

      {/* 用户评论输入弹窗 */}
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

      {/* 用户评论列表面板 - 从顶部展开 */}
      <CommentsListPanel
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

      {/* Brewlia 注释 Tooltip */}
      <AnnotationTooltip
        hoveredAnnotation={hoveredAnnotation}
        tooltipPosition={tooltipPosition}
        currentTheme={currentTheme}
        isDark={isDark}
        enableAnimations={enableAnimations}
        t={t}
      />

      {/* 移动端底部控制栏 */}
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

      {/* 灯箱组件 */}
      <Lightbox
        src={lightboxImage}
        isDark={isDark}
        onClose={() => setLightboxImage(null)}
        t={t}
      />
    </motion.div>
  )
}
