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

export type ShortcutDescKey =
  | 'shortcutDescNextArticle'
  | 'shortcutDescPrevArticle'
  | 'shortcutDescOpenReader'
  | 'shortcutDescCloseReader'
  | 'shortcutDescFocusSearch'
  | 'shortcutDescToggleRead'
  | 'shortcutDescToggleStar'
  | 'shortcutDescMarkAllRead'
  | 'shortcutDescRefreshSource'
  | 'shortcutDescAddSource'
  | 'shortcutDescShowHelp'

interface KeyboardShortcut {
  key: string
  descriptionKey: ShortcutDescKey
  category: 'navigation' | 'article' | 'source' | 'other'
}

export const BREW_SHORTCUTS: KeyboardShortcut[] = [
  {
    key: 'j / ↓',
    descriptionKey: 'shortcutDescNextArticle',
    category: 'navigation',
  },
  {
    key: 'k / ↑',
    descriptionKey: 'shortcutDescPrevArticle',
    category: 'navigation',
  },
  {
    key: 'o / Enter',
    descriptionKey: 'shortcutDescOpenReader',
    category: 'navigation',
  },
  {
    key: 'Escape',
    descriptionKey: 'shortcutDescCloseReader',
    category: 'navigation',
  },
  {
    key: '/',
    descriptionKey: 'shortcutDescFocusSearch',
    category: 'navigation',
  },

  { key: 'm', descriptionKey: 'shortcutDescToggleRead', category: 'article' },
  { key: 's', descriptionKey: 'shortcutDescToggleStar', category: 'article' },
  {
    key: 'Shift + A',
    descriptionKey: 'shortcutDescMarkAllRead',
    category: 'article',
  },

  { key: 'r', descriptionKey: 'shortcutDescRefreshSource', category: 'source' },
  { key: 'a', descriptionKey: 'shortcutDescAddSource', category: 'source' },

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

  const getCurrentIndex = useCallback(() => {
    if (!selectedItem) return -1
    return items.findIndex((item) => item.id === selectedItem.id)
  }, [items, selectedItem])

  const selectPrevious = useCallback(() => {
    const currentIndex = getCurrentIndex()
    if (currentIndex > 0) {
      onSelectItem(items[currentIndex - 1])
    } else if (currentIndex === -1 && items.length > 0) {
      onSelectItem(items[0])
    }
  }, [getCurrentIndex, items, onSelectItem])

  const selectNext = useCallback(() => {
    const currentIndex = getCurrentIndex()
    if (currentIndex < items.length - 1) {
      onSelectItem(items[currentIndex + 1])
    } else if (currentIndex === -1 && items.length > 0) {
      onSelectItem(items[0])
    }
  }, [getCurrentIndex, items, onSelectItem])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!enabled) return

      const target = e.target as HTMLElement
      const isInputFocused =
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable

      const now = Date.now()
      if (now - lastKeyTime.current < 50) return
      lastKeyTime.current = now

      if (e.key === 'Escape') {
        e.preventDefault()
        onCloseReader?.()

        if (document.activeElement === searchInputRef?.current) {
          searchInputRef.current?.blur()
        }
        return
      }

      if (isInputFocused) return

      switch (e.key.toLowerCase()) {
        case 'j':
        case 'arrowdown':
          e.preventDefault()
          selectNext()
          break

        case 'k':
        case 'arrowup':
          e.preventDefault()
          selectPrevious()
          break

        case 'o':
        case 'enter':
          e.preventDefault()
          if (!selectedItem && items.length > 0) {
            onSelectItem(items[0])
          }
          break

        case 'm':
          if (selectedItem) {
            e.preventDefault()
            onToggleRead?.(selectedItem)
          }
          break

        case 's':
          if (selectedItem) {
            e.preventDefault()
            onToggleStar?.(selectedItem)
          }
          break

        case 'r':
          e.preventDefault()
          onRefresh?.()
          break

        case 'a':
          if (e.shiftKey) {
            e.preventDefault()
            onMarkAllRead?.()
          } else {
            e.preventDefault()
            onAddSource?.()
          }
          break

        case '/':
          e.preventDefault()
          searchInputRef?.current?.focus()
          break

        case '?':
          e.preventDefault()
          onShowHelp?.()
          break
      }
    },
    [
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
    ],
  )

  useEffect(() => {
    if (!enabled) return

    window.addEventListener('keydown', handleKeyDown)
    return () => {
      window.removeEventListener('keydown', handleKeyDown)
    }
  }, [enabled, handleKeyDown])
}
