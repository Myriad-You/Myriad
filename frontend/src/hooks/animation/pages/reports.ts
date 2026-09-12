import { useCallback, useEffect, useRef, useState } from 'react'
import {
  getPageIntervalManager,
  isPageVisible,
  onVisibility,
  registerPageCleanup,
} from '../core'
import { Feature, hasFeature } from '../pageFeatures'

const PAGE_ID = 'reports'

/** startPage('reports') 由 useRouteScheduler 统一调用。 */
export function useReportsScheduler(): void {
  useEffect(() => {
    return () => cleanupReports()
  }, [])
}

export function useReportsVisibility(): boolean {
  const [visible, setVisible] = useState(() => isPageVisible())

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Visibility)) {
      return
    }
    return onVisibility(setVisible)
  }, [])

  return visible
}

function getIntervalManager() {
  return getPageIntervalManager(PAGE_ID)
}

export function useReportsVisibilityInterval(
  callback: () => void,
  delay: number | null,
): void {
  const savedCallback = useRef(callback)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const visible = useReportsVisibility()

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Interval) || delay === null) {
      return
    }

    if (visible) {
      intervalRef.current = setInterval(() => {
        savedCallback.current()
      }, delay)
      getIntervalManager().add(intervalRef.current)
    }

    return () => {
      if (intervalRef.current !== null) {
        getIntervalManager().remove(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [delay, visible])
}

export function useReportsInterval(
  callback: () => void,
  delay: number | null,
): void {
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Interval) || delay === null) {
      return
    }

    const id = setInterval(() => savedCallback.current(), delay)
    getIntervalManager().add(id)

    return () => {
      getIntervalManager().remove(id)
    }
  }, [delay])
}

export function useReportsTimeout(
  callback: () => void,
  delay: number | null,
): void {
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (delay === null) return

    const id = setTimeout(() => savedCallback.current(), delay)
    return () => clearTimeout(id)
  }, [delay])
}

let _reportsRafId: number | null = null
const _reportsRafCallbacks = new Map<symbol, (time: number) => void>()

function startReportsRafLoop() {
  if (_reportsRafId !== null) return

  const loop = (time: number) => {
    for (const cb of _reportsRafCallbacks.values()) {
      cb(time)
    }
    if (_reportsRafCallbacks.size > 0) {
      _reportsRafId = requestAnimationFrame(loop)
    } else {
      _reportsRafId = null
    }
  }

  _reportsRafId = requestAnimationFrame(loop)
}

export function useReportsRaf(
  callback: (time: number) => void,
  active = true,
): void {
  const keyRef = useRef(Symbol('reports'))
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.RAF) || !active) {
      return
    }

    const key = keyRef.current
    _reportsRafCallbacks.set(key, (time) => savedCallback.current(time))
    startReportsRafLoop()

    return () => {
      _reportsRafCallbacks.delete(key)
    }
  }, [active])
}

export function useReportsRafThrottle<T extends (...args: any[]) => void>(
  callback: T,
  deps: React.DependencyList = [],
): T {
  const rafId = useRef<number | null>(null)
  const lastArgs = useRef<any[]>([])

  const throttled = useCallback((...args: any[]) => {
    lastArgs.current = args
    if (rafId.current === null) {
      rafId.current = requestAnimationFrame(() => {
        rafId.current = null
        callback(...lastArgs.current)
      })
    }
  }, deps) as T

  useEffect(() => {
    return () => {
      if (rafId.current !== null) {
        cancelAnimationFrame(rafId.current)
      }
    }
  }, [])

  return throttled
}

let _reportsReadQueue: Array<() => void> = []
let _reportsWriteQueue: Array<() => void> = []
let _reportsBatchScheduled = false

function flushReportsBatch() {
  // 先读后写，避免强制重排。
  const reads = _reportsReadQueue
  const writes = _reportsWriteQueue
  _reportsReadQueue = []
  _reportsWriteQueue = []
  _reportsBatchScheduled = false

  for (const read of reads) read()
  for (const write of writes) write()
}

export function useReportsBatchDom(): {
  batchRead: (callback: () => void) => void
  batchWrite: (callback: () => void) => void
} {
  const batchRead = useCallback((callback: () => void) => {
    if (!hasFeature(PAGE_ID, Feature.DOMBatch)) {
      callback()
      return
    }
    _reportsReadQueue.push(callback)
    if (!_reportsBatchScheduled) {
      _reportsBatchScheduled = true
      requestAnimationFrame(flushReportsBatch)
    }
  }, [])

  const batchWrite = useCallback((callback: () => void) => {
    if (!hasFeature(PAGE_ID, Feature.DOMBatch)) {
      callback()
      return
    }
    _reportsWriteQueue.push(callback)
    if (!_reportsBatchScheduled) {
      _reportsBatchScheduled = true
      requestAnimationFrame(flushReportsBatch)
    }
  }, [])

  return { batchRead, batchWrite }
}

export function cleanupReports(): void {
  getPageIntervalManager(PAGE_ID).cleanup()

  if (_reportsRafId !== null) {
    cancelAnimationFrame(_reportsRafId)
    _reportsRafId = null
  }
  _reportsRafCallbacks.clear()

  _reportsReadQueue = []
  _reportsWriteQueue = []
  _reportsBatchScheduled = false
}

registerPageCleanup(PAGE_ID, cleanupReports)
