import { Feature, getFeatureList, hasFeature } from './pageFeatures'

export type Unsubscribe = () => void

let currentPageId: string | null = null

let isActive = true

let visibilityInitialized = false
let messageChannelInitialized = false
let resizeObserverInitialized = false
let intersectionInitialized = false
let idleSchedulerInitialized = false

let _isPageVisible = true
let _visibilityHandler: (() => void) | null = null
const _visibilitySubscribers = new Set<(visible: boolean) => void>()

function initVisibility() {
  if (visibilityInitialized || typeof document === 'undefined') return

  if (currentPageId && !hasFeature(currentPageId, Feature.Visibility)) {
    if (import.meta.env.DEV) {
      console.warn(`[Core] Visibility not enabled for page: ${currentPageId}`)
    }
  }
  visibilityInitialized = true

  _isPageVisible = !document.hidden
  _visibilityHandler = () => {
    _isPageVisible = !document.hidden

    for (const sub of _visibilitySubscribers) {
      try {
        sub(_isPageVisible)
      } catch {}
    }
  }
  document.addEventListener('visibilitychange', _visibilityHandler, {
    passive: true,
  })
}

export function onVisibility(
  callback: (visible: boolean) => void,
): Unsubscribe {
  initVisibility()
  _visibilitySubscribers.add(callback)
  return () => {
    _visibilitySubscribers.delete(callback)
  }
}

export function isPageVisible(): boolean {
  if (!visibilityInitialized) initVisibility()
  return _isPageVisible
}

let _channel: MessageChannel | null = null
let _pendingCallbacks: Array<() => void> = []

function initMessageChannel() {
  if (messageChannelInitialized) return
  messageChannelInitialized = true

  if (typeof MessageChannel !== 'undefined') {
    _channel = new MessageChannel()
    _channel.port1.onmessage = () => {
      const cbs = _pendingCallbacks
      _pendingCallbacks = []
      for (let i = 0; i < cbs.length; i++) cbs[i]()
    }
  }
}

/** 比 setTimeout(0) 快（MessageChannel）。 */
export function scheduleTask(callback: () => void): void {
  initMessageChannel()
  if (_channel) {
    _pendingCallbacks.push(callback)
    if (_pendingCallbacks.length === 1) {
      _channel.port2.postMessage(null)
    }
  } else {
    setTimeout(callback, 0)
  }
}

export function yieldToMain(): Promise<void> {
  return new Promise((resolve) => scheduleTask(resolve))
}

let _cachedNow = 0
let _nowValid = false

/** 同一帧内复用缓存时间戳。 */
export function now(): number {
  if (!_nowValid) {
    _cachedNow = performance.now()
    _nowValid = true
    queueMicrotask(() => {
      _nowValid = false
    })
  }
  return _cachedNow
}

export function refreshNow(): number {
  _cachedNow = performance.now()
  _nowValid = true
  return _cachedNow
}

let _resizeObserver: ResizeObserver | null = null
const _resizeCallbacks = new WeakMap<
  Element,
  (entry: ResizeObserverEntry) => void
>()
const _resizeElements = new Set<Element>()
let _resizeBatch: ResizeObserverEntry[] = []
let _resizeScheduled = false
const RESIZE_THROTTLE = 50
let _lastResizeTime = 0

function initResizeObserver() {
  if (resizeObserverInitialized || typeof ResizeObserver === 'undefined') return
  resizeObserverInitialized = true

  _resizeObserver = new ResizeObserver((entries) => {
    if (!_isPageVisible) return

    const nowTime = performance.now()
    if (nowTime - _lastResizeTime < RESIZE_THROTTLE) {
      _resizeBatch.push(...entries)
      if (!_resizeScheduled) {
        _resizeScheduled = true
        setTimeout(flushResizeBatch, RESIZE_THROTTLE)
      }
      return
    }

    _lastResizeTime = nowTime
    for (const entry of entries) {
      const cb = _resizeCallbacks.get(entry.target)
      if (cb) cb(entry)
    }
  })
}

function flushResizeBatch() {
  _resizeScheduled = false
  const batch = _resizeBatch
  _resizeBatch = []
  _lastResizeTime = performance.now()

  for (const entry of batch) {
    const cb = _resizeCallbacks.get(entry.target)
    if (cb) cb(entry)
  }
}

export function observeResize(
  element: Element,
  callback: (entry: ResizeObserverEntry) => void,
): Unsubscribe {
  initResizeObserver()
  if (!_resizeObserver) return () => {}

  _resizeCallbacks.set(element, callback)
  _resizeElements.add(element)
  _resizeObserver.observe(element)

  return () => {
    _resizeCallbacks.delete(element)
    _resizeElements.delete(element)
    _resizeObserver?.unobserve(element)
  }
}

const _intersectionObservers = new Map<string, IntersectionObserver>()
const _intersectionCallbacks = new WeakMap<
  Element,
  {
    callback: (entry: IntersectionObserverEntry) => void
    key: string
  }
