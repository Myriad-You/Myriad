import type { Dispatch, MutableRefObject, SetStateAction } from 'react'
import type {
  BrewItem,
  BrewItemPreview,
  BrewSource,
} from '../../types/brew'
import type { BrewViewMode } from './logic/board'

import type { ArticleLoader, OpenArticleOptions } from './useArticleOpen'
import { useCallback, useEffect, useRef } from 'react'
import * as brewApi from '../../services/brewApi'
import { brewItemState } from '../../utils/brewItemState'
import { reportBrewError } from './brewNotice'
import { trackBrew } from './brewTrack'
import { isSiteSource } from './logic/board'
import {
  dropItem,
} from './logic/itemState'
import { readingQueue, readingQueueFromStories } from './logic/readingQueue'
import { brewItemFromWebSearch, webSearchInList } from './logic/webSearchItem'
import {
  loadLatestStory,
  peekFeedStoriesLoose,
} from './pageData'

interface StarTarget {
  id: number
  is_starred: boolean
  source_id?: number
}

type ReadingListNav = {
  currentList?: {
    items: Array<{
      id: number
      title: string
      author?: string | null
      sourceName?: string | null
      publishedAt?: string | null
      summary?: string | null
      relevanceReason?: string | null
      link?: string | null
      content?: string | null
      fromWebSearch?: boolean
    }>
  } | null
  goToArticle: (index: number) => void
} | null

