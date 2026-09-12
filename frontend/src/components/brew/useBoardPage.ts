/** skin 不进口。 */
import type { BrewItemPreview, BrewSource } from '../../types/brew'
import type { BrewBoard, SourceSortMode } from './logic/board'
import type { FeedStory, FeedStorySlot } from './logic/feedStories'
import type { HomeBoardNote } from './logic/homeBoard'
import type { BrewViewerRole } from './logic/score'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useI18n } from '../../contexts/I18nContext'

import {
  collectSourceCategories,
  filterSourcesByQuery,
  sortSourcesForBoard,
  sourcesForBoard,
} from './logic/board'
import { paintReadyStories, storiesAreFresh, storiesForSource } from './logic/feedStories'
import { noteSourceKey } from './logic/homeBoard'
import {
  loadFeedStories,
  loadHomeBoardNotes,
  peekFeedStories,
  peekFeedStoriesLoose,
} from './pageData'
import { useArticleFlags } from './useArticleFlags'

export function useBoardCatalog(
  sources: BrewSource[],
  board: BrewBoard,
  searchQuery: string,
  sortMode: SourceSortMode,
  role: BrewViewerRole,
  now: number,
) {
  const { locale } = useI18n()
  const categories = useMemo(() => collectSourceCategories(sources), [sources])
  const filtered = useMemo(
    () => filterSourcesByQuery(sourcesForBoard(sources, board), searchQuery),
    [sources, board, searchQuery],
  )
  const sorted = useMemo(
    () => sortSourcesForBoard(filtered, sortMode, role, now, locale),
    [filtered, sortMode, role, now, locale],
  )
  return { categories, filtered, sorted }
}

export function useBoardNotes(
  board: BrewBoard,
  sources: Array<{ id: number; source_type: string }>,
): HomeBoardNote[] {
  const key = useMemo(() => noteSourceKey(sources), [sources])
  const sourcesRef = useRef(sources)
  sourcesRef.current = sources
  const [notes, setNotes] = useState<HomeBoardNote[]>([])

  useEffect(() => {
    if (board !== 'notes') return
    if (!key) {
      setNotes([])
      return
    }
    const controller = new AbortController()
    void loadHomeBoardNotes(sourcesRef.current, controller.signal)
      .then((next) => {
        if (!controller.signal.aborted) setNotes(next)
      })
      .catch(() => {
        if (!controller.signal.aborted) setNotes([])
      })
    return () => {
      controller.abort()
    }
  }, [board, key])

  return notes
}

export function useFeedStories(
  board: BrewBoard,
  readySource: BrewSource | null,
  onToggleStar?: (item: BrewItemPreview) => void | false | Promise<void | false>,
): { stories: FeedStory[]; onStar: (item: BrewItemPreview) => void } {
  const flags = useArticleFlags()
  const readySourceId = readySource?.id ?? null
  const stamp = readySource?.last_success_at ?? 0
  const [local, setLocal] = useState<{
    id: number
    stamp: number
    items: FeedStory[]
  } | null>(null)
  const localRef = useRef(local)
  localRef.current = local
  const slotsRef = useRef<Map<number, FeedStorySlot>>(new Map())

  useEffect(() => {
    if (board !== 'feeds' || readySourceId == null) return
    const controller = new AbortController()
    const exact = peekFeedStories(readySourceId, stamp)
    const slot = slotsRef.current.get(readySourceId)
    const painted = paintReadyStories(
      stamp,
      exact,
      peekFeedStoriesLoose(readySourceId),
      slot,
    )
    if (painted) setLocal({ id: readySourceId, stamp, items: painted })
    if (exact && slot?.stamp !== stamp) {
      slotsRef.current.set(readySourceId, { stamp, items: exact })
    }
    if (!storiesAreFresh(stamp, exact, slot)) {
      void loadFeedStories(readySourceId, stamp, controller.signal)
        .then((items) => {
          if (controller.signal.aborted) return
          slotsRef.current.set(readySourceId, { stamp, items })
          setLocal({ id: readySourceId, stamp, items })
        })
        .catch(() => {
          /* 换源失败不闪空 */
        })
    }
    return () => {
      controller.abort()
    }
  }, [board, readySourceId, stamp])

  const painted =
    readySourceId == null
      ? null
      : paintReadyStories(
          stamp,
          peekFeedStories(readySourceId, stamp),
          peekFeedStoriesLoose(readySourceId),
          slotsRef.current.get(readySourceId) ??
            (local?.id === readySourceId
              ? { stamp: local.stamp, items: local.items }
              : undefined),
        )
  const stories = storiesForSource(
    painted && readySourceId != null
      ? { id: readySourceId, items: painted }
      : null,
    readySource,
  )

  const onStar = useCallback(
    (item: BrewItemPreview) => onToggleStar?.(flags.project(item)),
    [onToggleStar, flags],
  )

  return { stories: stories.map(story => flags.project(story)), onStar }
}
