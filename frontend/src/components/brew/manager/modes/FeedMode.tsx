/**
 * Feed 模式组件 - 单个订阅源文章列表视图
 */

import type { FeedModeConfig } from './types'
import {
  LuCheckCircle as CheckCircle,
  LuChevronLeft as ChevronLeft,
  LuExternalLink as ExternalLink,
  LuRefreshCw as RefreshCw,
  LuRss as Rss,
} from '@lib/icons'
import { motion } from 'framer-motion'
import { SPRING_SNAPPY } from './constants'

export interface FeedModeProps {
  variant: 'mobile' | 'desktop'
  feedMode: FeedModeConfig
  isAdmin?: boolean
  isAuthenticated?: boolean
  t: {
    backToSourceList: string
    articlesCount: string
    tipUnreadCount: string
    refreshSource: string
    markAllAsRead: string
    visitWebsite: string
  }
}

export function FeedMode({
  variant,
  feedMode,
  isAdmin = false,
  isAuthenticated = false,
  t,
}: FeedModeProps) {
  const isMobile = variant === 'mobile'

  return (
    <motion.div
      key={`feed-bar-${variant}`}
      initial={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      transition={SPRING_SNAPPY}
      className="flex items-center gap-1.5 px-2 py-2 rounded-2xl bg-white/90 dark:bg-neutral-900/90 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
    >
      {/* 返回按钮 */}
      <motion.button
        onClick={feedMode.onBack}
        className="p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 rounded-xl transition-colors"
        whileTap={{ scale: 0.95 }}
        title={t.backToSourceList}
        aria-label={t.backToSourceList}
      >
        <ChevronLeft className="w-5 h-5" />
      </motion.button>

      {/* 订阅源信息 */}
      <div className="flex items-center gap-2 h-10 px-2 min-w-0 flex-1">
        {feedMode.source.icon
          ? (
              <img
                src={feedMode.source.icon}
                alt=""
                className="w-7 h-7 rounded-lg object-cover flex-shrink-0"
              />
            )
          : (
              <div
                className="w-7 h-7 rounded-lg flex items-center justify-center flex-shrink-0"
                style={{ backgroundColor: feedMode.source.theme_color || '#F97316' }}
              >
                <Rss className="w-4 h-4 text-white" />
              </div>
            )}
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-gray-800 dark:text-gray-100 truncate">
            {feedMode.source.name}
          </h3>
          <div className="flex items-center gap-1.5 text-[10px] text-gray-500 dark:text-gray-400">
            <span>{t.articlesCount.replace('{count}', String(feedMode.total))}</span>
            {feedMode.source.unread_count > 0 && (
              <span
                className="font-medium"
                style={{ color: feedMode.source.theme_color || '#F97316' }}
              >
                {t.tipUnreadCount.replace('{count}', String(feedMode.source.unread_count))}
              </span>
            )}
          </div>
        </div>
      </div>

      {/* 分隔线 */}
      <div className="w-px h-6 bg-gray-200 dark:bg-neutral-700" />

      {/* 刷新按钮 - 仅管理员可见 */}
      {isAdmin && (
        <motion.button
          onClick={feedMode.onRefresh}
          disabled={feedMode.isRefreshing}
          className="p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 rounded-xl transition-colors disabled:opacity-50"
          whileTap={{ scale: 0.95 }}
          title={t.refreshSource}
          aria-label={t.refreshSource}
        >
          <RefreshCw className={`w-4 h-4 ${feedMode.isRefreshing ? 'animate-spin' : ''}`} />
        </motion.button>
      )}

      {/* 全部已读按钮 */}
      {isAuthenticated && feedMode.source.unread_count > 0 && (
        <motion.button
          onClick={feedMode.onMarkAllRead}
          className="p-2.5 text-gray-500 hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/20 rounded-xl transition-colors"
          whileTap={{ scale: 0.95 }}
          title={t.markAllAsRead}
          aria-label={t.markAllAsRead}
        >
          <CheckCircle className="w-4 h-4" />
        </motion.button>
      )}

      {/* 访问网站按钮 */}
      {feedMode.source.site_url && (
        <motion.a
          href={feedMode.source.site_url}
          target="_blank"
          rel="noopener noreferrer"
          className="p-2.5 text-gray-500 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-blue-900/20 rounded-xl transition-colors"
          whileTap={{ scale: 0.95 }}
          title={t.visitWebsite}
          aria-label={t.visitWebsite}
        >
          <ExternalLink className="w-4 h-4" />
        </motion.a>
      )}
    </motion.div>
  )
}
