/**
 * StarredMode - 收藏文章模式
 */

import type { StarredModeConfig } from './types'
import {
  LuChevronLeft as ChevronLeft,
  LuEdit3 as Edit3,
  LuStar as Star,
} from '@lib/icons'
import { motion } from 'framer-motion'
import { SPRING_SNAPPY } from './constants'

export interface StarredModeProps {
  variant: 'mobile' | 'desktop'
  starredMode: StarredModeConfig
  t: {
    backToSourceList: string
    starredArticles: string
    starredCount: string
    editMode: string
  }
}

export function StarredMode({
  variant,
  starredMode,
  t,
}: StarredModeProps) {
  const isMobile = variant === 'mobile'

  return (
    <motion.div
      key={`starred-bar-${variant}`}
      initial={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      transition={SPRING_SNAPPY}
      className="flex items-center gap-1.5 px-2 py-2 rounded-2xl bg-white/90 dark:bg-neutral-900/90 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
    >
      {/* 返回按钮 */}
      <motion.button
        onClick={starredMode.onBack}
        className="p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 rounded-xl transition-colors"
        whileTap={{ scale: 0.95 }}
        title={t.backToSourceList}
        aria-label={t.backToSourceList}
      >
        <ChevronLeft className="w-5 h-5" />
      </motion.button>

      {/* 收藏信息 */}
      <div className="flex items-center gap-2 h-10 px-2 min-w-0 flex-1">
        <div className="w-7 h-7 rounded-lg bg-amber-500 flex items-center justify-center flex-shrink-0">
          <Star className="w-4 h-4 text-white" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-gray-800 dark:text-gray-100 truncate">
            {t.starredArticles}
          </h3>
          <div className="text-[10px] text-gray-500 dark:text-gray-400">
            {t.starredCount.replace('{count}', String(starredMode.total))}
          </div>
        </div>
      </div>

      {/* 编辑按钮 */}
      {starredMode.total > 0 && (
        <>
          <div className="w-px h-6 bg-gray-200 dark:bg-neutral-700" />
          <motion.button
            onClick={starredMode.onEnterEditMode}
            className="p-2.5 text-gray-500 hover:text-amber-500 hover:bg-amber-50 dark:hover:bg-amber-900/20 rounded-xl transition-colors"
            whileTap={{ scale: 0.95 }}
            title={t.editMode}
            aria-label={t.editMode}
          >
            <Edit3 className="w-4 h-4" />
          </motion.button>
        </>
      )}
    </motion.div>
  )
}
