import type {
  CommentItem,
  CreateCommentRequest,
} from '../../../../services/brewApi'
import type { ReaderCopy, ThemeKey } from '../types'
import { useCallback, useEffect, useRef, useState } from 'react'
import * as brewApi from '../../../../services/brewApi'
import { userFacingError } from '../../../../utils/userFacingError'
import { RequestTurn } from '../../logic/requestTurn'
import { highlightAnchoredComments } from '../commentAnchors'
import { useArticleTaskScope } from './useArticleTaskScope'

export interface UseCommentsOptions {
  itemId: number
  isAuthenticated: boolean
  showToastMessage: (message: string, duration?: number) => void
  t: ReaderCopy
}

export interface SelectionRange {
  start: number
  end: number
  contextBefore: string
  contextAfter: string
}

export interface UseCommentsReturn {
  comments: CommentItem[]
  commentsLoading: boolean
  hasComments: boolean

  showCommentPopup: boolean
  commentPopupPosition: { x: number; y: number }
  selectedText: string
  selectionRange: SelectionRange | null
  commentInput: string
  commentSubmitting: boolean

  showCommentsPanel: boolean

  replyingTo: CommentItem | null
  replyInput: string
  replySubmitting: boolean
  expandedComments: Set<number>
  commentReplies: Record<number, CommentItem[]>

  commentTooltip: { comment: CommentItem; x: number; y: number } | null

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

  highlightComments: (
    html: string,
    commentList: CommentItem[],
    theme: ThemeKey,
  ) => string

  commentsLoadingRef: React.RefObject<boolean>
}

export function useComments({
  itemId,
  isAuthenticated,
  showToastMessage,
  t,
}: UseCommentsOptions): UseCommentsReturn {
  const captureTask = useArticleTaskScope(itemId)
  const turns = useRef(new RequestTurn())
  const itemAbort = useRef(new AbortController())
  useEffect(() => {
    const controller = new AbortController()
    itemAbort.current = controller
    return () => {
      controller.abort()
      turns.current.cancel()
    }
  }, [itemId])
  const [comments, setComments] = useState<CommentItem[]>([])
  const [commentsLoading, setCommentsLoading] = useState(false)
  const [hasComments, setHasComments] = useState(false)

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

  const [showCommentsPanel, setShowCommentsPanel] = useState(false)

  const [replyingTo, setReplyingTo] = useState<CommentItem | null>(null)
  const [replyInput, setReplyInput] = useState('')
  const [replySubmitting, setReplySubmitting] = useState(false)
  const [expandedComments, setExpandedComments] = useState<Set<number>>(
    new Set(),
  )
  const [commentReplies, setCommentReplies] = useState<
    Record<number, CommentItem[]>
  >({})

  const [commentTooltip, setCommentTooltip] = useState<{
    comment: CommentItem
    x: number
    y: number
  } | null>(null)

  const commentsLoadingRef = useRef(false)

  const loadComments = useCallback(async () => {
    if (!isAuthenticated || commentsLoadingRef.current) return

    const isCurrent = captureTask()
    const signal = turns.current.begin()
    commentsLoadingRef.current = true
    setCommentsLoading(true)
    try {
      const response = await brewApi.getComments(itemId, undefined, { signal })
      if (!isCurrent() || signal.aborted) return
      if (response.success && response.comments) {
        setComments(response.comments)
        setHasComments(response.comments.length > 0)
      }
    } catch (err) {
      if (!isCurrent() || signal.aborted) return
      console.error('Failed to load comments:', err)
      showToastMessage(userFacingError(err, t.errors.commentLoadFailed))
    } finally {
      commentsLoadingRef.current = false
      if (isCurrent() && !signal.aborted) setCommentsLoading(false)
    }
  }, [captureTask, isAuthenticated, itemId, showToastMessage, t.errors.commentLoadFailed])

  const submitComment = useCallback(async () => {
    if (
      !isAuthenticated ||
      !selectedText ||
      !commentInput.trim() ||
      commentSubmitting
    ) {
      return
    }

    const isCurrent = captureTask()
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
      if (!isCurrent()) return
      if (response.success && response.comment) {
        setComments((prev) => [...prev, response.comment!])
        setHasComments(true)
        showToastMessage(t.brew.commentAdded)

        setShowCommentPopup(false)
        setSelectedText('')
        setSelectionRange(null)
        setCommentInput('')
      }
    } catch (err) {
      if (!isCurrent()) return
      console.error('Failed to submit comment:', err)
      showToastMessage(userFacingError(err, t.brew.addCommentFailed))
    } finally {
      if (isCurrent()) setCommentSubmitting(false)
    }
  }, [
    captureTask,
    isAuthenticated,
    selectedText,
    commentInput,
    commentSubmitting,
    selectionRange,
    itemId,
    showToastMessage,
    t,
  ])

  const deleteComment = useCallback(
    async (commentId: number) => {
      const isCurrent = captureTask()
      try {
        const response = await brewApi.deleteComment(commentId)
        if (!isCurrent()) return
        if (response.success) {
          setComments((prev) => prev.filter((c) => c.id !== commentId))
          setHasComments(comments.length > 1)
          showToastMessage(t.brew.commentDeleted)
        }
      } catch (err) {
        if (!isCurrent()) return
        console.error('Failed to delete comment:', err)
        showToastMessage(userFacingError(err, t.errors?.commentDeleteFailed))
      }
    },
    [captureTask, comments.length, showToastMessage, t],
  )

  const loadReplies = useCallback(async (commentId: number) => {
    const isCurrent = captureTask()
    const signal = itemAbort.current.signal
    try {
      const response = await brewApi.getCommentReplies(commentId, undefined, {
        signal,
      })
      if (!isCurrent() || signal.aborted) return
      if (response.success) {
        setCommentReplies((prev) => ({
          ...prev,
          [commentId]: response.replies,
        }))
      }
    } catch (err) {
      if (!isCurrent() || signal.aborted) return
      console.error('Failed to load replies:', err)
      showToastMessage(userFacingError(err, t.errors.commentRepliesLoadFailed))
    }
  }, [captureTask, showToastMessage, t.errors.commentRepliesLoadFailed])

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

  const submitReply = useCallback(async () => {
    if (
      !isAuthenticated ||
      !replyingTo ||
      !replyInput.trim() ||
      replySubmitting
    ) {
      return
    }

    const isCurrent = captureTask()
    setReplySubmitting(true)
    try {
      const topLevelCommentId = replyingTo.parent_id || replyingTo.id
      const response = await brewApi.createReply(
        itemId,
        topLevelCommentId,
        replyInput.trim(),
      )
      if (!isCurrent()) return

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
      if (!isCurrent()) return
      console.error('Failed to submit reply:', err)
      showToastMessage(t.brew.addReplyFailed)
    } finally {
      if (isCurrent()) setReplySubmitting(false)
    }
  }, [
    captureTask,
    isAuthenticated,
    replyingTo,
    replyInput,
    replySubmitting,
    itemId,
    showToastMessage,
    t,
  ])

  const highlightComments = highlightAnchoredComments

  return {
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
    loadReplies,
    toggleReplies,
    submitReply,

    highlightComments,

    commentsLoadingRef,
  }
}
