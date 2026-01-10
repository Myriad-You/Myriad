/**
 * 阅读器评论面板组件
 * 包含: 评论列表、回复功能、删除功能
 */

import type { CommentItem } from '../../../services/brewApi'
import type { ThemeConfig } from './types'
import {
  LuChevronDown as ChevronDown,
  LuChevronUp as ChevronUp,
  LuMessageSquare as MessageSquare,
  LuReply as Reply,
  LuSend as Send,
  LuTrash2 as Trash2,
  LuX as X,
} from '@lib/icons'
import { AnimatePresence, motion } from 'framer-motion'
import {
  DATE_FORMAT_FULL,
  STYLE_MAX_HEIGHT_60VH,
} from './constants'

interface CommentsListPanelProps {
  // 主题
  currentTheme: ThemeConfig
  isDark: boolean

  // 面板控制
  showCommentsPanel: boolean
  setShowCommentsPanel: (show: boolean) => void

  // 评论数据
  comments: CommentItem[]
  commentsLoading: boolean

  // 回复状态
  replyingTo: CommentItem | null
  setReplyingTo: (comment: CommentItem | null) => void
  replyInput: string
  setReplyInput: (input: string) => void
  replySubmitting: boolean
  submitReply: () => void

  // 展开/收起回复
  expandedComments: Set<number>
  toggleReplies: (commentId: number) => void
  commentReplies: Record<number, CommentItem[]>

  // 删除
  deleteComment: (commentId: number) => void

  // 动画
  enableAnimations: boolean

  // 翻译
  t: Record<string, any>
}

