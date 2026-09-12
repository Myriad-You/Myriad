import type { FeedStory } from '../logic/feedStories'
import type { TopicNameKey } from '../logic/topics'
import type { TimeTranslations } from '../types'

import { forwardRef } from 'react'

import { getIconUrl, getImageUrl, getPlainText } from '../constants'
import { topicDisplayName, topicHue, topicNameKey } from '../logic/topics'
import { StoryCard } from '../ui/StoryCard'
import { brewRelativeTime } from './time'

export { BrewPick } from '../ui/Pick'

export type BrewStoryItem = FeedStory

export const BrewStory = forwardRef<
  HTMLDivElement,
  {
    item: BrewStoryItem
    times: TimeTranslations
    locale: string
    labels: {
      unread: string
      starred: string
      unstar: string
    } & Partial<Record<TopicNameKey, string>>
    onOpen: () => void
    onPeek?: () => void
    onPeekEnd?: () => void
    onToggleStar?: (item: BrewStoryItem) => void | false
    current?: boolean
    picked?: boolean
    picking?: boolean
    arrive?: number
  }
>((
  {
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
  },
  ref,
) => {
  const topicKey = item.topic ? topicNameKey(item.topic) : null
  return (
    <StoryCard
      ref={ref}
      railId={item.id}
      arrive={arrive}
      current={current}
      picked={picked}
      picking={picking}
      unreadLabel={labels.unread}
      starLabel={labels.starred}
      unstarLabel={labels.unstar}
      onOpen={onOpen}
      onPeek={onPeek}
      onPeekEnd={onPeekEnd}
      onToggleStar={onToggleStar ? () => onToggleStar(item) : undefined}
      face={{
        id: item.id,
        title: item.title,
        summary: item.summary ? getPlainText(item.summary) : '',
        cover: getImageUrl(item.image),
        source: item.source_name?.trim() || '',
        sourceIcon: getIconUrl(item.source_icon ?? null),
        when: brewRelativeTime(item.published_at, times, locale),
        topic:
          item.topic && topicKey
            ? topicDisplayName({ key: item.topic, nameKey: topicKey }, labels)
            : null,
        hue: item.topic ? topicHue(item.topic) : null,
        author: item.author,
        unread: !item.is_read,
        starred: !!item.is_starred,
      }}
    />
  )
})
