import type { ReadingProgress } from '../progressStore'
import type { ReaderCopy, TocItem } from '../types'
import { useCallback, useEffect, useRef, useState } from 'react'
import * as phantasiApi from '../../../../services/phantasiApi'
import { phantasiSubject } from '../../../../utils/phantasiSubject'
import { PhantasiSyncConflictError } from '../../../../utils/phantasiSyncConflict'
import { RequestTurn } from '../../logic/requestTurn'
import { progressOutbox } from '../progressOutbox'
import { createReadingProgress } from '../progressStore'
import { ProgressSync } from '../progressSync'
import { useArticleTaskScope } from './useArticleTaskScope'

interface UseReaderControlsOptions {
  articleRef: React.RefObject<HTMLElement | null>
  contentRef: React.RefObject<HTMLDivElement | null>
  itemId: number
  readProgress: number | null | undefined
  stateRevision?: number
  contentReady: boolean
  isAuthenticated: boolean
  adjustFontSize: (delta: number) => void
  showToastMessage: (message: string, duration?: number) => void
  t: ReaderCopy
}

interface UseReaderControlsReturn {
  readingProgress: ReadingProgress
  toc: TocItem[]
  setToc: (toc: TocItem[]) => void
  showToc: boolean
  setShowToc: (show: boolean) => void
  activeHeadingId: string
  scrollToHeading: (id: string) => void
  handleProgressPointerDown: () => void
  handleProgressPointerUp: () => void
  handleProgressPointerLeave: () => void
  syncPaused: boolean
  recoverLocalProgress: () => Promise<void>
  recoverRemoteProgress: () => Promise<void>
}

function scrollProgress(el: HTMLElement): number {
  const max = el.scrollHeight - el.clientHeight
  return max <= 0
    ? 100
    : Math.max(0, Math.min(100, Math.round((el.scrollTop / max) * 100)))
}

