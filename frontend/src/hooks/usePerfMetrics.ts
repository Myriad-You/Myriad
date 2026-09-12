import { useCallback, useEffect, useRef, useState } from 'react'

import { globalResourceLoader } from '../utils/resourceLoader'

import {
  configureAnimationCoordinator,
  coordinator,
  getFrameStats,
} from './animation'

export interface FrameMetrics {
  fps: number
  avgFrameTime: number
  isLowFps: boolean
  maxFrameMs: number
  p95FrameMs: number
  jankRatio: number
  jankThresholdMs: number
  detectedRefreshRate: number
  lowFpsThreshold: number
  refreshRateDetected: boolean
  isMonitoring: boolean
}

export interface MemoryMetrics {
  usedMB: number
  limitMB: number
  usedPercent: number
}

export interface StabilityMetrics {
  /** 累计 CLS（忽略 hadRecentInput）。 */
  cls: number
  fcpMs: number | null
  longTaskCount: number
  lastLongTaskMs: number
}

export interface AnimationSnapshot {
  total: number
  running: number
  items: Array<{
    name: string
    target: string
    durationMs: number | null
    infinite: boolean
    playState: string
  }>
}

export interface CoordinatorSnapshot {
  /** 瞬时占槽 — 空闲时几乎总是 0。 */
  activeSlots: number
  maxConcurrent: number
  baseConcurrent: number
  waitingQueue: number
  delayedQueue: number
  totalQueued: number
  inBurstMode: boolean
  peakActiveSlots: number
  totalScheduled: number
  totalAcquired: number
  pageReady: boolean
  currentPageId: string | null
  statesSize: number
}

export interface ResourceSnapshot {
  queued: number
  active: number
  completed: number
  failed: number
}

export interface PerfSnapshot {
  frame: FrameMetrics
  memory: MemoryMetrics | null
  stability: StabilityMetrics
  animations: AnimationSnapshot
  coordinator: CoordinatorSnapshot
  resource: ResourceSnapshot
}

function shortSelector(el: Element | null): string {
  if (!el) return '—'
  const tag = el.tagName?.toLowerCase() || '?'
  const id = el.id ? `#${el.id}` : ''
  let cls = ''
  if (typeof el.className === 'string' && el.className) {
    const first = el.className
      .split(/\s+/)
      .find((c) => c && !c.startsWith('_'))
    if (first) cls = `.${first}`
  }
  return `${tag}${id}${cls}`.slice(0, 36)
}

function readMemory(): MemoryMetrics | null {
  const perf = performance as Performance & {
    memory?: {
      usedJSHeapSize: number
      jsHeapSizeLimit: number
    }
  }
  if (!perf.memory) return null
  const { usedJSHeapSize, jsHeapSizeLimit } = perf.memory
  if (!jsHeapSizeLimit) return null
  return {
    usedMB: Math.round(usedJSHeapSize / 1048576),
    limitMB: Math.round(jsHeapSizeLimit / 1048576),
    usedPercent: Math.round((usedJSHeapSize / jsHeapSizeLimit) * 100),
  }
}

function readFrame(): FrameMetrics {
  const s = getFrameStats()
  return {
    fps: s.fps,
    avgFrameTime: Math.round(s.avgFrameTime * 10) / 10,
    isLowFps: s.isLowFps,
    maxFrameMs: s.maxFrameMs,
    p95FrameMs: s.p95FrameMs,
    jankRatio: Math.round(s.jankRatio * 1000) / 1000,
    jankThresholdMs: s.jankThresholdMs,
    detectedRefreshRate: s.detectedRefreshRate,
    lowFpsThreshold: s.lowFpsThreshold,
    refreshRateDetected: s.refreshRateDetected,
    isMonitoring: s.isMonitoring,
  }
}

function readCoordinator(): CoordinatorSnapshot {
  const s = coordinator.getConcurrencyStatus()
  return {
    activeSlots: s.activeSlots,
    maxConcurrent: s.maxConcurrent,
    baseConcurrent: s.baseConcurrent,
    waitingQueue: s.waitingQueue,
    delayedQueue: s.delayedQueue,
    totalQueued: s.totalQueued,
    inBurstMode: s.inBurstMode,
    peakActiveSlots: s.peakActiveSlots,
    totalScheduled: s.totalScheduled,
    totalAcquired: s.totalAcquired,
    pageReady: s.pageReady,
    currentPageId: s.currentPageId,
    statesSize: s.statesSize,
  }
}

