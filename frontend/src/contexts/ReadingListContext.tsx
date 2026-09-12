import type { ReactNode } from 'react'
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from 'react'
import { brewSubject } from '../utils/brewSubject'

export interface ReadingListItem {
  id: number
  title: string
  author?: string
  sourceName?: string
  publishedAt?: string
  summary?: string
  relevanceReason?: string
  link?: string
  /** May still need a fetch for web-search items. */
  content?: string
  /** AI web search, not DB. */
  fromWebSearch?: boolean
}

export interface ReadingList {
  id: string
  name: string
  criteria: string
  items: ReadingListItem[]
  createdAt: Date
}

export interface ReadingProgress {
  listId: string
  currentIndex: number
  readIds: Set<number>
}

interface ReadingListContextType {
  currentList: ReadingList | null
  progress: ReadingProgress | null
  history: ReadingList[]

  setReadingList: (list: ReadingList) => void
  clearReadingList: () => void
  goToArticle: (index: number) => void
  markReadAndNext: () => ReadingListItem | null
  getPrevious: () => ReadingListItem | null
  /** Does not mark the current item read. */
  getNext: () => ReadingListItem | null
  getCurrentItem: () => ReadingListItem | null
  isInReadingList: (itemId: number) => boolean
  getPositionInfo: (itemId: number) => {
    index: number
    total: number
    hasPrev: boolean
    hasNext: boolean
  } | null
}

const ReadingListContext = createContext<ReadingListContextType | null>(null)

export function ReadingListProvider({ children }: { children: ReactNode }) {
  const subject = useSyncExternalStore(
    brewSubject.subscribe,
    brewSubject.getSnapshot,
    brewSubject.getSnapshot,
  )
  const [currentList, setCurrentListState] = useState<ReadingList | null>(null)
  const [progress, setProgress] = useState<ReadingProgress | null>(null)
  const [history, setHistory] = useState<ReadingList[]>([])
  const [generation, setGeneration] = useState(subject.generation)
  if (generation !== subject.generation) {
    setGeneration(subject.generation)
    setCurrentListState(null)
    setProgress(null)
    setHistory([])
  }

  const setReadingList = useCallback((list: ReadingList) => {
    setCurrentListState(list)
    setProgress({
      listId: list.id,
      currentIndex: 0,
      readIds: new Set(),
    })
    setHistory((prev) => {
      const filtered = prev.filter((h) => h.id !== list.id)
      return [list, ...filtered].slice(0, 10)
    })
  }, [])

  const clearReadingList = useCallback(() => {
    setCurrentListState(null)
    setProgress(null)
  }, [])

  const goToArticle = useCallback(
    (index: number) => {
      if (!currentList || index < 0 || index >= currentList.items.length) return
      setProgress((prev) => (prev ? { ...prev, currentIndex: index } : null))
    },
    [currentList],
  )

  const getCurrentItem = useCallback((): ReadingListItem | null => {
    if (!currentList || !progress) return null
    return currentList.items[progress.currentIndex] || null
  }, [currentList, progress])

  const getPrevious = useCallback((): ReadingListItem | null => {
    if (!currentList || !progress || progress.currentIndex <= 0) return null
    return currentList.items[progress.currentIndex - 1]
  }, [currentList, progress])

  const getNext = useCallback((): ReadingListItem | null => {
    if (
      !currentList ||
      !progress ||
      progress.currentIndex >= currentList.items.length - 1
    ) {
      return null
    }
    return currentList.items[progress.currentIndex + 1]
  }, [currentList, progress])

  const markReadAndNext = useCallback((): ReadingListItem | null => {
    if (!currentList || !progress) return null

    const currentItem = currentList.items[progress.currentIndex]
    const newReadIds = new Set(progress.readIds)
    if (currentItem) {
      newReadIds.add(currentItem.id)
    }

    if (progress.currentIndex < currentList.items.length - 1) {
      setProgress({
        ...progress,
        currentIndex: progress.currentIndex + 1,
        readIds: newReadIds,
      })
      return currentList.items[progress.currentIndex + 1]
    } else {
      setProgress({ ...progress, readIds: newReadIds })
      return null
    }
  }, [currentList, progress])

  const isInReadingList = useCallback(
    (itemId: number): boolean => {
      if (!currentList) return false
      return currentList.items.some((item) => item.id === itemId)
    },
    [currentList],
  )

  const getPositionInfo = useCallback(
    (itemId: number) => {
      if (!currentList) return null
      const index = currentList.items.findIndex((item) => item.id === itemId)
      if (index === -1) return null
      return {
        index,
        total: currentList.items.length,
        hasPrev: index > 0,
        hasNext: index < currentList.items.length - 1,
      }
    },
    [currentList],
  )

  useEffect(() => {
    const handleSetReadingList = (event: CustomEvent<ReadingList>) => {
      if (!brewSubject.getSnapshot().active) return
      console.log(
        '[ReadingListContext] Received set-reading-list event:',
        event.detail,
      )
      setReadingList(event.detail)
    }

    window.addEventListener(
      'agent:set-reading-list',
      handleSetReadingList as EventListener,
    )
    return () => {
      window.removeEventListener(
        'agent:set-reading-list',
        handleSetReadingList as EventListener,
      )
    }
  }, [setReadingList])

  const value = useMemo(
    () => ({
      currentList,
      progress,
      history,
      setReadingList,
      clearReadingList,
      goToArticle,
      markReadAndNext,
      getPrevious,
      getNext,
      getCurrentItem,
      isInReadingList,
      getPositionInfo,
    }),
    [
      currentList,
      progress,
      history,
      setReadingList,
      clearReadingList,
      goToArticle,
      markReadAndNext,
      getPrevious,
      getNext,
      getCurrentItem,
      isInReadingList,
      getPositionInfo,
    ],
  )

  return (
    <ReadingListContext.Provider value={value}>
      {children}
    </ReadingListContext.Provider>
  )
}

export function useReadingList() {
  const context = useContext(ReadingListContext)
  if (!context) {
    throw new Error('useReadingList must be used within a ReadingListProvider')
  }
  return context
}

/** Returns null outside the provider. */
export function useReadingListOptional() {
  return useContext(ReadingListContext)
}