export function useReaderControls({
  articleRef,
  contentRef,
  itemId,
  readProgress,
  stateRevision,
  contentReady,
  isAuthenticated,
  adjustFontSize,
  showToastMessage,
  t,
}: UseReaderControlsOptions): UseReaderControlsReturn {
  const captureTask = useArticleTaskScope(itemId)
  const recoverTurn = useRef(new RequestTurn())
  useEffect(() => () => recoverTurn.current.cancel(), [itemId])
  const readingProgress = useRef(createReadingProgress()).current
  const [syncPaused, setSyncPaused] = useState(false)
  const progressRafRef = useRef<number | null>(null)
  const pinnedScrollTopRef = useRef<number | null>(null)
  const syncRef = useRef<ProgressSync | null>(null)
  const expectedRevisionRef = useRef(stateRevision)
  const conflictNoticeRef = useRef(() => {})
  conflictNoticeRef.current = () => showToastMessage(t.phantasi.readingSyncConflict, 8000)
  useEffect(() => {
    expectedRevisionRef.current = stateRevision
  }, [itemId, stateRevision])
  useEffect(() => {
    if (!isAuthenticated || itemId <= 0) return
    const subject = phantasiSubject.getSnapshot()
    setSyncPaused(false)
    const persist =
      typeof sessionStorage === 'undefined'
        ? undefined
        : progressOutbox(
            sessionStorage,
            subject.key,
            subject.generation,
            itemId,
          )
    const sync = new ProgressSync(async (progress, observedAt) => {
      phantasiSubject.assert(subject)
      const confirmedRevision = await phantasiApi.updateReadProgress(itemId, progress, {
        isRead: progress >= 95 ? true : undefined,
        observedAt,
        expectedRevision: expectedRevisionRef.current,
      })
      if (confirmedRevision !== undefined) expectedRevisionRef.current = confirmedRevision
    }, subject.signal, (error) => {
      if (error instanceof PhantasiSyncConflictError && syncRef.current === sync) {
        setSyncPaused(true)
        conflictNoticeRef.current()
      }
    }, persist)
    syncRef.current = sync
    const hidden = () => {
      if (document.visibilityState === 'hidden') void sync.flush()
    }
    document.addEventListener('visibilitychange', hidden)
    return () => {
      document.removeEventListener('visibilitychange', hidden)
      syncRef.current = null
      setSyncPaused(false)
      sync.release()
    }
  }, [itemId, isAuthenticated])
  const pinProgress = useCallback((value: number) => {
    const el = articleRef.current
    if (!el) return
    if (progressRafRef.current !== null) {
      cancelAnimationFrame(progressRafRef.current)
      progressRafRef.current = null
    }
    const prevBehavior = el.style.scrollBehavior
    el.style.scrollBehavior = 'auto'
    if (value > 0 && value < 100) {
      const max = el.scrollHeight - el.clientHeight
      if (max > 0) el.scrollTop = (value / 100) * max
    } else {
      el.scrollTo({ top: 0, behavior: 'auto' })
    }
    el.style.scrollBehavior = prevBehavior
    // 定位只更新显示；浏览器随后发出的 scroll 不能把已读完的 100 写成 0。
    pinnedScrollTopRef.current = el.scrollTop
    readingProgress.set(scrollProgress(el))
  }, [articleRef])
  const recoverLocalProgress = useCallback(async () => {
    const isCurrent = captureTask()
    const signal = recoverTurn.current.begin()
    try {
      const fresh = await phantasiApi.getItem(itemId, undefined, { signal })
      if (!isCurrent() || signal.aborted) return
      expectedRevisionRef.current = fresh.state_revision
      syncRef.current?.resume()
      setSyncPaused(false)
    } catch (error) {
      if (!isCurrent() || signal.aborted) return
      throw error
    }
  }, [captureTask, itemId])
  const recoverRemoteProgress = useCallback(async () => {
    const isCurrent = captureTask()
    const signal = recoverTurn.current.begin()
    try {
      const fresh = await phantasiApi.getItem(itemId, undefined, { signal })
      if (!isCurrent() || signal.aborted) return
      expectedRevisionRef.current = fresh.state_revision
      const progress =
        typeof fresh.read_progress === 'number'
          ? Math.max(0, Math.min(100, Math.round(fresh.read_progress)))
          : 0
      syncRef.current?.adopt(progress)
      pinProgress(progress)
      setSyncPaused(false)
    } catch (error) {
      if (!isCurrent() || signal.aborted) return
      throw error
    }
  }, [captureTask, itemId, pinProgress])
  const [toc, setToc] = useState<TocItem[]>([])
  const [showToc, setShowToc] = useState(false)
  const [activeHeadingId, setActiveHeadingId] = useState<string>('')
  const activeHeadingIdRef = useRef<string>('')
  const [headingHistory, setHeadingHistory] = useState<string[]>([])
  const progressLongPressRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const isLongPressRef = useRef(false)

  // 正文动画后再灌 DOM；contentReady 后再钉进度。behavior:auto 避开容器 smooth。
  useEffect(() => {
    const saved =
      typeof readProgress === 'number' && readProgress > 0
        ? Math.min(100, Math.round(readProgress))
        : 0
    readingProgress.set(saved < 100 ? saved : 0)
    if (!contentReady) return
    const frame = requestAnimationFrame(() => pinProgress(saved))
    return () => cancelAnimationFrame(frame)
  }, [itemId, contentReady])

  const updateReadingProgress = useCallback(() => {
    const el = articleRef.current
    if (!el || !contentReady) return
    if (pinnedScrollTopRef.current === el.scrollTop) return
    pinnedScrollTopRef.current = null
    const progress = scrollProgress(el)
    if (!Number.isFinite(progress)) return
    // Capture before RAF: close may happen before the next paint.
    if (isAuthenticated) syncRef.current?.record(progress)
    if (progressRafRef.current !== null)
      cancelAnimationFrame(progressRafRef.current)
    progressRafRef.current = requestAnimationFrame(() => {
      progressRafRef.current = null
      readingProgress.set(progress)
    })
  }, [isAuthenticated, itemId, contentReady])

  useEffect(
    () => () => {
      if (progressRafRef.current !== null)
        cancelAnimationFrame(progressRafRef.current)
      progressRafRef.current = null
      if (progressLongPressRef.current)
        clearTimeout(progressLongPressRef.current)
    },
    [itemId],
  )

  activeHeadingIdRef.current = activeHeadingId

  useEffect(() => {
    const article = articleRef.current
    const content = contentRef.current
    if (!article || !content || toc.length === 0) return

    const headings = Iterator.from(content.querySelectorAll('h1, h2, h3, h4, h5, h6'))
      .filter((item) => !item.closest('.note-widget'))
      .toArray() as HTMLElement[]
    if (headings.length === 0) return

    let raf = 0
    const handleScrollForToc = () => {
      if (raf) return
      raf = requestAnimationFrame(() => {
        raf = 0
        // The result is the last qualifying heading in DOM order. Search from
        // that end and stop, without caching positions that images/fonts can move.
        const heading = headings.findLast(
          (item) => item.getBoundingClientRect().top <= 150,
        )
        const currentId = heading?.id ?? ''

        const prevId = activeHeadingIdRef.current
        if (currentId !== prevId) {
          if (currentId && prevId) {
            setHeadingHistory((prev) => {
              const newHistory = prev.filter((id) => id !== prevId)
              newHistory.push(prevId)
              return newHistory.slice(-20)
            })
          }
          setActiveHeadingId(currentId)
        }
      })
    }

    article.addEventListener('scroll', handleScrollForToc, { passive: true })
    return () => {
      article.removeEventListener('scroll', handleScrollForToc)
      if (raf) cancelAnimationFrame(raf)
    }
  }, [toc])

  useEffect(() => {
    const article = articleRef.current
    if (article) {
      article.addEventListener('scroll', updateReadingProgress)
      return () => article.removeEventListener('scroll', updateReadingProgress)
    }
  }, [updateReadingProgress])

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

  const goToPreviousHeading = useCallback(() => {
    if (headingHistory.length > 0) {
      const prevId = headingHistory.at(-1)!
      setHeadingHistory((prev) => prev.slice(0, -1))
      scrollToHeading(prevId)
    } else if (activeHeadingId && toc.length > 0) {
      const currentIndex = toc.findIndex((t) => t.id === activeHeadingId)
      if (currentIndex > 0) {
        scrollToHeading(toc[currentIndex - 1].id)
      }
    }
  }, [headingHistory, activeHeadingId, toc, scrollToHeading])

  const scrollToTop = useCallback(() => {
    if (articleRef.current) {
      articleRef.current.scrollTo({
        top: 0,
        behavior: 'smooth',
      })
      setHeadingHistory([])
      setActiveHeadingId('')
    }
  }, [])

  const handleProgressPointerDown = useCallback(() => {
    isLongPressRef.current = false
    progressLongPressRef.current = setTimeout(() => {
      isLongPressRef.current = true
      scrollToTop()
    }, 500)
  }, [scrollToTop])

  const handleProgressPointerUp = useCallback(() => {
    if (progressLongPressRef.current) {
      clearTimeout(progressLongPressRef.current)
      progressLongPressRef.current = null
    }
    if (!isLongPressRef.current) {
      goToPreviousHeading()
    }
  }, [goToPreviousHeading])

  const handleProgressPointerLeave = useCallback(() => {
    if (progressLongPressRef.current) {
      clearTimeout(progressLongPressRef.current)
      progressLongPressRef.current = null
    }
  }, [])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (
        e.target instanceof HTMLElement &&
        e.target.closest('input, textarea, select, [contenteditable="true"]')
      ) {
        return
}
      if (e.key === '+' || e.key === '=') adjustFontSize(1)
      if (e.key === '-') adjustFontSize(-1)
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [adjustFontSize])

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
    syncPaused,
    recoverLocalProgress,
    recoverRemoteProgress,
  }
}
