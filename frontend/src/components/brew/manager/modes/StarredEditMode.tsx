/**
 * StarredEditMode - 收藏编辑模式
 */

import type { StarredModeConfig } from './types'
import {
  LuCheckSquare as CheckSquare,
  LuLoader2 as Loader2,
  LuMinusSquare as MinusSquare,
  LuSquare as Square,
  LuStar as Star,
  LuX as X,
} from '@lib/icons'
import { motion } from 'framer-motion'
import { SPRING_SNAPPY } from './constants'

export interface StarredEditModeProps {
  variant: 'mobile' | 'desktop'
  starredMode: StarredModeConfig
  t: {
    exitEdit: string
    selectAllToggle: string
    selectedCount: string
    selectArticles: string
    unstar: string
  }
}

export function StarredEditMode({
  variant,
  starredMode,
  t,
}: StarredEditModeProps) {
  const isMobile = variant === 'mobile'

  return (
    <motion.div
      key={`starred-edit-bar-${variant}`}
      initial={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      transition={SPRING_SNAPPY}
      className="flex items-center gap-1.5 px-2 py-2 rounded-2xl bg-white/90 dark:bg-neutral-900/90 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
    >
      {/* 退出编辑 */}
      <motion.button
        onClick={starredMode.onExitEditMode}
        className="p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 rounded-xl transition-colors"
        whileTap={{ scale: 0.95 }}
        title={t.exitEdit}
        aria-label={t.exitEdit}
      >
        <X className="w-5 h-5" />
      </motion.button>

      {/* 选择信息 */}
      <div className="flex items-center gap-2 h-10 px-2 min-w-0 flex-1">
        <motion.button
          onClick={starredMode.onSelectAll}
          className="p-1.5 text-gray-500 hover:text-amber-500 rounded-lg transition-colors"
          whileTap={{ scale: 0.95 }}
          title={t.selectAllToggle}
          aria-label={t.selectAllToggle}
        >
          {starredMode.selectedIds.size === starredMode.total
            ? (
                <CheckSquare className="w-5 h-5 text-amber-500" />
              )
            : starredMode.selectedIds.size > 0
              ? (
                  <MinusSquare className="w-5 h-5 text-amber-500" />
                )
              : (
                  <Square className="w-5 h-5" />
                )}
        </motion.button>
        <span className="text-sm text-gray-600 dark:text-gray-300">
          {starredMode.selectedIds.size > 0
            ? t.selectedCount.replace('{count}', String(starredMode.selectedIds.size))
            : t.selectArticles}
        </span>
      </div>

      {/* 取消收藏按钮 */}
      <div className="w-px h-6 bg-gray-200 dark:bg-neutral-700" />
      <motion.button
        onClick={starredMode.onBatchUnstar}
        disabled={starredMode.selectedIds.size === 0 || starredMode.isProcessing}
        className="p-2.5 text-gray-500 hover:text-amber-500 hover:bg-amber-50 dark:hover:bg-amber-900/20 rounded-xl transition-colors disabled:opacity-50"
        whileTap={{ scale: 0.95 }}
        title={t.unstar}
        aria-label={t.unstar}
      >
        {starredMode.isProcessing
          ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            )
          : (
              <Star className="w-4 h-4" />
            )}
      </motion.button>
    </motion.div>
  )
}
