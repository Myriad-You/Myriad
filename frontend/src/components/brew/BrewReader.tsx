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

import type { AnnotationItem, AnnotationType } from '../../services/brewliaApi'

import type { BrewItem, SourceType } from '../../types/brew'
import {
  LuCalendar as Calendar,
  LuClock as Clock,
  LuUser as User,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { API_URL as CONFIG_API_URL } from '../../config'
import { proxyImageUrl } from '../../utils/proxyImageUrl'
import { useI18n } from '../../contexts/I18nContext'
import { useNavigation } from '../../contexts/NavigationContext'
import { usePageContentOptional } from '../../contexts/PageContentContext'

import { useReadingListOptional } from '../../contexts/ReadingListContext'
import {
  brewAnimationPresets,
  getBrewTransition,
  useBrewAnimationConfig,
} from '../../hooks/animation'
import { isExlight } from '../../hooks/useAnimationLevel'
import * as brewApi from '../../services/brewApi'
import * as brewliaApi from '../../services/brewliaApi'
import {
  loadEmbedData,
  playNeteaseSong,
  processEmbeds,
} from '../../utils/embedProcessor'
import { escapeHtml } from '../../utils/inputSanitizer'
import { processRssContent } from '../../utils/rssContentProcessor'
import {
  AnnotationTooltip,
  CommentInputPopup,
  CommentsListPanel,
  CommentTooltip,
  Lightbox,
  MobileReaderBar,
  ReaderLeftPanel,
  ReaderRightPanel,
  STYLE_READER_CONTAINER,
  STYLE_SCROLL_SMOOTH,
  useAnnotations,
  useComments,
  usePodcast,
  useReaderSettings,
} from './reader'

// API URL
const API_URL = CONFIG_API_URL

// iframe 保存/恢复
// 在 innerHTML 更新前保存已加载的 iframe，更新后恢复，避免重新加载导致闪烁
interface SavedIframe {
  key: string
  element: HTMLElement
}

function saveEmbedElements(container: HTMLElement): SavedIframe[] {
  const saved: SavedIframe[] = []

  // 保存 bilibili 嵌入（通过 data-video-id 匹配）
  container
    .querySelectorAll('.brew-bilibili-embed[data-video-id]')
    .forEach((el) => {
      const videoId = el.getAttribute('data-video-id')
      if (videoId && el.querySelector('iframe')) {
        saved.push({ key: `bilibili:${videoId}`, element: el as HTMLElement })
      }
    })

  // 保存 RSS 内容中的 iframe 包装器（通过 iframe src 匹配）
  container.querySelectorAll('.rss-content-iframe-wrapper').forEach((el) => {
    const iframe = el.querySelector('iframe')
    if (iframe) {
      const src = iframe.getAttribute('src') || iframe.src || ''
      if (src) {
        saved.push({ key: `rss:${src}`, element: el as HTMLElement })
      }
    }
  })

  // 保存后处理阶段包装的 iframe（通过 iframe src 匹配）
  container
    .querySelectorAll('iframe[data-iframe-wrapped="true"]')
    .forEach((iframe) => {
      if (
        iframe.closest('.brew-bilibili-embed') ||
        iframe.closest('.rss-content-iframe-wrapper')
      ) {
        return
      }
      const wrapper = iframe.parentElement
      if (wrapper) {
        const src =
          iframe.getAttribute('src') || (iframe as HTMLIFrameElement).src || ''
        if (src) {
          saved.push({ key: `wrapped:${src}`, element: wrapper })
        }
      }
    })

  // 保存已加载数据的嵌入卡片（网易云音乐、Steam、GitHub）
  // 避免 overlay 变化时丢失 data-loaded 状态导致重新 fetch
  container
    .querySelectorAll(
      '.brew-netease-music[data-loaded="true"], .brew-netease-music[data-loaded="loading"]',
    )
    .forEach((el) => {
      const songId = el.getAttribute('data-song-id')
      if (songId) {
        saved.push({ key: `netease:${songId}`, element: el as HTMLElement })
      }
    })
  container
    .querySelectorAll(
      '.brew-steam-game[data-loaded="true"], .brew-steam-game[data-loaded="loading"]',
    )
    .forEach((el) => {
      const appId = el.getAttribute('data-app-id')
      if (appId) {
        saved.push({ key: `steam:${appId}`, element: el as HTMLElement })
      }
    })
  container
    .querySelectorAll(
      '.brew-github-repo[data-loaded="true"], .brew-github-repo[data-loaded="loading"]',
    )
    .forEach((el) => {
      const repo = el.getAttribute('data-repo')
      if (repo) {
        saved.push({ key: `github:${repo}`, element: el as HTMLElement })
      }
    })

  // 从 DOM 摘出保存的元素（防止 innerHTML 赋值时销毁它们）
  saved.forEach((s) => s.element.remove())

  return saved
}

function restoreEmbedElements(
  container: HTMLElement,
  saved: SavedIframe[],
): void {
  if (saved.length === 0) return
  const savedMap = new Map(saved.map((s) => [s.key, s.element]))
  const restored = new Set<string>()

  // 通用恢复：按 selector + key 生成器匹配
  const restoreBySelector = (
    selector: string,
    keyFn: (el: Element) => string | null,
  ) => {
    container.querySelectorAll(selector).forEach((newEl) => {
      const key = keyFn(newEl)
      if (key && !restored.has(key)) {
        const savedEl = savedMap.get(key)
        if (savedEl && newEl.parentNode) {
          newEl.parentNode.replaceChild(savedEl, newEl)
          restored.add(key)
        }
      }
    })
  }

  // 恢复 bilibili 嵌入
  restoreBySelector('.brew-bilibili-embed[data-video-id]', (el) =>
    el.getAttribute('data-video-id')
      ? `bilibili:${el.getAttribute('data-video-id')}`
      : null,
  )

  // 恢复 RSS iframe 包装器
  restoreBySelector('.rss-content-iframe-wrapper', (el) => {
    const src = el.querySelector('iframe')?.getAttribute('src') || ''
    return src ? `rss:${src}` : null
  })

  // 恢复已加载的嵌入卡片（避免重新 fetch API 数据）
  restoreBySelector('.brew-netease-music[data-song-id]', (el) =>
    el.getAttribute('data-song-id')
      ? `netease:${el.getAttribute('data-song-id')}`
      : null,
  )
  restoreBySelector('.brew-steam-game[data-app-id]', (el) =>
    el.getAttribute('data-app-id')
      ? `steam:${el.getAttribute('data-app-id')}`
      : null,
  )
  restoreBySelector('.brew-github-repo[data-repo]', (el) =>
    el.getAttribute('data-repo')
      ? `github:${el.getAttribute('data-repo')}`
      : null,
  )

  // 恢复后处理包装的 iframe
  container.querySelectorAll('iframe').forEach((newIframe) => {
    if (
      newIframe.closest('.brew-bilibili-embed') ||
      newIframe.closest('.rss-content-iframe-wrapper')
    ) {
      return
    }
    const src = newIframe.getAttribute('src') || newIframe.src || ''
    const key = `wrapped:${src}`
    if (src && !restored.has(key)) {
      const savedEl = savedMap.get(key)
      if (savedEl && newIframe.parentNode) {
        newIframe.parentNode.replaceChild(savedEl, newIframe)
        restored.add(key)
      }
    }
  })
}

// Dual-path: hotlink CDNs via proxy; otherwise original URL for display
function getImageUrl(imageUrl: string | null): string | null {
  if (!imageUrl) return null
  if (imageUrl.startsWith('/api/') || imageUrl.startsWith(`${API_URL}/api/`)) {
    return imageUrl.startsWith('/api/') ? `${API_URL}${imageUrl}` : imageUrl
  }
  return proxyImageUrl(imageUrl) ?? imageUrl
}

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
  // 阅读列表导航回调（从 Brew.tsx 传入）
  onNavigateToArticle?: (articleId: number) => void
  // 全局文章列表导航（非阅读列表时使用）
  articleList?: BrewItem[]
  currentArticleIndex?: number
}

// 目录项类型
interface TocItem {
  id: string
  text: string
  level: number
}

