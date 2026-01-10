/**
 * 搜索模式组件
 */

import { LuSearch as Search, LuX as X } from '@lib/icons'
import { motion } from 'framer-motion'
import { SPRING_SNAPPY } from './constants'

export interface SearchModeProps {
  variant: 'mobile' | 'desktop'
  searchQuery: string
  setSearchQuery?: (query: string) => void
  filteredCount: number
  onClose: () => void
  t: {
    searchSources: string
    resultsCount: string
    closeSearch: string
  }
}

export function SearchMode({
  variant,
  searchQuery,
  setSearchQuery,
  filteredCount,
  onClose,
  t,
}: SearchModeProps) {
  const isMobile = variant === 'mobile'

  return (
    <motion.div
      key={`search-bar-${variant}`}
      initial={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: isMobile ? -8 : 8, scale: 0.96 }}
      transition={SPRING_SNAPPY}
      className="flex items-center gap-1.5 px-2 py-2 rounded-2xl bg-white/90 dark:bg-neutral-900/90 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
    >
      <div className={`flex items-center gap-2 px-3 h-10 ${isMobile ? 'flex-1' : ''}`}>
        <Search className="w-4 h-4 text-gray-400 flex-shrink-0" />
        <input
          type="text"
          value={searchQuery}
          onChange={e => setSearchQuery?.(e.target.value)}
          placeholder={t.searchSources}
          autoFocus
          className={`${isMobile ? 'flex-1' : 'w-40 sm:w-56'} bg-transparent border-none outline-none ring-0 text-sm text-gray-700 dark:text-gray-200 placeholder:text-gray-400 focus:outline-none focus:ring-0 focus:border-none appearance-none`}
          style={{ boxShadow: 'none', background: 'transparent', WebkitAppearance: 'none' }}
        />
        <span className="text-xs text-gray-400 flex-shrink-0 pr-1">
          {t.resultsCount.replace('{count}', String(filteredCount))}
        </span>
      </div>
      <button
        onClick={onClose}
        className="p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 rounded-xl transition-colors flex-shrink-0"
        title={t.closeSearch}
        aria-label={t.closeSearch}
      >
        <X className="w-5 h-5" />
      </button>
    </motion.div>
  )
}
