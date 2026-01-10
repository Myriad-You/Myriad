/**
 * CategoryFeedMode - 分类合并文章列表模式
 */

import type { CategoryFeedModeConfig } from './types'
import {
  LuCheckCircle as CheckCircle,
  LuChevronLeft as ChevronLeft,
} from '@lib/icons'
import { motion } from 'framer-motion'
import { SPRING_SNAPPY } from './constants'

export interface CategoryFeedModeProps {
  variant: 'mobile' | 'desktop'
  categoryFeedMode: CategoryFeedModeConfig
  isAuthenticated?: boolean
  t: {
    backToAllSources: string
    totalArticles: string
    tipUnreadCount: string
    markAllAsRead: string
  }
}

export function CategoryFeedMode({
  variant,
  categoryFeedMode,
  isAuthenticated = false,
  t,
}: CategoryFeedModeProps) {
  const isMobile = variant === 'mobile'

  return (
    <motion.div
      key={`category-feed-bar-${variant}`}
      initial={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      transition={SPRING_SNAPPY}
      className="flex items-center gap-1.5 px-2 py-2 rounded-2xl bg-white/90 dark:bg-neutral-900/90 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
    >
      {/* 返回按钮 */}
      <motion.button
        onClick={categoryFeedMode.onBack}
        className="p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 rounded-xl transition-colors"
        whileTap={{ scale: 0.95 }}
        title={t.backToAllSources}
        aria-label={t.backToAllSources}
      >
        <ChevronLeft className="w-5 h-5" />
      </motion.button>

      {/* 分类信息 */}
      <div className="flex items-center gap-2 h-10 px-2 min-w-0 flex-1">
        <div className="w-7 h-7 rounded-lg flex items-center justify-center flex-shrink-0 bg-gradient-to-br from-blue-500 to-purple-500">
          <svg className="w-4 h-4 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" />
          </svg>
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-gray-800 dark:text-gray-100 truncate">
            {categoryFeedMode.categoryLabel}
          </h3>
          <div className="flex items-center gap-1.5 text-[10px] text-gray-500 dark:text-gray-400">
            <span>{t.totalArticles.replace('{count}', String(categoryFeedMode.total))}</span>
            {categoryFeedMode.unreadCount > 0 && (
              <span className="font-medium text-blue-500">
                {t.tipUnreadCount.replace('{count}', String(categoryFeedMode.unreadCount))}
              </span>
            )}
          </div>
        </div>
      </div>

      {/* 全部已读按钮 */}
      {isAuthenticated && categoryFeedMode.unreadCount > 0 && (
        <>
          <div className="w-px h-6 bg-gray-200 dark:bg-neutral-700" />
          <motion.button
            onClick={categoryFeedMode.onMarkAllRead}
            className="p-2.5 text-gray-500 hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/20 rounded-xl transition-colors"
            whileTap={{ scale: 0.95 }}
            title={t.markAllAsRead}
            aria-label={t.markAllAsRead}
          >
            <CheckCircle className="w-4 h-4" />
          </motion.button>
        </>
      )}
    </motion.div>
  )
}