>()
const _intersectionElements = new Set<Element>()

function getIntersectionKey(threshold: number, rootMargin: string): string {
  return `${threshold}:${rootMargin}`
}

function getOrCreateIntersectionObserver(
  threshold: number,
  rootMargin: string,
): IntersectionObserver {
  const key = getIntersectionKey(threshold, rootMargin)
  let observer = _intersectionObservers.get(key)

  if (!observer) {
    observer = new IntersectionObserver(
      (entries) => {
        if (!_isPageVisible) return
        for (const entry of entries) {
          const info = _intersectionCallbacks.get(entry.target)
          if (info) info.callback(entry)
        }
      },
      { threshold, rootMargin },
    )
    _intersectionObservers.set(key, observer)
    intersectionInitialized = true
  }

  return observer
}

export function observeIntersection(
  element: Element,
  callback: (entry: IntersectionObserverEntry) => void,
  options: { threshold?: number; rootMargin?: string } = {},
): Unsubscribe {
  const { threshold = 0, rootMargin = '0px' } = options
  const key = getIntersectionKey(threshold, rootMargin)
  const observer = getOrCreateIntersectionObserver(threshold, rootMargin)

  _intersectionCallbacks.set(element, { callback, key })
  _intersectionElements.add(element)
  observer.observe(element)

  return () => {
    const info = _intersectionCallbacks.get(element)
    if (info) {
      const obs = _intersectionObservers.get(info.key)
      obs?.unobserve(element)
    }
    _intersectionCallbacks.delete(element)
    _intersectionElements.delete(element)
  }
}

interface IdleTask {
  id: string
  task: () => void
  priority: number
}

let _idleTasks: IdleTask[] = []
let _idleCallbackId: number | null = null
const _registeredTasks = new Set<string>()

function scheduleIdleRun() {
  if (_idleCallbackId !== null || _idleTasks.length === 0 || !_isPageVisible)
    return

  const run =
    typeof requestIdleCallback !== 'undefined'
      ? requestIdleCallback
      : (cb: IdleRequestCallback) =>
          setTimeout(
            () => cb({ didTimeout: false, timeRemaining: () => 50 }),
            1,
          )

  _idleCallbackId = run(
    (deadline) => {
      _idleCallbackId = null

      while (
        _idleTasks.length > 0 &&
        (deadline.timeRemaining() > 2 || deadline.didTimeout)
      ) {
        const task = _idleTasks.shift()!
        _registeredTasks.delete(task.id)
        try {
          task.task()
        } catch {}
      }

      if (_idleTasks.length > 0) scheduleIdleRun()
    },
    { timeout: 2000 },
  ) as number

  idleSchedulerInitialized = true
}

export function scheduleIdle(
  id: string,
  task: () => void,
  priority: 'low' | 'normal' | 'high' = 'normal',
): Unsubscribe {
  if (_registeredTasks.has(id)) {
    return () => cancelIdle(id)
  }

  const p = priority === 'high' ? 2 : priority === 'normal' ? 1 : 0
  _idleTasks.push({ id, task, priority: p })
  _registeredTasks.add(id)

  for (let i = _idleTasks.length - 1; i > 0; i--) {
    if (_idleTasks[i].priority > _idleTasks[i - 1].priority) {
      ;[_idleTasks[i], _idleTasks[i - 1]] = [_idleTasks[i - 1], _idleTasks[i]]
    } else {
      break
    }
  }

  scheduleIdleRun()
  return () => cancelIdle(id)
}

export function cancelIdle(id: string): boolean {
  const idx = _idleTasks.findIndex((t) => t.id === id)
  if (idx !== -1) {
    _idleTasks = _idleTasks.toSpliced(idx, 1)
    _registeredTasks.delete(id)
    return true
  }
  return false
}

let _reads: Array<() => void> = []
let _writes: Array<() => void> = []
let _domBatchScheduled = false

function flushDomBatch() {
  _domBatchScheduled = false

  // 先读后写，避免强制重排。
  const reads = _reads
  _reads = []
  for (const r of reads) {
    try {
      r()
    } catch {}
  }

  const writes = _writes
  _writes = []
  for (const w of writes) {
    try {
      w()
    } catch {}
  }
}

export function batchRead(callback: () => void): void {
  _reads.push(callback)
  if (!_domBatchScheduled) {
    _domBatchScheduled = true
    requestAnimationFrame(flushDomBatch)
  }
}

export function batchWrite(callback: () => void): void {
  _writes.push(callback)
  if (!_domBatchScheduled) {
    _domBatchScheduled = true
    requestAnimationFrame(flushDomBatch)
  }
}

const _pageCleanupRegistry = new Map<string, () => void>()

export function registerPageCleanup(pageId: string, cleanup: () => void): void {
  _pageCleanupRegistry.set(pageId, cleanup)
}

