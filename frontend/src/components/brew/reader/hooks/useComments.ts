/**
 * 用户评论 Hook
 * 管理评论的加载、创建、删除、回复等功能
 */

import type {
  CommentItem,
  CreateCommentRequest,
} from '../../../../services/brewApi'
import type { ThemeKey } from '../types'
import { useCallback, useRef, useState } from 'react'
import * as brewApi from '../../../../services/brewApi'
import { userFacingError } from '../../../../utils/userFacingError'

export interface UseCommentsOptions {
  itemId: number
  isAuthenticated: boolean
  showToastMessage: (message: string, duration?: number) => void
  t: Record<string, any>
}

export interface SelectionRange {
  start: number
  end: number
  contextBefore: string
  contextAfter: string
}

export interface UseCommentsReturn {
  // 评论列表状态
  comments: CommentItem[]
  commentsLoading: boolean
  hasComments: boolean

  // 评论弹窗状态
  showCommentPopup: boolean
  commentPopupPosition: { x: number; y: number }
  selectedText: string
  selectionRange: SelectionRange | null
  commentInput: string
  commentSubmitting: boolean

  // 评论面板状态
  showCommentsPanel: boolean

  // 回复状态
  replyingTo: CommentItem | null
  replyInput: string
  replySubmitting: boolean
  expandedComments: Set<number>
  commentReplies: Record<number, CommentItem[]>

  // 评论 tooltip
  commentTooltip: { comment: CommentItem; x: number; y: number } | null

  // 操作
  setComments: (comments: CommentItem[]) => void
  setShowCommentPopup: (show: boolean) => void
  setCommentPopupPosition: (position: { x: number; y: number }) => void
  setSelectedText: (text: string) => void
  setSelectionRange: (range: SelectionRange | null) => void
  setCommentInput: (input: string) => void
  setShowCommentsPanel: (show: boolean) => void
  setReplyingTo: (comment: CommentItem | null) => void
  setReplyInput: (input: string) => void
  setCommentTooltip: (
    tooltip: { comment: CommentItem; x: number; y: number } | null,
  ) => void

  loadComments: () => Promise<void>
  submitComment: () => Promise<void>
  deleteComment: (commentId: number) => Promise<void>
  loadReplies: (commentId: number) => Promise<void>
  toggleReplies: (commentId: number) => Promise<void>
  submitReply: () => Promise<void>

  // 高亮函数
  highlightComments: (
    html: string,
    commentList: CommentItem[],
    theme: ThemeKey,
  ) => string

  // Refs
  commentsLoadingRef: React.RefObject<boolean>
}

