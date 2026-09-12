import type { CommentItem } from '../../../../services/brewApi'
import type { AnnotationItem, AnnotationType } from '../../../../services/brewliaApi'
import type { ReaderCopy } from '../types'
import { useCallback, useEffect, useRef } from 'react'
import { playNeteaseSong } from '../../../../utils/embedProcessor'

export interface UseContentEventsOptions {
  setFocusedCommentIds: (ids: number[]) => void
  contentRef: React.RefObject<HTMLDivElement | null>
  comments: CommentItem[]
  isAuthenticated: boolean
  showCommentPopup: boolean
  showAnnotations: boolean
  hoverTimeoutRef: React.RefObject<ReturnType<typeof setTimeout> | null>
  setLightboxImage: (src: string | null) => void
  setCommentTooltip: (
    tooltip: { comment: CommentItem; x: number; y: number } | null,
  ) => void
  setShowCommentsPanel: (show: boolean) => void
  setHoveredAnnotation: (annotation: AnnotationItem | null) => void
  setTooltipPosition: (position: { x: number; y: number }) => void
  setSelectedText: (text: string) => void
  setCommentPopupPosition: (position: { x: number; y: number }) => void
  setSelectionRange: (
    range: {
      start: number
      end: number
      contextBefore: string
      contextAfter: string
    } | null,
  ) => void
  setShowCommentPopup: (show: boolean) => void
  setCommentInput: (input: string) => void
  showToastMessage: (message: string, duration?: number) => void
  t: ReaderCopy
}

export interface UseContentEventsReturn {
  handleTooltipMouseEnter: () => void
  handleTooltipMouseLeave: () => void
}

