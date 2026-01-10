/**
 * Brew 文章阅读器组件
 * 全屏沉浸式阅读体验，两侧悬浮控制栏
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
import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useNavigation } from '../../contexts/NavigationContext'
import { brewAnimationPresets, getBrewTransition, useBrewAnimationConfig } from '../../hooks/animation/pages/brew'
import * as brewliaApi from '../../services/brewliaApi'
import { loadEmbedData, playNeteaseSong, processEmbeds } from '../../utils/embedProcessor'
import { processRssContent } from '../../utils/rssContentProcessor'
import {
  AnnotationTooltip,
  CommentInputPopup,
  CommentsListPanel,
  CommentTooltip,
  Lightbox,
  MobileReaderBar,
  FONT_OPTIONS as READER_FONT_OPTIONS,
  LAYOUT_OPTIONS as READER_LAYOUT_OPTIONS,
  THEMES as READER_THEMES,
  ReaderLeftPanel,
  ReaderRightPanel,
  STYLE_READER_CONTAINER,
  STYLE_SCROLL_SMOOTH,
  TRANSITION_FAST,
  useAnnotations,
  useComments,
  usePodcast,
  useReaderSettings,
} from './reader'

// API URL
const API_URL = import.meta.env.PUBLIC_API_URL || ''

// 处理图片 URL - 封面图等外部图片通过代理访问
function getImageUrl(imageUrl: string | null): string | null {
  if (!imageUrl)
    return null
  // 已经是本地路径或代理路径，直接使用
  if (imageUrl.startsWith('/api/') || imageUrl.startsWith(`${API_URL}/api/`)) {
    return imageUrl.startsWith('/api/') ? `${API_URL}${imageUrl}` : imageUrl
  }
  // 外部 URL，使用图片代理
  if (imageUrl.startsWith('http://') || imageUrl.startsWith('https://')) {
    return `${API_URL}/api/proxy/image?url=${encodeURIComponent(imageUrl)}`
  }
  return imageUrl
}

interface BrewReaderProps {
  item: BrewItem
  onClose: () => void
  onToggleStar: () => void
  isAuthenticated?: boolean // 是否已登录（游客隐藏收藏按钮）
  isAdmin?: boolean // 是否为管理员（游客/普通用户隐藏重新生成按钮）
  sourceType?: SourceType // 来源类型（brewlia 时显示 AI 功能）
}

// 目录项类型
interface TocItem {
  id: string
  text: string
  level: number
}

// 使用导入的常量
const FONT_OPTIONS = READER_FONT_OPTIONS
const THEMES = READER_THEMES
const LAYOUT_OPTIONS = READER_LAYOUT_OPTIONS

export default function BrewReader({ item, onClose, onToggleStar, isAuthenticated = false, isAdmin = false, sourceType }: BrewReaderProps) {
  const { t } = useI18n()
  const contentRef = useRef<HTMLDivElement>(null)
  const articleRef = useRef<HTMLElement>(null)

  // 沉浸模式 - 进入阅读器时隐藏导航栏和控制面板
  const { setImmersiveMode } = useNavigation()

  // 动画配置 - 根据设备性能自适应
  const animConfig = useBrewAnimationConfig()
  const readerTransition = useMemo(() => getBrewTransition(animConfig, 'reader'), [animConfig])
  const enableAnimations = animConfig.level !== 'none'

  // WebKit 优化：延迟渲染内容，让入场动画先完成
  const [contentReady, setContentReady] = useState(!enableAnimations)

  // Brewlia AI 功能标识
  const isBrewlia = sourceType === 'brewlia'

  // 阅读设置 - 使用自定义 hook
  const {
    fontSize,
    lineHeight,
    fontFamily,
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
  const [toc, setToc] = useState<TocItem[]>([])
  const [showToc, setShowToc] = useState(false)
  const [activeHeadingId, setActiveHeadingId] = useState<string>('')
  const [headingHistory, setHeadingHistory] = useState<string[]>([]) // 标题访问历史
  const progressLongPressRef = useRef<ReturnType<typeof setTimeout> | null>(null)
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
    setShowAnnotations,
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
    selectionRange,
    commentInput,
    commentSubmitting,
    showCommentsPanel,
    replyingTo,
    replyInput,
    replySubmitting,
    expandedComments,
    commentReplies,
    commentTooltip,
    setComments,
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
    commentsLoadingRef,
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
    podcastError,
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
    groupedVoices,
    setShowPodcastPlayer,
    setShowVoiceSettings,
    loadPodcast,
    regeneratePodcast,
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
    podcastListRef,
  } = usePodcast({
    itemId: item.id,
    sourceId: item.source_id,
    isBrewlia,
    showToastMessage,
    t,
  })

  // 侧边栏按钮样式 - useMemo 缓存
  const sideButtonClass = useMemo(() =>
    `p-2.5 rounded-xl transition-all duration-200 ${currentTheme.secondary} hover:${currentTheme.text} ${
      isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'
    }`, [currentTheme.secondary, currentTheme.text, isDark])

  // 处理文章内容（带注释、评论高亮和嵌入内容）
  // WebKit 优化：使用状态 + useEffect 异步处理，避免阻塞首次渲染
  const [processedContent, setProcessedContent] = useState<string>('')

  useEffect(() => {
    // 如果内容还没准备好，不处理
    if (!contentReady)
      return

    let content = item.content || item.summary || `<p class="opacity-50">${t.brew.noContent}</p>`

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

    // 2. 处理 AI 注释高亮
    if (showAnnotations && annotations.length > 0) {
      content = brewliaApi.highlightAnnotations(content, annotations)
    }

    // 3. 处理用户评论高亮
    if (comments.length > 0) {
      content = highlightComments(content, comments, theme)
    }

    setProcessedContent(content)
  }, [contentReady, item.content, item.summary, item.link, showAnnotations, annotations, comments, highlightComments, isDark, theme, t.brew.noContent])

  // WebKit 优化：延迟渲染内容，让入场动画先完成
  // 这避免了同时执行动画 + 大量 DOM 渲染导致的卡顿
  useEffect(() => {
    if (!enableAnimations) {
      setContentReady(true)
      return
    }

    // 使用 requestAnimationFrame 确保在下一帧开始前设置
    // 延迟时间略长于动画时长，确保动画完成
    const delay = (readerTransition.duration * 1000) + 50
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
    if (!item.published_at)
      return ''
    return new Date(item.published_at).toLocaleDateString('zh-CN', {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    })
  }, [item.published_at])

  // 计算阅读进度
  const updateReadingProgress = useCallback(() => {
    if (articleRef.current) {
      const { scrollTop, scrollHeight, clientHeight } = articleRef.current
      const progress = Math.min(100, Math.round((scrollTop / (scrollHeight - clientHeight)) * 100))
      setReadingProgress(isNaN(progress) ? 0 : progress)
    }
  }, [])

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
        if (img.dataset.sizeProcessed)
          return
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
          else {
            img.style.maxWidth = '100%'
          }
        }

        if (img.complete && img.naturalWidth > 0) {
          handleImageLoad()
        }
        else {
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
        if (pre.parentElement?.classList.contains('code-block-wrapper'))
          return

        // 创建包装容器
        const wrapper = document.createElement('div')
        wrapper.className = 'code-block-wrapper relative group my-5'

        // 将 pre 移入 wrapper
        pre.parentNode?.insertBefore(wrapper, pre)
        wrapper.appendChild(pre)

        // 添加复制按钮 - 代码块背景始终是深色的，所以按钮用浅色样式
        const copyBtn = document.createElement('button')
        copyBtn.className = 'absolute top-3 right-3 p-1.5 rounded-lg opacity-0 group-hover:opacity-100 transition-all duration-200 bg-white/10 hover:bg-white/20 text-white/60 hover:text-white/90 backdrop-blur-sm'
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
          }
          catch (err) {
            console.error('复制失败:', err)
          }
        })

        wrapper.appendChild(copyBtn)
      })

      // 解析标题生成目录
      const headings = contentRef.current.querySelectorAll('h1, h2, h3, h4, h5, h6')
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
  }, [processedContent]) // 依赖 processedContent 确保嵌入卡片已渲染

  // 处理评论高亮和嵌入卡片的点击和悬停事件
  useEffect(() => {
    if (!contentRef.current)
      return

    const handleContentClick = async (e: Event) => {
      const target = e.target as HTMLElement

      // 检查是否点击了图片（需要排除嵌入卡片内的图片）
      if (target.tagName === 'IMG') {
        const img = target as HTMLImageElement
        // 检查图片是否在嵌入卡片内（brew-embed-card, brew-embed-exempt, brew-bilibili-embed 等）
        const isInEmbedCard = img.closest('.brew-embed-card, .brew-embed-exempt, .brew-bilibili-embed, .brew-netease-music, .brew-steam-game, .brew-bilibili-video')

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
          }
          catch (error) {
            console.error('[BrewReader] 播放网易云音乐失败:', error)
            showToastMessage(t.brew.playFailed)
          }
          finally {
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
            const commentEl = document.querySelector(`[data-panel-comment-id="${commentId}"]`)
            if (commentEl) {
              commentEl.scrollIntoView({ behavior: 'smooth', block: 'center' })
            }
          }, 100)
        }
      }
    }

    // 处理悬停显示 tooltip
    const handleMouseOver = (e: Event) => {
      const target = e.target as HTMLElement
      const highlight = target.closest('.user-comment-highlight') as HTMLElement

      if (highlight) {
        const commentId = highlight.getAttribute('data-comment-id')
        if (commentId) {
          const comment = comments.find(c => c.id === Number.parseInt(commentId))
          if (comment) {
            const rect = highlight.getBoundingClientRect()
            setCommentTooltip({
              comment,
              x: rect.left + rect.width / 2,
              y: rect.top - 8,
            })
          }
        }
      }
    }

    const handleMouseOut = (e: Event) => {
      const relatedTarget = (e as MouseEvent).relatedTarget as HTMLElement

      // 如果移出的目标不是评论相关元素，隐藏 tooltip
      if (!relatedTarget?.closest('.user-comment-highlight')
        && !relatedTarget?.closest('.comment-tooltip')) {
        setCommentTooltip(null)
      }
    }

    contentRef.current.addEventListener('click', handleContentClick)
    contentRef.current.addEventListener('mouseover', handleMouseOver)
    contentRef.current.addEventListener('mouseout', handleMouseOut)

    return () => {
      contentRef.current?.removeEventListener('click', handleContentClick)
      contentRef.current?.removeEventListener('mouseover', handleMouseOver)
      contentRef.current?.removeEventListener('mouseout', handleMouseOut)
    }
  }, [comments])

  // 监听滚动更新当前标题
  useEffect(() => {
    const article = articleRef.current
    const content = contentRef.current
    if (!article || !content || toc.length === 0)
      return

    // 缓存标题元素引用，避免每次滚动都调用 querySelectorAll
    const headings = Array.from(content.querySelectorAll('h1, h2, h3, h4, h5, h6')) as HTMLElement[]
    if (headings.length === 0)
      return

    const handleScrollForToc = () => {
      let currentId = ''

      for (const heading of headings) {
        const rect = heading.getBoundingClientRect()
        // 标题进入视口上方 150px 范围内就算当前标题
        if (rect.top <= 150) {
          currentId = heading.id
        }
      }

      if (currentId !== activeHeadingId) {
        // 记录标题访问历史（去重，只记录最近 20 个）
        if (currentId && activeHeadingId) {
          setHeadingHistory((prev) => {
            const newHistory = prev.filter(id => id !== activeHeadingId)
            newHistory.push(activeHeadingId)
            return newHistory.slice(-20)
          })
        }
        setActiveHeadingId(currentId)
      }
    }

    article.addEventListener('scroll', handleScrollForToc, { passive: true })
    return () => article.removeEventListener('scroll', handleScrollForToc)
  }, [toc, activeHeadingId])

  // 监听滚动
  useEffect(() => {
    const article = articleRef.current
    if (article) {
      article.addEventListener('scroll', updateReadingProgress)
      return () => article.removeEventListener('scroll', updateReadingProgress)
    }
  }, [updateReadingProgress])

  // 复制链接
  const handleShare = async () => {
    try {
      await navigator.clipboard.writeText(item.link)
      showToastMessage(t.brew.linkCopied)
    }
    catch (err) {
      console.error('Failed to copy:', err)
    }
  }

  // 跳转到指定标题 - useCallback 缓存
  const scrollToHeading = useCallback((id: string) => {
    const heading = document.getElementById(id)
    if (heading && articleRef.current) {
      const articleRect = articleRef.current.getBoundingClientRect()
      const headingRect = heading.getBoundingClientRect()
      const scrollTop = articleRef.current.scrollTop + headingRect.top - articleRect.top - 80

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
      setHeadingHistory(prev => prev.slice(0, -1))
      scrollToHeading(prevId)
      showToastMessage(t.brew.backToPrevParagraph, 1500)
    }
    else if (activeHeadingId && toc.length > 0) {
      // 没有历史时，跳转到当前标题的上一个
      const currentIndex = toc.findIndex(t => t.id === activeHeadingId)
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
      if (e.key === 'Escape')
        onClose()
      if (e.key === '+' || e.key === '=')
        adjustFontSize(1)
      if (e.key === '-')
        adjustFontSize(-1)
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
    if (!isAuthenticated)
      return

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
        contextAfter: fullText.slice(textIndex + text.length, textIndex + text.length + 50),
      })
    }

    setShowCommentPopup(true)
  }, [isAuthenticated])

  // 监听选择事件
  useEffect(() => {
    if (!isAuthenticated)
      return

    let selectionTimer: ReturnType<typeof setTimeout> | null = null

    const handleMouseUp = () => {
      // 延迟执行，等待选择完成
      selectionTimer = setTimeout(handleTextSelection, 10)
    }

    document.addEventListener('mouseup', handleMouseUp)
    return () => {
      document.removeEventListener('mouseup', handleMouseUp)
      if (selectionTimer)
        clearTimeout(selectionTimer)
    }
  }, [isAuthenticated, handleTextSelection])

  // 监听选中状态变化，当选中被移除时关闭弹窗
  useEffect(() => {
    if (!showCommentPopup)
      return

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
    return () => document.removeEventListener('selectionchange', handleSelectionChange)
  }, [showCommentPopup])

  // 点击其他地方关闭评论弹窗
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      // 如果点击的是评论弹窗内部或评论高亮，不关闭
      if (target.closest('.comment-popup')
        || target.closest('.user-comment-highlight')) {
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
  const handleScrollToAnnotation = useCallback((annotation: AnnotationItem) => {
    scrollToAnnotation(annotation, contentRef, articleRef)
  }, [scrollToAnnotation])

  // 监听注释 hover 事件（优化稳定性）
  useEffect(() => {
    if (!contentRef.current || !showAnnotations)
      return

    const handleMouseOver = (e: MouseEvent) => {
      const target = (e.target as HTMLElement).closest('.brewlia-annotation') as HTMLElement
      if (!target)
        return

      // 清除之前的延迟隐藏
      if (hoverTimeoutRef.current) {
        clearTimeout(hoverTimeoutRef.current)
        hoverTimeoutRef.current = null
      }

      const term = decodeURIComponent(target.dataset.term || '')
      const explanation = decodeURIComponent(target.dataset.explanation || '')
      const type = target.dataset.type as AnnotationType || 'term'

      const rect = target.getBoundingClientRect()
      setTooltipPosition({
        x: rect.left + rect.width / 2,
        y: rect.top - 8,
      })
      setHoveredAnnotation({ term, explanation, type })
    }

    const handleMouseOut = (e: MouseEvent) => {
      const target = (e.target as HTMLElement).closest('.brewlia-annotation')
      const relatedTarget = (e.relatedTarget as HTMLElement)?.closest?.('.brewlia-annotation')

      // 如果移动到另一个注释，不隐藏
      if (target && !relatedTarget) {
        // 延迟隐藏，避免闪烁
        hoverTimeoutRef.current = setTimeout(() => {
          setHoveredAnnotation(null)
        }, 150)
      }
    }

    const container = contentRef.current
    container.addEventListener('mouseover', handleMouseOver)
    container.addEventListener('mouseout', handleMouseOut)

    return () => {
      container.removeEventListener('mouseover', handleMouseOver)
      container.removeEventListener('mouseout', handleMouseOut)
      if (hoverTimeoutRef.current) {
        clearTimeout(hoverTimeoutRef.current)
      }
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
  const resetHideTimer = useCallback((delay = 4000) => {
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
  }, [closeAllTooltips])

  // 显示控制栏
  const showPanelsIfAllowed = useCallback(() => {
    if (cooldownRef.current || isScrollingRef.current)
      return
    setShowPanels(true)
    resetHideTimer()
  }, [resetHideTimer])

  // 滚动时：向上滚动显示控制栏2秒，向下滚动隐藏
  useEffect(() => {
    const article = articleRef.current
    if (!article)
      return

    let scrollEndTimer: ReturnType<typeof setTimeout> | null = null

    const handleScroll = () => {
      const currentScrollTop = article.scrollTop
      const isScrollingUp = currentScrollTop < lastScrollTopRef.current
      lastScrollTopRef.current = currentScrollTop

      // 清除之前的定时器
      if (scrollEndTimer)
        clearTimeout(scrollEndTimer)

      // 向上滚动（往之前内容滑动）时显示控制栏
      if (isScrollingUp && currentScrollTop > 10) {
        isScrollingRef.current = false
        setShowPanels(true)
        resetHideTimer(2000) // 显示2秒后自动隐藏
      }
      else if (!isScrollingUp) {
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
      if (scrollEndTimer)
        clearTimeout(scrollEndTimer)
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
      const isInLeftControlZone = e.clientX >= contentLeft - controlZoneWidth && e.clientX <= contentLeft
      const isInRightControlZone = e.clientX >= contentRight && e.clientX <= contentRight + controlZoneWidth
      const isInControlZone = isInLeftControlZone || isInRightControlZone

      // 更新悬停状态
      isHoveringControlsRef.current = isInControlZone

      // 控制栏区域立即响应，其他区域节流
      if (isInControlZone) {
        // 控制栏区域：直接显示，不节流，不自动隐藏
        if (cooldownRef.current)
          return
        isScrollingRef.current = false // 允许覆盖滚动隐藏
        setShowPanels(true)
        // 清除隐藏定时器，鼠标在控制栏区域时不隐藏
        if (hideTimerRef.current) {
          clearTimeout(hideTimerRef.current)
          hideTimerRef.current = null
        }
      }
      else {
        // 非控制栏区域：节流 500ms
        if (now - lastMouseMoveRef.current < 500)
          return
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
      if (hideTimerRef.current)
        clearTimeout(hideTimerRef.current)
    }
  }, [showPanelsIfAllowed, resetHideTimer, layout, showPanels])

  // 初始化隐藏计时器
  useEffect(() => {
    resetHideTimer()
    return () => {
      if (hideTimerRef.current)
        clearTimeout(hideTimerRef.current)
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
    const bgColor = theme === 'light' ? '#f8f5ec' : theme === 'sepia' ? '#f4ecd8' : theme === 'dark' ? '#1a1a1a' : '#0d1117'
    return {
      top: { background: `linear-gradient(to bottom, ${bgColor} 0%, ${bgColor}00 100%)` },
      bottom: { background: `linear-gradient(to top, ${bgColor} 0%, ${bgColor}00 100%)` },
    }
  }, [theme])

  return (
    <motion.div
      {...readerAnimProps}
      style={STYLE_READER_CONTAINER}
      className={`fixed inset-0 z-50 ${currentTheme.bg}`}
      data-brew-reader="true"
    >
      {/* 顶部进度条 */}
      <div className="absolute top-0 left-0 right-0 h-0.5 z-10">
        <motion.div
          className="h-full bg-gradient-to-r from-amber-500 to-orange-500"
          style={{ width: `${readingProgress}%` }}
          initial={enableAnimations ? { width: 0 } : false}
          animate={enableAnimations ? { width: `${readingProgress}%` } : undefined}
          transition={enableAnimations ? TRANSITION_FAST : undefined}
        />
      </div>

      {/* 顶部淡出遮罩 */}
      <div
        className="absolute top-0 left-0 right-0 h-24 pointer-events-none z-[5]"
        style={maskGradientStyles.top}
      />

      {/* 底部淡入遮罩 */}
      <div
        className="absolute bottom-0 left-0 right-0 h-24 pointer-events-none z-[5]"
        style={maskGradientStyles.bottom}
      />

      {/* 主内容区 - 三栏布局 */}
      <article
        ref={articleRef}
        className="h-full overflow-y-auto overflow-x-hidden"
        style={STYLE_SCROLL_SMOOTH}
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
            handlePlayPause={podcastState === 'playing' ? handlePodcastPause : handlePodcastPlay}
            handleStop={handlePodcastStop}
            handlePrevious={handlePodcastPrev}
            handleNext={handlePodcastNext}
            handleDialogueClick={handlePodcastSeek}
            handleProgressPointerDown={handleProgressPointerDown}
            handleProgressPointerUp={handleProgressPointerUp}
            handleProgressPointerLeave={handleProgressPointerLeave}
            enableAnimations={enableAnimations}
            sideButtonClass={sideButtonClass}
            onMouseEnter={() => { isHoveringControlsRef.current = true }}
            onMouseLeave={() => { isHoveringControlsRef.current = false; resetHideTimer(2000) }}
            t={t}
          />

          {/* 正文内容 */}
          <div className={`w-full ${currentLayout.width} px-6 py-16 transition-all duration-300`}>
            {/* 标题 */}
            <h1
              className={`text-3xl font-bold ${currentTheme.text} leading-tight mb-6`}
              style={{ fontFamily: currentFont.family }}
            >
              {item.title}
            </h1>

            {/* 元信息 */}
            <div className={`flex flex-wrap items-center gap-4 text-sm ${currentTheme.secondary} mb-8 pb-8 border-b ${currentTheme.border}`}>
              <span className="flex items-center gap-1.5">
                {item.source_icon && (
                  <img src={item.source_icon} alt="" className="w-4 h-4 rounded" />
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
                  {t.brew.readingTime.replace('{time}', String(item.reading_time))}
                </span>
              )}
              {item.word_count && (
                <span>
                  {item.word_count.toLocaleString()}
                  {' '}
                  {t.brew.wordCount}
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
              prose prose-lg max-w-none ${currentTheme.text}
              
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
              prose-a:break-words [&_a]:overflow-wrap-anywhere
              
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
              
              /* ====== RSS 内容适配样式 ====== */
              
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
              
              /* RSS iframe 包装 - 响应式容器 */
              [&_.rss-content-iframe-wrapper]:relative [&_.rss-content-iframe-wrapper]:w-full
              [&_.rss-content-iframe-wrapper]:my-5 [&_.rss-content-iframe-wrapper]:rounded-xl
              [&_.rss-content-iframe-wrapper]:overflow-hidden
              [&_.rss-content-iframe-wrapper.aspect-video]:pb-[56.25%]
              [&_.rss-content-iframe-wrapper.aspect-\\[3\\/1\\]]:pb-[33.33%]
              [&_.rss-content-iframe-wrapper_iframe]:absolute [&_.rss-content-iframe-wrapper_iframe]:inset-0
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
              [&_.rss-content-link]:break-words [&_.rss-content-link]:underline
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
              
              ${isDark
      ? `
                /* === 暗色主题 === */
                prose-invert
                
                /* 链接 */
                prose-a:text-blue-400 prose-a:decoration-blue-400/40
                hover:prose-a:text-blue-300 hover:prose-a:decoration-blue-300/60
                
                /* 引用块 */
                prose-blockquote:bg-white/[0.03]
                
                /* 行内代码 */
                prose-code:bg-white/[0.08] prose-code:text-amber-200/90
                
                /* 代码块 */
                prose-pre:bg-white/[0.04]
                
                /* 分隔线 */
                prose-hr:bg-white/[0.06]
                
                /* 表格 */
                [&_table]:bg-white/[0.02]
                [&_thead]:bg-white/[0.03]
                [&_tbody_tr:nth-child(even)]:bg-white/[0.02]
                
                /* 数学公式 */
                [&_.katex-display]:bg-white/[0.03]
                
                /* details */
                [&_details]:bg-white/[0.03]
                [&_summary:hover]:bg-white/[0.05]
                
                /* kbd */
                [&_kbd]:bg-white/[0.08]
                
                /* mark */
                [&_mark]:text-amber-200 [&_mark]:bg-amber-500/20
                
                /* ====== RSS 内容样式 - 暗色主题 ====== */
                
                /* RSS 引用块 */
                [&_.rss-content-blockquote]:bg-white/[0.03] [&_.rss-content-blockquote]:border-white/10
                
                /* RSS 代码 */
                [&_.rss-content-pre]:bg-white/[0.04]
                [&_.rss-content-inline-code]:bg-white/[0.08]
                
                /* RSS 表格 */
                [&_.rss-content-table]:border-white/10
                [&_.rss-content-thead]:bg-white/[0.05]
                [&_.rss-content-th]:border-white/10
                [&_.rss-content-td]:border-white/10
                [&_.rss-content-tr:nth-child(even)]:bg-white/[0.02]
                
                /* RSS 描述列表 */
                [&_.rss-content-dd]:border-white/10
                
                /* RSS 折叠 */
                [&_.rss-content-details]:bg-white/[0.03]
                [&_.rss-content-summary]:hover:bg-white/[0.05]
                [&_.rss-content-details[open]_.rss-content-summary]:border-white/10
                
                /* RSS kbd */
                [&_.rss-content-kbd]:bg-white/[0.08] [&_.rss-content-kbd]:border-white/20
                
                /* RSS mark */
                [&_.rss-content-mark]:bg-yellow-500/30
                
                /* RSS abbr */
                [&_.rss-content-abbr]:border-white/30
                
                /* RSS 分隔线 */
                [&_.rss-content-hr]:bg-white/10
                
                /* RSS 类别标签 */
                [&_.rss-content-category]:bg-white/10
                
                /* RSS 侧边栏 */
                [&_.rss-content-aside]:bg-white/[0.03]
                
                /* Brewlia 注释样式 - 暗色主题 */
                [&_.brewlia-annotation]:cursor-help [&_.brewlia-annotation]:rounded [&_.brewlia-annotation]:px-0.5
                [&_.brewlia-annotation]:transition-all [&_.brewlia-annotation]:duration-200
                [&_.brewlia-annotation]:border-b-2 [&_.brewlia-annotation]:border-dotted
                [&_.brewlia-annotation[data-type="term"]]:text-orange-300 [&_.brewlia-annotation[data-type="term"]]:bg-orange-500/15 [&_.brewlia-annotation[data-type="term"]]:border-orange-400/50
                [&_.brewlia-annotation[data-type="reference"]]:text-blue-300 [&_.brewlia-annotation[data-type="reference"]]:bg-blue-500/15 [&_.brewlia-annotation[data-type="reference"]]:border-blue-400/50
                [&_.brewlia-annotation[data-type="implicit"]]:text-purple-300 [&_.brewlia-annotation[data-type="implicit"]]:bg-purple-500/15 [&_.brewlia-annotation[data-type="implicit"]]:border-purple-400/50
                [&_.brewlia-annotation[data-type="context"]]:text-green-300 [&_.brewlia-annotation[data-type="context"]]:bg-green-500/15 [&_.brewlia-annotation[data-type="context"]]:border-green-400/50
                [&_.brewlia-annotation[data-type="abbreviation"]]:text-pink-300 [&_.brewlia-annotation[data-type="abbreviation"]]:bg-pink-500/15 [&_.brewlia-annotation[data-type="abbreviation"]]:border-pink-400/50
                [&_.brewlia-annotation:hover]:ring-2 [&_.brewlia-annotation:hover]:ring-current/30
                [&_.brewlia-highlight-flash]:animate-pulse [&_.brewlia-highlight-flash]:ring-2 [&_.brewlia-highlight-flash]:ring-purple-400
                
                /* ====== Notion 内容样式 - 暗色主题 ====== */
                
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
                [&_.notion-callout]:bg-white/[0.04] [&_.notion-callout]:border [&_.notion-callout]:border-white/10
                [&_.notion-callout-icon]:text-xl [&_.notion-callout-icon]:flex-shrink-0
                [&_.notion-callout-icon]:w-6 [&_.notion-callout-icon]:h-6 [&_.notion-callout-icon]:object-contain
                [&_.notion-callout-content]:flex-1 [&_.notion-callout-content]:min-w-0
                
                /* Notion Quote */
                [&_.notion-quote]:pl-4 [&_.notion-quote]:py-1 [&_.notion-quote]:my-4
                [&_.notion-quote]:bg-white/[0.04] [&_.notion-quote]:rounded-xl
                
                /* Notion Todo */
                [&_.notion-todo]:flex [&_.notion-todo]:items-start [&_.notion-todo]:gap-2 [&_.notion-todo]:my-1
                [&_.notion-checkbox]:w-5 [&_.notion-checkbox]:h-5 [&_.notion-checkbox]:flex-shrink-0
                [&_.notion-checkbox]:border-2 [&_.notion-checkbox]:border-white/30 [&_.notion-checkbox]:rounded
                [&_.notion-checkbox.checked]:bg-blue-500 [&_.notion-checkbox.checked]:border-blue-500
                [&_.notion-checkbox.checked]:after:content-['✓'] [&_.notion-checkbox.checked]:after:text-white
                [&_.notion-checkbox.checked]:after:text-xs [&_.notion-checkbox.checked]:after:flex
                [&_.notion-checkbox.checked]:after:items-center [&_.notion-checkbox.checked]:after:justify-center
                [&_.notion-todo-text.checked]:line-through [&_.notion-todo-text.checked]:opacity-60
                
                /* Notion Toggle */
                [&_.notion-toggle]:bg-white/[0.03] [&_.notion-toggle]:border [&_.notion-toggle]:border-white/10
                [&_.notion-toggle]:rounded-xl [&_.notion-toggle]:my-3
                [&_.notion-toggle_summary]:px-4 [&_.notion-toggle_summary]:py-3
                
                /* Notion Image */
                [&_.notion-image]:my-6
                [&_.notion-image_img]:rounded-xl [&_.notion-image_img]:w-full
                [&_.notion-image_figcaption]:text-center [&_.notion-image_figcaption]:text-sm
                [&_.notion-image_figcaption]:mt-2 [&_.notion-image_figcaption]:opacity-60
                
                /* Notion Video */
                [&_.notion-video]:my-6
                [&_.notion-video-embed]:relative [&_.notion-video-embed]:w-full
                [&_.notion-video-embed]:pb-[56.25%] [&_.notion-video-embed]:rounded-xl [&_.notion-video-embed]:overflow-hidden
                [&_.notion-video-embed_iframe]:absolute [&_.notion-video-embed_iframe]:inset-0
                [&_.notion-video-embed_iframe]:w-full [&_.notion-video-embed_iframe]:h-full
                [&_.notion-video_video]:w-full [&_.notion-video_video]:rounded-xl
                
                /* Notion Audio */
                [&_.notion-audio]:my-4
                [&_.notion-audio_audio]:w-full
                
                /* Notion Bookmark */
                [&_.notion-bookmark]:flex [&_.notion-bookmark]:items-center [&_.notion-bookmark]:gap-3
                [&_.notion-bookmark]:p-4 [&_.notion-bookmark]:my-4 [&_.notion-bookmark]:rounded-xl
                [&_.notion-bookmark]:bg-white/[0.04] [&_.notion-bookmark]:border [&_.notion-bookmark]:border-white/10
                [&_.notion-bookmark]:no-underline [&_.notion-bookmark]:hover:bg-white/[0.06]
                [&_.notion-bookmark-icon]:text-lg
                [&_.notion-bookmark-title]:font-medium [&_.notion-bookmark-title]:flex-1
                [&_.notion-bookmark-url]:text-sm [&_.notion-bookmark-url]:opacity-50 [&_.notion-bookmark-url]:truncate [&_.notion-bookmark-url]:max-w-48
                
                /* Notion Link Preview */
                [&_.notion-link-preview]:inline-flex [&_.notion-link-preview]:items-center [&_.notion-link-preview]:gap-1.5
                [&_.notion-link-preview]:px-2 [&_.notion-link-preview]:py-0.5 [&_.notion-link-preview]:rounded-md
                [&_.notion-link-preview]:bg-white/[0.06] [&_.notion-link-preview]:no-underline
                [&_.notion-link-preview]:hover:bg-white/[0.1]
                
                /* Notion File */
                [&_.notion-file]:inline-flex [&_.notion-file]:items-center [&_.notion-file]:gap-2
                [&_.notion-file]:px-3 [&_.notion-file]:py-2 [&_.notion-file]:my-2 [&_.notion-file]:rounded-lg
                [&_.notion-file]:bg-white/[0.04] [&_.notion-file]:border [&_.notion-file]:border-white/10
                [&_.notion-file]:no-underline [&_.notion-file]:hover:bg-white/[0.08]
                
                /* Notion Embed */
                [&_.notion-embed]:my-6
                [&_.notion-embed-wrapper]:relative [&_.notion-embed-wrapper]:w-full
                [&_.notion-embed-wrapper]:pb-[56.25%] [&_.notion-embed-wrapper]:rounded-xl [&_.notion-embed-wrapper]:overflow-hidden
                [&_.notion-embed-wrapper_iframe]:absolute [&_.notion-embed-wrapper_iframe]:inset-0
                [&_.notion-embed-wrapper_iframe]:w-full [&_.notion-embed-wrapper_iframe]:h-full
                
                /* Notion PDF */
                [&_.notion-pdf]:my-6
                [&_.notion-pdf-embed]:w-full [&_.notion-pdf-embed]:h-[600px] [&_.notion-pdf-embed]:rounded-xl
                [&_.notion-pdf-embed]:border [&_.notion-pdf-embed]:border-white/10
                
                /* Notion Equation */
                [&_.notion-equation]:my-4 [&_.notion-equation]:py-4 [&_.notion-equation]:px-6
                [&_.notion-equation]:bg-white/[0.03] [&_.notion-equation]:rounded-xl
                [&_.notion-equation]:overflow-x-auto [&_.notion-equation]:text-center
                
                /* Notion Table */
                [&_.notion-table]:w-full [&_.notion-table]:my-4 [&_.notion-table]:border-collapse
                [&_.notion-table]:rounded-xl [&_.notion-table]:overflow-hidden
                [&_.notion-table_td]:px-3 [&_.notion-table_td]:py-2
                [&_.notion-table_td]:border [&_.notion-table_td]:border-white/10
                [&_.notion-table.has-header_tr:first-child]:bg-white/[0.05]
                [&_.notion-table.has-header_tr:first-child_td]:font-medium
                
                /* Notion Columns */
                [&_.notion-columns]:flex [&_.notion-columns]:gap-4 [&_.notion-columns]:my-4
                [&_.notion-column]:flex-1 [&_.notion-column]:min-w-0
                
                /* Notion Page Link */
                [&_.notion-page-link]:inline-flex [&_.notion-page-link]:items-center [&_.notion-page-link]:gap-1.5
                [&_.notion-page-link]:px-2 [&_.notion-page-link]:py-1 [&_.notion-page-link]:rounded-md
                [&_.notion-page-link]:bg-white/[0.04] [&_.notion-page-link]:no-underline
                [&_.notion-page-link]:hover:bg-white/[0.08]
                
                /* Notion Code */
                [&_.notion-code]:my-4
                [&_.notion-code_pre]:rounded-xl [&_.notion-code_pre]:overflow-x-auto
                [&_.notion-code_figcaption]:text-center [&_.notion-code_figcaption]:text-sm
                [&_.notion-code_figcaption]:mt-2 [&_.notion-code_figcaption]:opacity-50
                
              `
      : `
                /* === 浅色主题 === */
                
                /* 链接 */
                prose-a:text-amber-700 prose-a:decoration-amber-600/30
                hover:prose-a:text-amber-800 hover:prose-a:decoration-amber-700/50
                
                /* 引用块 */
                prose-blockquote:bg-black/[0.02]
                
                /* 行内代码 */
                prose-code:bg-black/[0.04] prose-code:text-amber-800
                
                /* 代码块 */
                prose-pre:bg-[#282c34] prose-pre:text-[#abb2bf]
                
                /* 分隔线 */
                prose-hr:bg-black/[0.06]
                
                /* 表格 */
                [&_table]:bg-black/[0.01]
                [&_thead]:bg-black/[0.02]
                [&_tbody_tr:nth-child(even)]:bg-black/[0.015]
                
                /* 数学公式 */
                [&_.katex-display]:bg-black/[0.02]
                
                /* details */
                [&_details]:bg-black/[0.02]
                [&_summary:hover]:bg-black/[0.04]
                
                /* kbd */
                [&_kbd]:bg-black/[0.05]
                
                /* mark */
                [&_mark]:text-amber-900 [&_mark]:bg-amber-400/30
                
                /* ====== RSS 内容样式 - 浅色主题 ====== */
                
                /* RSS 引用块 */
                [&_.rss-content-blockquote]:bg-black/[0.02] [&_.rss-content-blockquote]:border-black/10
                
                /* RSS 代码 */
                [&_.rss-content-pre]:bg-[#282c34] [&_.rss-content-pre]:text-[#abb2bf]
                [&_.rss-content-inline-code]:bg-black/[0.04]
                
                /* RSS 表格 */
                [&_.rss-content-table]:border-black/10
                [&_.rss-content-thead]:bg-black/[0.03]
                [&_.rss-content-th]:border-black/10
                [&_.rss-content-td]:border-black/10
                [&_.rss-content-tr:nth-child(even)]:bg-black/[0.015]
                
                /* RSS 描述列表 */
                [&_.rss-content-dd]:border-black/10
                
                /* RSS 折叠 */
                [&_.rss-content-details]:bg-black/[0.02]
                [&_.rss-content-summary]:hover:bg-black/[0.04]
                [&_.rss-content-details[open]_.rss-content-summary]:border-black/10
                
                /* RSS kbd */
                [&_.rss-content-kbd]:bg-black/[0.05] [&_.rss-content-kbd]:border-black/10
                
                /* RSS mark */
                [&_.rss-content-mark]:bg-yellow-200/60
                
                /* RSS abbr */
                [&_.rss-content-abbr]:border-black/30
                
                /* RSS 分隔线 */
                [&_.rss-content-hr]:bg-black/10
                
                /* RSS 类别标签 */
                [&_.rss-content-category]:bg-black/5
                
                /* RSS 侧边栏 */
                [&_.rss-content-aside]:bg-black/[0.02]
                
                /* Brewlia 注释样式 - 浅色主题 */
                [&_.brewlia-annotation]:cursor-help [&_.brewlia-annotation]:rounded [&_.brewlia-annotation]:px-0.5
                [&_.brewlia-annotation]:transition-all [&_.brewlia-annotation]:duration-200
                [&_.brewlia-annotation]:border-b-2 [&_.brewlia-annotation]:border-dotted
                [&_.brewlia-annotation[data-type="term"]]:text-orange-700 [&_.brewlia-annotation[data-type="term"]]:bg-orange-100 [&_.brewlia-annotation[data-type="term"]]:border-orange-400
                [&_.brewlia-annotation[data-type="reference"]]:text-blue-700 [&_.brewlia-annotation[data-type="reference"]]:bg-blue-100 [&_.brewlia-annotation[data-type="reference"]]:border-blue-400
                [&_.brewlia-annotation[data-type="implicit"]]:text-purple-700 [&_.brewlia-annotation[data-type="implicit"]]:bg-purple-100 [&_.brewlia-annotation[data-type="implicit"]]:border-purple-400
                [&_.brewlia-annotation[data-type="context"]]:text-green-700 [&_.brewlia-annotation[data-type="context"]]:bg-green-100 [&_.brewlia-annotation[data-type="context"]]:border-green-400
                [&_.brewlia-annotation[data-type="abbreviation"]]:text-pink-700 [&_.brewlia-annotation[data-type="abbreviation"]]:bg-pink-100 [&_.brewlia-annotation[data-type="abbreviation"]]:border-pink-400
                [&_.brewlia-annotation:hover]:ring-2 [&_.brewlia-annotation:hover]:ring-current/30
                [&_.brewlia-highlight-flash]:animate-pulse [&_.brewlia-highlight-flash]:ring-2 [&_.brewlia-highlight-flash]:ring-purple-500
                
                /* figcaption */
                prose-figcaption:text-current
                
                /* ====== Notion 内容样式 - 浅色主题 ====== */
                
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
                [&_.notion-callout]:bg-black/[0.02] [&_.notion-callout]:border [&_.notion-callout]:border-black/5
                [&_.notion-callout-icon]:text-xl [&_.notion-callout-icon]:flex-shrink-0
                [&_.notion-callout-icon]:w-6 [&_.notion-callout-icon]:h-6 [&_.notion-callout-icon]:object-contain
                [&_.notion-callout-content]:flex-1 [&_.notion-callout-content]:min-w-0
                
                /* Notion Quote */
                [&_.notion-quote]:pl-4 [&_.notion-quote]:py-1 [&_.notion-quote]:my-4
                [&_.notion-quote]:bg-black/[0.04] [&_.notion-quote]:rounded-xl
                
                /* Notion Todo */
                [&_.notion-todo]:flex [&_.notion-todo]:items-start [&_.notion-todo]:gap-2 [&_.notion-todo]:my-1
                [&_.notion-checkbox]:w-5 [&_.notion-checkbox]:h-5 [&_.notion-checkbox]:flex-shrink-0
                [&_.notion-checkbox]:border-2 [&_.notion-checkbox]:border-black/20 [&_.notion-checkbox]:rounded
                [&_.notion-checkbox.checked]:bg-blue-500 [&_.notion-checkbox.checked]:border-blue-500
                [&_.notion-checkbox.checked]:after:content-['✓'] [&_.notion-checkbox.checked]:after:text-white
                [&_.notion-checkbox.checked]:after:text-xs [&_.notion-checkbox.checked]:after:flex
                [&_.notion-checkbox.checked]:after:items-center [&_.notion-checkbox.checked]:after:justify-center
                [&_.notion-todo-text.checked]:line-through [&_.notion-todo-text.checked]:opacity-60
                
                /* Notion Toggle */
                [&_.notion-toggle]:bg-black/[0.02] [&_.notion-toggle]:border [&_.notion-toggle]:border-black/5
                [&_.notion-toggle]:rounded-xl [&_.notion-toggle]:my-3
                [&_.notion-toggle_summary]:px-4 [&_.notion-toggle_summary]:py-3
                
                /* Notion Image */
                [&_.notion-image]:my-6
                [&_.notion-image_img]:rounded-xl [&_.notion-image_img]:w-full
                [&_.notion-image_figcaption]:text-center [&_.notion-image_figcaption]:text-sm
                [&_.notion-image_figcaption]:mt-2 [&_.notion-image_figcaption]:opacity-60
                
                /* Notion Video */
                [&_.notion-video]:my-6
                [&_.notion-video-embed]:relative [&_.notion-video-embed]:w-full
                [&_.notion-video-embed]:pb-[56.25%] [&_.notion-video-embed]:rounded-xl [&_.notion-video-embed]:overflow-hidden
                [&_.notion-video-embed_iframe]:absolute [&_.notion-video-embed_iframe]:inset-0
                [&_.notion-video-embed_iframe]:w-full [&_.notion-video-embed_iframe]:h-full
                [&_.notion-video_video]:w-full [&_.notion-video_video]:rounded-xl
                
                /* Notion Audio */
                [&_.notion-audio]:my-4
                [&_.notion-audio_audio]:w-full
                
                /* Notion Bookmark */
                [&_.notion-bookmark]:flex [&_.notion-bookmark]:items-center [&_.notion-bookmark]:gap-3
                [&_.notion-bookmark]:p-4 [&_.notion-bookmark]:my-4 [&_.notion-bookmark]:rounded-xl
                [&_.notion-bookmark]:bg-black/[0.02] [&_.notion-bookmark]:border [&_.notion-bookmark]:border-black/5
                [&_.notion-bookmark]:no-underline [&_.notion-bookmark]:hover:bg-black/[0.04]
                [&_.notion-bookmark-icon]:text-lg
                [&_.notion-bookmark-title]:font-medium [&_.notion-bookmark-title]:flex-1
                [&_.notion-bookmark-url]:text-sm [&_.notion-bookmark-url]:opacity-50 [&_.notion-bookmark-url]:truncate [&_.notion-bookmark-url]:max-w-48
                
                /* Notion Link Preview */
                [&_.notion-link-preview]:inline-flex [&_.notion-link-preview]:items-center [&_.notion-link-preview]:gap-1.5
                [&_.notion-link-preview]:px-2 [&_.notion-link-preview]:py-0.5 [&_.notion-link-preview]:rounded-md
                [&_.notion-link-preview]:bg-black/[0.03] [&_.notion-link-preview]:no-underline
                [&_.notion-link-preview]:hover:bg-black/[0.06]
                
                /* Notion File */
                [&_.notion-file]:inline-flex [&_.notion-file]:items-center [&_.notion-file]:gap-2
                [&_.notion-file]:px-3 [&_.notion-file]:py-2 [&_.notion-file]:my-2 [&_.notion-file]:rounded-lg
                [&_.notion-file]:bg-black/[0.02] [&_.notion-file]:border [&_.notion-file]:border-black/5
                [&_.notion-file]:no-underline [&_.notion-file]:hover:bg-black/[0.04]
                
                /* Notion Embed */
                [&_.notion-embed]:my-6
                [&_.notion-embed-wrapper]:relative [&_.notion-embed-wrapper]:w-full
                [&_.notion-embed-wrapper]:pb-[56.25%] [&_.notion-embed-wrapper]:rounded-xl [&_.notion-embed-wrapper]:overflow-hidden
                [&_.notion-embed-wrapper_iframe]:absolute [&_.notion-embed-wrapper_iframe]:inset-0
                [&_.notion-embed-wrapper_iframe]:w-full [&_.notion-embed-wrapper_iframe]:h-full
                
                /* Notion PDF */
                [&_.notion-pdf]:my-6
                [&_.notion-pdf-embed]:w-full [&_.notion-pdf-embed]:h-[600px] [&_.notion-pdf-embed]:rounded-xl
                [&_.notion-pdf-embed]:border [&_.notion-pdf-embed]:border-black/10
                
                /* Notion Equation */
                [&_.notion-equation]:my-4 [&_.notion-equation]:py-4 [&_.notion-equation]:px-6
                [&_.notion-equation]:bg-black/[0.02] [&_.notion-equation]:rounded-xl
                [&_.notion-equation]:overflow-x-auto [&_.notion-equation]:text-center
                
                /* Notion Table */
                [&_.notion-table]:w-full [&_.notion-table]:my-4 [&_.notion-table]:border-collapse
                [&_.notion-table]:rounded-xl [&_.notion-table]:overflow-hidden
                [&_.notion-table_td]:px-3 [&_.notion-table_td]:py-2
                [&_.notion-table_td]:border [&_.notion-table_td]:border-black/10
                [&_.notion-table.has-header_tr:first-child]:bg-black/[0.03]
                [&_.notion-table.has-header_tr:first-child_td]:font-medium
                
                /* Notion Columns */
                [&_.notion-columns]:flex [&_.notion-columns]:gap-4 [&_.notion-columns]:my-4
                [&_.notion-column]:flex-1 [&_.notion-column]:min-w-0
                
                /* Notion Page Link */
                [&_.notion-page-link]:inline-flex [&_.notion-page-link]:items-center [&_.notion-page-link]:gap-1.5
                [&_.notion-page-link]:px-2 [&_.notion-page-link]:py-1 [&_.notion-page-link]:rounded-md
                [&_.notion-page-link]:bg-black/[0.02] [&_.notion-page-link]:no-underline
                [&_.notion-page-link]:hover:bg-black/[0.04]
                
                /* Notion Code */
                [&_.notion-code]:my-4
                [&_.notion-code_pre]:rounded-xl [&_.notion-code_pre]:overflow-x-auto
                [&_.notion-code_figcaption]:text-center [&_.notion-code_figcaption]:text-sm
                [&_.notion-code_figcaption]:mt-2 [&_.notion-code_figcaption]:opacity-50
              `}
            `}
              style={{
                fontSize: `${fontSize}px`,
                lineHeight,
                fontFamily: currentFont.family,
              }}
              onClick={e => e.stopPropagation()}
            >
              {/* WebKit 优化：动画期间显示简单占位，避免同时渲染大量 DOM */}
              {contentReady
                ? (
                    <div dangerouslySetInnerHTML={{ __html: processedContent }} />
                  )
                : (
                    <div className="space-y-4 animate-pulse">
                      <div className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`} style={{ width: '90%' }} />
                      <div className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`} style={{ width: '100%' }} />
                      <div className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`} style={{ width: '85%' }} />
                      <div className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`} style={{ width: '95%' }} />
                      <div className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`} style={{ width: '70%' }} />
                    </div>
                  )}
            </div>

            {/* 音频播放器 */}
            {item.audio_url && (
              <div
                className={`mt-8 p-4 rounded-2xl ${currentTheme.surfaceSolid} border ${currentTheme.border}`}
                onClick={e => e.stopPropagation()}
              >
                <p className={`text-sm ${currentTheme.secondary} mb-3`}>{t.brew.audioLabel}</p>
                <audio src={item.audio_url} controls className="w-full" />
              </div>
            )}

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
            onMouseEnter={() => { isHoveringControlsRef.current = true }}
            onMouseLeave={() => { isHoveringControlsRef.current = false; resetHideTimer(2000) }}
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
            initial={enableAnimations ? { opacity: 0, y: 50, scale: 0.95 } : false}
            animate={enableAnimations ? { opacity: 1, y: 0, scale: 1 } : undefined}
            exit={enableAnimations ? { opacity: 0, y: 50, scale: 0.95 } : undefined}
            transition={enableAnimations ? { duration: 0.25, ease: [0.16, 1, 0.3, 1] } : undefined}
            className={`fixed bottom-8 inset-x-0 mx-auto w-fit px-4 py-2 rounded-xl shadow-lg z-[60] ${
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
        setCommentTooltip={setCommentTooltip}
        currentTheme={currentTheme}
        isDark={isDark}
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
        handlePlayPause={podcastState === 'playing' ? handlePodcastPause : handlePodcastPlay}
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
        onTouchStart={() => { isHoveringControlsRef.current = true }}
        onTouchEnd={() => { isHoveringControlsRef.current = false; resetHideTimer(2000) }}
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