export default function CommentsListPanel({
  currentTheme,
  isDark,
  showCommentsPanel,
  setShowCommentsPanel,
  comments,
  commentsLoading,
  replyingTo,
  setReplyingTo,
  replyInput,
  setReplyInput,
  replySubmitting,
  submitReply,
  expandedComments,
  toggleReplies,
  commentReplies,
  deleteComment,
  enableAnimations,
  t,
}: CommentsListPanelProps) {
  return (
    <AnimatePresence>
      {showCommentsPanel && (
        <>
          {/* 点击空白处关闭的遮罩层 */}
          <motion.div
            initial={enableAnimations ? { opacity: 0 } : false}
            animate={enableAnimations ? { opacity: 1 } : undefined}
            exit={enableAnimations ? { opacity: 0 } : undefined}
            transition={enableAnimations ? { duration: 0.2 } : undefined}
            className="fixed inset-0 z-[59] bg-black/20 backdrop-blur-[1px]"
            onClick={() => setShowCommentsPanel(false)}
          />
          <motion.div
            initial={enableAnimations ? { opacity: 0, y: -24, height: 0 } : false}
            animate={enableAnimations ? { opacity: 1, y: 0, height: 'auto' } : undefined}
            exit={enableAnimations ? { opacity: 0, y: -24, height: 0 } : undefined}
            transition={enableAnimations ? { duration: 0.3, ease: [0.16, 1, 0.3, 1] } : undefined}
            className={`fixed left-0 right-0 top-0 z-[60] shadow-2xl border-b ${currentTheme.border} ${currentTheme.surfaceSolid} overflow-hidden`}
            style={STYLE_MAX_HEIGHT_60VH}
          >
            {/* 面板头部 */}
            <div className={`flex items-center justify-between px-6 py-3 border-b ${currentTheme.border}`}>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => setShowCommentsPanel(false)}
                  className={`p-1 rounded-lg ${currentTheme.secondary} hover:${currentTheme.text} transition-all duration-200 ease-out`}
                  title={t.brew.closeCommentPanel}
                >
                  <X className="w-4 h-4" />
                </button>
                <MessageSquare className={`w-5 h-5 ${currentTheme.accent}`} />
                <h3 className={`font-medium ${currentTheme.text}`}>{t.brew.myComments}</h3>
                <span className={`text-xs px-1.5 py-0.5 rounded ${isDark ? 'bg-white/10' : 'bg-black/10'} ${currentTheme.secondary}`}>
                  {comments.length}
                </span>
              </div>
            </div>

            {/* 评论列表 - 横向滚动布局 */}
            <div className="overflow-x-auto overflow-y-hidden p-4">
              {commentsLoading ? (
                <div className="flex items-center justify-center py-8">
                  <div className="w-6 h-6 border-2 border-amber-500/30 border-t-amber-500 rounded-full animate-spin" />
                </div>
              ) : comments.length === 0 ? (
                <div className={`text-center py-8 ${currentTheme.secondary}`}>
                  <MessageSquare className="w-12 h-12 mx-auto mb-3 opacity-30" />
                  <p className="text-sm">{t.brew.noComments}</p>
                  <p className="text-xs mt-1 opacity-70">{t.brew.selectTextToAddComment}</p>
                </div>
              ) : (
                <div className="flex gap-4 pb-2">
                  {comments.map(comment => (
                    <div
                      key={comment.id}
                      data-panel-comment-id={comment.id}
                      className={`flex-shrink-0 w-80 rounded-xl border ${currentTheme.border} ${isDark ? 'bg-white/5' : 'bg-black/[0.02]'} group overflow-hidden`}
                    >
                      {/* 选中的原文 */}
                      {comment.selected_text && (
                        <div className={`px-4 py-2.5 ${isDark ? 'bg-white/5' : 'bg-black/[0.03]'} border-b ${currentTheme.border}`}>
                          <p className={`text-xs ${currentTheme.secondary} mb-1`}>{t.brew.originalExcerpt}</p>
                          <p className={`text-sm ${currentTheme.text} line-clamp-2 leading-relaxed`}>
                            "
                            {comment.selected_text}
                            "
                          </p>
                        </div>
                      )}

                      {/* 评论主体 */}
                      <div className="p-4">
                        {/* 用户信息 */}
                        <div className={`flex items-center gap-2 mb-2.5 ${currentTheme.secondary}`}>
                          {comment.user_avatar
                            ? (
                                <img
                                  src={comment.user_avatar}
                                  alt=""
                                  className="w-5 h-5 rounded-full"
                                />
                              )
                            : (
                                <div className={`w-5 h-5 rounded-full ${isDark ? 'bg-white/20' : 'bg-black/10'}`} />
                              )}
                          <span className={`text-xs font-medium ${currentTheme.text}`}>
                            {comment.user_display_name || comment.user_name || t.brew.anonymousUser}
                          </span>
                          <span className="text-xs opacity-50">·</span>
                          <span className="text-xs opacity-70">
                            {new Date(comment.created_at).toLocaleDateString('zh-CN', DATE_FORMAT_FULL)}
                          </span>
                        </div>

                        {/* 评论内容 */}
                        <p className={`text-sm ${currentTheme.text} leading-relaxed`}>{comment.comment}</p>

                        {/* 操作栏 */}
                        <div className="flex items-center justify-between mt-3 pt-3 border-t border-dashed" style={{ borderColor: isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.08)' }}>
                          <div className="flex items-center gap-2">
                            {/* 回复数量和展开按钮 */}
                            {(comment.reply_count || 0) > 0 && (
                              <button
                                onClick={() => toggleReplies(comment.id)}
                                className={`flex items-center gap-1 text-xs px-2 py-1 rounded-md ${currentTheme.secondary} hover:${isDark ? 'bg-white/10' : 'bg-black/5'} transition-all duration-200 ease-out`}
                              >
                                {expandedComments.has(comment.id)
                                  ? (
                                      <ChevronUp className="w-3.5 h-3.5" />
                                    )
                                  : (
                                      <ChevronDown className="w-3.5 h-3.5" />
                                    )}
                                {comment.reply_count}
                                {t.brew.replyCountSuffix}
                              </button>
                            )}
                          </div>
                          <div className="flex items-center gap-1">
                            {/* 回复按钮 */}
                            <button
                              onClick={() => setReplyingTo(comment)}
                              className={`p-1.5 rounded-md ${currentTheme.secondary} hover:${currentTheme.text} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'} transition-all`}
                              title={t.brew.reply}
                            >
                              <Reply className="w-4 h-4" />
                            </button>
                            {/* 删除按钮 */}
                            <button
                              onClick={() => deleteComment(comment.id)}
                              className="p-1.5 rounded-md text-red-500/70 hover:text-red-500 hover:bg-red-500/10 transition-all"
                              title={t.brew.deleteComment}
                            >
                              <Trash2 className="w-4 h-4" />
                            </button>
                          </div>
                        </div>
                      </div>

                      {/* 回复输入框 */}
                      {replyingTo?.id === comment.id && (
                        <div className="px-4 pb-4">
                          <p className={`text-xs ${currentTheme.secondary} mb-1.5`}>
                            {t.brew.replyTo}
                            {' '}
                            @
                            {replyingTo.user_display_name || replyingTo.user_name || t.brew.anonymousUser}
                          </p>
                          <div className="flex items-center gap-2">
                            <input
                              type="text"
                              value={replyInput}
                              onChange={e => setReplyInput(e.target.value)}
                              placeholder={t.brew.writeYourReply}
                              className={`flex-1 px-3 py-1.5 text-sm rounded-lg border ${currentTheme.border} ${currentTheme.bg} ${currentTheme.text} placeholder:${currentTheme.secondary} focus:outline-none focus:ring-2 focus:ring-amber-500/50`}
                              autoFocus
                              onKeyDown={(e) => {
                                if (e.key === 'Enter' && !e.shiftKey) {
                                  e.preventDefault()
                                  submitReply()
                                }
                                else if (e.key === 'Escape') {
                                  setReplyingTo(null)
                                  setReplyInput('')
                                }
                              }}
                            />
                            <button
                              onClick={submitReply}
                              disabled={!replyInput.trim() || replySubmitting}
                              className="flex-shrink-0 w-7 h-7 flex items-center justify-center rounded-lg bg-amber-500 text-white hover:bg-amber-600 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                              title={t.brew.send}
                            >
                              <Send className="w-3.5 h-3.5" />
                            </button>
                            <button
                              onClick={() => {
                                setReplyingTo(null)
                                setReplyInput('')
                              }}
                              className={`flex-shrink-0 w-7 h-7 flex items-center justify-center rounded-lg ${isDark ? 'bg-white/10 hover:bg-white/20' : 'bg-black/5 hover:bg-black/10'} ${currentTheme.secondary} transition-colors`}
                              title={t.brew.cancel}
                            >
                              <X className="w-3.5 h-3.5" />
                            </button>
                          </div>
                        </div>
                      )}

                      {/* 回复列表 */}
                      {expandedComments.has(comment.id) && commentReplies[comment.id] && (
                        <div className="px-4 pb-4 space-y-3 max-h-48 overflow-y-auto">
                          {commentReplies[comment.id].map(reply => (
                            <div
                              key={reply.id}
                              className={`relative pl-3 py-2 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/[0.02]'} group/reply`}
                            >
                              {/* 回复用户信息 */}
                              <div className={`flex items-center gap-2 mb-1.5 ${currentTheme.secondary}`}>
                                {reply.user_avatar
                                  ? (
                                      <img
                                        src={reply.user_avatar}
                                        alt=""
                                        className="w-4 h-4 rounded-full"
                                      />
                                    )
                                  : (
                                      <div className={`w-4 h-4 rounded-full ${isDark ? 'bg-white/20' : 'bg-black/10'}`} />
                                    )}
                                <span className={`text-xs ${currentTheme.text}`}>
                                  {reply.user_display_name || reply.user_name || t.brew.anonymousUser}
                                </span>
                                <span className="text-xs opacity-50">·</span>
                                <span className="text-xs opacity-60">
                                  {new Date(reply.created_at).toLocaleDateString('zh-CN', {
                                    month: 'short',
                                    day: 'numeric',
                                    hour: '2-digit',
                                    minute: '2-digit',
                                  })}
                                </span>
                              </div>
                              {/* 回复内容 */}
                              <p className={`text-sm ${currentTheme.text} leading-relaxed pr-12`}>{reply.comment}</p>
                              {/* 操作按钮 */}
                              <div className="absolute right-2 top-2 flex items-center gap-0.5 opacity-0 group-hover/reply:opacity-100 transition-opacity">
                                <button
                                  onClick={() => setReplyingTo(reply)}
                                  className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'} transition-all`}
                                  title={t.brew.reply}
                                >
                                  <Reply className="w-3.5 h-3.5" />
                                </button>
                                <button
                                  onClick={() => deleteComment(reply.id)}
                                  className="p-1 rounded text-red-500/70 hover:text-red-500 hover:bg-red-500/10 transition-all"
                                  title={t.brew.deleteReply}
                                >
                                  <Trash2 className="w-3.5 h-3.5" />
                                </button>
                              </div>
                            </div>
                          ))}

                          {/* 回复某条回复的输入框 */}
                          {replyingTo && commentReplies[comment.id]?.some(r => r.id === replyingTo.id) && (
                            <div className="pl-3">
                              <p className={`text-xs ${currentTheme.secondary} mb-1.5`}>
                                {t.brew.replyTo}
                                {' '}
                                @
                                {replyingTo.user_display_name || replyingTo.user_name || t.brew.anonymousUser}
                              </p>
                              <div className="flex items-center gap-2">
                                <input
                                  type="text"
                                  value={replyInput}
                                  onChange={e => setReplyInput(e.target.value)}
                                  placeholder={t.brew.writeYourReply}
                                  className={`flex-1 px-3 py-1.5 text-sm rounded-lg border ${currentTheme.border} ${currentTheme.bg} ${currentTheme.text} placeholder:${currentTheme.secondary} focus:outline-none focus:ring-2 focus:ring-amber-500/50`}
                                  autoFocus
                                  onKeyDown={(e) => {
                                    if (e.key === 'Enter' && !e.shiftKey) {
                                      e.preventDefault()
                                      submitReply()
                                    }
                                    else if (e.key === 'Escape') {
                                      setReplyingTo(null)
                                      setReplyInput('')
                                    }
                                  }}
                                />
                                <button
                                  onClick={submitReply}
                                  disabled={!replyInput.trim() || replySubmitting}
                                  className="flex-shrink-0 w-7 h-7 flex items-center justify-center rounded-lg bg-amber-500 text-white hover:bg-amber-600 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                                  title={t.brew.send}
                                >
                                  <Send className="w-3.5 h-3.5" />
                                </button>
                                <button
                                  onClick={() => {
                                    setReplyingTo(null)
                                    setReplyInput('')
                                  }}
                                  className={`flex-shrink-0 w-7 h-7 flex items-center justify-center rounded-lg ${isDark ? 'bg-white/10 hover:bg-white/20' : 'bg-black/5 hover:bg-black/10'} ${currentTheme.secondary} transition-colors`}
                                  title={t.brew.cancel}
                                >
                                  <X className="w-3.5 h-3.5" />
                                </button>
                              </div>
                            </div>
                          )}
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  )
}
