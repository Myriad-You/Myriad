/**
 * DefaultMode - 默认模式
 * 动态提示 + 排序 + 功能按钮
 */

import type { ControlMode, DynamicTip, SortMode, SortOption } from './types'
import {
  LuArrowUpDown as ArrowUpDown,
  LuCheck as Check,
  LuChevronDown as ChevronDown,
  LuEdit3 as Edit3,
  LuKeyboard as Keyboard,
  LuPlus as Plus,
  LuSearch as Search,
} from '@lib/icons'
import { AnimatePresence, motion } from 'framer-motion'
import { SPRING_SNAPPY, TRANSITION_NORMAL, TRANSITION_SLOW } from './constants'

export interface DefaultModeProps {
  variant: 'mobile' | 'desktop'
  // 动态提示
  tip: DynamicTip
  tipKey: string | number
  // 排序
  sortMode: SortMode
  sortOptions: SortOption[]
  currentSortOption: SortOption
  showSortDropdown: boolean
  setShowSortDropdown: (show: boolean) => void
  sortDropdownRef: React.RefObject<HTMLDivElement>
  onSortModeChange?: (mode: SortMode) => void
  // 模式切换
  onModeChange: (mode: ControlMode) => void
  // 权限
  isAdmin: boolean
  hasAddSource: boolean
  // 翻译
  t: {
    sortMethod: string
    search: string
    editMode: string
    edit: string
    shortcuts: string
    addSubscription: string
    add: string
    [key: string]: string
  }
  // 图标 URL 处理
  getIconUrl?: (iconUrl: string | null | undefined) => string | null
}