function readResource(): ResourceSnapshot {
  const s = globalResourceLoader.getStats()
  return {
    queued: s.queued,
    active: s.active,
    completed: s.completed,
    failed: s.failed,
  }
}

function scanAnimations(limit = 12): AnimationSnapshot {
  const monitor = document.querySelector('[data-perf-monitor]')
  const items: AnimationSnapshot['items'] = []
  let total = 0
  let running = 0

  try {
    const all = document.getAnimations?.() ?? []
    for (const anim of all) {
      const effect = anim.effect as KeyframeEffect | null
      const target = effect?.target as Element | null
      if (target && monitor?.contains(target)) continue

      total++
      const isRunning = anim.playState === 'running'
      if (isRunning) running++

      if (items.length < limit && isRunning) {
        const timing = effect?.getTiming?.()
        const duration =
          typeof timing?.duration === 'number' ? timing.duration : null
        items.push({
          name:
            (anim as CSSAnimation).animationName ||
            (anim as Animation).id ||
            'anonymous',
          target: shortSelector(target),
          durationMs: duration,
          infinite: timing?.iterations === Infinity,
          playState: anim.playState,
        })
      }
    }
  } catch {

  }

  return { total, running, items }
}

function shallowEqualSnapshot(
  a: PerfSnapshot,
  b: PerfSnapshot,
  includeAnimations: boolean,
): boolean {
  if (a.frame.fps !== b.frame.fps) return false
  if (a.frame.avgFrameTime !== b.frame.avgFrameTime) return false
  if (a.frame.isLowFps !== b.frame.isLowFps) return false
  if (a.frame.maxFrameMs !== b.frame.maxFrameMs) return false
  if (a.frame.p95FrameMs !== b.frame.p95FrameMs) return false
  if (a.frame.jankRatio !== b.frame.jankRatio) return false
  if (a.frame.detectedRefreshRate !== b.frame.detectedRefreshRate) return false
  if (a.frame.isMonitoring !== b.frame.isMonitoring) return false

  if ((a.memory?.usedMB ?? -1) !== (b.memory?.usedMB ?? -1)) return false
  if ((a.memory?.usedPercent ?? -1) !== (b.memory?.usedPercent ?? -1))
    return false

  if (a.stability.cls !== b.stability.cls) return false
  if (a.stability.fcpMs !== b.stability.fcpMs) return false
  if (a.stability.longTaskCount !== b.stability.longTaskCount) return false
  if (a.stability.lastLongTaskMs !== b.stability.lastLongTaskMs) return false

  if (a.coordinator.activeSlots !== b.coordinator.activeSlots) return false
  if (a.coordinator.maxConcurrent !== b.coordinator.maxConcurrent) return false
  if (a.coordinator.waitingQueue !== b.coordinator.waitingQueue) return false
  if (a.coordinator.delayedQueue !== b.coordinator.delayedQueue) return false
  if (a.coordinator.inBurstMode !== b.coordinator.inBurstMode) return false
  if (a.coordinator.peakActiveSlots !== b.coordinator.peakActiveSlots)
    return false
  if (a.coordinator.totalScheduled !== b.coordinator.totalScheduled)
    return false
  if (a.coordinator.totalAcquired !== b.coordinator.totalAcquired) return false
  if (a.coordinator.pageReady !== b.coordinator.pageReady) return false
  if (a.coordinator.statesSize !== b.coordinator.statesSize) return false

  if (a.resource.queued !== b.resource.queued) return false
  if (a.resource.active !== b.resource.active) return false
  if (a.resource.completed !== b.resource.completed) return false
  if (a.resource.failed !== b.resource.failed) return false

  if (includeAnimations) {
    if (a.animations.total !== b.animations.total) return false
    if (a.animations.running !== b.animations.running) return false
  }

  return true
}

const EMPTY_ANIMATIONS: AnimationSnapshot = {
  total: 0,
  running: 0,
  items: [],
}

function initialSnapshot(): PerfSnapshot {
  return {
    frame: readFrame(),
    memory: readMemory(),
    stability: {
      cls: 0,
      fcpMs: null,
      longTaskCount: 0,
      lastLongTaskMs: 0,
    },
    animations: EMPTY_ANIMATIONS,
    coordinator: readCoordinator(),
    resource: readResource(),
  }
}