export function useBrewItemActions({
  isAuthenticated,
  openArticle,
  viewMode,
  selectedItem,
  setItems,
  setTotal,
  setError,
  itemsRef,
  unselectStarred,
  navigate,
  readingList,
  labels,
}: {
  openArticle: (
    target: BrewItem | ArticleLoader,
    options?: OpenArticleOptions,
  ) => Promise<BrewItem | undefined>
  isAuthenticated: boolean
  viewMode: BrewViewMode
  selectedItem: BrewItem | null
  setItems: Dispatch<SetStateAction<BrewItem[]>>
  setTotal: Dispatch<SetStateAction<number>>
  setError: (message: string) => void
  itemsRef: MutableRefObject<BrewItem[]>
  unselectStarred: (id: number) => void
  navigate: (to: string) => void
  readingList: ReadingListNav
  labels: {
    starFailed: string
    readingFailed: string
    loadFailed: string
    webSearch: string
  }
}) {
  const pendingReads = useRef(new Set<number>())
  const pendingStars = useRef(new Set<number>())
  const markOpened = useCallback(
    async (item: BrewItem) => {
      trackBrew('BREW_OPEN_ITEM', item.source_id || item.id, 1500)
      if (
        !isAuthenticated ||
        item.fromWebSearch ||
        pendingReads.current.has(item.id)
      ) {
        return
}
      if (!item.is_read) {
        pendingReads.current.add(item.id)
        brewItemState.preview(item.id, { is_read: true })
        try {
          await brewApi.markRead(item.id)
        } catch (err) {
          brewItemState.discardPreview(item.id, { is_read: true })
          reportBrewError(err, labels.readingFailed, setError)
        } finally {
          pendingReads.current.delete(item.id)
        }
      }
    },
    [
      isAuthenticated,
      openArticle,
      setError,
      labels.readingFailed,
    ],
  )

  const markOpenedRef = useRef(markOpened)
  markOpenedRef.current = markOpened
  useEffect(() => {
    if (selectedItem) void markOpenedRef.current(selectedItem)
    // Opening is an item transition; field updates must not re-mark unread items.
  }, [selectedItem?.id, isAuthenticated])

  const select = useCallback(
    async (item: BrewItem) => {
      const origin =
        viewMode === 'starred'
          ? 'starred'
          : viewMode === 'topic-feed'
            ? 'topic'
            : 'feeds'
      await openArticle(
        item.fromWebSearch
          ? item
          : (signal) => brewApi.getItem(item.id, undefined, { signal }),
        {
          queue: readingQueue(origin, itemsRef.current),
        },
      )
    },
    [openArticle, viewMode, itemsRef],
  )

  const openPreview = useCallback(
    async (preview: BrewItemPreview, source: BrewSource) => {
      if (isSiteSource(source)) return
      await openArticle(
        (signal) => brewApi.getItem(preview.id, undefined, { signal }),
        {
          queue: readingQueueFromStories(
            'feeds',
            peekFeedStoriesLoose(source.id),
            [preview],
          ),
        },
      )
    },
    [openArticle],
  )

  const openLatest = useCallback(
    async (source: BrewSource) => {
      if (isSiteSource(source)) return
      await openArticle(
        async (signal) => {
          const first = await loadLatestStory(source, signal)
          return first
            ? brewApi.getItem(first.id, undefined, { signal })
            : null
        },
        {
          queue: readingQueueFromStories(
            'feeds',
            peekFeedStoriesLoose(source.id),
            [],
          ),
        },
      )
    },
    [openArticle],
  )

  const navigateToArticle = useCallback(
    async (articleId: number) => {
      const webHit = webSearchInList(readingList?.currentList?.items, articleId)
      if (webHit && readingList) {
        console.log(
          '[Brew] Navigating to web search article from reading list:',
          webHit.item.title,
        )
        await openArticle(brewItemFromWebSearch(webHit.item, labels.webSearch))
        readingList.goToArticle(webHit.index)
        return
      }

      await openArticle(
        (signal) => brewApi.getItem(articleId, undefined, { signal }),
      )
    },
    [
      readingList,
      openArticle,
      itemsRef,
      setError,
      markOpened,
      labels.webSearch,
      labels.loadFailed,
    ],
  )

  const toggleStar = useCallback(
    (item: StarTarget): false | Promise<void | false> => {
      if (!isAuthenticated) {
        navigate('/login')
        return false
      }
      if (pendingStars.current.has(item.id)) return false
      pendingStars.current.add(item.id)
      const nextStarred = !item.is_starred
      brewItemState.preview(item.id, { is_starred: nextStarred })
      return (async () => {
        try {
          if (item.is_starred) {
            await brewApi.unstarItem(item.id)
            trackBrew('BREW_UNSTAR', item.source_id || item.id, 1000)
          } else {
            await brewApi.starItem(item.id)
            trackBrew('BREW_STAR', item.source_id || item.id, 1000)
          }
          const next = !item.is_starred
          if (viewMode === 'starred' && !next) {
            setItems((prev) => dropItem(prev, item.id))
            setTotal((prev) => Math.max(0, prev - 1))
            unselectStarred(item.id)
          }
        } catch (err) {
          brewItemState.discardPreview(item.id, { is_starred: nextStarred })
          reportBrewError(err, labels.starFailed, setError)
          return false
        } finally {
          pendingStars.current.delete(item.id)
        }
      })()
    },
    [
      isAuthenticated,
      navigate,
      viewMode,
      selectedItem?.id,
      unselectStarred,
      setItems,
      setTotal,
      setError,
      labels.starFailed,
    ],
  )

  const toggleRead = useCallback(
    async (item: BrewItem) => {
      if (
        !isAuthenticated ||
        item.fromWebSearch ||
        pendingReads.current.has(item.id)
      ) {
        return
}
      pendingReads.current.add(item.id)
      const nextRead = !item.is_read
      brewItemState.preview(item.id, { is_read: nextRead })
      try {
        if (item.is_read) await brewApi.markUnread(item.id)
        else await brewApi.markRead(item.id)
      } catch (err) {
        brewItemState.discardPreview(item.id, { is_read: nextRead })
        reportBrewError(err, labels.readingFailed, setError)
      } finally {
        pendingReads.current.delete(item.id)
      }
    },
    [
      isAuthenticated,
      selectedItem?.id,
      setError,
      labels.readingFailed,
    ],
  )

  const markAllRead = useCallback(async () => {
    try {
      await brewApi.markAllRead({})
    } catch (err) {
      reportBrewError(err, labels.readingFailed, setError)
    }
  }, [setError, labels.readingFailed])

  return {
    select,
    openPreview,
    openLatest,
    navigateToArticle,
    toggleStar,
    toggleRead,
    markAllRead,
  }
}
