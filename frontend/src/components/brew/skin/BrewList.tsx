import type { BrewItem } from '../../../types/brew'
import { useCallback, useRef } from 'react'

import { useI18n } from '../../../contexts/I18nContext'
import { Spinner } from '../../Spinner'
import {
  BrewEmpty,
  BrewEmptyAction,
  BrewEmptyRow,
} from '../ui/Empty'
import { StoryGrid } from '../ui/StoryCard'
import { BrewStory } from './BrewStory'
import { useBrewTimes } from './time'
import '../ui/brew.css'

export interface BrewListViewProps {
  items: BrewItem[]
  selectedItem: BrewItem | null
  loading: boolean
  hasMore: boolean
  total: number
  onItemSelect: (item: BrewItem) => void
  onLoadMore: () => void
  editMode?: boolean
  selectedIds?: Set<number>
  onItemSelectToggle?: (id: number) => void
  onToggleStar?: (item: BrewItem) => void
  emptyText?: string
  emptyAction?: { label: string; onClick: () => void }
}

export default function BrewListView({
  items,
  selectedItem,
  loading,
  hasMore,
  total,
  onItemSelect,
  onLoadMore,
  editMode,
  selectedIds,
  onItemSelectToggle,
  onToggleStar,
  emptyText,
  emptyAction,
}: BrewListViewProps) {
  const { t, locale, format } = useI18n()
  const observerRef = useRef<IntersectionObserver | null>(null)
  const times = useBrewTimes()
  const labels = t.brew

  const lastItemRef = useCallback(
    (node: HTMLDivElement | null) => {
      if (loading) return
      if (observerRef.current) observerRef.current.disconnect()
      observerRef.current = new IntersectionObserver(
        (entries) => {
          if (entries[0].isIntersecting && hasMore) onLoadMore()
        },
        { rootMargin: '100px' },
      )
      if (node) observerRef.current.observe(node)
    },
    [loading, hasMore, onLoadMore],
  )

  if (items.length === 0 && !loading) {
    return (
      <BrewEmpty>
        {emptyAction ? (
          <BrewEmptyRow>
            <p>{emptyText || t.brew.noArticles}</p>
            <BrewEmptyAction onClick={emptyAction.onClick}>
              {emptyAction.label}
            </BrewEmptyAction>
          </BrewEmptyRow>
        ) : (
          <p>{emptyText || t.brew.noArticles}</p>
        )}
      </BrewEmpty>
    )
  }

  return (
    <StoryGrid>
      {items.map((item, index) => {
        const last = index === items.length - 1
        return (
          <BrewStory
            key={item.id}
            ref={last ? lastItemRef : undefined}
            item={{
              id: item.id,
              title: item.title,
              summary: item.summary,
              image: item.image,
              published_at: item.published_at,
              is_read: item.is_read,
              is_starred: item.is_starred,
              topic: item.topic,
              author: item.author,
              source_name: item.source_name,
              source_icon: item.source_icon,
            }}
            times={times}
            locale={locale}
            labels={labels}
            current={selectedItem?.id === item.id}
            arrive={index < 8 ? index : undefined}
            picking={!!editMode}
            picked={!!selectedIds?.has(item.id)}
            onOpen={() => {
              if (editMode) onItemSelectToggle?.(item.id)
              else onItemSelect(item)
            }}
            onToggleStar={
              editMode || !onToggleStar
                ? undefined
                : (story) => {
                    onToggleStar({
                      ...item,
                      is_starred: !!story.is_starred,
                    })
                  }
            }
          />
        )
      })}
      {loading ? (
        <div className="brew-stories__more">
          <Spinner size="md" />
        </div>
      ) : null}
      {!loading && !hasMore && items.length > 0 ? (
        <p className="brew-stories__more">
          {format(t.brew.loadedAllArticles, { count: total })}
        </p>
      ) : null}
    </StoryGrid>
  )
}