export function usePerfMetrics(isExpanded: boolean) {
  const [snapshot, setSnapshot] = useState<PerfSnapshot>(initialSnapshot)
  const stabilityRef = useRef(snapshot.stability)
  const snapshotRef = useRef(snapshot)
  snapshotRef.current = snapshot
  stabilityRef.current = snapshot.stability

  const commit = useCallback(
    (next: PerfSnapshot, includeAnimations: boolean) => {
      if (shallowEqualSnapshot(snapshotRef.current, next, includeAnimations)) {
        return
      }
      snapshotRef.current = next
      setSnapshot(next)
    },
    [],
  )

  useEffect(() => {
    const tick = () => {
      const prev = snapshotRef.current
      // 收缩：只更新顶栏需要的 fps / memory。
      if (!isExpanded) {
        const next: PerfSnapshot = {
          ...prev,
          frame: readFrame(),
          memory: readMemory(),
          stability: stabilityRef.current,
        }
        commit(next, false)
        return
      }

      const next: PerfSnapshot = {
        frame: readFrame(),
        memory: readMemory(),
        stability: stabilityRef.current,
        animations: scanAnimations(),
        coordinator: readCoordinator(),
        resource: readResource(),
      }
      commit(next, true)
    }

    tick()
    const ms = isExpanded ? 1000 : 2000
    const id = window.setInterval(tick, ms)
    return () => window.clearInterval(id)
  }, [isExpanded, commit])

  useEffect(() => {
    // PerformanceObserver 仅展开时挂载。
    if (!isExpanded || !('PerformanceObserver' in window)) return

    const observers: PerformanceObserver[] = []

    try {
      const lt = new PerformanceObserver((list) => {
        let added = 0
        let lastMs = stabilityRef.current.lastLongTaskMs
        for (const entry of list.getEntries()) {
          added++
          lastMs = Math.round(entry.duration)
        }
        if (added === 0) return
        const stability = {
          ...stabilityRef.current,
          longTaskCount: stabilityRef.current.longTaskCount + added,
          lastLongTaskMs: lastMs,
        }
        stabilityRef.current = stability
        commit(
          { ...snapshotRef.current, stability },
          true,
        )
      })
      lt.observe({ entryTypes: ['longtask'] })
      observers.push(lt)
    } catch {

    }

    try {
      const clsObs = new PerformanceObserver((list) => {
        let delta = 0
        for (const entry of list.getEntries()) {
          const ls = entry as PerformanceEntry & {
            value?: number
            hadRecentInput?: boolean
          }
          if (ls.hadRecentInput) continue
          if (typeof ls.value === 'number') delta += ls.value
        }
        if (delta === 0) return
        const stability = {
          ...stabilityRef.current,
          cls:
            Math.round((stabilityRef.current.cls + delta) * 10000) / 10000,
        }
        stabilityRef.current = stability
        commit({ ...snapshotRef.current, stability }, true)
      })
      clsObs.observe({ type: 'layout-shift', buffered: true })
      observers.push(clsObs)
    } catch {

    }

    try {
      const paint = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (entry.name === 'first-contentful-paint') {
            const stability = {
              ...stabilityRef.current,
              fcpMs: Math.round(entry.startTime),
            }
            stabilityRef.current = stability
            commit({ ...snapshotRef.current, stability }, true)
          }
        }
      })
      paint.observe({ type: 'paint', buffered: true })
      observers.push(paint)
    } catch {

    }

    try {
      const paints = performance.getEntriesByType('paint')
      for (const entry of paints) {
        if (entry.name === 'first-contentful-paint') {
          const stability = {
            ...stabilityRef.current,
            fcpMs: Math.round(entry.startTime),
          }
          stabilityRef.current = stability
          commit({ ...snapshotRef.current, stability }, false)
        }
      }
    } catch {

    }

    return () => {
      for (const o of observers) o.disconnect()
    }
  }, [isExpanded, commit])

  const refreshAnimations = useCallback(() => {
    const animations = scanAnimations()
    commit({ ...snapshotRef.current, animations }, true)
  }, [commit])

  const resetLongTasks = useCallback(() => {
    const stability = {
      ...stabilityRef.current,
      longTaskCount: 0,
      lastLongTaskMs: 0,
    }
    stabilityRef.current = stability
    commit({ ...snapshotRef.current, stability }, isExpanded)
  }, [commit, isExpanded])

  const resetCls = useCallback(() => {
    const stability = { ...stabilityRef.current, cls: 0 }
    stabilityRef.current = stability
    commit({ ...snapshotRef.current, stability }, isExpanded)
  }, [commit, isExpanded])

  return {
    snapshot,
    refreshAnimations,
    resetLongTasks,
    resetCls,
    configureCoordinator: configureAnimationCoordinator,
  }
}
