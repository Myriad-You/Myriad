import type { QuickAccessItem } from './types'
import { FaStar, LuChevronRight } from '@lib/icons'
import React from 'react'

interface ConfigNavItemProps {
  item: QuickAccessItem
  isActive: boolean
  isFavorite: boolean
  /** 'all' 组里已收藏的行在桌面端由 CSS 隐藏（收藏组已列出），移动端滑轨仍需要它 */
  group: 'all' | 'favorites'
  onSelect: (section: string) => void
  onToggleFavorite: (id: string) => void
}

/** 侧边栏一行：图标 + 名称 + 收藏星（整行可点，星单独可点） */
export const ConfigNavItem = React.memo<ConfigNavItemProps>(
  ({ item, isActive, isFavorite, group, onSelect, onToggleFavorite }) => {
    const handleSelect = React.useCallback(() => {
      onSelect(item.section)
    }, [onSelect, item.section])

    const handleKeyDown = React.useCallback(
      (e: React.KeyboardEvent) => {
        if (e.key !== 'Enter' && e.key !== ' ') return
        e.preventDefault()
        handleSelect()
      },
      [handleSelect],
    )

    const handleFavoriteClick = React.useCallback(
      (e: React.MouseEvent) => {
        e.stopPropagation()
        onToggleFavorite(item.id)
      },
      [onToggleFavorite, item.id],
    )

    return (
      <div
        role="button"
        tabIndex={0}
        aria-current={isActive ? 'page' : undefined}
        title={item.description}
        onClick={handleSelect}
        onKeyDown={handleKeyDown}
        className={`config-nav-item${isActive ? ' is-active' : ''}${
          group === 'all' && isFavorite ? ' is-pinned' : ''
        }`}
      >
        <span className="config-nav-item-icon">{item.icon}</span>
        <span className="config-nav-item-label">{item.label}</span>
        <button
          type="button"
          onClick={handleFavoriteClick}
          aria-pressed={isFavorite}
          className={`config-nav-item-star${isFavorite ? ' is-active' : ''}`}
          aria-label={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
        >
          <FaStar />
        </button>
        <span className="config-nav-item-chevron" aria-hidden>
          <LuChevronRight size={16} />
        </span>
      </div>
    )
  },
)

ConfigNavItem.displayName = 'ConfigNavItem'
