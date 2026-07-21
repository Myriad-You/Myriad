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
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { IslandShell } from '../../../shared/control-island'
import {
  ISLAND_BTN,
  ISLAND_GLASS,
  TRANSITION_NORMAL,
  TRANSITION_SLOW,
} from './constants'

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
  sortDropdownRef: React.RefObject<HTMLDivElement | null>
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
      <IslandShell variant="mobile" motionKey="default-bar-mobile">
        {/* 动态提示 */}
        <div className="flex items-center gap-2 h-9 px-2 min-w-0 flex-1 overflow-hidden">
          <AnimatePresence mode="wait">
            <motion.div
              key={tipKey}
              initial={{ opacity: 0, y: 6, filter: 'blur(4px)' }}
              animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
              exit={{ opacity: 0, y: -6, filter: 'blur(4px)' }}
              transition={TRANSITION_SLOW}
              className="flex items-center gap-2"
            >
              {tip.iconUrl && getIconUrl ? (
                <img
                  src={getIconUrl(tip.iconUrl) || ''}
                  alt=""
                  className="w-6 h-6 shrink-0 object-contain drop-shadow-sm"
                  loading="lazy"
                  onLoad={(e) => {
                    // Soft-fail proxy returns 1×1 PNG for dead remote icons — hide, no emoji fallback
                    const img = e.currentTarget
                    if (img.naturalWidth <= 1 && img.naturalHeight <= 1) {
                      img.style.display = 'none'
                    }
                  }}
                  onError={(e) => {
                    // 网站图标丢失：只隐藏，不回退 emoji
                    ;(e.target as HTMLImageElement).style.display = 'none'
                  }}
                />
              ) : tip.icon ? (
                <span className="text-base shrink-0">{tip.icon}</span>
              ) : null}
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
          <button
            onClick={() => setShowSortDropdown(!showSortDropdown)}
            className={ISLAND_BTN}
            title={t.sortMethod}
            aria-label={t.sortMethod}
          >
            <ArrowUpDown className="w-4 h-4" />
          </button>

          <AnimatePresence>
            {showSortDropdown && (
              <motion.div
                initial={{ opacity: 0, y: -4, scale: 0.95 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: -4, scale: 0.95 }}
                transition={TRANSITION_NORMAL}
                className={`absolute top-full mt-2 right-0 w-36 ${ISLAND_GLASS} overflow-hidden py-1 z-100`}
              >
                {sortOptions.map((option) => (
                  <button
                    key={option.value}
                    onClick={() => {
                      onSortModeChange?.(option.value)
                      setShowSortDropdown(false)
                    }}
                    className={`w-full px-3 py-2 text-left text-sm flex items-center gap-2 transition-colors ${
                      sortMode === option.value
                        ? 'text-orange-500 bg-orange-500/10'
                        : 'text-gray-600 dark:text-gray-300 hover:bg-black/4 dark:hover:bg-white/6'
                    }`}
                  >
                    {option.icon}
                    <span>{t[option.labelKey]}</span>
                    {sortMode === option.value && (
                      <Check className="w-3 h-3 ml-auto" />
                    )}
                  </button>
                ))}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </IslandShell>
    )
  }

  // 桌面端版本 - 完整功能
  return (
    <IslandShell
      variant="desktop"
      motionKey="default-bar"
      className="flex-col sm:flex-row"
    >
      {/* 动态提示 */}
      <div className="flex items-center gap-2 h-9 px-2.5 min-w-44 overflow-hidden">
        <AnimatePresence mode="wait">
          <motion.div
            key={tipKey}
            initial={{ opacity: 0, y: 8, filter: 'blur(4px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            exit={{ opacity: 0, y: -8, filter: 'blur(4px)' }}
            transition={TRANSITION_SLOW}
            className="flex items-center gap-2"
          >
            {tip.iconUrl && getIconUrl ? (
              <img
                src={getIconUrl(tip.iconUrl) || ''}
                alt=""
                className="w-6 h-6 shrink-0 object-contain drop-shadow-sm"
                loading="lazy"
                onLoad={(e) => {
                  const img = e.currentTarget
                  if (img.naturalWidth <= 1 && img.naturalHeight <= 1) {
                    img.style.display = 'none'
                  }
                }}
                onError={(e) => {
                  // 网站图标丢失：只隐藏，不回退 emoji
                  ;(e.target as HTMLImageElement).style.display = 'none'
                }}
              />
            ) : tip.icon ? (
              <span className="text-base shrink-0">{tip.icon}</span>
            ) : null}
            <div className="flex flex-col justify-center leading-tight">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-200 truncate max-w-36">
                {tip.main}
              </span>
              <span className="text-xs text-gray-400 dark:text-gray-500 truncate max-w-36">
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
          <button
            onClick={() => setShowSortDropdown(!showSortDropdown)}
            className={`${ISLAND_BTN} group`}
            title={t.sortMethod}
            aria-label={t.sortMethod}
          >
            <ArrowUpDown className="w-4 h-4" />
            <span className="text-xs font-medium hidden sm:inline">
              {t[currentSortOption.labelKey]}
            </span>
            <ChevronDown
              className={`w-3 h-3 transition-transform ${showSortDropdown ? 'rotate-180' : ''}`}
            />
          </button>

          <AnimatePresence>
            {showSortDropdown && (
              <motion.div
                initial={{ opacity: 0, y: 4, scale: 0.95 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 4, scale: 0.95 }}
                transition={TRANSITION_NORMAL}
                className={`absolute bottom-full mb-2 left-0 w-36 ${ISLAND_GLASS} overflow-hidden py-1`}
              >
                {sortOptions.map((option) => (
                  <button
                    key={option.value}
                    onClick={() => {
                      onSortModeChange?.(option.value)
                      setShowSortDropdown(false)
                    }}
                    className={`w-full px-3 py-2 text-left text-sm flex items-center gap-2 transition-colors ${
                      sortMode === option.value
                        ? 'text-orange-500 bg-orange-500/10'
                        : 'text-gray-600 dark:text-gray-300 hover:bg-black/4 dark:hover:bg-white/6'
                    }`}
                  >
                    {option.icon}
                    <span>{t[option.labelKey]}</span>
                    {sortMode === option.value && (
                      <Check className="w-3 h-3 ml-auto" />
                    )}
                  </button>
                ))}
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {/* 搜索按钮 */}
        <button
          onClick={() => onModeChange('search')}
          className={ISLAND_BTN}
          title={t.search}
          aria-label={t.search}
        >
          <Search className="w-4 h-4" />
          <span className="text-xs font-medium hidden sm:inline">
            {t.search}
          </span>
        </button>

        {/* 编辑模式按钮 - 仅管理员可见 */}
        {isAdmin && (
          <button
            onClick={() => onModeChange('edit')}
            className={ISLAND_BTN}
            title={t.editMode}
            aria-label={t.editMode}
          >
            <Edit3 className="w-4 h-4" />
            <span className="text-xs font-medium hidden sm:inline">
              {t.edit}
            </span>
          </button>
        )}

        {/* 快捷键按钮 */}
        <button
          onClick={() => onModeChange('keyboard')}
          className={ISLAND_BTN}
          title={t.shortcuts}
          aria-label={t.shortcuts}
        >
          <Keyboard className="w-4 h-4" />
          <span className="text-xs font-medium hidden sm:inline">
            {t.shortcuts}
          </span>
        </button>

        {/* 添加订阅按钮 - 仅管理员可见 */}
        {isAdmin && hasAddSource && (
          <button
            onClick={() => onModeChange('add')}
            className="flex items-center justify-center gap-1.5 h-9 px-4 rounded-xl bg-linear-to-r from-orange-500 to-amber-500 hover:from-orange-600 hover:to-amber-600 text-white text-sm font-medium shadow-sm shadow-orange-500/25 hover:shadow-md hover:shadow-orange-500/30 hover:scale-102 active:scale-97 transition-all duration-200 ease-out"
            title={t.addSubscription}
            aria-label={t.addSubscription}
          >
            <Plus className="w-4 h-4" />
            <span className="text-xs font-medium">{t.add}</span>
          </button>
        )}
      </div>
    </IslandShell>
  )
}
