/**
 * 阅读器 Tooltip 组件集合
 * 包含: AI注释Tooltip、评论Tooltip、评论输入弹窗
 */

import type { MouseEvent } from 'react'

import type { CommentItem } from '../../../services/brewApi'
import type { AnnotationType } from '../../../services/brewliaApi'
import type { ThemeConfig } from './types'
import {
  LuCheck as Check,
  LuCopy as Copy,
  LuMessageSquare as MessageSquare,
  LuSend as Send,
} from '@lib/icons'

import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useCallback, useEffect, useRef, useState } from 'react'
import * as brewliaApi from '../../../services/brewliaApi'
import { Spinner } from '../../Spinner'
import { DATE_FORMAT_SHORT } from './constants'

// ================== AI 注释 Tooltip ==================

interface AnnotationTooltipProps {
  hoveredAnnotation: {
    term: string
    explanation: string
    type: AnnotationType
  } | null
  tooltipPosition: { x: number; y: number }
  currentTheme: ThemeConfig
  isDark: boolean
  enableAnimations: boolean
  t: Record<string, any>
}

export function AnnotationTooltip({
  hoveredAnnotation,
  tooltipPosition,
  currentTheme,
  isDark,
  enableAnimations,
  t,
}: AnnotationTooltipProps) {
  return (
    <AnimatePresence>
      {hoveredAnnotation && (
        <motion.div
          initial={
            enableAnimations
              ? { opacity: 0, x: '-50%', y: 'calc(-100% + 8px)', scale: 0.96 }
              : false
          }
          animate={
            enableAnimations
              ? { opacity: 1, x: '-50%', y: '-100%', scale: 1 }
              : undefined
          }
          exit={
            enableAnimations
              ? { opacity: 0, x: '-50%', y: 'calc(-100% + 8px)', scale: 0.96 }
              : undefined
          }
          transition={
            enableAnimations
              ? { duration: 0.15, ease: [0.22, 1, 0.36, 1] }
              : undefined
          }
          className={`fixed z-50 max-w-xs w-max min-w-0 px-3 py-2.5 rounded-xl shadow-xl border ${currentTheme.border} ${currentTheme.surfaceSolid}`}
          style={{
            left: Math.max(
              16,
              Math.min(tooltipPosition.x, window.innerWidth - 320),
            ),
            top: Math.max(16, tooltipPosition.y),
            x: '-50%',
            y: '-100%',
            pointerEvents: 'none' as const,
            // 长词 / URL 编码串不得撑破视口
            maxWidth: 'min(20rem, calc(100vw - 2rem))',
          }}
        >
          <div className="flex items-center gap-2 mb-1.5 min-w-0">
            <span
              className={`text-xs px-1.5 py-0.5 rounded shrink-0 whitespace-nowrap ${
                brewliaApi.ANNOTATION_TYPE_CONFIG[hoveredAnnotation.type]
                  ?.bgColor || 'bg-gray-100'
              } ${
                brewliaApi.ANNOTATION_TYPE_CONFIG[hoveredAnnotation.type]
                  ?.color || 'text-gray-600'
              }`}
            >
              {/* 只显示完整类型名，不要 label[0] + label 造成「背 背景」 */}
              {brewliaApi.ANNOTATION_TYPE_CONFIG[hoveredAnnotation.type]
                ?.label || t.brew.annotationFallback}
            </span>
            <span
              className={`text-sm font-medium ${currentTheme.text} min-w-0 flex-1 truncate`}
            >
              {hoveredAnnotation.term}
            </span>
          </div>
          <p
            className={`text-xs ${currentTheme.secondary} leading-relaxed break-words [overflow-wrap:anywhere]`}
          >
            {hoveredAnnotation.explanation}
          </p>
          {/* 小三角指示器 */}
          <div
            className="absolute left-1/2 -translate-x-1/2 bottom-0 translate-y-full w-0 h-0 border-l-6 border-r-6 border-t-6 border-transparent"
            style={{
              borderTopColor: isDark ? '#242424' : '#fff9f0',
            }}
          />
        </motion.div>
      )}
    </AnimatePresence>
  )
}

// ================== 评论 Tooltip ==================

interface CommentTooltipProps {
  commentTooltip: { comment: CommentItem; x: number; y: number } | null
  currentTheme: ThemeConfig
  isDark: boolean
  enableAnimations: boolean
  t: Record<string, any>
  onMouseEnter?: () => void
  onMouseLeave?: () => void
}

