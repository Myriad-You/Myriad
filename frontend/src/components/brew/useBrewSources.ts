/** skin 不进口。 */
import type {
  AddSourceInput,
  BrewSource,
  BrewStats,
  UpdateSourceRequest,
} from '../../types/brew'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import * as brewApi from '../../services/brewApi'
import { brewItemState } from '../../utils/brewItemState'
import { reportBrewError } from './brewNotice'
import { RequestTurn } from './logic/requestTurn'
import { useArticleFlags } from './useArticleFlags'

export function useBrewSources(
  isAuthenticated: boolean,
  labels: { loadFailed: string; refreshFailed: string },
  setError: (message: string) => void,
) {
  const flags = useArticleFlags()
  const flagsRevision = flags.getSnapshot()
  const [rawSources, setSources] = useState<BrewSource[]>([])
  const sources = useMemo(() => rawSources.map(source => ({ ...source, recent_items: source.recent_items?.map(item => flags.project(item)) })), [rawSources, flagsRevision])
  const [sourcesLoaded, setSourcesLoaded] = useState(false)
  const [stats, setStats] = useState<BrewStats | null>(null)
  const [booting, setBooting] = useState(true)

  const sourceRequest = useRef(0)
  const statsRequest = useRef(0)
  const sourceTurns = useRef(new RequestTurn())
  const statsTurns = useRef(new RequestTurn())

  const loadSources = useCallback(async () => {
    const request = ++sourceRequest.current
    const signal = sourceTurns.current.begin()
    try {
      const data = await brewApi.getSources(undefined, { signal })
      if (signal.aborted || request !== sourceRequest.current) return
      setSourcesLoaded(true)
      setSources(data)
    } catch (err) {
      if (signal.aborted || request !== sourceRequest.current) return
      reportBrewError(err, labels.loadFailed, setError)
    }
  }, [labels.loadFailed, setError])

  const loadStats = useCallback(async () => {
    const request = ++statsRequest.current
    const signal = statsTurns.current.begin()
    try {
      const next = await brewApi.getStats(undefined, { signal })
      if (signal.aborted || request !== statsRequest.current) return
      setStats(next)
    } catch (err) {
      if (signal.aborted || request !== statsRequest.current) return
      console.error('Failed to load stats:', err)
    }
  }, [])

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined
    const unsubscribe = brewItemState.subscribeMutations(() => {
      sourceTurns.current.cancel()
      statsTurns.current.cancel()
      sourceRequest.current++
      statsRequest.current++
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        void loadStats()
        void loadSources()
      }, 100)
    })
    return () => { unsubscribe(); if (timer) clearTimeout(timer) }
  }, [loadSources, loadStats])

  useEffect(() => {
    let cancelled = false
    setBooting(true)
    void Promise.all([loadSources(), loadStats()]).finally(() => {
      if (!cancelled) setBooting(false)
    })
    return () => {
      cancelled = true
      sourceTurns.current.cancel()
      statsTurns.current.cancel()
      sourceRequest.current++
      statsRequest.current++
    }
  }, [loadSources, loadStats])

  const refreshRef = useRef(() => {})
  refreshRef.current = () => {
    void loadSources()
    void loadStats()
  }

  useEffect(() => {
    if (!isAuthenticated) return
    let closed = false
    let ws: WebSocket | null = null
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null

    const connect = () => {
      if (closed) return
      try {
        ws = brewApi.createBrewWebSocket(
          (notification) => {
            console.debug('[Brew] WS notification', notification)
            refreshRef.current()
          },
          () => {
            if (closed) return
            reconnectTimer = setTimeout(connect, 5000)
          },
        )
        ws.onclose = () => {
          if (closed) return
          reconnectTimer = setTimeout(connect, 5000)
        }
      } catch (err) {
        console.warn('[Brew] WS connect failed', err)
        reconnectTimer = setTimeout(connect, 8000)
      }
    }
    connect()

    return () => {
      closed = true
      if (reconnectTimer) clearTimeout(reconnectTimer)
      try {
        ws?.close()
      } catch {
        /* ignore */
      }
    }
  }, [isAuthenticated])

  const reloadBoard = useCallback(() => {
    void loadSources()
    void loadStats()
  }, [loadSources, loadStats])

  const updateSource = useCallback(
    async (id: number, data: UpdateSourceRequest) => {
      const updated = await brewApi.updateSource(id, data)
      setSources((prev) =>
        prev.map((source) => (source.id === id ? updated : source)),
      )
      void loadStats()
    },
    [loadStats],
  )

  const importOpml = useCallback(
    async (content: string, signal?: AbortSignal) => {
      const result = await brewApi.importOpml(
        content,
        undefined,
        signal ? { signal } : undefined,
      )
      if (!signal?.aborted) reloadBoard()
      return {
        imported: result.imported || 0,
        skipped: result.skipped || 0,
      }
    },
    [reloadBoard],
  )

  const removeSources = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return
      await Promise.all(ids.map((id) => brewApi.deleteSource(id)))
      const dropped = new Set(ids)
      setSources((prev) =>
        Iterator.from(prev)
          .filter((source) => !dropped.has(source.id))
          .toArray(),
      )
      void loadStats()
    },
    [loadStats],
  )

  const addSource = useCallback(
    async ({
      url,
      name,
      category,
      icon,
      sourceType,
      feedType,
      notionToken,
    }: AddSourceInput) => {
      const source = await brewApi.addSource({
        url,
        name,
        category,
        source_type: sourceType,
        feed_type: feedType,
        extra_config: notionToken ? { token: notionToken } : undefined,
      })
      if (icon && source.id) {
        const updated = await brewApi.updateSource(source.id, { icon })
        setSources((prev) => [...prev, updated])
      } else {
        setSources((prev) => [...prev, source])
      }
      void loadStats()
    },
    [loadStats],
  )

  const discoverSource = useCallback(
    async (url: string, signal?: AbortSignal) => {
      return brewApi.discoverSource(url.trim(), undefined, { signal })
    },
    [],
  )

  const generateStyleTags = useCallback(
    async (sourceId: number, signal?: AbortSignal) => {
      return brewApi.generateStyleTags(sourceId, signal)
    },
    [],
  )

  const refreshSource = useCallback(
    async (sourceId: number) => {
      try {
        const newCount = await brewApi.refreshSource(sourceId)
        if (newCount > 0) reloadBoard()
      } catch (err) {
        reportBrewError(err, labels.refreshFailed, setError)
      }
    },
    [reloadBoard, labels.refreshFailed, setError],
  )

  return {
    sources,
    setSources,
    sourcesLoaded,
    stats,
    setStats,
    booting,
    loadSources,
    loadStats,
    reloadBoard,
    addSource,
    updateSource,
    importOpml,
    removeSources,
    refreshSource,
    discoverSource,
    generateStyleTags,
  }
}