export function useComments({
  itemId,
  isAuthenticated,
  showToastMessage,
  t,
}: UseCommentsOptions): UseCommentsReturn {
  // 评论列表状态
  const [comments, setComments] = useState<CommentItem[]>([])
  const [commentsLoading, setCommentsLoading] = useState(false)
  const [hasComments, setHasComments] = useState(false)

  // 评论弹窗状态
  const [showCommentPopup, setShowCommentPopup] = useState(false)
  const [commentPopupPosition, setCommentPopupPosition] = useState({
    x: 0,
    y: 0,
  })
  const [selectedText, setSelectedText] = useState('')
  const [selectionRange, setSelectionRange] = useState<SelectionRange | null>(
    null,
  )
  const [commentInput, setCommentInput] = useState('')
  const [commentSubmitting, setCommentSubmitting] = useState(false)

  // 评论面板状态
  const [showCommentsPanel, setShowCommentsPanel] = useState(false)

  // 回复状态
  const [replyingTo, setReplyingTo] = useState<CommentItem | null>(null)
  const [replyInput, setReplyInput] = useState('')
  const [replySubmitting, setReplySubmitting] = useState(false)
  const [expandedComments, setExpandedComments] = useState<Set<number>>(
    new Set(),
  )
  const [commentReplies, setCommentReplies] = useState<
    Record<number, CommentItem[]>
  >({})

  // 评论 tooltip
  const [commentTooltip, setCommentTooltip] = useState<{
    comment: CommentItem
    x: number
    y: number
  } | null>(null)

  // Refs
  const commentsLoadingRef = useRef(false)

  // 加载评论
  const loadComments = useCallback(async () => {
    if (!isAuthenticated || commentsLoadingRef.current) return

    commentsLoadingRef.current = true
    setCommentsLoading(true)
    try {
      const response = await brewApi.getComments(itemId)
      if (response.success && response.comments) {
        setComments(response.comments)
        setHasComments(response.comments.length > 0)
      }
    } catch (err) {
      console.error('Failed to load comments:', err)
      showToastMessage(userFacingError(err, t.errors.commentLoadFailed))
    } finally {
      setCommentsLoading(false)
      commentsLoadingRef.current = false
    }
  }, [isAuthenticated, itemId, showToastMessage, t.errors.commentLoadFailed])

  // 提交评论
  const submitComment = useCallback(async () => {
    if (
      !isAuthenticated ||
      !selectedText ||
      !commentInput.trim() ||
      commentSubmitting
    ) {
      return
    }

    setCommentSubmitting(true)
    try {
      const request: CreateCommentRequest = {
        selected_text: selectedText,
        comment: commentInput.trim(),
        start_offset: selectionRange?.start ?? 0,
        end_offset: selectionRange?.end ?? 0,
        context_before: selectionRange?.contextBefore ?? '',
        context_after: selectionRange?.contextAfter ?? '',
        color: '#fef08a',
        is_public: false,
      }

      const response = await brewApi.createComment(itemId, request)
      if (response.success && response.comment) {
        setComments((prev) => [...prev, response.comment!])
        setHasComments(true)
        showToastMessage(t.brew.commentAdded)

        // 清理状态
        setShowCommentPopup(false)
        setSelectedText('')
        setSelectionRange(null)
        setCommentInput('')
      }
    } catch (err) {
      console.error('Failed to submit comment:', err)
      showToastMessage(userFacingError(err, t.brew.addCommentFailed))
    } finally {
      setCommentSubmitting(false)
    }
  }, [
    isAuthenticated,
    selectedText,
    commentInput,
    commentSubmitting,
    selectionRange,
    itemId,
    showToastMessage,
    t,
  ])

  // 删除评论
  const deleteComment = useCallback(
    async (commentId: number) => {
      try {
        const response = await brewApi.deleteComment(commentId)
        if (response.success) {
          setComments((prev) => prev.filter((c) => c.id !== commentId))
          setHasComments(comments.length > 1)
          showToastMessage(t.brew.commentDeleted)
        }
      } catch (err) {
        console.error('Failed to delete comment:', err)
        showToastMessage(userFacingError(err, t.errors?.commentDeleteFailed))
      }
    },
    [comments.length, showToastMessage, t],
  )

  // 加载回复
  const loadReplies = useCallback(async (commentId: number) => {
    try {
      const response = await brewApi.getCommentReplies(commentId)
      if (response.success) {
        setCommentReplies((prev) => ({
          ...prev,
          [commentId]: response.replies,
        }))
      }
    } catch (err) {
      console.error('Failed to load replies:', err)
      showToastMessage(userFacingError(err, t.errors.commentRepliesLoadFailed))
    }
  }, [showToastMessage, t.errors.commentRepliesLoadFailed])

  // 展开/收起回复
  const toggleReplies = useCallback(
    async (commentId: number) => {
      const isExpanded = expandedComments.has(commentId)
      if (isExpanded) {
        setExpandedComments((prev) => {
          const next = new Set(prev)
          next.delete(commentId)
          return next
        })
      } else {
        setExpandedComments((prev) => new Set(prev).add(commentId))
        if (!commentReplies[commentId]) {
          await loadReplies(commentId)
        }
      }
    },
    [expandedComments, commentReplies, loadReplies],
  )

  // 提交回复
  const submitReply = useCallback(async () => {
    if (
      !isAuthenticated ||
      !replyingTo ||
      !replyInput.trim() ||
      replySubmitting
    ) {
      return
    }

    setReplySubmitting(true)
    try {
      const topLevelCommentId = replyingTo.parent_id || replyingTo.id
      const response = await brewApi.createReply(
        itemId,
        topLevelCommentId,
        replyInput.trim(),
      )

      if (response.success && response.comment) {
        setCommentReplies((prev) => ({
          ...prev,
          [topLevelCommentId]: [
            ...(prev[topLevelCommentId] || []),
            response.comment,
          ],
        }))
        setComments((prev) =>
          prev.map((c) =>
            c.id === topLevelCommentId
              ? { ...c, reply_count: (c.reply_count || 0) + 1 }
              : c,
          ),
        )
        setExpandedComments((prev) => new Set(prev).add(topLevelCommentId))

        showToastMessage(t.brew.replyAdded)
        setReplyingTo(null)
        setReplyInput('')
      } else {
        console.error('[Reply] Failed:', response.error)
        showToastMessage(response.error || t.brew.addReplyFailed)
      }
    } catch (err) {
      console.error('Failed to submit reply:', err)
      showToastMessage(t.brew.addReplyFailed)
    } finally {
      setReplySubmitting(false)
    }
  }, [
    isAuthenticated,
    replyingTo,
    replyInput,
    replySubmitting,
    itemId,
    showToastMessage,
    t,
  ])

  // 高亮评论（适配主题，豁免嵌入卡片）
  const highlightComments = useCallback(
    (html: string, commentList: CommentItem[], theme: ThemeKey): string => {
      if (!commentList.length) return html

      // 提取并保存需要豁免的嵌入卡片
      const exemptElements: { placeholder: string; content: string }[] = []
      let result = html

      const exemptRegex =
        /<[^>]*data-embed-exempt="true"[^>]*>[\s\S]*?<\/[^>]+>/gi
      result = result.replace(exemptRegex, (match) => {
        const placeholder = `___EXEMPT_EMBED_${exemptElements.length}___`
        exemptElements.push({ placeholder, content: match })
        return placeholder
      })

      // 按长度降序排序
      const sortedComments = [...commentList].sort(
        (a, b) => b.selected_text.length - a.selected_text.length,
      )

      // 主题颜色
      const defaultColors: Record<ThemeKey, string> = {
        light: '#fef08a',
        sepia: '#f5d78e',
        dark: '#854d0e',
        night: '#1e3a5f',
      }
      const borderColors: Record<ThemeKey, string> = {
        light: '#eab308',
        sepia: '#ca8a04',
        dark: '#fbbf24',
        night: '#3b82f6',
      }
      const defaultColor = defaultColors[theme] || '#fef08a'
      const borderColor = borderColors[theme] || '#eab308'

      const isValidColor = (color: string): boolean => {
        return /^#([0-9A-F]{3}|[0-9A-F]{6}|[0-9A-F]{8})$/i.test(color)
      }

      // 高亮处理
      for (const comment of sortedComments) {
        if (!comment.selected_text) continue

        const escapedText = comment.selected_text.replace(
          /[.*+?^${}()|[\]\\]/g,
          '\\$&',
        )
        const regex = new RegExp(`(?<!<[^>]*)${escapedText}(?![^<]*>)`, 'g')

        const bgColor =
          comment.color && isValidColor(comment.color)
            ? comment.color
            : defaultColor
        const underlineColor =
          comment.color && isValidColor(comment.color)
            ? comment.color
            : borderColor

        // id 声明为 number，但这里是拼进 HTML 属性的裸插值 —— 用 Number()
        // 把"后端某天返回字符串"这一类失误挡在属性注入之外。
        const safeCommentId = Number(comment.id)
        if (!Number.isFinite(safeCommentId)) continue

        result = result.replace(
          regex,
          (match) =>
            `<mark class="user-comment-highlight" data-comment-id="${safeCommentId}" style="background-color: ${bgColor}40; cursor: pointer; border-radius: 2px; padding: 0 2px; border-bottom: 2px solid ${underlineColor};">${match}</mark>`,
        )
      }

      // 还原豁免的嵌入卡片
      for (const { placeholder, content } of exemptElements) {
        result = result.replace(placeholder, content)
      }

      return result
    },
    [],
  )

  return {
    // 评论列表状态
    comments,
    commentsLoading,
    hasComments,

    // 评论弹窗状态
    showCommentPopup,
    commentPopupPosition,
    selectedText,
    selectionRange,
    commentInput,
    commentSubmitting,

    // 评论面板状态
    showCommentsPanel,

    // 回复状态
    replyingTo,
    replyInput,
    replySubmitting,
    expandedComments,
    commentReplies,

    // 评论 tooltip
    commentTooltip,

    // 操作
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
    loadReplies,
    toggleReplies,
    submitReply,

    highlightComments,

    commentsLoadingRef,
  }
}
