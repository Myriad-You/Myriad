/**
 * KeyboardMode - 快捷键模式
 */

import {
  LuKeyboard as Keyboard,
  LuX as X,
} from '@lib/icons'
import { motion } from 'framer-motion'
import { SPRING_SMOOTH } from './constants'

interface KeyboardShortcut {
  key: string
  descriptionKey?: string
  description?: string
  category: string
}

export interface KeyboardModeProps {
  variant: 'mobile' | 'desktop'
  groupedShortcuts: {
    navigation: KeyboardShortcut[]
    article: KeyboardShortcut[]
    source: KeyboardShortcut[]
    other: KeyboardShortcut[]
  }
  categoryLabels: Record<string, string>
  onClose: () => void
  t: {
    keyboardShortcuts: string
    close: string
    [key: string]: string
  }
}

export function KeyboardMode({
  variant,
  groupedShortcuts,
  categoryLabels,
  onClose,
  t,
}: KeyboardModeProps) {
  const isMobile = variant === 'mobile'

  // 移动端不显示快捷键模式
  if (isMobile) {
    return null
  }

  return (
    <motion.div
      key="keyboard-bar"
      initial={{ opacity: 0, y: 12, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 12, scale: 0.95 }}
      transition={SPRING_SMOOTH}
      className="flex flex-col rounded-2xl bg-white/95 dark:bg-neutral-900/95 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10 w-[28rem] max-w-[90vw] overflow-hidden"
    >
      {/* 快捷键内容 - 两列布局 */}
      <div className="p-4 max-h-[50vh] overflow-y-auto">
        <div className="grid grid-cols-2 gap-3">
          {Object.entries(groupedShortcuts).map(([cat, shortcuts]) => {
            if (shortcuts.length === 0)
              return null
            return (
              <div
                key={cat}
                className="p-2.5 rounded-xl bg-gray-50/80 dark:bg-neutral-800/40 border border-gray-100 dark:border-neutral-700/50"
              >
                <div className="flex items-center gap-2 mb-2 pb-1.5 border-b border-gray-100 dark:border-neutral-700/50">
                  <span className="text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    {categoryLabels[cat]}
                  </span>
                  <span className="text-[9px] px-1 py-0.5 rounded-full bg-gray-200/60 dark:bg-neutral-700/60 text-gray-400 dark:text-gray-500 ml-auto">
                    {shortcuts.length}
                  </span>
                </div>
                <div className="space-y-1">
                  {shortcuts.map(shortcut => (
                    <div
                      key={shortcut.key}
                      className="flex items-center justify-between py-1 px-0.5"
                    >
                      <span className="text-xs text-gray-600 dark:text-gray-400 truncate mr-2">
                        {shortcut.descriptionKey ? (t[shortcut.descriptionKey] || shortcut.descriptionKey) : shortcut.description}
                      </span>
                      <kbd className="min-w-[18px] px-1.5 py-0.5 bg-white dark:bg-neutral-700 border border-gray-200 dark:border-neutral-600 rounded text-[10px] text-gray-500 dark:text-gray-400 font-mono text-center shadow-sm flex-shrink-0">
                        {shortcut.key.split(' / ')[0]}
                      </kbd>
                    </div>
                  ))}
                </div>
              </div>
            )
          })}
        </div>
      </div>

      {/* 底部标题栏 */}
      <div className="flex items-center justify-between px-4 py-2.5 border-t border-gray-100 dark:border-neutral-800 bg-gray-50/50 dark:bg-neutral-800/30">
        <div className="flex items-center gap-2">
          <Keyboard className="w-4 h-4 text-orange-500" />
          <span className="text-sm font-medium text-gray-700 dark:text-gray-200">
            {t.keyboardShortcuts}
          </span>
        </div>
        <button
          onClick={onClose}
          className="p-1.5 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors"
          title={t.close}
          aria-label={t.close}
        >
          <X className="w-4 h-4" />
        </button>
      </div>
    </motion.div>
  )
}
