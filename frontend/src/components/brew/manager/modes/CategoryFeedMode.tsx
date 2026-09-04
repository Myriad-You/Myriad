/**
 * CategoryFeedMode - 分类合并文章列表模式
 */

import type { CategoryFeedModeConfig } from './types'

import {
  LuCheckCircle as CheckCircle,
  LuChevronLeft as ChevronLeft,
  LuEdit3 as Edit3,
} from '@lib/icons'
import { IslandShell } from '../../../shared/control-island'
import { ISLAND_BTN, ISLAND_DIVIDER } from './constants'

export interface CategoryFeedModeProps {
  variant: 'mobile' | 'desktop'
  categoryFeedMode: CategoryFeedModeConfig
  isAuthenticated?: boolean
  t: {
    backToAllSources: string
    totalArticles: string
    tipUnreadCount: string
    markAllAsRead: string
    noteWrite: string
  }
}

export function CategoryFeedMode({
  variant,
  categoryFeedMode,
  isAuthenticated = false,
  t,
}: CategoryFeedModeProps) {
  return (
    <IslandShell variant={variant} motionKey={`category-feed-bar-${variant}`}>
      {/* 返回按钮 */}
      <button
        onClick={categoryFeedMode.onBack}
        className={ISLAND_BTN}
        title={t.backToAllSources}
        aria-label={t.backToAllSources}
      >
        <ChevronLeft className="w-4.5 h-4.5" />
      </button>

      {/* 分类信息 */}
      <div className="flex items-center gap-2 h-9 px-2 min-w-0 flex-1">
        <div className="w-7 h-7 rounded-lg flex items-center justify-center shrink-0 bg-linear-to-br from-blue-500 to-purple-500">
          <svg
            className="w-4 h-4 text-white"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z"
            />
          </svg>
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-gray-800 dark:text-gray-100 truncate">
            {categoryFeedMode.categoryLabel}
          </h3>
          <div className="flex items-center gap-1.5 text-[10px] text-gray-500 dark:text-gray-400">
            <span>
              {t.totalArticles.replace(
                '{count}',
                String(categoryFeedMode.total),
              )}
            </span>
            {categoryFeedMode.unreadCount > 0 && (
              <span className="font-medium text-blue-500">
                {t.tipUnreadCount.replace(
                  '{count}',
                  String(categoryFeedMode.unreadCount),
                )}
              </span>
            )}
          </div>
        </div>
      </div>

      {/* 写手记。只有管理员拿得到这个回调 */}
      {categoryFeedMode.onWriteNote && (
        <>
          <div className={ISLAND_DIVIDER} />
          <button
            onClick={categoryFeedMode.onWriteNote}
            className="flex items-center justify-center gap-1.5 h-9 px-3.5 rounded-xl bg-linear-to-r from-orange-500 to-amber-500 hover:from-orange-600 hover:to-amber-600 text-white text-xs font-medium shadow-sm shadow-orange-500/25 transition-all duration-200 ease-out"
            title={t.noteWrite}
            aria-label={t.noteWrite}
          >
            <Edit3 className="w-4 h-4" />
            <span className="hidden sm:inline">{t.noteWrite}</span>
          </button>
        </>
      )}

      {/* 全部已读按钮 */}
      {isAuthenticated && categoryFeedMode.unreadCount > 0 && (
        <>
          <div className={ISLAND_DIVIDER} />
          <button
            onClick={categoryFeedMode.onMarkAllRead}
            className={`${ISLAND_BTN} hover:text-green-600! dark:hover:text-green-400! hover:bg-green-500/10!`}
            title={t.markAllAsRead}
            aria-label={t.markAllAsRead}
          >
            <CheckCircle className="w-4 h-4" />
          </button>
        </>
      )}
    </IslandShell>
  )
}
