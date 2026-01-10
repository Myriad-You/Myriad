/**
 * Brew 阅读键盘快捷键 Hook
 *
 * 快捷键列表:
 * - j / ArrowDown: 下一篇文章
 * - k / ArrowUp: 上一篇文章
 * - o / Enter: 打开/关闭阅读器
 * - m: 切换已读/未读
 * - s: 切换收藏
 * - r: 刷新当前订阅源
 * - a: 添加新订阅源
 * - n: 新建分类
 * - /: 聚焦搜索框
 * - Escape: 关闭阅读器/弹窗
 * - Shift + A: 全部标记已读
 * - ?: 显示快捷键帮助
 */

import type { BrewItem } from '../types/brew'
import { useCallback, useEffect, useRef } from 'react'

interface UseBrewKeyboardOptions {
  items: BrewItem[]
  selectedItem: BrewItem | null
  enabled?: boolean
  onSelectItem: (item: BrewItem | null) => void
  onToggleRead?: (item: BrewItem) => void
  onToggleStar?: (item: BrewItem) => void
  onRefresh?: () => void
  onAddSource?: () => void
  onMarkAllRead?: () => void
  onCloseReader?: () => void
  onShowHelp?: () => void
  searchInputRef?: React.RefObject<HTMLInputElement>
}

interface KeyboardShortcut {
  key: string
  descriptionKey: string // 翻译键，如 'shortcutDescNextArticle'
  category: 'navigation' | 'article' | 'source' | 'other'
}

// 导出快捷键列表（供帮助弹窗使用）
export const BREW_SHORTCUTS: KeyboardShortcut[] = [
  // 导航
  { key: 'j / ↓', descriptionKey: 'shortcutDescNextArticle', category: 'navigation' },
  { key: 'k / ↑', descriptionKey: 'shortcutDescPrevArticle', category: 'navigation' },
  { key: 'o / Enter', descriptionKey: 'shortcutDescOpenReader', category: 'navigation' },
  { key: 'Escape', descriptionKey: 'shortcutDescCloseReader', category: 'navigation' },
  { key: '/', descriptionKey: 'shortcutDescFocusSearch', category: 'navigation' },

  // 文章操作
  { key: 'm', descriptionKey: 'shortcutDescToggleRead', category: 'article' },
  { key: 's', descriptionKey: 'shortcutDescToggleStar', category: 'article' },
  { key: 'Shift + A', descriptionKey: 'shortcutDescMarkAllRead', category: 'article' },

  // 订阅源操作
  { key: 'r', descriptionKey: 'shortcutDescRefreshSource', category: 'source' },
  { key: 'a', descriptionKey: 'shortcutDescAddSource', category: 'source' },

  // 其他
  { key: '?', descriptionKey: 'shortcutDescShowHelp', category: 'other' },
]

export function useBrewKeyboard({
  items,
  selectedItem,
  enabled = true,
  onSelectItem,
  onToggleRead,
  onToggleStar,
  onRefresh,
  onAddSource,
  onMarkAllRead,
  onCloseReader,
  onShowHelp,
  searchInputRef,
}: UseBrewKeyboardOptions) {
  const lastKeyTime = useRef<number>(0)

  // 获取当前选中项的索引
  const getCurrentIndex = useCallback(() => {
    if (!selectedItem)
      return -1
    return items.findIndex(item => item.id === selectedItem.id)
  }, [items, selectedItem])

  // 选择上一篇
  const selectPrevious = useCallback(() => {
    const currentIndex = getCurrentIndex()
    if (currentIndex > 0) {
      onSelectItem(items[currentIndex - 1])
    }
    else if (currentIndex === -1 && items.length > 0) {
      // 如果没有选中，选择第一篇
      onSelectItem(items[0])
    }
  }, [getCurrentIndex, items, onSelectItem])

  // 选择下一篇
  const selectNext = useCallback(() => {
    const currentIndex = getCurrentIndex()
    if (currentIndex < items.length - 1) {
      onSelectItem(items[currentIndex + 1])
    }
    else if (currentIndex === -1 && items.length > 0) {
      // 如果没有选中，选择第一篇
      onSelectItem(items[0])
    }
  }, [getCurrentIndex, items, onSelectItem])

  // 切换阅读器
  const toggleReader = useCallback(() => {
    if (selectedItem) {
      // 如果已有选中项，关闭阅读器
      onCloseReader?.()
    }
    else if (items.length > 0) {
      // 如果没有选中项，选择第一篇
      onSelectItem(items[0])
    }
  }, [selectedItem, items, onSelectItem, onCloseReader])

  // 键盘事件处理
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    // 如果禁用或焦点在输入框中，忽略
    if (!enabled)
      return

    const target = e.target as HTMLElement
    const isInputFocused
      = target.tagName === 'INPUT'
        || target.tagName === 'TEXTAREA'
        || target.isContentEditable

    // 防止快速连续触发
    const now = Date.now()
    if (now - lastKeyTime.current < 50)
      return
    lastKeyTime.current = now

    // 某些快捷键在输入框中也需要工作
    if (e.key === 'Escape') {
      e.preventDefault()
      onCloseReader?.()
      // 如果搜索框有焦点，取消焦点
      if (document.activeElement === searchInputRef?.current) {
        searchInputRef.current?.blur()
      }
      return
    }

    // 输入框聚焦时忽略其他快捷键
    if (isInputFocused)
      return

    switch (e.key.toLowerCase()) {
      // 导航：下一篇
      case 'j':
      case 'arrowdown':
        e.preventDefault()
        selectNext()
        break

      // 导航：上一篇
      case 'k':
      case 'arrowup':
        e.preventDefault()
        selectPrevious()
        break

      // 打开/关闭阅读器
      case 'o':
      case 'enter':
        e.preventDefault()
        if (!selectedItem && items.length > 0) {
          onSelectItem(items[0])
        }
        break

      // 切换已读/未读
      case 'm':
        if (selectedItem) {
          e.preventDefault()
          onToggleRead?.(selectedItem)
        }
        break

      // 切换收藏
      case 's':
        if (selectedItem) {
          e.preventDefault()
          onToggleStar?.(selectedItem)
        }
        break

      // 刷新
      case 'r':
        e.preventDefault()
        onRefresh?.()
        break

      // 添加订阅源
      case 'a':
        if (e.shiftKey) {
          // Shift + A: 全部标记已读
          e.preventDefault()
          onMarkAllRead?.()
        }
        else {
          e.preventDefault()
          onAddSource?.()
        }
        break

      // 聚焦搜索框
      case '/':
        e.preventDefault()
        searchInputRef?.current?.focus()
        break

      // 显示帮助
      case '?':
        e.preventDefault()
        onShowHelp?.()
        break
    }
  }, [
    enabled,
    selectedItem,
    items,
    selectNext,
    selectPrevious,
    onSelectItem,
    onToggleRead,
    onToggleStar,
    onRefresh,
    onAddSource,
    onMarkAllRead,
    onCloseReader,
    onShowHelp,
    searchInputRef,
  ])

  // 注册键盘事件
  useEffect(() => {
    if (!enabled)
      return

    window.addEventListener('keydown', handleKeyDown)
    return () => {
      window.removeEventListener('keydown', handleKeyDown)
    }
  }, [enabled, handleKeyDown])
}