export default function BrewReader({
  item,
  onClose,
  onToggleStar,
  isAuthenticated = false,
  isAdmin = false,
  sourceType,
  shareUrl,
  onNavigateToArticle,
  articleList,
  currentArticleIndex,
}: BrewReaderProps) {
  const { t } = useI18n()
  const contentRef = useRef<HTMLDivElement>(null)
  const articleRef = useRef<HTMLElement>(null)

  // 阅读列表上下文 - 用于顺序阅读导航
  const readingList = useReadingListOptional()
  const positionInfo = readingList?.getPositionInfo(item.id)

  // 切换文章：恢复服务端进度或回到顶部
  useEffect(() => {
    lastSyncedProgressRef.current = -1
    if (progressSyncTimerRef.current) {
      clearTimeout(progressSyncTimerRef.current)
      progressSyncTimerRef.current = null
    }
    const saved =
      typeof item.read_progress === 'number' && item.read_progress > 0
        ? Math.min(100, Math.round(item.read_progress))
        : 0
    setReadingProgress(saved)
    // Apply scroll after layout
    requestAnimationFrame(() => {
      const el = articleRef.current
      if (!el) return
      if (saved > 0 && saved < 100) {
        const max = el.scrollHeight - el.clientHeight
        if (max > 0) el.scrollTop = (saved / 100) * max
      } else {
        el.scrollTo({ top: 0 })
      }
    })
  }, [item.id])

  // 沉浸模式 - 进入阅读器时隐藏导航栏和控制面板
  const { setImmersiveMode } = useNavigation()

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

  const [readingProgress, setReadingProgress] = useState(0)
  const progressRafRef = useRef<number | null>(null)
  const lastSyncedProgressRef = useRef<number>(-1)
  const progressSyncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const [showToast, setShowToast] = useState<string | null>(null)
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null) // Toast 定时器，防止泄漏
  const [showPanels, setShowPanels] = useState(true)
  const [showMobileControls, setShowMobileControls] = useState(false) // 移动端控制条展开状态
  const [lightboxImage, setLightboxImage] = useState<string | null>(null) // 灯箱图片
  const hideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const lastMouseMoveRef = useRef<number>(0)
  const isScrollingRef = useRef(false)
  const cooldownRef = useRef(false) // 冷却期，防止刚隐藏就显示
  const lastScrollTopRef = useRef(0) // 上次滚动位置，用于判断滚动方向
  const isHoveringControlsRef = useRef(false) // 鼠标是否在控制栏区域
  const tooltipHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null) // 评论 tooltip 延迟关闭定时器
  const isHoveringTooltipRef = useRef(false) // 鼠标是否在评论 tooltip 上
  const [toc, setToc] = useState<TocItem[]>([])
  const [showToc, setShowToc] = useState(false)
  const [activeHeadingId, setActiveHeadingId] = useState<string>('')
  const activeHeadingIdRef = useRef<string>('')
  const [headingHistory, setHeadingHistory] = useState<string[]>([]) // 标题访问历史
  const progressLongPressRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const isLongPressRef = useRef(false)

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

  // 评论 ref（供事件处理器读取，避免重新绑定监听器）
  const commentsRef = useRef(comments)
  commentsRef.current = comments

  // 评论 tooltip 悬停管理回调
  const clearTooltipHideTimer = useCallback(() => {
    if (tooltipHideTimerRef.current) {
      clearTimeout(tooltipHideTimerRef.current)
      tooltipHideTimerRef.current = null
    }
  }, [])

  const startTooltipHideTimer = useCallback(
    (delay = 200) => {
      if (tooltipHideTimerRef.current) return // 已有定时器运行中
      tooltipHideTimerRef.current = setTimeout(() => {
        tooltipHideTimerRef.current = null
        setCommentTooltip(null)
      }, delay)
    },
    [setCommentTooltip],
  )

  const handleTooltipMouseEnter = useCallback(() => {
    isHoveringTooltipRef.current = true
    clearTooltipHideTimer()
  }, [clearTooltipHideTimer])

  const handleTooltipMouseLeave = useCallback(() => {
    isHoveringTooltipRef.current = false
    startTooltipHideTimer()
  }, [startTooltipHideTimer])

  // 清理 tooltip 隐藏定时器
  useEffect(() => {
    return () => clearTooltipHideTimer()
  }, [clearTooltipHideTimer])

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

  // 基础内容（不含注释/评论高亮）—— 仅在文章内容或暗色模式变化时重新计算
  // 与 iframe 嵌入等重型内容绑定，避免频繁重建导致闪烁
  const baseContent = useMemo(() => {
    if (!contentReady) return ''

    // 网络搜索文章：直接显示 AI 生成的摘要（不再支持加载原文）
    // 必须 HTML 转义，禁止把模型/搜索文本当 HTML 注入
    if (item.fromWebSearch && !item.content) {
      const hasSummary = item.summary && item.summary.trim().length > 20
      if (hasSummary) {
        const paragraphs = item
          .summary!.split(/\n\n|\n/)
          .filter((p) => p.trim())
        const summaryHtml = paragraphs
          .map((p) => `<p>${escapeHtml(p.trim())}</p>`)
          .join('\n')
        return `<div class="web-search-summary">
          ${summaryHtml}
          <p class="web-search-note">以上内容由 AI 根据网络搜索结果生成</p>
        </div>`
      }
      return `<div class="web-search-summary">
        <p class="opacity-60">暂无内容摘要</p>
      </div>`
    }

    let content =
      item.content ||
      item.summary ||
      `<p class="opacity-50">${t.brew.noContent}</p>`

    // 0. 首先处理 RSS 内容格式（清理危险标签、适配各类 HTML 标签样式）
    content = processRssContent(content, {
      isDark,
      lazyLoadImages: true,
      removeTrackingParams: true,
      removeEmptyTags: true,
      baseUrl: item.link || undefined,
    })

    // 1. 处理嵌入内容（iframe、特定链接转卡片）
    content = processEmbeds(content, isDark)
    return content
  }, [
    contentReady,
    item.content,
    item.summary,
    item.link,
    item.fromWebSearch,
    isDark,
    t.brew.noContent,
  ])

  // 内容渲染 ref（取代 dangerouslySetInnerHTML，避免 iframe 重建导致闪烁）
  const contentInnerRef = useRef<HTMLDivElement>(null)
  const prevBaseContentRef = useRef('')

  // 统一内容渲染：基础内容变化时全量更新，仅注释/评论变化时保留已加载的 iframe
  useEffect(() => {
    const container = contentInnerRef.current
    if (!container || !baseContent) return

    const isBaseChanged = prevBaseContentRef.current !== baseContent
    prevBaseContentRef.current = baseContent

    // 构建包含注释和评论高亮的最终 HTML
    let displayHtml = baseContent
    if (showAnnotations && annotations.length > 0) {
      displayHtml = brewliaApi.highlightAnnotations(displayHtml, annotations)
    }
    if (comments.length > 0) {
      displayHtml = highlightComments(displayHtml, comments, theme)
    }

    if (!isBaseChanged && container.childElementCount > 0) {
      // 仅覆盖层变化：保留已加载的 iframe 和嵌入卡片避免闪烁和重复 API 调用
      const saved = saveEmbedElements(container)
      container.innerHTML = displayHtml
      restoreEmbedElements(container, saved)
    } else {
      // 基础内容变化：直接全量替换
      container.innerHTML = displayHtml
    }
  }, [
    baseContent,
    showAnnotations,
    annotations,
    comments,
    highlightComments,
    theme,
  ])

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

  // 沉浸模式控制 - 进入阅读器时隐藏导航岛和控制面板
  useEffect(() => {
    setImmersiveMode(true)
    return () => {
      setImmersiveMode(false)
    }
  }, [setImmersiveMode])

  // 格式化日期 - useMemo 缓存
  const formattedDate = useMemo(() => {
    if (!item.published_at) return ''
    return new Date(item.published_at).toLocaleDateString('zh-CN', {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    })
  }, [item.published_at])

  // 计算阅读进度（本地 UI + 登录用户 debounce 同步到服务端）
  const updateReadingProgress = useCallback(() => {
    // RAF 节流：每帧最多更新一次，避免每像素滚动都触发 React re-render
    if (progressRafRef.current !== null) return
    progressRafRef.current = requestAnimationFrame(() => {
      progressRafRef.current = null
      if (articleRef.current) {
        const { scrollTop, scrollHeight, clientHeight } = articleRef.current
        const denom = scrollHeight - clientHeight
        const progress =
          denom <= 0
            ? 100
            : Math.min(100, Math.round((scrollTop / denom) * 100))
        if (Number.isNaN(progress)) return
        setReadingProgress((prev) => (prev === progress ? prev : progress))

        if (!isAuthenticated) return
        // Debounce server sync (2s) and only when delta ≥ 5% or finished
        if (progressSyncTimerRef.current) {
          clearTimeout(progressSyncTimerRef.current)
        }
        progressSyncTimerRef.current = setTimeout(() => {
          progressSyncTimerRef.current = null
          const last = lastSyncedProgressRef.current
          if (progress < 100 && last >= 0 && Math.abs(progress - last) < 5) {
            return
          }
          lastSyncedProgressRef.current = progress
          void brewApi
            .updateReadProgress(item.id, progress, {
              isRead: progress >= 95 ? true : undefined,
            })
            .catch(() => {
              /* best-effort */
            })
        }, 2000)
      }
    })
  }, [isAuthenticated, item.id])

  // Flush progress on unmount / article leave
  useEffect(() => {
    return () => {
      if (progressSyncTimerRef.current) {
        clearTimeout(progressSyncTimerRef.current)
        progressSyncTimerRef.current = null
      }
      if (!isAuthenticated) return
      const p = lastSyncedProgressRef.current
      // readingProgress state may be stale in cleanup; use last known via ref only if we ever set it
      void p
    }
  }, [isAuthenticated, item.id])

  // 处理内容中的图片和链接
  useEffect(() => {
    if (contentRef.current) {
      const images = contentRef.current.querySelectorAll('img')
      images.forEach((img) => {
        // 跳过嵌入卡片内的图片（它们有自己的样式）
        if (img.closest('.brew-embed-card')) {
          return
        }

        // 跳过已处理的图片
        if (img.dataset.sizeProcessed) return
        img.dataset.sizeProcessed = 'true'

        // 添加基础样式
        img.classList.add('rounded-xl', 'h-auto', 'my-4', 'mx-auto', 'block')

        // 检测正方形图片并限制宽度
        const handleImageLoad = () => {
          const ratio = img.naturalWidth / img.naturalHeight
          const isSquare = ratio >= 0.8 && ratio <= 1.25
          const isSmall = img.naturalWidth <= 200 && img.naturalHeight <= 200

          if (isSquare || isSmall) {
            // 正方形或小图限制宽度到 35%
            img.style.maxWidth = '35%'
          }
        }

        if (img.complete && img.naturalWidth > 0) {
          handleImageLoad()
        } else {
          img.addEventListener('load', handleImageLoad, { once: true })
        }
      })

      const links = contentRef.current.querySelectorAll('a')
      links.forEach((link) => {
        link.target = '_blank'
        link.rel = 'noopener noreferrer'
      })

      // 处理代码块：添加样式和复制按钮
      const codeBlocks = contentRef.current.querySelectorAll('pre')
      codeBlocks.forEach((pre) => {
        // 跳过已处理的代码块
        if (pre.parentElement?.classList.contains('code-block-wrapper')) return

        // 创建包装容器
        const wrapper = document.createElement('div')
        wrapper.className = 'code-block-wrapper relative group my-5'

        // 将 pre 移入 wrapper
        pre.parentNode?.insertBefore(wrapper, pre)
        wrapper.appendChild(pre)

        // 添加复制按钮 - 代码块背景始终是深色的，所以按钮用浅色样式
        const copyBtn = document.createElement('button')
        copyBtn.className =
          'absolute top-3 right-3 p-1.5 rounded-lg opacity-0 group-hover:opacity-100 transition-all duration-200 bg-white/10 hover:bg-white/20 text-white/60 hover:text-white/90 backdrop-blur-sm'
        copyBtn.innerHTML = `<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"></path></svg>`
        copyBtn.title = '复制代码'

        copyBtn.addEventListener('click', async (e) => {
          e.preventDefault()
          e.stopPropagation()
          const code = pre.textContent || ''
          try {
            await navigator.clipboard.writeText(code)
            // 显示成功状态
            copyBtn.innerHTML = `<svg class="w-4 h-4 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path></svg>`
            setTimeout(() => {
              copyBtn.innerHTML = `<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"></path></svg>`
            }, 2000)
          } catch (err) {
            console.error('复制失败:', err)
          }
        })

        wrapper.appendChild(copyBtn)
      })

      // 处理透传的原始 iframe（YouTube 等未被 embedProcessor 识别的嵌入）
      // 用 aspect-ratio 容器包裹，替代不稳定的 padding-bottom hack，防止滚动时闪缩
      const iframes = contentRef.current.querySelectorAll('iframe')
      iframes.forEach((iframe) => {
        // 跳过已在 brew-embed 容器内的（bilibili 等已处理的嵌入）
        if (
          iframe.closest(
            '.brew-embed-card, .brew-bilibili-embed, .rss-content-iframe-wrapper',
          )
        ) {
          return
        }
        // 跳过已处理的
        if (iframe.dataset.iframeWrapped) return
        iframe.dataset.iframeWrapped = 'true'

        const src = iframe.src || iframe.getAttribute('src') || ''
        const isMusicEmbed =
          src.includes('music.163.com') ||
          src.includes('spotify.com') ||
          src.includes('xiami.com')

        const wrapper = document.createElement('div')
        wrapper.className = `my-5 rounded-xl overflow-hidden ${isMusicEmbed ? 'aspect-3/1' : 'aspect-video'}`

        // 移除 iframe 的固定 width/height，让容器控制尺寸
        iframe.removeAttribute('width')
        iframe.removeAttribute('height')
        iframe.classList.add('w-full', 'h-full', 'border-0')
        iframe.setAttribute('loading', 'lazy')

        iframe.parentNode?.insertBefore(wrapper, iframe)
        wrapper.appendChild(iframe)
      })

      // 解析标题生成目录
      const headings = contentRef.current.querySelectorAll(
        'h1, h2, h3, h4, h5, h6',
      )
      const tocItems: TocItem[] = []

      headings.forEach((heading, index) => {
        const level = Number.parseInt(heading.tagName[1])
        const text = heading.textContent?.trim() || ''
        const id = `heading-${index}-${text.slice(0, 20).replace(/\s+/g, '-').toLowerCase()}`

        // 为标题添加 id
        heading.id = id

        if (text) {
          tocItems.push({ id, text, level })
        }
      })

      setToc(tocItems)

      // 自动加载嵌入卡片数据（网易云音乐封面、歌名等）
      // 延迟执行以确保 DOM 已完全渲染
      const loadTimer = setTimeout(() => {
        if (contentRef.current) {
          loadEmbedData(contentRef.current).catch((err) => {
            console.error('[BrewReader] 加载嵌入数据失败:', err)
          })
        }
      }, 100)

      return () => clearTimeout(loadTimer)
    }
  }, [baseContent, showAnnotations, annotations, comments, theme]) // 内容更新后重新处理 DOM

  // 处理评论高亮和嵌入卡片的点击和悬停事件
  useEffect(() => {
    if (!contentRef.current) return

    const handleContentClick = async (e: Event) => {
      const target = e.target as HTMLElement

      // 检查是否点击了图片（需要排除嵌入卡片内的图片）
      if (target.tagName === 'IMG') {
        const img = target as HTMLImageElement
        // 检查图片是否在嵌入卡片内（brew-embed-card, brew-embed-exempt, brew-bilibili-embed 等）
        const isInEmbedCard = img.closest(
          '.brew-embed-card, .brew-embed-exempt, .brew-bilibili-embed, .brew-netease-music, .brew-steam-game, .brew-bilibili-video',
        )

        if (img.src && !isInEmbedCard) {
          e.preventDefault()
          e.stopPropagation()
          setLightboxImage(img.src)
          return
        }
        // 如果是嵌入卡片内的图片，不阻止事件，让它继续冒泡到卡片处理
      }

      // 检查是否点击了网易云音乐嵌入卡片
      const neteaseCard = target.closest('.brew-netease-music')
      if (neteaseCard) {
        e.preventDefault()
        e.stopPropagation()

        const songId = neteaseCard.getAttribute('data-song-id')
        if (songId) {
          try {
            // 显示加载状态
            neteaseCard.classList.add('opacity-50', 'pointer-events-none')
            await playNeteaseSong(songId)
            showToastMessage(t.brew.startPlaying)
          } catch (error) {
            console.error('[BrewReader] 播放网易云音乐失败:', error)
            showToastMessage(t.brew.playFailed)
          } finally {
            neteaseCard.classList.remove('opacity-50', 'pointer-events-none')
          }
        }
        return
      }

      // 检查是否点击了高亮文本
      const highlight = target.closest('.user-comment-highlight')

      if (highlight) {
        e.preventDefault()
        e.stopPropagation()

        const commentId = highlight.getAttribute('data-comment-id')
        if (commentId) {
          // 隐藏 tooltip
          setCommentTooltip(null)
          // 打开评论面板
          setShowCommentsPanel(true)
          // 可选：滚动到对应评论
          setTimeout(() => {
            const commentEl = document.querySelector(
              `[data-panel-comment-id="${commentId}"]`,
            )
            if (commentEl) {
              commentEl.scrollIntoView({ behavior: 'smooth', block: 'center' })
            }
          }, 100)
        }
      }
    }

    // 处理悬停显示 tooltip + 离开高亮区域时启动延迟关闭
    let lastHoveredCommentId: string | null = null
    const handleMouseOver = (e: Event) => {
      const target = e.target as HTMLElement
      const highlight = target.closest('.user-comment-highlight') as HTMLElement

      if (highlight) {
        // 鼠标在高亮区域，取消任何待关闭的定时器
        clearTooltipHideTimer()
        const commentId = highlight.getAttribute('data-comment-id')
        // 同一个评论不重复设置，避免创建新对象引用触发重渲染
        if (commentId && commentId !== lastHoveredCommentId) {
          lastHoveredCommentId = commentId
          const comment = commentsRef.current.find(
            (c) => c.id === Number.parseInt(commentId),
          )
          if (comment) {
            const rect = highlight.getBoundingClientRect()
            setCommentTooltip({
              comment,
              x: rect.left + rect.width / 2,
              y: rect.top - 8,
            })
          }
        }
      } else {
        lastHoveredCommentId = null
        // 鼠标离开高亮区域到文章其他内容，启动延迟关闭（如果不在 tooltip 上）
        if (!isHoveringTooltipRef.current) {
          startTooltipHideTimer()
        }
      }
    }

    // 鼠标离开内容区域（可能是移向 tooltip 或其他区域）
    const handleContentMouseLeave = () => {
      if (!isHoveringTooltipRef.current) {
        startTooltipHideTimer()
      }
    }

    contentRef.current.addEventListener('click', handleContentClick)
    contentRef.current.addEventListener('mouseover', handleMouseOver)
    contentRef.current.addEventListener('mouseleave', handleContentMouseLeave)

    return () => {
      contentRef.current?.removeEventListener('click', handleContentClick)
      contentRef.current?.removeEventListener('mouseover', handleMouseOver)
      contentRef.current?.removeEventListener(
        'mouseleave',
        handleContentMouseLeave,
      )
    }
  }, []) // 挂载一次，通过 commentsRef 读取最新评论

  // 保持 ref 与 state 同步，供滚动回调读取（避免把 activeHeadingId 放入 effect 依赖）
  activeHeadingIdRef.current = activeHeadingId

  // 监听滚动更新当前标题
  useEffect(() => {
    const article = articleRef.current
    const content = contentRef.current
    if (!article || !content || toc.length === 0) return

    // 缓存标题元素引用，避免每次滚动都调用 querySelectorAll
    const headings = Array.from(
      content.querySelectorAll('h1, h2, h3, h4, h5, h6'),
    ) as HTMLElement[]
    if (headings.length === 0) return

    const handleScrollForToc = () => {
      let currentId = ''

      for (const heading of headings) {
        const rect = heading.getBoundingClientRect()
        // 标题进入视口上方 150px 范围内就算当前标题
        if (rect.top <= 150) {
          currentId = heading.id
        }
      }

      const prevId = activeHeadingIdRef.current
      if (currentId !== prevId) {
        // 记录标题访问历史（去重，只记录最近 20 个）
        if (currentId && prevId) {
          setHeadingHistory((prev) => {
            const newHistory = prev.filter((id) => id !== prevId)
            newHistory.push(prevId)
            return newHistory.slice(-20)
          })
        }
        setActiveHeadingId(currentId)
      }
    }

    article.addEventListener('scroll', handleScrollForToc, { passive: true })
    return () => article.removeEventListener('scroll', handleScrollForToc)
  }, [toc])

  // 监听滚动
  useEffect(() => {
    const article = articleRef.current
    if (article) {
      article.addEventListener('scroll', updateReadingProgress)
      return () => article.removeEventListener('scroll', updateReadingProgress)
    }
  }, [updateReadingProgress])

  // 复制链接：自有内容用站内规范 URL，外部订阅仍用原文
  const handleShare = async () => {
    try {
      const url = (shareUrl && shareUrl.trim()) || item.link
      await navigator.clipboard.writeText(url)
      showToastMessage(t.brew.linkCopied)
    } catch (err) {
      console.error('Failed to copy:', err)
    }
  }

  // 跳转到指定标题 - useCallback 缓存
  const scrollToHeading = useCallback((id: string) => {
    const heading = document.getElementById(id)
    if (heading && articleRef.current) {
      const articleRect = articleRef.current.getBoundingClientRect()
      const headingRect = heading.getBoundingClientRect()
      const scrollTop =
        articleRef.current.scrollTop + headingRect.top - articleRect.top - 80

      articleRef.current.scrollTo({
        top: scrollTop,
        behavior: 'smooth',
      })

      setActiveHeadingId(id)
    }
  }, [])

  // 返回上一个标题 - useCallback 缓存
  const goToPreviousHeading = useCallback(() => {
    if (headingHistory.length > 0) {
      const prevId = headingHistory[headingHistory.length - 1]
      setHeadingHistory((prev) => prev.slice(0, -1))
      scrollToHeading(prevId)
      showToastMessage(t.brew.backToPrevParagraph, 1500)
    } else if (activeHeadingId && toc.length > 0) {
      // 没有历史时，跳转到当前标题的上一个
      const currentIndex = toc.findIndex((t) => t.id === activeHeadingId)
      if (currentIndex > 0) {
        scrollToHeading(toc[currentIndex - 1].id)
        showToastMessage(t.brew.backToPrevParagraph, 1500)
      }
    }
  }, [headingHistory, activeHeadingId, toc, scrollToHeading, showToastMessage])

  // 返回顶部 - useCallback 缓存
  const scrollToTop = useCallback(() => {
    if (articleRef.current) {
      articleRef.current.scrollTo({
        top: 0,
        behavior: 'smooth',
      })
      setHeadingHistory([])
      setActiveHeadingId('')
      showToastMessage(t.brew.backToTop, 1500)
    }
  }, [showToastMessage])

  // 进度按钮按下 - useCallback 缓存
  const handleProgressPointerDown = useCallback(() => {
    isLongPressRef.current = false
    progressLongPressRef.current = setTimeout(() => {
      isLongPressRef.current = true
      scrollToTop()
    }, 500) // 500ms 触发长按
  }, [scrollToTop])

  // 进度按钮抬起 - useCallback 缓存
  const handleProgressPointerUp = useCallback(() => {
    if (progressLongPressRef.current) {
      clearTimeout(progressLongPressRef.current)
      progressLongPressRef.current = null
    }
    // 如果不是长按，则执行点击
    if (!isLongPressRef.current) {
      goToPreviousHeading()
    }
  }, [goToPreviousHeading])

  // 进度按钮离开 - useCallback 缓存
  const handleProgressPointerLeave = useCallback(() => {
    if (progressLongPressRef.current) {
      clearTimeout(progressLongPressRef.current)
      progressLongPressRef.current = null
    }
  }, [])

  // 键盘快捷键
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      if (e.key === '+' || e.key === '=') adjustFontSize(1)
      if (e.key === '-') adjustFontSize(-1)
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onClose])

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

  // 处理文本选择
  const handleTextSelection = useCallback(() => {
    if (!isAuthenticated) return

    const selection = window.getSelection()
    if (!selection || selection.isCollapsed || !selection.rangeCount) {
      return
    }

    const text = selection.toString().trim()
    if (!text || text.length < 2 || text.length > 500) {
      return
    }

    // 确保选择在文章内容区域内
    const range = selection.getRangeAt(0)
    if (!contentRef.current?.contains(range.commonAncestorContainer)) {
      return
    }

    // 获取选中文本的位置信息
    const rect = range.getBoundingClientRect()

    // 使用视口坐标（因为弹窗是 fixed 定位）
    const x = rect.left + rect.width / 2
    const y = rect.top

    setSelectedText(text)
    setCommentPopupPosition({ x, y }) // 视口坐标

    // 获取上下文
    const fullText = contentRef.current?.textContent || ''
    const textIndex = fullText.indexOf(text)
    if (textIndex !== -1) {
      setSelectionRange({
        start: textIndex,
        end: textIndex + text.length,
        contextBefore: fullText.slice(Math.max(0, textIndex - 50), textIndex),
        contextAfter: fullText.slice(
          textIndex + text.length,
          textIndex + text.length + 50,
        ),
      })
    }

    setShowCommentPopup(true)
  }, [isAuthenticated])

  // 监听选择事件
  useEffect(() => {
    if (!isAuthenticated) return

    let selectionTimer: ReturnType<typeof setTimeout> | null = null

    const handleMouseUp = () => {
      // 延迟执行，等待选择完成
      selectionTimer = setTimeout(handleTextSelection, 10)
    }

    document.addEventListener('mouseup', handleMouseUp)
    return () => {
      document.removeEventListener('mouseup', handleMouseUp)
      if (selectionTimer) clearTimeout(selectionTimer)
    }
  }, [isAuthenticated, handleTextSelection])

  // 监听选中状态变化，当选中被移除时关闭弹窗
  useEffect(() => {
    if (!showCommentPopup) return

    const handleSelectionChange = () => {
      // 如果焦点在评论弹窗内部（比如 textarea），不要关闭弹窗
      const activeElement = document.activeElement
      if (activeElement?.closest('.comment-popup')) {
        return
      }

      const selection = window.getSelection()
      // 如果选中被清除（没有选中或选中为空），关闭弹窗
      if (!selection || selection.isCollapsed || !selection.toString().trim()) {
        setShowCommentPopup(false)
        setSelectedText('')
        setSelectionRange(null)
        setCommentInput('')
      }
    }

    document.addEventListener('selectionchange', handleSelectionChange)
    return () =>
      document.removeEventListener('selectionchange', handleSelectionChange)
  }, [showCommentPopup])

  // 点击其他地方关闭评论弹窗
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      // 如果点击的是评论弹窗内部或评论高亮，不关闭
      if (
        target.closest('.comment-popup') ||
        target.closest('.user-comment-highlight')
      ) {
        return
      }
      if (showCommentPopup) {
        setShowCommentPopup(false)
        setSelectedText('')
        setSelectionRange(null)
        setCommentInput('')
        // 关闭弹窗时清除浏览器选中状态
        window.getSelection()?.removeAllRanges()
      }
    }

    // 使用 click 而不是 mousedown，避免在文本选择时触发
    document.addEventListener('click', handleClickOutside)
    return () => document.removeEventListener('click', handleClickOutside)
  }, [showCommentPopup])

  // 包装 scrollToAnnotation 以传入 refs
  const handleScrollToAnnotation = useCallback(
    (annotation: AnnotationItem) => {
      scrollToAnnotation(annotation, contentRef, articleRef)
    },
    [scrollToAnnotation],
  )

  // 监听注释 hover 事件（优化稳定性）
  useEffect(() => {
    if (!contentRef.current || !showAnnotations) return

    const handleMouseOver = (e: MouseEvent) => {
      const target = (e.target as HTMLElement).closest(
        '.brewlia-annotation',
      ) as HTMLElement

      if (target) {
        // 鼠标在注释上：清除隐藏定时器，显示 tooltip
        if (hoverTimeoutRef.current) {
          clearTimeout(hoverTimeoutRef.current)
          hoverTimeoutRef.current = null
        }

        const term = decodeURIComponent(target.dataset.term || '')
        const explanation = decodeURIComponent(target.dataset.explanation || '')
        const type = (target.dataset.type as AnnotationType) || 'term'

        const rect = target.getBoundingClientRect()
        setTooltipPosition({
          x: rect.left + rect.width / 2,
          y: rect.top - 8,
        })
        setHoveredAnnotation({ term, explanation, type })
      } else {
        // 鼠标移到非注释元素：启动延迟隐藏
        if (!hoverTimeoutRef.current) {
          hoverTimeoutRef.current = setTimeout(() => {
            hoverTimeoutRef.current = null
            setHoveredAnnotation(null)
          }, 150)
        }
      }
    }

    // 鼠标离开内容区域：兜底隐藏
    const handleMouseLeave = () => {
      if (!hoverTimeoutRef.current) {
        hoverTimeoutRef.current = setTimeout(() => {
          hoverTimeoutRef.current = null
          setHoveredAnnotation(null)
        }, 150)
      }
    }

    const container = contentRef.current
    container.addEventListener('mouseover', handleMouseOver)
    container.addEventListener('mouseleave', handleMouseLeave)

    return () => {
      container.removeEventListener('mouseover', handleMouseOver)
      container.removeEventListener('mouseleave', handleMouseLeave)
      if (hoverTimeoutRef.current) {
        clearTimeout(hoverTimeoutRef.current)
        hoverTimeoutRef.current = null
      }
      setHoveredAnnotation(null)
    }
  }, [showAnnotations])

  // 关闭所有附属的 tooltip 和面板
  const closeAllTooltips = useCallback(() => {
    setShowToc(false)
    setShowBrewliaPanel(false)
    setShowPodcastPlayer(false)
    setShowVoiceSettings(false)
    setHoveredAnnotation(null)
    setCommentTooltip(null)
  }, [])

  // 自动隐藏控制栏
  const resetHideTimer = useCallback(
    (delay = 4000) => {
      if (hideTimerRef.current) {
        clearTimeout(hideTimerRef.current)
      }
      hideTimerRef.current = setTimeout(() => {
        // 如果鼠标在控制栏区域，不隐藏
        if (isHoveringControlsRef.current) {
          resetHideTimer(delay)
          return
        }
        setShowPanels(false)
        // 关闭所有附属的 tooltip
        closeAllTooltips()
        // 进入冷却期
        cooldownRef.current = true
        setTimeout(() => {
          cooldownRef.current = false
        }, 800)
      }, delay)
    },
    [closeAllTooltips],
  )

  // 显示控制栏
  const showPanelsIfAllowed = useCallback(() => {
    if (cooldownRef.current || isScrollingRef.current) return
    setShowPanels(true)
    resetHideTimer()
  }, [resetHideTimer])

  // 滚动时：向上滚动显示控制栏2秒，向下滚动隐藏
  useEffect(() => {
    const article = articleRef.current
    if (!article) return

    let scrollEndTimer: ReturnType<typeof setTimeout> | null = null

    const handleScroll = () => {
      const currentScrollTop = article.scrollTop
      const isScrollingUp = currentScrollTop < lastScrollTopRef.current
      lastScrollTopRef.current = currentScrollTop

      // 清除之前的定时器
      if (scrollEndTimer) clearTimeout(scrollEndTimer)

      // 向上滚动（往之前内容滑动）时显示控制栏
      if (isScrollingUp && currentScrollTop > 10) {
        isScrollingRef.current = false
        setShowPanels(true)
        resetHideTimer(2000) // 显示2秒后自动隐藏
      } else if (!isScrollingUp) {
        // 向下滚动时隐藏（如果鼠标不在控制栏区域）
        if (!isScrollingRef.current && !isHoveringControlsRef.current) {
          isScrollingRef.current = true
          setShowPanels(false)
          // 关闭所有附属的 tooltip
          closeAllTooltips()
        }
      }

      // 停止滚动后的处理
      scrollEndTimer = setTimeout(() => {
        isScrollingRef.current = false
      }, 150)
    }

    article.addEventListener('scroll', handleScroll, { passive: true })
    return () => {
      article.removeEventListener('scroll', handleScroll)
      if (scrollEndTimer) clearTimeout(scrollEndTimer)
    }
  }, [resetHideTimer, closeAllTooltips])

  // 鼠标移动时显示（节流处理 + 控制栏区域检测）
  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      const now = Date.now()
      const windowWidth = window.innerWidth

      // 根据当前布局计算内容区宽度
      // narrow: max-w-3xl = 768px, wide: max-w-4xl = 896px
      const layoutMaxWidth = layout === 'wide' ? 896 : 768
      const contentWidth = Math.min(layoutMaxWidth, windowWidth - 48) // 减去 px-6 左右内边距
      const contentLeft = (windowWidth - contentWidth) / 2
      const contentRight = contentLeft + contentWidth

      // 控制栏区域：内容区两侧各 80px 范围内（控制栏宽度约 60px + margin）
      const controlZoneWidth = 80
      const isInLeftControlZone =
        e.clientX >= contentLeft - controlZoneWidth && e.clientX <= contentLeft
      const isInRightControlZone =
        e.clientX >= contentRight &&
        e.clientX <= contentRight + controlZoneWidth
      const isInControlZone = isInLeftControlZone || isInRightControlZone

      // 更新悬停状态
      isHoveringControlsRef.current = isInControlZone

      // 控制栏区域立即响应，其他区域节流
      if (isInControlZone) {
        // 控制栏区域：直接显示，不节流，不自动隐藏
        if (cooldownRef.current) return
        isScrollingRef.current = false // 允许覆盖滚动隐藏
        setShowPanels(true)
        // 清除隐藏定时器，鼠标在控制栏区域时不隐藏
        if (hideTimerRef.current) {
          clearTimeout(hideTimerRef.current)
          hideTimerRef.current = null
        }
      } else {
        // 非控制栏区域：节流 500ms
        if (now - lastMouseMoveRef.current < 500) return
        lastMouseMoveRef.current = now
        showPanelsIfAllowed()
      }
    }

    // 鼠标离开控制栏区域时重新设置隐藏定时器
    const handleMouseLeave = () => {
      isHoveringControlsRef.current = false
      if (showPanels) {
        resetHideTimer(2000)
      }
    }

    window.addEventListener('mousemove', handleMouseMove, { passive: true })
    document.addEventListener('mouseleave', handleMouseLeave)
    return () => {
      window.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseleave', handleMouseLeave)
      if (hideTimerRef.current) clearTimeout(hideTimerRef.current)
    }
  }, [showPanelsIfAllowed, resetHideTimer, layout, showPanels])

  // 初始化隐藏计时器
  useEffect(() => {
    resetHideTimer()
    return () => {
      if (hideTimerRef.current) clearTimeout(hideTimerRef.current)
    }
  }, [resetHideTimer])

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

  // 遮罩渐变背景样式 - useMemo 缓存避免每次渲染重新创建对象
  const maskGradientStyles = useMemo(() => {
    const bgColor =
      theme === 'light'
        ? '#f8f5ec'
        : theme === 'sepia'
          ? '#f4ecd8'
          : theme === 'dark'
            ? '#1a1a1a'
            : '#0d1117'
    return {
      top: {
        background: `linear-gradient(to bottom, ${bgColor} 0%, ${bgColor}00 100%)`,
      },
      bottom: {
        background: `linear-gradient(to top, ${bgColor} 0%, ${bgColor}00 100%)`,
      },
    }
  }, [theme])

  return (
    <motion.div
      {...readerAnimProps}
      style={STYLE_READER_CONTAINER}
      className={`fixed inset-0 z-50 ${currentTheme.bg}`}
      data-brew-reader="true"
    >
      {/* 顶部进度条 - 用 scaleX 替代 width 动画，避免触发 layout recalculation */}
      <div className="absolute top-0 left-0 right-0 h-0.5 z-10 overflow-hidden">
        <div
          className="h-full w-full bg-linear-to-r from-amber-500 to-orange-500 origin-left"
          style={{ transform: `scaleX(${readingProgress / 100})` }}
        />
      </div>

      {/* 顶部淡出遮罩 */}
      <div
        className="absolute top-0 left-0 right-0 h-24 pointer-events-none z-5"
        style={maskGradientStyles.top}
      />

      {/* 底部淡入遮罩 */}
      <div
        className="absolute bottom-0 left-0 right-0 h-24 pointer-events-none z-5"
        style={maskGradientStyles.bottom}
      />

      {/* 主内容区 - 三栏布局 */}
      {/* transform: translateZ(0) 将滚动容器提升为独立合成层，避免 sticky 子元素回流影响主线程 */}
      <article
        ref={articleRef}
        className="h-full overflow-y-auto overflow-x-hidden"
        style={{ ...STYLE_SCROLL_SMOOTH, transform: 'translateZ(0)' }}
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

          {/* 正文内容 */}
          <div
            className={`w-full ${currentLayout.width} px-6 py-16 transition-all duration-300`}
          >
            {/* 标题 */}
            <h1
              className={`text-3xl font-bold ${currentTheme.text} leading-tight mb-6`}
              style={{ fontFamily: currentFont.family }}
            >
              {item.title}
            </h1>

            {/* 元信息 */}
            <div
              className={`flex flex-wrap items-center gap-4 text-sm ${currentTheme.secondary} mb-8 pb-8 border-b ${currentTheme.border}`}
            >
              <span className="flex items-center gap-1.5">
                {item.source_icon && (
                  <img
                    src={item.source_icon}
                    alt=""
                    className="w-4 h-4 rounded"
                  />
                )}
                {item.source_name}
              </span>
              {item.author && (
                <span className="flex items-center gap-1.5">
                  <User className="w-4 h-4" />
                  {item.author}
                </span>
              )}
              {item.published_at && (
                <span className="flex items-center gap-1.5">
                  <Calendar className="w-4 h-4" />
                  {formattedDate}
                </span>
              )}
              {item.reading_time && (
                <span className="flex items-center gap-1.5">
                  <Clock className="w-4 h-4" />
                  {t.brew.readingTime.replace(
                    '{time}',
                    String(item.reading_time),
                  )}
                </span>
              )}
              {item.word_count && (
                <span>
                  {item.word_count.toLocaleString()} {t.brew.wordCount}
                </span>
              )}
            </div>

            {/* 封面图 */}
            {item.image && (
              <div className="mb-8">
                <img
                  src={getImageUrl(item.image) || ''}
                  alt=""
                  className="w-full rounded-2xl"
                  loading="lazy"
                />
              </div>
            )}

            {/* 正文内容 */}
            <div
              ref={contentRef}
              className={`
              prose prose-lg max-w-none min-w-0 ${currentTheme.text}
              /* 超长 URL / 无空格串不得撑破阅读器 */
              break-words [overflow-wrap:anywhere]

              /* 标题 - 简洁无装饰 */
              prose-headings:font-semibold prose-headings:leading-snug
              prose-h1:text-[1.5em] prose-h1:mt-8 prose-h1:mb-4
              prose-h2:text-[1.25em] prose-h2:mt-7 prose-h2:mb-3
              prose-h3:text-[1.1em] prose-h3:mt-6 prose-h3:mb-2
              prose-h4:text-[1em] prose-h4:mt-5 prose-h4:mb-2 prose-h4:font-medium

              /* 段落 */
              prose-p:my-[1em]

              /* 加粗文本 - 明确样式防止被覆盖 */
              prose-strong:font-bold prose-strong:no-underline
              [&_strong]:font-bold [&_strong]:no-underline [&_strong]:not-italic
              [&_b]:font-bold [&_b]:no-underline [&_b]:not-italic

              /* 删除线 - 仅对 del/s/strike 应用 */
              [&_del]:line-through [&_del]:opacity-60
              [&_s]:line-through [&_s]:opacity-60
              [&_strike]:line-through [&_strike]:opacity-60

              /* 链接 - 简洁下划线 + 防溢出 */
              prose-a:font-normal prose-a:underline prose-a:underline-offset-2
              prose-a:decoration-1 prose-a:transition-colors
              prose-a:wrap-break-word [&_a]:overflow-wrap-anywhere

              /* 列表 - 紧凑 */
              prose-ul:my-4 prose-ul:pl-5
              prose-ol:my-4 prose-ol:pl-5
              prose-li:my-1 prose-li:pl-0.5

              /* 引用块 - 轻盈圆角 */
              prose-blockquote:not-italic prose-blockquote:font-normal
              prose-blockquote:border-0 prose-blockquote:rounded-2xl
              prose-blockquote:px-5 prose-blockquote:py-4 prose-blockquote:my-5

              /* 行内代码 - 柔和 */
              prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded-lg
              prose-code:text-[0.9em] prose-code:font-normal
              prose-code:before:content-none prose-code:after:content-none

              /* 代码块 - 干净圆角 + 相对定位（支持复制按钮） */
              prose-pre:rounded-2xl prose-pre:px-5 prose-pre:py-4
              prose-pre:overflow-x-auto prose-pre:text-[0.875em]
              prose-pre:leading-relaxed prose-pre:relative
              /* 代码块内的code不要额外样式 */
              [&_pre_code]:p-0 [&_pre_code]:bg-transparent [&_pre_code]:rounded-none
              [&_pre_code]:text-inherit
              /* 代码块容器（有复制按钮时）*/
              [&_.code-block-wrapper]:my-5

              /* 图片 - 自然圆角 */
              prose-img:rounded-2xl prose-img:mx-auto prose-img:my-5

              /* 分隔线 - 极简 */
              prose-hr:my-8 prose-hr:border-0 prose-hr:h-px

              /* 表格 - 简约 */
              prose-table:my-5 prose-table:w-full prose-table:text-[0.9em]
              prose-thead:border-0
              prose-th:py-2.5 prose-th:px-3 prose-th:text-left prose-th:font-medium
              prose-td:py-2 prose-td:px-3
              [&_table]:rounded-xl [&_table]:overflow-hidden

              /* KaTeX 数学公式 */
              [&_.katex]:text-[1.05em]
              [&_.katex-display]:my-5 [&_.katex-display]:py-4 [&_.katex-display]:px-4
              [&_.katex-display]:overflow-x-auto [&_.katex-display]:rounded-2xl

              /* figure */
              prose-figure:my-6
              prose-figcaption:text-center prose-figcaption:text-[0.85em] prose-figcaption:mt-2
              prose-figcaption:opacity-60

              /* details 折叠 */
              [&_details]:my-4 [&_details]:rounded-2xl [&_details]:overflow-hidden
              [&_summary]:cursor-pointer [&_summary]:py-3 [&_summary]:px-4
              [&_summary]:font-medium [&_summary]:select-none
              [&_details[open]_summary]:mb-2

              /* kbd 按键 */
              [&_kbd]:px-1.5 [&_kbd]:py-0.5 [&_kbd]:rounded-lg
              [&_kbd]:text-[0.8em] [&_kbd]:font-mono

              /* mark 高亮 */
              [&_mark]:px-1 [&_mark]:rounded-md [&_mark]:bg-transparent

              /* 脚注 */
              prose-footnotes:text-[0.85em] prose-footnotes:mt-8 prose-footnotes:opacity-70

              /* 嵌入卡片通用样式 */
              [&_.brew-embed-card]:my-6 [&_.brew-embed-card]:font-sans
              [&_.brew-embed-card]:text-base [&_.brew-embed-card]:leading-normal
              [&_.brew-embed-card_*]:no-underline

              /* RSS 内容适配样式 */

              /* RSS 图片 - 响应式 + 圆角 */
              [&_.rss-content-image]:rounded-xl [&_.rss-content-image]:max-w-full
              [&_.rss-content-image]:h-auto [&_.rss-content-image]:mx-auto
              [&_.rss-content-image]:block [&_.rss-content-image]:my-5

              /* RSS 图片容器 figure */
              [&_.rss-content-figure]:my-6 [&_.rss-content-figure]:text-center
              [&_.rss-content-figcaption]:text-[0.85em] [&_.rss-content-figcaption]:mt-2
              [&_.rss-content-figcaption]:opacity-60

              /* RSS 视频 */
              [&_.rss-content-video]:rounded-xl [&_.rss-content-video]:w-full
              [&_.rss-content-video]:my-5

              /* RSS 音频 */
              [&_.rss-content-audio]:w-full [&_.rss-content-audio]:my-4

              /* RSS iframe 包装 - 响应式容器（aspect-ratio 替代 padding-bottom hack）*/
              [&_.rss-content-iframe-wrapper]:w-full
              [&_.rss-content-iframe-wrapper]:my-5 [&_.rss-content-iframe-wrapper]:rounded-xl
              [&_.rss-content-iframe-wrapper]:overflow-hidden
              [&_.rss-content-iframe-wrapper.aspect-video]:aspect-video
              [&_.rss-content-iframe-wrapper.aspect-wide]:aspect-3/1
              [&_.rss-content-iframe-wrapper_iframe]:w-full [&_.rss-content-iframe-wrapper_iframe]:h-full
              [&_.rss-content-iframe-wrapper_iframe]:border-0

              /* RSS 表格包装 - 横向滚动 */
              [&_.rss-content-table-wrapper]:overflow-x-auto [&_.rss-content-table-wrapper]:my-5
              [&_.rss-content-table-wrapper]:rounded-xl
              [&_.rss-content-table]:w-full [&_.rss-content-table]:text-[0.9em]
              [&_.rss-content-table]:border-collapse
              [&_.rss-content-th]:py-2 [&_.rss-content-th]:px-3
              [&_.rss-content-th]:text-left [&_.rss-content-th]:font-medium
              [&_.rss-content-td]:py-2 [&_.rss-content-td]:px-3

              /* RSS 描述列表 */
              [&_.rss-content-dl]:my-4
              [&_.rss-content-dt]:font-semibold [&_.rss-content-dt]:mt-3
              [&_.rss-content-dd]:ml-4 [&_.rss-content-dd]:pl-4 [&_.rss-content-dd]:mt-1

              /* RSS 折叠组件 */
              [&_.rss-content-details]:my-4 [&_.rss-content-details]:rounded-xl
              [&_.rss-content-details]:overflow-hidden
              [&_.rss-content-summary]:cursor-pointer [&_.rss-content-summary]:py-3
              [&_.rss-content-summary]:px-4 [&_.rss-content-summary]:font-medium
              [&_.rss-content-summary]:select-none [&_.rss-content-summary]:transition-colors
              [&_.rss-content-details[open]_.rss-content-summary]:border-b

              /* RSS 链接 - 文字断行 */
              [&_.rss-content-link]:wrap-break-word [&_.rss-content-link]:underline
              [&_.rss-content-link]:underline-offset-2 [&_.rss-content-link]:decoration-1

              /* RSS kbd 按键样式 */
              [&_.rss-content-kbd]:px-1.5 [&_.rss-content-kbd]:py-0.5
              [&_.rss-content-kbd]:rounded [&_.rss-content-kbd]:text-[0.85em]
              [&_.rss-content-kbd]:font-mono [&_.rss-content-kbd]:border

              /* RSS mark 高亮 */
              [&_.rss-content-mark]:px-0.5 [&_.rss-content-mark]:rounded

              /* RSS abbr 缩写 */
              [&_.rss-content-abbr]:border-b [&_.rss-content-abbr]:border-dashed
              [&_.rss-content-abbr]:cursor-help

              /* RSS 分隔线 */
              [&_.rss-content-hr]:border-0 [&_.rss-content-hr]:h-px [&_.rss-content-hr]:my-8

              /* RSS 删除线/插入 */
              [&_.rss-content-del]:line-through [&_.rss-content-del]:opacity-60
              [&_.rss-content-ins]:underline

              /* RSS 小号文本 */
              [&_.rss-content-small]:text-[0.85em] [&_.rss-content-small]:opacity-80

              /* RSS 上下标 */
              [&_.rss-content-sup]:text-[0.75em]
              [&_.rss-content-sub]:text-[0.75em]

              /* RSS 时间戳 */
              [&_.rss-content-time]:tabular-nums

              /* RSS 类别标签 */
              [&_.rss-content-category]:inline-block [&_.rss-content-category]:px-2
              [&_.rss-content-category]:py-0.5 [&_.rss-content-category]:text-xs
              [&_.rss-content-category]:rounded-full [&_.rss-content-category]:mr-1

              /* RSS 语义标签 */
              [&_.rss-content-aside]:my-4 [&_.rss-content-aside]:p-4
              [&_.rss-content-aside]:rounded-xl [&_.rss-content-aside]:opacity-80
              [&_.rss-content-header]:mb-4
              [&_.rss-content-footer]:mt-4 [&_.rss-content-footer]:text-sm
              [&_.rss-content-footer]:opacity-70

              /* 网络搜索摘要样式 - 浅色主题 */
              /* 摘要容器 - 不设置固定字体大小，继承阅读器设置 */
              [&_.web-search-summary]:leading-[inherit]

              /* 摘要段落 - 继承阅读器的行高和字体 */
              [&_.web-search-summary_p]:mb-4 [&_.web-search-summary_p]:last:mb-0

              /* 页脚区域 */
              [&_.web-search-footer]:mt-8 [&_.web-search-footer]:pt-6
              [&_.web-search-footer]:border-t [&_.web-search-footer]:border-black/10
              [&_.web-search-footer]:flex [&_.web-search-footer]:items-center
              [&_.web-search-footer]:justify-between [&_.web-search-footer]:gap-4

              /* AI 生成说明 */
              [&_.web-search-note]:text-sm [&_.web-search-note]:opacity-50
              [&_.web-search-note]:m-0

              /* 原文链接按钮 */
              [&_.web-search-link]:inline-flex [&_.web-search-link]:items-center
              [&_.web-search-link]:gap-1.5 [&_.web-search-link]:text-sm
              [&_.web-search-link]:px-3 [&_.web-search-link]:py-1.5
              [&_.web-search-link]:rounded-lg [&_.web-search-link]:no-underline
              [&_.web-search-link]:bg-black/5 [&_.web-search-link]:hover:bg-black/10
              [&_.web-search-link]:transition-colors

              ${
                isDark
                  ? `
                /* 暗色主题 */
                prose-invert

                /* 链接 */
                prose-a:text-blue-400 prose-a:decoration-blue-400/40
                hover:prose-a:text-blue-300 hover:prose-a:decoration-blue-300/60

                /* 引用块 */
                prose-blockquote:bg-white/3

                /* 行内代码 */
                prose-code:bg-white/8 prose-code:text-amber-200/90

                /* 代码块 */
                prose-pre:bg-white/4

                /* 分隔线 */
                prose-hr:bg-white/6

                /* 表格 */
                [&_table]:bg-white/2
                [&_thead]:bg-white/3
                [&_tbody_tr:nth-child(even)]:bg-white/2

                /* 数学公式 */
                [&_.katex-display]:bg-white/3

                /* details */
                [&_details]:bg-white/3
                [&_summary:hover]:bg-white/5

                /* kbd */
                [&_kbd]:bg-white/8

                /* mark */
                [&_mark]:text-amber-200 [&_mark]:bg-amber-500/20

                /* RSS 内容样式 - 暗色主题 */

                /* RSS 引用块 */
                [&_.rss-content-blockquote]:bg-white/3 [&_.rss-content-blockquote]:border-white/10

                /* RSS 代码 */
                [&_.rss-content-pre]:bg-white/4
                [&_.rss-content-inline-code]:bg-white/8

                /* RSS 表格 */
                [&_.rss-content-table]:border-white/10
                [&_.rss-content-thead]:bg-white/5
                [&_.rss-content-th]:border-white/10
                [&_.rss-content-td]:border-white/10
                [&_.rss-content-tr:nth-child(even)]:bg-white/2

                /* RSS 描述列表 */
                [&_.rss-content-dd]:border-white/10

                /* RSS 折叠 */
                [&_.rss-content-details]:bg-white/3
                [&_.rss-content-summary]:hover:bg-white/5
                [&_.rss-content-details[open]_.rss-content-summary]:border-white/10

                /* RSS kbd */
                [&_.rss-content-kbd]:bg-white/8 [&_.rss-content-kbd]:border-white/20

                /* RSS mark */
                [&_.rss-content-mark]:bg-yellow-500/30

                /* RSS abbr */
                [&_.rss-content-abbr]:border-white/30

                /* RSS 分隔线 */
                [&_.rss-content-hr]:bg-white/10

                /* RSS 类别标签 */
                [&_.rss-content-category]:bg-white/10

                /* RSS 侧边栏 */
                [&_.rss-content-aside]:bg-white/3

                /* Brewlia 注释样式 - 暗色主题 */
                [&_.brewlia-annotation]:cursor-help [&_.brewlia-annotation]:rounded [&_.brewlia-annotation]:px-0.5
                [&_.brewlia-annotation]:transition-all [&_.brewlia-annotation]:duration-200
                [&_.brewlia-annotation]:border-b-2 [&_.brewlia-annotation]:border-dotted
                [&_.brewlia-annotation]:break-words [&_.brewlia-annotation]:[overflow-wrap:anywhere]
                [&_.brewlia-annotation[data-type="term"]]:text-orange-300 [&_.brewlia-annotation[data-type="term"]]:bg-orange-500/15 [&_.brewlia-annotation[data-type="term"]]:border-orange-400/50
                [&_.brewlia-annotation[data-type="reference"]]:text-blue-300 [&_.brewlia-annotation[data-type="reference"]]:bg-blue-500/15 [&_.brewlia-annotation[data-type="reference"]]:border-blue-400/50
                [&_.brewlia-annotation[data-type="implicit"]]:text-purple-300 [&_.brewlia-annotation[data-type="implicit"]]:bg-purple-500/15 [&_.brewlia-annotation[data-type="implicit"]]:border-purple-400/50
                [&_.brewlia-annotation[data-type="context"]]:text-green-300 [&_.brewlia-annotation[data-type="context"]]:bg-green-500/15 [&_.brewlia-annotation[data-type="context"]]:border-green-400/50
                [&_.brewlia-annotation[data-type="abbreviation"]]:text-pink-300 [&_.brewlia-annotation[data-type="abbreviation"]]:bg-pink-500/15 [&_.brewlia-annotation[data-type="abbreviation"]]:border-pink-400/50
                [&_.brewlia-annotation:hover]:ring-2 [&_.brewlia-annotation:hover]:ring-current/30
                [&_.brewlia-highlight-flash]:animate-pulse [&_.brewlia-highlight-flash]:ring-2 [&_.brewlia-highlight-flash]:ring-purple-400

                /* 网络搜索摘要样式 - 暗色主题 */
                /* 页脚区域 - 暗色主题 */
                [&_.web-search-footer]:border-white/10

                /* 原文链接按钮 - 暗色主题 */
                [&_.web-search-link]:bg-white/10 [&_.web-search-link]:hover:bg-white/15

                /* Notion 内容样式 - 暗色主题 */

                /* Notion 颜色 - 文字 */
                [&_.notion-gray]:text-gray-400
                [&_.notion-brown]:text-amber-400
                [&_.notion-orange]:text-orange-400
                [&_.notion-yellow]:text-yellow-400
                [&_.notion-green]:text-green-400
                [&_.notion-blue]:text-blue-400
                [&_.notion-purple]:text-purple-400
                [&_.notion-pink]:text-pink-400
                [&_.notion-red]:text-red-400

                /* Notion 颜色 - 背景 */
                [&_.notion-bg-gray]:bg-gray-500/20 [&_.notion-bg-gray]:px-1 [&_.notion-bg-gray]:rounded
                [&_.notion-bg-brown]:bg-amber-500/20 [&_.notion-bg-brown]:px-1 [&_.notion-bg-brown]:rounded
                [&_.notion-bg-orange]:bg-orange-500/20 [&_.notion-bg-orange]:px-1 [&_.notion-bg-orange]:rounded
                [&_.notion-bg-yellow]:bg-yellow-500/20 [&_.notion-bg-yellow]:px-1 [&_.notion-bg-yellow]:rounded
                [&_.notion-bg-green]:bg-green-500/20 [&_.notion-bg-green]:px-1 [&_.notion-bg-green]:rounded
                [&_.notion-bg-blue]:bg-blue-500/20 [&_.notion-bg-blue]:px-1 [&_.notion-bg-blue]:rounded
                [&_.notion-bg-purple]:bg-purple-500/20 [&_.notion-bg-purple]:px-1 [&_.notion-bg-purple]:rounded
                [&_.notion-bg-pink]:bg-pink-500/20 [&_.notion-bg-pink]:px-1 [&_.notion-bg-pink]:rounded
                [&_.notion-bg-red]:bg-red-500/20 [&_.notion-bg-red]:px-1 [&_.notion-bg-red]:rounded

                /* Notion Callout */
                [&_.notion-callout]:flex [&_.notion-callout]:items-start [&_.notion-callout]:gap-3
                [&_.notion-callout]:p-4 [&_.notion-callout]:my-4 [&_.notion-callout]:rounded-xl
                [&_.notion-callout]:bg-white/4 [&_.notion-callout]:border [&_.notion-callout]:border-white/10
                [&_.notion-callout-icon]:text-xl [&_.notion-callout-icon]:shrink-0
                [&_.notion-callout-icon]:w-6 [&_.notion-callout-icon]:h-6 [&_.notion-callout-icon]:object-contain
                [&_.notion-callout-content]:flex-1 [&_.notion-callout-content]:min-w-0

                /* Notion Quote */
                [&_.notion-quote]:pl-4 [&_.notion-quote]:py-1 [&_.notion-quote]:my-4
                [&_.notion-quote]:bg-white/4 [&_.notion-quote]:rounded-xl

                /* Notion Todo */
                [&_.notion-todo]:flex [&_.notion-todo]:items-start [&_.notion-todo]:gap-2 [&_.notion-todo]:my-1
                [&_.notion-checkbox]:w-5 [&_.notion-checkbox]:h-5 [&_.notion-checkbox]:shrink-0
                [&_.notion-checkbox]:border-2 [&_.notion-checkbox]:border-white/30 [&_.notion-checkbox]:rounded
                [&_.notion-checkbox.checked]:bg-blue-500 [&_.notion-checkbox.checked]:border-blue-500
                [&_.notion-checkbox.checked]:after:content-['✓'] [&_.notion-checkbox.checked]:after:text-white
                [&_.notion-checkbox.checked]:after:text-xs [&_.notion-checkbox.checked]:after:flex
                [&_.notion-checkbox.checked]:after:items-center [&_.notion-checkbox.checked]:after:justify-center
                [&_.notion-todo-text.checked]:line-through [&_.notion-todo-text.checked]:opacity-60

                /* Notion Toggle */
                [&_.notion-toggle]:bg-white/3 [&_.notion-toggle]:border [&_.notion-toggle]:border-white/10
                [&_.notion-toggle]:rounded-xl [&_.notion-toggle]:my-3
                [&_.notion-toggle_summary]:px-4 [&_.notion-toggle_summary]:py-3

                /* Notion Image */
                [&_.notion-image]:my-6
                [&_.notion-image_img]:rounded-xl [&_.notion-image_img]:w-full
                [&_.notion-image_figcaption]:text-center [&_.notion-image_figcaption]:text-sm
                [&_.notion-image_figcaption]:mt-2 [&_.notion-image_figcaption]:opacity-60

                /* Notion Video */
                [&_.notion-video]:my-6
                [&_.notion-video-embed]:aspect-video [&_.notion-video-embed]:w-full
                [&_.notion-video-embed]:rounded-xl [&_.notion-video-embed]:overflow-hidden
                [&_.notion-video-embed_iframe]:w-full [&_.notion-video-embed_iframe]:h-full
                [&_.notion-video_video]:w-full [&_.notion-video_video]:rounded-xl

                /* Notion Audio */
                [&_.notion-audio]:my-4
                [&_.notion-audio_audio]:w-full

                /* Notion Bookmark */
                [&_.notion-bookmark]:flex [&_.notion-bookmark]:items-center [&_.notion-bookmark]:gap-3
                [&_.notion-bookmark]:p-4 [&_.notion-bookmark]:my-4 [&_.notion-bookmark]:rounded-xl
                [&_.notion-bookmark]:bg-white/4 [&_.notion-bookmark]:border [&_.notion-bookmark]:border-white/10
                [&_.notion-bookmark]:no-underline [&_.notion-bookmark]:hover:bg-white/6
                [&_.notion-bookmark-icon]:text-lg
                [&_.notion-bookmark-title]:font-medium [&_.notion-bookmark-title]:flex-1
                [&_.notion-bookmark-url]:text-sm [&_.notion-bookmark-url]:opacity-50 [&_.notion-bookmark-url]:truncate [&_.notion-bookmark-url]:max-w-48

                /* Notion Link Preview */
                [&_.notion-link-preview]:inline-flex [&_.notion-link-preview]:items-center [&_.notion-link-preview]:gap-1.5
                [&_.notion-link-preview]:px-2 [&_.notion-link-preview]:py-0.5 [&_.notion-link-preview]:rounded-md
                [&_.notion-link-preview]:bg-white/6 [&_.notion-link-preview]:no-underline
                [&_.notion-link-preview]:hover:bg-white/10

                /* Notion File */
                [&_.notion-file]:inline-flex [&_.notion-file]:items-center [&_.notion-file]:gap-2
                [&_.notion-file]:px-3 [&_.notion-file]:py-2 [&_.notion-file]:my-2 [&_.notion-file]:rounded-lg
                [&_.notion-file]:bg-white/4 [&_.notion-file]:border [&_.notion-file]:border-white/10
                [&_.notion-file]:no-underline [&_.notion-file]:hover:bg-white/8

                /* Notion Embed */
                [&_.notion-embed]:my-6
                [&_.notion-embed-wrapper]:aspect-video [&_.notion-embed-wrapper]:w-full
                [&_.notion-embed-wrapper]:rounded-xl [&_.notion-embed-wrapper]:overflow-hidden
                [&_.notion-embed-wrapper_iframe]:w-full [&_.notion-embed-wrapper_iframe]:h-full

                /* Notion PDF */
                [&_.notion-pdf]:my-6
                [&_.notion-pdf-embed]:w-full [&_.notion-pdf-embed]:h-150 [&_.notion-pdf-embed]:rounded-xl
                [&_.notion-pdf-embed]:border [&_.notion-pdf-embed]:border-white/10

                /* Notion Equation */
                [&_.notion-equation]:my-4 [&_.notion-equation]:py-4 [&_.notion-equation]:px-6
                [&_.notion-equation]:bg-white/3 [&_.notion-equation]:rounded-xl
                [&_.notion-equation]:overflow-x-auto [&_.notion-equation]:text-center

                /* Notion Table */
                [&_.notion-table]:w-full [&_.notion-table]:my-4 [&_.notion-table]:border-collapse
                [&_.notion-table]:rounded-xl [&_.notion-table]:overflow-hidden
                [&_.notion-table_td]:px-3 [&_.notion-table_td]:py-2
                [&_.notion-table_td]:border [&_.notion-table_td]:border-white/10
                [&_.notion-table.has-header_tr:first-child]:bg-white/5
                [&_.notion-table.has-header_tr:first-child_td]:font-medium

                /* Notion Columns */
                [&_.notion-columns]:flex [&_.notion-columns]:gap-4 [&_.notion-columns]:my-4
                [&_.notion-column]:flex-1 [&_.notion-column]:min-w-0

                /* Notion Page Link */
                [&_.notion-page-link]:inline-flex [&_.notion-page-link]:items-center [&_.notion-page-link]:gap-1.5
                [&_.notion-page-link]:px-2 [&_.notion-page-link]:py-1 [&_.notion-page-link]:rounded-md
                [&_.notion-page-link]:bg-white/4 [&_.notion-page-link]:no-underline
                [&_.notion-page-link]:hover:bg-white/8

                /* Notion Code */
                [&_.notion-code]:my-4
                [&_.notion-code_pre]:rounded-xl [&_.notion-code_pre]:overflow-x-auto
                [&_.notion-code_figcaption]:text-center [&_.notion-code_figcaption]:text-sm
                [&_.notion-code_figcaption]:mt-2 [&_.notion-code_figcaption]:opacity-50

              `
                  : `
                /* 浅色主题 */

                /* 链接 */
                prose-a:text-amber-700 prose-a:decoration-amber-600/30
                hover:prose-a:text-amber-800 hover:prose-a:decoration-amber-700/50

                /* 引用块 */
                prose-blockquote:bg-black/2

                /* 行内代码 */
                prose-code:bg-black/4 prose-code:text-amber-800

                /* 代码块 */
                prose-pre:bg-[#282c34] prose-pre:text-[#abb2bf]

                /* 分隔线 */
                prose-hr:bg-black/6

                /* 表格 */
                [&_table]:bg-black/1
                [&_thead]:bg-black/2
                [&_tbody_tr:nth-child(even)]:bg-black/1.5

                /* 数学公式 */
                [&_.katex-display]:bg-black/2

                /* details */
                [&_details]:bg-black/2
                [&_summary:hover]:bg-black/4

                /* kbd */
                [&_kbd]:bg-black/5

                /* mark */
                [&_mark]:text-amber-900 [&_mark]:bg-amber-400/30

                /* RSS 内容样式 - 浅色主题 */

                /* RSS 引用块 */
                [&_.rss-content-blockquote]:bg-black/2 [&_.rss-content-blockquote]:border-black/10

                /* RSS 代码 */
                [&_.rss-content-pre]:bg-[#282c34] [&_.rss-content-pre]:text-[#abb2bf]
                [&_.rss-content-inline-code]:bg-black/4

                /* RSS 表格 */
                [&_.rss-content-table]:border-black/10
                [&_.rss-content-thead]:bg-black/3
                [&_.rss-content-th]:border-black/10
                [&_.rss-content-td]:border-black/10
                [&_.rss-content-tr:nth-child(even)]:bg-black/1.5

                /* RSS 描述列表 */
                [&_.rss-content-dd]:border-black/10

                /* RSS 折叠 */
                [&_.rss-content-details]:bg-black/2
                [&_.rss-content-summary]:hover:bg-black/4
                [&_.rss-content-details[open]_.rss-content-summary]:border-black/10

                /* RSS kbd */
                [&_.rss-content-kbd]:bg-black/5 [&_.rss-content-kbd]:border-black/10

                /* RSS mark */
                [&_.rss-content-mark]:bg-yellow-200/60

                /* RSS abbr */
                [&_.rss-content-abbr]:border-black/30

                /* RSS 分隔线 */
                [&_.rss-content-hr]:bg-black/10

                /* RSS 类别标签 */
                [&_.rss-content-category]:bg-black/5

                /* RSS 侧边栏 */
                [&_.rss-content-aside]:bg-black/2

                /* Brewlia 注释样式 - 浅色主题 */
                [&_.brewlia-annotation]:cursor-help [&_.brewlia-annotation]:rounded [&_.brewlia-annotation]:px-0.5
                [&_.brewlia-annotation]:transition-all [&_.brewlia-annotation]:duration-200
                [&_.brewlia-annotation]:border-b-2 [&_.brewlia-annotation]:border-dotted
                [&_.brewlia-annotation]:break-words [&_.brewlia-annotation]:[overflow-wrap:anywhere]
                [&_.brewlia-annotation[data-type="term"]]:text-orange-700 [&_.brewlia-annotation[data-type="term"]]:bg-orange-100 [&_.brewlia-annotation[data-type="term"]]:border-orange-400
                [&_.brewlia-annotation[data-type="reference"]]:text-blue-700 [&_.brewlia-annotation[data-type="reference"]]:bg-blue-100 [&_.brewlia-annotation[data-type="reference"]]:border-blue-400
                [&_.brewlia-annotation[data-type="implicit"]]:text-purple-700 [&_.brewlia-annotation[data-type="implicit"]]:bg-purple-100 [&_.brewlia-annotation[data-type="implicit"]]:border-purple-400
                [&_.brewlia-annotation[data-type="context"]]:text-green-700 [&_.brewlia-annotation[data-type="context"]]:bg-green-100 [&_.brewlia-annotation[data-type="context"]]:border-green-400
                [&_.brewlia-annotation[data-type="abbreviation"]]:text-pink-700 [&_.brewlia-annotation[data-type="abbreviation"]]:bg-pink-100 [&_.brewlia-annotation[data-type="abbreviation"]]:border-pink-400
                [&_.brewlia-annotation:hover]:ring-2 [&_.brewlia-annotation:hover]:ring-current/30
                [&_.brewlia-highlight-flash]:animate-pulse [&_.brewlia-highlight-flash]:ring-2 [&_.brewlia-highlight-flash]:ring-purple-500

                /* figcaption */
                prose-figcaption:text-current

                /* Notion 内容样式 - 浅色主题 */

                /* Notion 颜色 - 文字 */
                [&_.notion-gray]:text-gray-500
                [&_.notion-brown]:text-amber-700
                [&_.notion-orange]:text-orange-600
                [&_.notion-yellow]:text-yellow-600
                [&_.notion-green]:text-green-600
                [&_.notion-blue]:text-blue-600
                [&_.notion-purple]:text-purple-600
                [&_.notion-pink]:text-pink-600
                [&_.notion-red]:text-red-600

                /* Notion 颜色 - 背景 */
                [&_.notion-bg-gray]:bg-gray-100 [&_.notion-bg-gray]:px-1 [&_.notion-bg-gray]:rounded
                [&_.notion-bg-brown]:bg-amber-100 [&_.notion-bg-brown]:px-1 [&_.notion-bg-brown]:rounded
                [&_.notion-bg-orange]:bg-orange-100 [&_.notion-bg-orange]:px-1 [&_.notion-bg-orange]:rounded
                [&_.notion-bg-yellow]:bg-yellow-100 [&_.notion-bg-yellow]:px-1 [&_.notion-bg-yellow]:rounded
                [&_.notion-bg-green]:bg-green-100 [&_.notion-bg-green]:px-1 [&_.notion-bg-green]:rounded
                [&_.notion-bg-blue]:bg-blue-100 [&_.notion-bg-blue]:px-1 [&_.notion-bg-blue]:rounded
                [&_.notion-bg-purple]:bg-purple-100 [&_.notion-bg-purple]:px-1 [&_.notion-bg-purple]:rounded
                [&_.notion-bg-pink]:bg-pink-100 [&_.notion-bg-pink]:px-1 [&_.notion-bg-pink]:rounded
                [&_.notion-bg-red]:bg-red-100 [&_.notion-bg-red]:px-1 [&_.notion-bg-red]:rounded

                /* Notion Callout */
                [&_.notion-callout]:flex [&_.notion-callout]:items-start [&_.notion-callout]:gap-3
                [&_.notion-callout]:p-4 [&_.notion-callout]:my-4 [&_.notion-callout]:rounded-xl
                [&_.notion-callout]:bg-black/2 [&_.notion-callout]:border [&_.notion-callout]:border-black/5
                [&_.notion-callout-icon]:text-xl [&_.notion-callout-icon]:shrink-0
                [&_.notion-callout-icon]:w-6 [&_.notion-callout-icon]:h-6 [&_.notion-callout-icon]:object-contain
                [&_.notion-callout-content]:flex-1 [&_.notion-callout-content]:min-w-0

                /* Notion Quote */
                [&_.notion-quote]:pl-4 [&_.notion-quote]:py-1 [&_.notion-quote]:my-4
                [&_.notion-quote]:bg-black/4 [&_.notion-quote]:rounded-xl

                /* Notion Todo */
                [&_.notion-todo]:flex [&_.notion-todo]:items-start [&_.notion-todo]:gap-2 [&_.notion-todo]:my-1
                [&_.notion-checkbox]:w-5 [&_.notion-checkbox]:h-5 [&_.notion-checkbox]:shrink-0
                [&_.notion-checkbox]:border-2 [&_.notion-checkbox]:border-black/20 [&_.notion-checkbox]:rounded
                [&_.notion-checkbox.checked]:bg-blue-500 [&_.notion-checkbox.checked]:border-blue-500
                [&_.notion-checkbox.checked]:after:content-['✓'] [&_.notion-checkbox.checked]:after:text-white
                [&_.notion-checkbox.checked]:after:text-xs [&_.notion-checkbox.checked]:after:flex
                [&_.notion-checkbox.checked]:after:items-center [&_.notion-checkbox.checked]:after:justify-center
                [&_.notion-todo-text.checked]:line-through [&_.notion-todo-text.checked]:opacity-60

                /* Notion Toggle */
                [&_.notion-toggle]:bg-black/2 [&_.notion-toggle]:border [&_.notion-toggle]:border-black/5
                [&_.notion-toggle]:rounded-xl [&_.notion-toggle]:my-3
                [&_.notion-toggle_summary]:px-4 [&_.notion-toggle_summary]:py-3

                /* Notion Image */
                [&_.notion-image]:my-6
                [&_.notion-image_img]:rounded-xl [&_.notion-image_img]:w-full
                [&_.notion-image_figcaption]:text-center [&_.notion-image_figcaption]:text-sm
                [&_.notion-image_figcaption]:mt-2 [&_.notion-image_figcaption]:opacity-60

                /* Notion Video */
                [&_.notion-video]:my-6
                [&_.notion-video-embed]:aspect-video [&_.notion-video-embed]:w-full
                [&_.notion-video-embed]:rounded-xl [&_.notion-video-embed]:overflow-hidden
                [&_.notion-video-embed_iframe]:w-full [&_.notion-video-embed_iframe]:h-full
                [&_.notion-video_video]:w-full [&_.notion-video_video]:rounded-xl

                /* Notion Audio */
                [&_.notion-audio]:my-4
                [&_.notion-audio_audio]:w-full

                /* Notion Bookmark */
                [&_.notion-bookmark]:flex [&_.notion-bookmark]:items-center [&_.notion-bookmark]:gap-3
                [&_.notion-bookmark]:p-4 [&_.notion-bookmark]:my-4 [&_.notion-bookmark]:rounded-xl
                [&_.notion-bookmark]:bg-black/2 [&_.notion-bookmark]:border [&_.notion-bookmark]:border-black/5
                [&_.notion-bookmark]:no-underline [&_.notion-bookmark]:hover:bg-black/4
                [&_.notion-bookmark-icon]:text-lg
                [&_.notion-bookmark-title]:font-medium [&_.notion-bookmark-title]:flex-1
                [&_.notion-bookmark-url]:text-sm [&_.notion-bookmark-url]:opacity-50 [&_.notion-bookmark-url]:truncate [&_.notion-bookmark-url]:max-w-48

                /* Notion Link Preview */
                [&_.notion-link-preview]:inline-flex [&_.notion-link-preview]:items-center [&_.notion-link-preview]:gap-1.5
                [&_.notion-link-preview]:px-2 [&_.notion-link-preview]:py-0.5 [&_.notion-link-preview]:rounded-md
                [&_.notion-link-preview]:bg-black/3 [&_.notion-link-preview]:no-underline
                [&_.notion-link-preview]:hover:bg-black/6

                /* Notion File */
                [&_.notion-file]:inline-flex [&_.notion-file]:items-center [&_.notion-file]:gap-2
                [&_.notion-file]:px-3 [&_.notion-file]:py-2 [&_.notion-file]:my-2 [&_.notion-file]:rounded-lg
                [&_.notion-file]:bg-black/2 [&_.notion-file]:border [&_.notion-file]:border-black/5
                [&_.notion-file]:no-underline [&_.notion-file]:hover:bg-black/4

                /* Notion Embed */
                [&_.notion-embed]:my-6
                [&_.notion-embed-wrapper]:aspect-video [&_.notion-embed-wrapper]:w-full
                [&_.notion-embed-wrapper]:rounded-xl [&_.notion-embed-wrapper]:overflow-hidden
                [&_.notion-embed-wrapper_iframe]:w-full [&_.notion-embed-wrapper_iframe]:h-full

                /* Notion PDF */
                [&_.notion-pdf]:my-6
                [&_.notion-pdf-embed]:w-full [&_.notion-pdf-embed]:h-150 [&_.notion-pdf-embed]:rounded-xl
                [&_.notion-pdf-embed]:border [&_.notion-pdf-embed]:border-black/10

                /* Notion Equation */
                [&_.notion-equation]:my-4 [&_.notion-equation]:py-4 [&_.notion-equation]:px-6
                [&_.notion-equation]:bg-black/2 [&_.notion-equation]:rounded-xl
                [&_.notion-equation]:overflow-x-auto [&_.notion-equation]:text-center

                /* Notion Table */
                [&_.notion-table]:w-full [&_.notion-table]:my-4 [&_.notion-table]:border-collapse
                [&_.notion-table]:rounded-xl [&_.notion-table]:overflow-hidden
                [&_.notion-table_td]:px-3 [&_.notion-table_td]:py-2
                [&_.notion-table_td]:border [&_.notion-table_td]:border-black/10
                [&_.notion-table.has-header_tr:first-child]:bg-black/3
                [&_.notion-table.has-header_tr:first-child_td]:font-medium

                /* Notion Columns */
                [&_.notion-columns]:flex [&_.notion-columns]:gap-4 [&_.notion-columns]:my-4
                [&_.notion-column]:flex-1 [&_.notion-column]:min-w-0

                /* Notion Page Link */
                [&_.notion-page-link]:inline-flex [&_.notion-page-link]:items-center [&_.notion-page-link]:gap-1.5
                [&_.notion-page-link]:px-2 [&_.notion-page-link]:py-1 [&_.notion-page-link]:rounded-md
                [&_.notion-page-link]:bg-black/2 [&_.notion-page-link]:no-underline
                [&_.notion-page-link]:hover:bg-black/4

                /* Notion Code */
                [&_.notion-code]:my-4
                [&_.notion-code_pre]:rounded-xl [&_.notion-code_pre]:overflow-x-auto
                [&_.notion-code_figcaption]:text-center [&_.notion-code_figcaption]:text-sm
                [&_.notion-code_figcaption]:mt-2 [&_.notion-code_figcaption]:opacity-50
              `
              }
            `}
              style={{
                fontSize: `${fontSize}px`,
                lineHeight,
                fontFamily: currentFont.family,
              }}
              onClick={(e) => e.stopPropagation()}
            >
              {/* WebKit 优化：动画期间显示简单占位，避免同时渲染大量 DOM */}
              {contentReady ? (
                <div ref={contentInnerRef} />
              ) : (
                <div className="space-y-4 animate-pulse">
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '90%' }}
                  />
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '100%' }}
                  />
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '85%' }}
                  />
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '95%' }}
                  />
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '70%' }}
                  />
                </div>
              )}
            </div>

            {/* 音频播放器 */}
            {item.audio_url && (
              <div
                className={`mt-8 p-4 rounded-2xl ${currentTheme.surfaceSolid} border ${currentTheme.border}`}
                onClick={(e) => e.stopPropagation()}
              >
                <p className={`text-sm ${currentTheme.secondary} mb-3`}>
                  {t.brew.audioLabel}
                </p>
                <audio src={item.audio_url} controls className="w-full" />
              </div>
            )}

            {/* 文章导航 - 上一篇/下一篇 */}
            {(() => {
              // 阅读列表导航
              if (positionInfo && onNavigateToArticle) {
                const prevItem = readingList?.getPrevious()
                const nextItem = readingList?.getNext()
                return (
                  <div
                    className={`mt-12 pt-8 border-t ${currentTheme.border} max-w-2xl mx-auto`}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <div
                      className={`text-center mb-6 ${currentTheme.secondary}`}
                    >
                      <span className="text-sm">
                        {readingList?.currentList?.name} ·
                        {positionInfo.index + 1} /{positionInfo.total}
                      </span>
                    </div>
                    <div className="flex gap-4">
                      <button
                        onClick={() => {
                          if (prevItem) {
                            readingList?.goToArticle(positionInfo.index - 1)
                            onNavigateToArticle(prevItem.id)
                          }
                        }}
                        disabled={!positionInfo.hasPrev}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-left transition-all ${positionInfo.hasPrev ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
                      >
                        <div
                          className={`text-xs ${currentTheme.secondary} mb-1 flex items-center gap-1`}
                        >
                          <svg
                            className="w-3 h-3 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M15 19l-7-7 7-7"
                            />
                          </svg>
                          上一篇
                        </div>
                        <div
                          className={`${currentTheme.text} font-medium truncate`}
                        >
                          {prevItem?.title || '没有了'}
                        </div>
                      </button>
                      <button
                        onClick={() => {
                          if (nextItem) {
                            readingList?.goToArticle(positionInfo.index + 1)
                            onNavigateToArticle(nextItem.id)
                          }
                        }}
                        disabled={!positionInfo.hasNext}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-right transition-all ${positionInfo.hasNext ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
                      >
                        <div
                          className={`text-xs ${currentTheme.secondary} mb-1 flex items-center justify-end gap-1`}
                        >
                          下一篇
                          <svg
                            className="w-3 h-3 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M9 5l7 7-7 7"
                            />
                          </svg>
                        </div>
                        <div
                          className={`${currentTheme.text} font-medium truncate`}
                        >
                          {nextItem?.title || '没有了'}
                        </div>
                      </button>
                    </div>
                  </div>
                )
              }
              // 全局文章列表导航
              if (
                articleList &&
                currentArticleIndex != null &&
                onNavigateToArticle
              ) {
                const hasPrev = currentArticleIndex > 0
                const hasNext = currentArticleIndex < articleList.length - 1
                const prevArticle = hasPrev
                  ? articleList[currentArticleIndex - 1]
                  : null
                const nextArticle = hasNext
                  ? articleList[currentArticleIndex + 1]
                  : null
                return (
                  <div
                    className={`mt-12 pt-8 border-t ${currentTheme.border} max-w-2xl mx-auto`}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <div className="flex gap-4">
                      <button
                        onClick={() => {
                          if (prevArticle) onNavigateToArticle(prevArticle.id)
                        }}
                        disabled={!hasPrev}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-left transition-all ${hasPrev ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
                      >
                        <div
                          className={`text-xs ${currentTheme.secondary} mb-1 flex items-center gap-1`}
                        >
                          <svg
                            className="w-3 h-3 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M15 19l-7-7 7-7"
                            />
                          </svg>
                          上一篇
                        </div>
                        <div
                          className={`${currentTheme.text} font-medium truncate`}
                        >
                          {prevArticle?.title || '没有了'}
                        </div>
                      </button>
                      <button
                        onClick={() => {
                          if (nextArticle) onNavigateToArticle(nextArticle.id)
                        }}
                        disabled={!hasNext}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-right transition-all ${hasNext ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
                      >
                        <div
                          className={`text-xs ${currentTheme.secondary} mb-1 flex items-center justify-end gap-1`}
                        >
                          下一篇
                          <svg
                            className="w-3 h-3 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M9 5l7 7-7 7"
                            />
                          </svg>
                        </div>
                        <div
                          className={`${currentTheme.text} font-medium truncate`}
                        >
                          {nextArticle?.title || '没有了'}
                        </div>
                      </button>
                    </div>
                  </div>
                )
              }
              return null
            })()}

            {/* 底部留白 */}
            <div className="h-20" />
          </div>

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
