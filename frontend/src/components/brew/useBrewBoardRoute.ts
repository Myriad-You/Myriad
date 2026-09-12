import type { BrewSource } from '../../types/brew'
import type { boardEntry, BrewBoard,
  BrewViewMode } from './logic/board'

import type { TopicNameKey } from './logic/topics'
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  eatSearchKeys,
  resolveBoardParam,
  viewForBoardEntry,
} from './logic/board'
import { topicNameKey } from './logic/topics'

export function useBrewBoardRoute(
  isAuthenticated: boolean,
  sources: BrewSource[],
  activeId: string,
  setActiveId: (id: string) => void,
  searchParams: URLSearchParams,
  setSearchParams: (
    next: URLSearchParams | ((prev: URLSearchParams) => URLSearchParams),
    opts?: { replace?: boolean },
  ) => void,
) {
  const [viewMode, setViewMode] = useState<BrewViewMode>('sources')
  const [board, setBoard] = useState<BrewBoard>('feeds')
  const [selectedTopic, setSelectedTopic] = useState<{
    key: string
    nameKey: TopicNameKey
  } | null>(null)
  const [railFocusId, setRailFocusId] = useState<number | null>(null)

  const applyBoardEntry = useCallback(
    (entry: ReturnType<typeof boardEntry>) => {
      setBoard(entry.board)
      setViewMode(viewForBoardEntry(entry, isAuthenticated))
    },
    [isAuthenticated],
  )

  const prevActiveIdRef = useRef(activeId)
  useEffect(() => {
    if (prevActiveIdRef.current === activeId) return
    prevActiveIdRef.current = activeId
    const entry = resolveBoardParam(activeId)
    if (entry) applyBoardEntry(entry)
  }, [activeId, applyBoardEntry])

  useEffect(() => {
    const raw = searchParams.get('board') ?? searchParams.get('category')
    if (!raw) return
    const entry = resolveBoardParam(raw)
    if (!entry) return

    applyBoardEntry(entry)
    prevActiveIdRef.current = entry.board
    setActiveId(entry.board)
    setSearchParams((prev) => eatSearchKeys(prev, ['board', 'category']), {
      replace: true,
    })
  }, [searchParams, applyBoardEntry, setActiveId, setSearchParams])

  const openTopic = useCallback((topicKey: string, nameKey: TopicNameKey) => {
    setSelectedTopic({ key: topicKey, nameKey })
    setViewMode('topic-feed')
  }, [])

  const focusSource = useCallback(
    (source: BrewSource) => {
      setBoard('feeds')
      setViewMode('sources')
      setActiveId('feeds')
      setRailFocusId(source.id)
    },
    [setActiveId],
  )

  useEffect(() => {
    const topicParam = searchParams.get('topic')
    if (topicParam) {
      const nameKey = topicNameKey(topicParam)
      if (nameKey) openTopic(topicParam, nameKey)
      setSearchParams((prev) => eatSearchKeys(prev, ['topic']), {
        replace: true,
      })
      return
    }

    const sourceParam = searchParams.get('source')
    if (!sourceParam) return
    const id = Number(sourceParam)
    if (!Number.isFinite(id)) return
    const target = sources.find((source) => source.id === id)
    if (!target) return

    focusSource(target)
    setSearchParams((prev) => eatSearchKeys(prev, ['source']), {
      replace: true,
    })
  }, [searchParams, sources, openTopic, focusSource, setSearchParams])

  const backFromTopic = useCallback(() => {
    setSelectedTopic(null)
    setViewMode('sources')
  }, [])

  const openStarred = useCallback(() => {
    setViewMode('starred')
  }, [])

  const backToFeeds = useCallback(() => {
    setBoard('feeds')
    setViewMode('sources')
    setActiveId('feeds')
  }, [setActiveId])

  return {
    viewMode,
    setViewMode,
    board,
    selectedTopic,
    railFocusId,
    openTopic,
    focusSource,
    backFromTopic,
    openStarred,
    backToFeeds,
  }
}