export function runPageCleanup(pageId: string): void {
  _pageCleanupRegistry.get(pageId)?.()
}

export function startPage(pageId: string): void {
  if (currentPageId === pageId) return

  if (currentPageId) {
    cleanupPage()
  }

  currentPageId = pageId
  isActive = true

  if (hasFeature(pageId, Feature.Visibility) && !visibilityInitialized) {
    initVisibility()
  }
}

function cleanupPage(): void {
  _idleTasks.length = 0
  _registeredTasks.clear()
  if (_idleCallbackId !== null) {
    if (typeof cancelIdleCallback !== 'undefined') {
      cancelIdleCallback(_idleCallbackId)
    }
    _idleCallbackId = null
  }

  _reads.length = 0
  _writes.length = 0
  _domBatchScheduled = false

  _resizeBatch.length = 0
  _resizeScheduled = false

  _pendingCallbacks.length = 0
}

export function pause(): void {
  isActive = false
}

export function resume(): void {
  isActive = true
  scheduleIdleRun()
}

export function getCurrentPageId(): string | null {
  return currentPageId
}

export function isSchedulerActive(): boolean {
  return isActive && _isPageVisible
}

interface PageResizeManager {
  observe: (
    element: Element,
    callback: (entry: ResizeObserverEntry) => void,
  ) => void
  unobserve: (element: Element) => void
  cleanup: () => void
}

const _pageResizeManagers = new Map<string, PageResizeManager>()

export function getPageResizeManager(pageId: string): PageResizeManager {
  let manager = _pageResizeManagers.get(pageId)
  if (manager) return manager

  let observer: ResizeObserver | null = null
  const callbacks = new Map<Element, (entry: ResizeObserverEntry) => void>()

  function getObserver(): ResizeObserver {
    if (!observer) {
      observer = new ResizeObserver((entries) => {
        for (const entry of entries) {
          const cb = callbacks.get(entry.target)
          if (cb) cb(entry)
        }
      })
    }
    return observer
  }

  manager = {
    observe(element, callback) {
      callbacks.set(element, callback)
      getObserver().observe(element)
    },
    unobserve(element) {
      callbacks.delete(element)
      observer?.unobserve(element)
    },
    cleanup() {
      observer?.disconnect()
      observer = null
      callbacks.clear()
      _pageResizeManagers.delete(pageId)
    },
  }

  _pageResizeManagers.set(pageId, manager)
  return manager
}

interface PageIntervalManager {
  add: (id: ReturnType<typeof setInterval>) => void
  remove: (id: ReturnType<typeof setInterval>) => void
  cleanup: () => void
}

const _pageIntervalManagers = new Map<string, PageIntervalManager>()

export function getPageIntervalManager(pageId: string): PageIntervalManager {
  let manager = _pageIntervalManagers.get(pageId)
  if (manager) return manager

  const intervals = new Set<ReturnType<typeof setInterval>>()

  manager = {
    add(id) {
      intervals.add(id)
    },
    remove(id) {
      clearInterval(id)
      intervals.delete(id)
    },
    cleanup() {
      for (const id of intervals) clearInterval(id)
      intervals.clear()
      _pageIntervalManagers.delete(pageId)
    },
  }

  _pageIntervalManagers.set(pageId, manager)
  return manager
}

export function getStats() {
  return {
    pageId: currentPageId,
    pageFeatures: currentPageId ? getFeatureList(currentPageId) : [],
    isActive,
    isPageVisible: _isPageVisible,
    initialized: {
      visibility: visibilityInitialized,
      messageChannel: messageChannelInitialized,
      resizeObserver: resizeObserverInitialized,
      intersection: intersectionInitialized,
      idleScheduler: idleSchedulerInitialized,
    },
    counts: {
      resizeElements: _resizeElements.size,
      intersectionElements: _intersectionElements.size,
      intersectionObservers: _intersectionObservers.size,
      pendingIdleTasks: _idleTasks.length,
      pendingReads: _reads.length,
      pendingWrites: _writes.length,
      visibilitySubscribers: _visibilitySubscribers.size,
    },
  }
}

export function destroy(): void {
  cleanupPage()

  if (_visibilityHandler && typeof document !== 'undefined') {
    document.removeEventListener('visibilitychange', _visibilityHandler)
    _visibilityHandler = null
  }
  _visibilitySubscribers.clear()
  visibilityInitialized = false

  if (_channel) {
    _channel.port1.close()
    _channel.port2.close()
    _channel = null
  }
  messageChannelInitialized = false

  if (_resizeObserver) {
    _resizeObserver.disconnect()
    _resizeObserver = null
  }
  _resizeElements.clear()
  resizeObserverInitialized = false

  for (const obs of _intersectionObservers.values()) {
    obs.disconnect()
  }
  _intersectionObservers.clear()
  _intersectionElements.clear()
  intersectionInitialized = false

  currentPageId = null
  isActive = true
}