export function DefaultMode({
  variant,
  tip,
  tipKey,
  sortMode,
  sortOptions,
  currentSortOption,
  showSortDropdown,
  setShowSortDropdown,
  sortDropdownRef,
  onSortModeChange,
  onModeChange,
  isAdmin,
  hasAddSource,
  t,
  getIconUrl,
}: DefaultModeProps) {
  const isMobile = variant === 'mobile'

  // 移动端版本 - 简化（只保留动态信息和排序）
  if (isMobile) {
    return (
      <motion.div
        key="default-bar-mobile"
        initial={{ opacity: 0, y: -8, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: -8, scale: 0.96 }}
        transition={SPRING_SNAPPY}
        className="flex items-center gap-1.5 px-2 py-2 rounded-2xl bg-white/90 dark:bg-neutral-900/90 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
      >
        {/* 动态提示 */}
        <div className="flex items-center gap-2 h-10 px-2 min-w-0 flex-1 overflow-hidden">
          <AnimatePresence mode="wait">
            <motion.div
              key={tipKey}
              initial={{ opacity: 0, y: 6, filter: 'blur(4px)' }}
              animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
              exit={{ opacity: 0, y: -6, filter: 'blur(4px)' }}
              transition={TRANSITION_SLOW}
              className="flex items-center gap-2"
            >
              {tip.iconUrl && getIconUrl
                ? (
                    <img
                      src={getIconUrl(tip.iconUrl) || ''}
                      alt=""
                      className="w-5 h-5 rounded flex-shrink-0 object-cover"
                      loading="lazy"
                      onError={(e) => {
                        (e.target as HTMLImageElement).style.display = 'none';
                        (e.target as HTMLImageElement).nextElementSibling?.classList.remove('hidden')
                      }}
                    />
                  )
                : null}
              <span className={`text-base flex-shrink-0 ${tip.iconUrl ? 'hidden' : ''}`}>
                {tip.icon}
              </span>
              <div className="flex flex-col justify-center leading-tight min-w-0">
                <span className="text-sm font-medium text-gray-700 dark:text-gray-200 truncate">
                  {tip.main}
                </span>
                <span className="text-xs text-gray-400 dark:text-gray-500 truncate">
                  {tip.sub}
                </span>
              </div>
            </motion.div>
          </AnimatePresence>
        </div>

        {/* 排序按钮 */}
        <div className="relative" ref={sortDropdownRef}>
          <motion.button
            onClick={() => setShowSortDropdown(!showSortDropdown)}
            className="p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 rounded-xl transition-colors"
            whileTap={{ scale: 0.95 }}
            title={t.sortMethod}
            aria-label={t.sortMethod}
          >
            <ArrowUpDown className="w-4 h-4" />
          </motion.button>

          <AnimatePresence>
            {showSortDropdown && (
              <motion.div
                initial={{ opacity: 0, y: -4, scale: 0.95 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: -4, scale: 0.95 }}
                transition={TRANSITION_NORMAL}
                className="absolute top-full mt-2 right-0 w-36 bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 rounded-xl shadow-lg overflow-hidden py-1 z-[100]"
              >
                {sortOptions.map(option => (
                  <button
                    key={option.value}
                    onClick={() => {
                      onSortModeChange?.(option.value)
                      setShowSortDropdown(false)
                    }}
                    className={`w-full px-3 py-2 text-left text-sm flex items-center gap-2 hover:bg-gray-50 dark:hover:bg-neutral-700 transition-colors ${
                      sortMode === option.value
                        ? 'text-orange-500 bg-orange-50 dark:bg-orange-900/20'
                        : 'text-gray-600 dark:text-gray-300'
                    }`}
                  >
                    {option.icon}
                    <span>{t[option.labelKey]}</span>
                    {sortMode === option.value && <Check className="w-3 h-3 ml-auto" />}
                  </button>
                ))}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </motion.div>
    )
  }

  // 桌面端版本 - 完整功能
  return (
    <motion.div
      key="default-bar"
      initial={{ opacity: 0, y: 8, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.96 }}
      transition={SPRING_SNAPPY}
      className="flex flex-col sm:flex-row items-center gap-1.5 px-2 py-2 rounded-2xl bg-white/90 dark:bg-neutral-900/90 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
    >
      {/* 动态提示 */}
      <div className="flex items-center gap-2 h-10 px-2.5 min-w-[11rem] overflow-hidden">
        <AnimatePresence mode="wait">
          <motion.div
            key={tipKey}
            initial={{ opacity: 0, y: 8, filter: 'blur(4px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            exit={{ opacity: 0, y: -8, filter: 'blur(4px)' }}
            transition={TRANSITION_SLOW}
            className="flex items-center gap-2"
          >
            {tip.iconUrl && getIconUrl
              ? (
                  <img
                    src={getIconUrl(tip.iconUrl) || ''}
                    alt=""
                    className="w-5 h-5 rounded flex-shrink-0 object-cover"
                    loading="lazy"
                    onError={(e) => {
                      (e.target as HTMLImageElement).style.display = 'none';
                      (e.target as HTMLImageElement).nextElementSibling?.classList.remove('hidden')
                    }}
                  />
                )
              : null}
            <span className={`text-base flex-shrink-0 ${tip.iconUrl ? 'hidden' : ''}`}>
              {tip.icon}
            </span>
            <div className="flex flex-col justify-center leading-tight">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-200 truncate max-w-[9rem]">
                {tip.main}
              </span>
              <span className="text-xs text-gray-400 dark:text-gray-500 truncate max-w-[9rem]">
                {tip.sub}
              </span>
            </div>
          </motion.div>
        </AnimatePresence>
      </div>

      {/* 按钮组 */}
      <div className="flex items-center gap-1.5">
        {/* 排序按钮 */}
        <div className="relative" ref={sortDropdownRef}>
          <motion.button
            onClick={() => setShowSortDropdown(!showSortDropdown)}
            className="group flex items-center gap-1.5 px-3 py-2 rounded-xl text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 transition-colors"
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            title={t.sortMethod}
            aria-label={t.sortMethod}
          >
            <ArrowUpDown className="w-4 h-4" />
            <span className="text-xs font-medium hidden sm:inline">{t[currentSortOption.labelKey]}</span>
            <ChevronDown className={`w-3 h-3 transition-transform ${showSortDropdown ? 'rotate-180' : ''}`} />
          </motion.button>

          <AnimatePresence>
            {showSortDropdown && (
              <motion.div
                initial={{ opacity: 0, y: 4, scale: 0.95 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 4, scale: 0.95 }}
                transition={TRANSITION_NORMAL}
                className="absolute bottom-full mb-2 left-0 w-36 bg-white dark:bg-neutral-800 border border-gray-200 dark:border-neutral-700 rounded-xl shadow-lg overflow-hidden py-1"
              >
                {sortOptions.map(option => (
                  <button
                    key={option.value}
                    onClick={() => {
                      onSortModeChange?.(option.value)
                      setShowSortDropdown(false)
                    }}
                    className={`w-full px-3 py-2 text-left text-sm flex items-center gap-2 hover:bg-gray-50 dark:hover:bg-neutral-700 transition-colors ${
                      sortMode === option.value
                        ? 'text-orange-500 bg-orange-50 dark:bg-orange-900/20'
                        : 'text-gray-600 dark:text-gray-300'
                    }`}
                  >
                    {option.icon}
                    <span>{t[option.labelKey]}</span>
                    {sortMode === option.value && <Check className="w-3 h-3 ml-auto" />}
                  </button>
                ))}
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {/* 搜索按钮 */}
        <motion.button
          onClick={() => onModeChange('search')}
          className="group flex items-center gap-1.5 px-3 py-2 rounded-xl text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 transition-colors"
          whileHover={{ scale: 1.02 }}
          whileTap={{ scale: 0.98 }}
          title={t.search}
          aria-label={t.search}
        >
          <Search className="w-4 h-4" />
          <span className="text-xs font-medium hidden sm:inline">{t.search}</span>
        </motion.button>

        {/* 编辑模式按钮 - 仅管理员可见 */}
        {isAdmin && (
          <motion.button
            onClick={() => onModeChange('edit')}
            className="group flex items-center gap-1.5 px-3 py-2 rounded-xl text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 transition-colors"
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            title={t.editMode}
            aria-label={t.editMode}
          >
            <Edit3 className="w-4 h-4" />
            <span className="text-xs font-medium hidden sm:inline">{t.edit}</span>
          </motion.button>
        )}

        {/* 快捷键按钮 */}
        <motion.button
          onClick={() => onModeChange('keyboard')}
          className="group flex items-center gap-1.5 px-3 py-2 rounded-xl text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-800 transition-colors"
          whileHover={{ scale: 1.02 }}
          whileTap={{ scale: 0.98 }}
          title={t.shortcuts}
          aria-label={t.shortcuts}
        >
          <Keyboard className="w-4 h-4" />
          <span className="text-xs font-medium hidden sm:inline">{t.shortcuts}</span>
        </motion.button>

        {/* 添加订阅按钮 - 仅管理员可见 */}
        {isAdmin && hasAddSource && (
          <motion.button
            onClick={() => onModeChange('add')}
            className="group flex items-center gap-1.5 px-3 py-2 rounded-xl bg-orange-500 hover:bg-orange-600 text-white transition-colors"
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            title={t.addSubscription}
            aria-label={t.addSubscription}
          >
            <Plus className="w-4 h-4" />
            <span className="text-xs font-medium">{t.add}</span>
          </motion.button>
        )}
      </div>
    </motion.div>
  )
}
