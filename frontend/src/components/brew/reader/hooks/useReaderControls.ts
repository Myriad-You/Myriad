/**
 * 阅读进度、目录跟踪、标题跳转与键盘快捷键
 */

import type { TocItem } from '../types'
import { useCallback, useEffect, useRef, useState } from 'react'
import * as brewApi from '../../../../services/brewApi'

export interface UseReaderControlsOptions {
  articleRef: React.RefObject<HTMLElement | null>
  contentRef: React.RefObject<HTMLDivElement | null>
  itemId: number
  readProgress: number | null | undefined
  contentReady: boolean
  isAuthenticated: boolean
  onClose: () => void
  adjustFontSize: (delta: number) => void
  showToastMessage: (message: string, duration?: number) => void
  t: Record<string, any>
}

export interface UseReaderControlsReturn {
  readingProgress: number
  toc: TocItem[]
  setToc: (toc: TocItem[]) => void
  showToc: boolean
  setShowToc: (show: boolean) => void
  activeHeadingId: string
  scrollToHeading: (id: string) => void
  handleProgressPointerDown: () => void
  handleProgressPointerUp: () => void
  handleProgressPointerLeave: () => void
}

export function useReaderControls({
  articleRef,
  contentRef,
  itemId,
  readProgress,
  contentReady,
  isAuthenticated,
  onClose,
  adjustFontSize,
  showToastMessage,
  t,
}: UseReaderControlsOptions): UseReaderControlsReturn {
  const [readingProgress, setReadingProgress] = useState(0)
  const progressRafRef = useRef<number | null>(null)
  const lastSyncedProgressRef = useRef<number>(-1)
  const progressSyncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const [toc, setToc] = useState<TocItem[]>([])
  const [showToc, setShowToc] = useState(false)
  const [activeHeadingId, setActiveHeadingId] = useState<string>('')
  const activeHeadingIdRef = useRef<string>('')
  const [headingHistory, setHeadingHistory] = useState<string[]>([])
  const progressLongPressRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const isLongPressRef = useRef(false)

  // 切换文章：恢复服务端进度或回到顶部。
  // 正文是动画后再灌进 DOM 的，只在 itemId 时滚一次会停在占位高度上；
  // contentReady 后再钉一次。behavior:auto 避开容器上的 smooth。
  useEffect(() => {
    lastSyncedProgressRef.current = -1
    if (progressSyncTimerRef.current) {
      clearTimeout(progressSyncTimerRef.current)
      progressSyncTimerRef.current = null
    }
    const saved =
      typeof readProgress === 'number' && readProgress > 0
        ? Math.min(100, Math.round(readProgress))
        : 0
    setReadingProgress(saved)
    const pin = () => {
      const el = articleRef.current
      if (!el) return
      // 复位必须瞬间完成：容器带 scroll-behavior: smooth，直接赋 scrollTop 也会
      // 平滑滚一段，正好落在新正文淡入的那 260ms 里，看起来像内容在往上飘。
      // `scrollTo` 的 behavior 只管它自己那一次，管不到赋值那一支。
      const prevBehavior = el.style.scrollBehavior
      el.style.scrollBehavior = 'auto'
      if (saved > 0 && saved < 100) {
        const max = el.scrollHeight - el.clientHeight
        if (max > 0) el.scrollTop = (saved / 100) * max
      } else {
        el.scrollTo({ top: 0, behavior: 'auto' })
      }
      el.style.scrollBehavior = prevBehavior
    }
    requestAnimationFrame(pin)
  }, [itemId, contentReady])

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
            .updateReadProgress(itemId, progress, {
              isRead: progress >= 95 ? true : undefined,
            })
            .catch(() => {
              /* best-effort */
            })
        }, 2000)
      }
    })
  }, [isAuthenticated, itemId])

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
  }, [isAuthenticated, itemId])

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

  return {
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
  }
}