export function useContentEvents({
  setFocusedCommentIds,
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
}: UseContentEventsOptions): UseContentEventsReturn {
  const commentsRef = useRef(comments)
  commentsRef.current = comments

  const tooltipHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isHoveringTooltipRef = useRef(false)

  const clearTooltipHideTimer = useCallback(() => {
    if (tooltipHideTimerRef.current) {
      clearTimeout(tooltipHideTimerRef.current)
      tooltipHideTimerRef.current = null
    }
  }, [])

  const startTooltipHideTimer = useCallback(
    (delay = 200) => {
      if (tooltipHideTimerRef.current) return
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

  useEffect(() => {
    return () => clearTooltipHideTimer()
  }, [clearTooltipHideTimer])

  useEffect(() => {
    if (!contentRef.current) return

    const container = contentRef.current
    let focusTimer: ReturnType<typeof setTimeout> | null = null
    const handleContentClick = async (e: Event) => {
      const target = e.target as HTMLElement

      if (target.tagName === 'IMG') {
        const img = target as HTMLImageElement
        // 嵌入卡片内的图片不拦。
        const isInEmbedCard = img.closest(
          '.brew-embed-card, .brew-embed-exempt, .brew-bilibili-embed, .brew-netease-music, .brew-steam-game, .brew-bilibili-video',
        )

        if (img.src && !isInEmbedCard) {
          e.preventDefault()
          e.stopPropagation()
          setLightboxImage(img.src)
          return
        }
      }

      const neteaseCard = target.closest('.brew-netease-music')
      if (neteaseCard) {
        e.preventDefault()
        e.stopPropagation()

        const songId = neteaseCard.getAttribute('data-song-id')
        if (songId) {
          try {
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

      const highlight = target.closest('.user-comment-highlight')

      if (highlight) {
        e.preventDefault()
        e.stopPropagation()

        const commentId = highlight.getAttribute('data-comment-id')
        if (commentId) {
          const ids = new Set<number>()
          let node: Element | null = highlight
          while (node && node !== container) {
            if (node.matches('.user-comment-highlight')) ids.add(Number(node.getAttribute('data-comment-id')))
            node = node.parentElement
          }
          setFocusedCommentIds(
            Iterator.from(ids).filter(Number.isFinite).toArray(),
          )
          setCommentTooltip(null)
          setShowCommentsPanel(true)
          if (focusTimer) clearTimeout(focusTimer)
          focusTimer = setTimeout(() => {
            const commentEl = document.querySelector(
              `[data-panel-comment-id="${commentId}"]`,
            )
            if (commentEl) {
              ;(commentEl as HTMLElement).focus({ preventScroll: true })
              commentEl.scrollIntoView({ behavior: 'smooth', block: 'center' })
            }
          }, 100)
        }
      }
    }

    let lastHoveredCommentId: string | null = null
    const handleMouseOver = (e: Event) => {
      const target = e.target as HTMLElement
      const highlight = target.closest('.user-comment-highlight') as HTMLElement

      if (highlight) {
        clearTooltipHideTimer()
        const commentId = highlight.getAttribute('data-comment-id')
        // 同一评论不重复 setState。
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
        if (!isHoveringTooltipRef.current) {
          startTooltipHideTimer()
        }
      }
    }

    const handleContentMouseLeave = () => {
      if (!isHoveringTooltipRef.current) {
        startTooltipHideTimer()
      }
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement
      if ((event.key === 'Enter' || event.key === ' ') && target.matches('.user-comment-highlight')) {
        event.preventDefault()
        target.click()
      }
    }
    container.addEventListener('keydown', handleKeyDown)
    contentRef.current.addEventListener('click', handleContentClick)
    contentRef.current.addEventListener('mouseover', handleMouseOver)
    contentRef.current.addEventListener('mouseleave', handleContentMouseLeave)

    return () => {
      if (focusTimer) clearTimeout(focusTimer)
      container.removeEventListener('keydown', handleKeyDown)
      container.removeEventListener('click', handleContentClick)
      contentRef.current?.removeEventListener('mouseover', handleMouseOver)
      contentRef.current?.removeEventListener(
        'mouseleave',
        handleContentMouseLeave,
      )
    }
  }, [])

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

    const range = selection.getRangeAt(0)
    if (!contentRef.current?.contains(range.commonAncestorContainer)) {
      return
    }

    const rect = range.getBoundingClientRect()

    const x = rect.left + rect.width / 2
    const y = rect.top

    setSelectedText(text)
    setCommentPopupPosition({ x, y })

    const fullText = contentRef.current?.textContent || ''
    const prefix = range.cloneRange()
    prefix.selectNodeContents(contentRef.current!)
    prefix.setEnd(range.startContainer, range.startOffset)
    const leadingWhitespace = range.toString().length - range.toString().trimStart().length
    const textIndex = prefix.toString().length + leadingWhitespace
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

  useEffect(() => {
    if (!isAuthenticated) return

    let selectionTimer: ReturnType<typeof setTimeout> | null = null

    const handleMouseUp = () => {
      selectionTimer = setTimeout(handleTextSelection, 10)
    }

    document.addEventListener('mouseup', handleMouseUp)
    return () => {
      document.removeEventListener('mouseup', handleMouseUp)
      if (selectionTimer) clearTimeout(selectionTimer)
    }
  }, [isAuthenticated, handleTextSelection])

  useEffect(() => {
    if (!showCommentPopup) return

    const handleSelectionChange = () => {
      // 焦点在评论弹窗内时不关弹窗。
      const activeElement = document.activeElement
      if (activeElement?.closest('.comment-popup')) {
        return
      }

      const selection = window.getSelection()
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

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      // 点评论弹窗或高亮时不关弹窗。
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
        window.getSelection()?.removeAllRanges()
      }
    }

    // 用 click 不用 mousedown，避免划词时触发。
    document.addEventListener('click', handleClickOutside)
    return () => document.removeEventListener('click', handleClickOutside)
  }, [showCommentPopup])

  useEffect(() => {
    if (!contentRef.current || !showAnnotations) return

    const handleMouseOver = (e: MouseEvent) => {
      const target = (e.target as HTMLElement).closest(
        '.brewlia-annotation',
      ) as HTMLElement

      if (target) {
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
        if (!hoverTimeoutRef.current) {
          hoverTimeoutRef.current = setTimeout(() => {
            hoverTimeoutRef.current = null
            setHoveredAnnotation(null)
          }, 150)
        }
      }
    }

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

  return {
    handleTooltipMouseEnter,
    handleTooltipMouseLeave,
  }
}