export function CommentTooltip({
  commentTooltip,
  currentTheme,
  isDark,
  enableAnimations,
  t,
  onMouseEnter,
  onMouseLeave,
}: CommentTooltipProps) {
  return (
    <AnimatePresence>
      {commentTooltip && (
        <motion.div
          onMouseEnter={onMouseEnter}
          onMouseLeave={onMouseLeave}
          initial={
            enableAnimations
              ? { opacity: 0, x: '-50%', y: 'calc(-100% + 10px)', scale: 0.95 }
              : false
          }
          animate={
            enableAnimations
              ? { opacity: 1, x: '-50%', y: '-100%', scale: 1 }
              : undefined
          }
          exit={
            enableAnimations
              ? { opacity: 0, x: '-50%', y: 'calc(-100% + 10px)', scale: 0.95 }
              : undefined
          }
          transition={
            enableAnimations
              ? { duration: 0.2, ease: [0.22, 1, 0.36, 1] }
              : undefined
          }
          className={`comment-tooltip fixed z-80 max-w-xs rounded-lg shadow-xl border ${currentTheme.border} ${currentTheme.surfaceSolid}`}
          style={{
            left: `${Math.max(16, Math.min(commentTooltip.x, window.innerWidth - 260))}px`,
            top: `${Math.max(16, commentTooltip.y - 12)}px`,
            x: '-50%',
            y: '-100%',
          }}
        >
          {/* 用户信息和时间 - 次要信息 */}
          <div
            className={`flex items-center gap-2 px-3 pt-2.5 pb-1.5 ${currentTheme.secondary}`}
          >
            {commentTooltip.comment.user_avatar ? (
              <img
                src={commentTooltip.comment.user_avatar}
                alt=""
                className="w-4 h-4 rounded-full opacity-80"
              />
            ) : (
              <div
                className={`w-4 h-4 rounded-full ${isDark ? 'bg-white/20' : 'bg-black/10'}`}
              />
            )}
            <span className="text-xs">
              {commentTooltip.comment.user_display_name ||
                commentTooltip.comment.user_name ||
                t.brew.anonymousUser}
            </span>
            <span className="text-xs opacity-60">·</span>
            <span className="text-xs opacity-60">
              {new Date(commentTooltip.comment.created_at).toLocaleDateString(
                'zh-CN',
                DATE_FORMAT_SHORT,
              )}
            </span>
          </div>
          {/* 评论内容 - 主要信息 */}
          <div className="px-3 pb-3">
            <p className={`text-sm leading-relaxed ${currentTheme.text}`}>
              {commentTooltip.comment.comment}
            </p>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

// ================== 评论输入弹窗 ==================

interface CommentInputPopupProps {
  showCommentPopup: boolean
  setShowCommentPopup: (show: boolean) => void
  commentPopupPosition: { x: number; y: number }
  selectedText: string
  setSelectedText: (text: string) => void
  commentInput: string
  setCommentInput: (input: string) => void
  commentSubmitting: boolean
  submitComment: () => void
  currentTheme: ThemeConfig
  isDark: boolean
  enableAnimations: boolean
  t: Record<string, any>
}

export function CommentInputPopup({
  showCommentPopup,
  setShowCommentPopup,
  commentPopupPosition,
  selectedText,
  setSelectedText,
  commentInput,
  setCommentInput,
  commentSubmitting,
  submitComment,
  currentTheme,
  isDark,
  enableAnimations,
  t,
}: CommentInputPopupProps) {
  // 内部状态：是否显示评论输入框
  const [showCommentInput, setShowCommentInput] = useState(false)
  // 复制成功反馈
  const [copySuccess, setCopySuccess] = useState(false)
  // 复制成功后延迟关闭的定时器
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // 当弹窗关闭时，重置内部状态
  useEffect(() => {
    if (!showCommentPopup) {
      setShowCommentInput(false)
      setCopySuccess(false)
    }
  }, [showCommentPopup])

  // 清理复制定时器
  useEffect(() => {
    return () => {
      if (copyTimerRef.current) {
        clearTimeout(copyTimerRef.current)
      }
    }
  }, [])

  // 复制选中文本
  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(selectedText)
      setCopySuccess(true)
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current)
      copyTimerRef.current = setTimeout(() => {
        copyTimerRef.current = null
        setCopySuccess(false)
        // 复制后关闭弹窗
        setShowCommentPopup(false)
        setSelectedText('')
      }, 800)
    } catch (err) {
      console.error('Failed to copy:', err)
    }
  }

  // 点击评论按钮
  const handleCommentClick = () => {
    setShowCommentInput(true)
  }

  // 关闭弹窗时重置状态
  const handleClose = () => {
    setShowCommentPopup(false)
    setSelectedText('')
    setCommentInput('')
    setShowCommentInput(false)
  }

  // 提交评论后重置
  const handleSubmit = () => {
    submitComment()
    // submitComment 内部会重置状态，这里重置本地状态
    setShowCommentInput(false)
  }

  // 防止点击按钮时清除浏览器的文本选中状态
  const preventSelectionClear = useCallback((e: MouseEvent) => {
    e.preventDefault()
  }, [])

  return (
    <AnimatePresence>
      {showCommentPopup && (
        <motion.div
          initial={
            enableAnimations ? { opacity: 0, scale: 0.92, y: 12 } : false
          }
          animate={
            enableAnimations ? { opacity: 1, scale: 1, y: 0 } : undefined
          }
          exit={
            enableAnimations ? { opacity: 0, scale: 0.92, y: 12 } : undefined
          }
          transition={
            enableAnimations
              ? { duration: 0.25, ease: [0.16, 1, 0.3, 1] }
              : undefined
          }
          className={`comment-popup fixed z-70 rounded-xl shadow-2xl border ${currentTheme.border} ${currentTheme.surfaceSolid}`}
          style={{
            left: Math.max(
              16,
              Math.min(
                commentPopupPosition.x - (showCommentInput ? 144 : 60),
                window.innerWidth - (showCommentInput ? 304 : 140),
              ),
            ),
            top: Math.max(
              16,
              commentPopupPosition.y - (showCommentInput ? 180 : 90),
            ),
          }}
          onClick={(e: MouseEvent) => e.stopPropagation()}
        >
          {!showCommentInput ? (
            /* 第一步：显示复制和评论按钮（上下布局） */
            <div className="flex flex-col p-1">
              <button
                onMouseDown={preventSelectionClear}
                onClick={handleCopy}
                className={`flex items-center gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors ${
                  copySuccess
                    ? 'bg-green-500 text-white'
                    : `${currentTheme.text} hover:bg-black/5 dark:hover:bg-white/10`
                }`}
              >
                {copySuccess ? (
                  <>
                    <Check className="w-4 h-4" />
                    <span>{t.brew.copied || '已复制'}</span>
                  </>
                ) : (
                  <>
                    <Copy className="w-4 h-4" />
                    <span>{t.brew.copy || '复制'}</span>
                  </>
                )}
              </button>
              <button
                onMouseDown={preventSelectionClear}
                onClick={handleCommentClick}
                className={`flex items-center gap-2 px-3 py-2 rounded-lg text-sm font-medium ${currentTheme.text} hover:bg-black/5 dark:hover:bg-white/10 transition-colors`}
              >
                <MessageSquare className="w-4 h-4" />
                <span>{t.brew.comment || '评论'}</span>
              </button>
            </div>
          ) : (
            /* 第二步：显示评论输入框 */
            <div className="w-72">
              {/* 选中的文本预览 */}
              <div
                className={`px-3 py-2 border-b ${currentTheme.border} ${isDark ? 'bg-white/5' : 'bg-black/5'} rounded-t-xl`}
              >
                <p className={`text-xs ${currentTheme.secondary} mb-1`}>
                  {t.brew.selectedText}
                </p>
                <p
                  className={`text-sm ${currentTheme.text} line-clamp-2 italic`}
                >
                  "{selectedText}"
                </p>
              </div>

              {/* 评论输入区 */}
              <div className="p-3">
                <textarea
                  value={commentInput}
                  onChange={(e) => setCommentInput(e.target.value)}
                  placeholder={t.brew.writeYourThoughts}
                  className={`w-full h-20 px-3 py-2 text-sm rounded-lg border ${currentTheme.border} ${currentTheme.bg} ${currentTheme.text} placeholder:${currentTheme.secondary} resize-none focus:outline-none focus:ring-2 focus:ring-amber-500/50`}
                  autoFocus
                  maxLength={500}
                />

                {/* 操作按钮 */}
                <div className="flex items-center justify-between mt-2">
                  <span className={`text-xs ${currentTheme.secondary}`}>
                    {commentInput.length}
                    /500
                  </span>
                  <div className="flex gap-2">
                    <button
                      onClick={handleClose}
                      className={`px-3 py-1.5 text-xs rounded-lg ${currentTheme.secondary} hover:${currentTheme.text} transition-colors`}
                    >
                      {t.brew.cancelAction}
                    </button>
                    <button
                      onClick={handleSubmit}
                      disabled={!commentInput.trim() || commentSubmitting}
                      className="px-3 py-1.5 text-xs rounded-lg bg-amber-500 text-white font-medium hover:bg-amber-600 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1"
                    >
                      {commentSubmitting ? (
                        <>
                          <Spinner size="xs" color="white" />
                          {t.brew.saving}
                        </>
                      ) : (
                        <>
                          <Send className="w-3 h-3" />
                          {t.brew.addComment}
                        </>
                      )}
                    </button>
                  </div>
                </div>
              </div>
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  )
}
