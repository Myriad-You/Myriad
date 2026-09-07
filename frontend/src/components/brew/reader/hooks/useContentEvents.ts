/**
 * 正文交互：评论高亮、注释悬停、划词评论
 */

import type { CommentItem } from '../../../../services/brewApi'
import type { AnnotationItem, AnnotationType } from '../../../../services/brewliaApi'
import { useCallback, useEffect, useRef } from 'react'
import { playNeteaseSong } from '../../../../utils/embedProcessor'

export interface UseContentEventsOptions {
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
  t: Record<string, any>
}

export interface UseContentEventsReturn {
  handleTooltipMouseEnter: () => void
  handleTooltipMouseLeave: () => void
}

export function useContentEvents({
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

  useEffect(() => {
    return () => clearTooltipHideTimer()
  }, [clearTooltipHideTimer])

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

  return {
    handleTooltipMouseEnter,
    handleTooltipMouseLeave,
  }
}
