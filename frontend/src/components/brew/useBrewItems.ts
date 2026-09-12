/** 订阅轨不走这里。 */
import type { BrewItem } from '../../types/brew'
import type { BrewViewMode } from './logic/board'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import * as brewApi from '../../services/brewApi'
import { reportBrewError } from './brewNotice'
import { itemListHasMore, itemListRequest } from './logic/itemList'
import { appendUniqueById } from './logic/itemState'
import { RequestTurn } from './logic/requestTurn'
import { useArticleFlags } from './useArticleFlags'

export function useBrewItems(
  loadFailed: string,
  setError: (message: string) => void,
  viewMode: BrewViewMode,
  topicKey?: string,
) {
  const flags = useArticleFlags()
  const flagsRevision = flags.getSnapshot()
  const [rawItems, setItems] = useState<BrewItem[]>([])
  const items = useMemo(() => rawItems.map(item => flags.project(item)).filter(item => viewMode !== 'starred' || item.is_starred !== false), [rawItems, flagsRevision, viewMode])
  const [itemsLoading, setItemsLoading] = useState(false)
  const [hasMore, setHasMore] = useState(true)
  const [total, setTotal] = useState(0)
  const nextCursorRef = useRef<string | null>(null)
  const membershipDirty = useRef(false)
  const loadRequestIdRef = useRef(0)
  const loadingRef = useRef(false)
  const turns = useRef(new RequestTurn())
  const itemsRef = useRef<BrewItem[]>([])
  itemsRef.current = items

  const loadItems = useCallback(
    async (reset = false) => {
      if (!reset && loadingRef.current) return
      if (viewMode !== 'starred' && !(viewMode === 'topic-feed' && topicKey)) {
        return
      }

      const requestId = ++loadRequestIdRef.current
      const signal = turns.current.begin()
      loadingRef.current = true
      setItemsLoading(true)
      try {
        const rebuild = reset || membershipDirty.current
        if (rebuild) nextCursorRef.current = null
        const cursor = rebuild ? undefined : nextCursorRef.current ?? undefined
        const paging = itemListRequest({ cursor, perPage: 20 })
        const data = await brewApi.getItemPreviews(
          {
            topic: viewMode === 'topic-feed' ? topicKey : undefined,
            filter: viewMode === 'starred' ? 'starred' : 'all',
            ...paging,
          },
          undefined,
          { signal },
        )
        if (signal.aborted || requestId !== loadRequestIdRef.current) return
        const collected = data.items.map((item) => ({ ...item, content: null }))
        if (rebuild) setItems(collected)
        else setItems(prev => appendUniqueById(prev, collected))
        membershipDirty.current = false
        nextCursorRef.current = data.next_cursor ?? null
        if (!cursor && data.total > 0) setTotal(data.total)
        const perPage = data.per_page > 0 ? data.per_page : 20
        setHasMore(itemListHasMore(data.next_cursor, data.items.length, perPage))
      } catch (err) {
        if (signal.aborted || requestId !== loadRequestIdRef.current) return
        if (membershipDirty.current) setHasMore(true)
        reportBrewError(err, loadFailed, setError)
      } finally {
        if (!signal.aborted && requestId === loadRequestIdRef.current) {
          loadingRef.current = false
          setItemsLoading(false)
        }
      }
    },
    [loadFailed, setError, viewMode, topicKey],
  )

  useEffect(() => {
    membershipDirty.current = false
    nextCursorRef.current = null
    setItems([])
    setTotal(0)
    setItemsLoading(false)
    setHasMore(true)
    void loadItems(true)
    return () => {
      turns.current.cancel()
      loadRequestIdRef.current++
      loadingRef.current = false
    }
  }, [loadItems])

  useEffect(() => {
    if (viewMode !== 'starred') return
    let timer: ReturnType<typeof setTimeout> | undefined
    const unsubscribe = flags.subscribeMutations(patch => {
      if (typeof patch.is_starred !== 'boolean') return
      membershipDirty.current = true
      turns.current.cancel()
      loadRequestIdRef.current++
      loadingRef.current = false
      setItemsLoading(false)
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => { void loadItems(true) }, 100)
    })
    return () => { unsubscribe(); if (timer) clearTimeout(timer) }
  }, [viewMode, loadItems])

  const loadMore = useCallback(() => {
    if (itemsLoading || (!hasMore && !membershipDirty.current)) return
    void loadItems(false)
  }, [hasMore, itemsLoading, loadItems])

  return {
    items,
    setItems,
    itemsRef,
    itemsLoading,
    hasMore,
    total,
    setTotal,
    loadItems,
    loadMore,
  }
}
