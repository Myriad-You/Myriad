import type { Ref } from 'react'
import type { FeedStory } from '../logic/feedStories'
import type { TopicNameKey } from '../logic/topics'

import type { TimeTranslations } from '../types'
import { memo } from 'react'

import { sameFeedStory } from '../logic/feedStories'
import { journalItemPath } from '../logic/journalRoutes'
import { StoryCard } from '../ui/StoryCard'
import { storyCardFace, storyCardInnerHtml } from './storyFace'

type PhantasiStoryItem = FeedStory

type StoryLabels = {
  unread: string
  starred: string
  unstar: string
} & Partial<Record<TopicNameKey, string>>

/** 卡片点开就是这篇的阅读页地址；聚合卡等非文章 id 不给链接。 */
function storyHref(item: PhantasiStoryItem): string | undefined {
  return item.id > 0 ? journalItemPath(item.id) : undefined
}

function samePlace(
  a?: { column: number; row: 1 | 2 },
  b?: { column: number; row: 1 | 2 },
): boolean {
  return a?.column === b?.column && a?.row === b?.row
}

export const PhantasiStory = memo(({
  ref,
  item,
  times,
  locale,
  labels,
  onOpen,
  onPeek,
  onPeekEnd,
  onToggleStar,
  current = false,
  picked = false,
  picking = false,
  arrive,
  place,
  railCol,
  eagerCover = false,
  holdCover = false,
  canStar,
}: {
  ref?: Ref<HTMLButtonElement>
  item: PhantasiStoryItem
  times: TimeTranslations
  locale: string
  labels: StoryLabels
  onOpen?: (item: PhantasiStoryItem) => void
  onPeek?: (item: PhantasiStoryItem) => void
  onPeekEnd?: () => void
  onToggleStar?: (item: PhantasiStoryItem) => void | false
  current?: boolean
  picked?: boolean
  picking?: boolean
  arrive?: number
  place?: { column: number; row: 1 | 2 }
  railCol?: number
  eagerCover?: boolean
  holdCover?: boolean
  canStar?: boolean
}) => {
  return (
    <StoryCard
      ref={ref}
      railId={item.id}
      arrive={arrive}
      place={place}
      railCol={railCol}
      current={current}
      picked={picked}
      picking={picking}
      unreadLabel={labels.unread}
      starLabel={labels.starred}
      unstarLabel={labels.unstar}
      href={onOpen ? storyHref(item) : undefined}
      onOpen={onOpen ? () => onOpen(item) : undefined}
      onPeek={onPeek ? () => onPeek(item) : undefined}
      onPeekEnd={onPeekEnd}
      onToggleStar={onToggleStar ? () => onToggleStar(item) : undefined}
      eagerCover={eagerCover}
      holdCover={holdCover}
      canStar={canStar}
      face={storyCardFace(item, times, locale, labels)}
    />
  )
}, (prev, next) => (
  (prev.item === next.item || sameFeedStory(prev.item, next.item))
  && prev.ref === next.ref
  && samePlace(prev.place, next.place)
  && prev.railCol === next.railCol
  && prev.locale === next.locale
  && prev.times === next.times
  && prev.labels === next.labels
  && prev.onOpen === next.onOpen
  && prev.onPeek === next.onPeek
  && prev.onPeekEnd === next.onPeekEnd
  && prev.onToggleStar === next.onToggleStar
  && prev.current === next.current
  && prev.picked === next.picked
  && prev.picking === next.picking
  && prev.arrive === next.arrive
  && prev.eagerCover === next.eagerCover
  && prev.holdCover === next.holdCover
  && prev.canStar === next.canStar
))
PhantasiStory.displayName = 'PhantasiStory'

const EMPTY_SLOTS: readonly { story: PhantasiStoryItem; column: number; row: 1 | 2 }[] = []

export const PhantasiStoryColumn = memo(({
  col,
  slots = EMPTY_SLOTS,
  holdCover,
  times,
  locale,
  labels,
  onOpen,
  onPeek,
  onPeekEnd,
  onToggleStar,
  canStar,
}: {
  col: number
  slots?: readonly { story: PhantasiStoryItem; column: number; row: 1 | 2 }[]
  holdCover: boolean
  times: TimeTranslations
  locale: string
  labels: StoryLabels
  onOpen?: (item: PhantasiStoryItem) => void
  onPeek?: (item: PhantasiStoryItem) => void
  onPeekEnd?: () => void
  onToggleStar?: (item: PhantasiStoryItem) => void | false
  canStar?: boolean
}) => {
  if (slots.length === 0) return null
  return (
    <>
      {slots.map((slot) => {
        const face = storyCardFace(slot.story, times, locale, labels)
        const showStar = canStar ?? !!onToggleStar
        const arrive = (slot.column - 1) * 2 + (slot.row - 1)
        return (
          <StoryCard
            key={`${col}:${slot.row}`}
            railId={slot.story.id}
            arrive={arrive < 8 ? arrive : undefined}
            place={slot}
            railCol={slot.column}
            unreadLabel={labels.unread}
            starLabel={labels.starred}
            unstarLabel={labels.unstar}
            href={storyHref(slot.story)}
            onOpen={onOpen ? () => onOpen(slot.story) : undefined}
            onPeek={onPeek ? () => onPeek(slot.story) : undefined}
            onPeekEnd={onPeekEnd}
            onToggleStar={onToggleStar ? () => onToggleStar(slot.story) : undefined}
            holdCover={holdCover}
            canStar={canStar}
            face={face}
            html={
              onOpen || onPeek || onPeekEnd || onToggleStar
                ? undefined
                : storyCardInnerHtml(
                    face,
                    labels.unread,
                    labels.starred,
                    labels.unstar,
                    holdCover,
                    showStar,
                  )
            }
          />
        )
      })}
    </>
  )
}, (prev, next) => (
  prev.col === next.col
  && prev.slots === next.slots
  && prev.times === next.times
  && prev.locale === next.locale
  && prev.labels === next.labels
  && prev.onOpen === next.onOpen
  && prev.onPeek === next.onPeek
  && prev.onPeekEnd === next.onPeekEnd
  && prev.onToggleStar === next.onToggleStar
  && prev.canStar === next.canStar
  && prev.holdCover === next.holdCover
))
PhantasiStoryColumn.displayName = 'PhantasiStoryColumn'
