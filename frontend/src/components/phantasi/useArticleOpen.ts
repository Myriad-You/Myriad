import type { PhantasiItem } from '../../types/phantasi'
import type { ReadingQueue } from './logic/readingQueue'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { phantasiSubject } from '../../utils/phantasiSubject'
import { cancelArticlePrefetch, prefetchArticleDetails } from './articlePrefetch'
import { neighborsInQueue } from './logic/readingQueue'
import { RequestTurn } from './logic/requestTurn'
import { reportPhantasiError } from './phantasiNotice'
import { useArticleFlags } from './useArticleFlags'

export type ArticleLoader = (
  signal: AbortSignal,
) => Promise<PhantasiItem | null | undefined>

export interface OpenArticleOptions {
  onOpened?: (item: PhantasiItem) => void
  /** Pass null to drop a previous queue (deep link / unrelated entry). */
  queue?: ReadingQueue | null
}

export function useArticleOpen(
  setError: (message: string) => void,
  loadFailed: string,
) {
  const flags = useArticleFlags()
  const flagsRevision = flags.getSnapshot()
  const [rawSelectedItem, setSelectedItem] = useState<PhantasiItem | null>(null)
  const selectedItem = useMemo(() => rawSelectedItem && !rawSelectedItem.fromWebSearch ? flags.project(rawSelectedItem) : rawSelectedItem, [rawSelectedItem, flagsRevision])
  const [queue, setQueue] = useState<ReadingQueue | null>(null)
  const [opening, setOpening] = useState(false)
  const turns = useRef(new RequestTurn())
  useEffect(
    () => () => {
      turns.current.cancel()
      cancelArticlePrefetch()
    },
    [],
  )

  const openArticle = useCallback(
    async (
      target: PhantasiItem | ArticleLoader,
      options?: OpenArticleOptions,
    ) => {
      const signal = turns.current.begin()
      const subject = phantasiSubject.getSnapshot()
      setOpening(true)
      try {
        phantasiSubject.assert(subject)
        const item =
          typeof target === 'function' ? await target(signal) : target
        if (signal.aborted) return
        phantasiSubject.assert(subject)
        if (!item) throw new Error(loadFailed)
        if (options && Object.hasOwn(options, 'queue')) setQueue(options.queue ?? null)
        setSelectedItem(item)
        const activeQueue =
          options && Object.hasOwn(options, 'queue')
            ? options.queue ?? null
            : queue
        const next = neighborsInQueue(activeQueue, item.id)?.next
        if (next && !item.fromWebSearch) prefetchArticleDetails([next.id])
        options?.onOpened?.(item)
        return item
      } catch (error) {
        if (!signal.aborted) reportPhantasiError(error, loadFailed, setError)
      } finally {
        if (!signal.aborted) setOpening(false)
      }
    },
    [loadFailed, setError, queue],
  )

  const cancelOpen = useCallback(() => {
    turns.current.cancel()
    cancelArticlePrefetch()
    setOpening(false)
  }, [])

  const closeArticle = useCallback(() => {
    cancelOpen()
    setSelectedItem(null)
    setQueue(null)
  }, [cancelOpen])

  return { selectedItem, setSelectedItem, opening, openArticle, cancelOpen, closeArticle, queue }
}
