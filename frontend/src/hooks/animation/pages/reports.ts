/**
 * 报告页专用调度器 Hooks
 *
 * 报告页功能需求（最复杂的页面）：
 * - Visibility: 可见性感知（暂停轮播、弹幕等）
 * - Interval: 定时器（卡片轮播 10s、字幕切换）
 * - RAF: 背景渐变动画
 * - DOMBatch: DOM 批量操作优化
 *
 * @example
 * ```tsx
 * import {
 *   useReportsScheduler,
 *   useReportsVisibilityInterval,
 *   useReportsRaf
 * } from '@hooks/animation/pages/reports';
 *
 * function Reports() {
 *   useReportsScheduler();
 *
 *   // 可见性感知轮播 - 页面隐藏时自动暂停
 *   useReportsVisibilityInterval(() => {
 *     nextCard();
 *   }, 10000);
 *
 *   return <ReportCards />;
 * }
 * ```
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { isPageVisible, onVisibility } from '../core'
import { Feature, hasFeature } from '../pageFeatures'

const PAGE_ID = 'reports'

// ==================== 页面初始化 ====================

/**
 * 报告页调度器初始化
 *
 * 注意：startPage('reports') 由 useRouteScheduler 统一调用
 */
export function useReportsScheduler(): void {
  useEffect(() => {
    return () => cleanupReports()
  }, [])
}

// ==================== 可见性 Hooks ====================

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

// ==================== Interval Hooks ====================

// 活跃的 interval 追踪
const _reportsIntervals = new Set<ReturnType<typeof setInterval>>()

/**
 * 报告页可见性感知定时器
 * 页面隐藏时自动暂停，可见时自动恢复
 *
 * @param callback 回调函数
 * @param delay 间隔时间，传入 null 禁用定时器
 */
export function useReportsVisibilityInterval(
  callback: () => void,
  delay: number | null,
): void {
  const savedCallback = useRef(callback)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const visible = useReportsVisibility()

  // 更新回调引用
  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Interval) || delay === null) {
      // 功能未启用或 delay 为 null，不执行定时器
      return
    }

    if (visible) {
      intervalRef.current = setInterval(() => {
        savedCallback.current()
      }, delay)
      _reportsIntervals.add(intervalRef.current)
    }

    return () => {
      if (intervalRef.current !== null) {
        clearInterval(intervalRef.current)
        _reportsIntervals.delete(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [delay, visible])
}

/**
 * 报告页普通定时器（不感知可见性）
 */
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
    _reportsIntervals.add(id)

    return () => {
      clearInterval(id)
      _reportsIntervals.delete(id)
    }
  }, [delay])
}

// ==================== Timeout Hooks ====================

/**
 * 报告页延时器
 */
export function useReportsTimeout(
  callback: () => void,
  delay: number | null,
): void {
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (delay === null)
      return

    const id = setTimeout(() => savedCallback.current(), delay)
    return () => clearTimeout(id)
  }, [delay])
}

// ==================== RAF Hooks ====================

// RAF 循环管理
let _reportsRafId: number | null = null
const _reportsRafCallbacks = new Map<symbol, (time: number) => void>()

function startReportsRafLoop() {
  if (_reportsRafId !== null)
    return

  const loop = (time: number) => {
    for (const cb of _reportsRafCallbacks.values()) {
      cb(time)
    }
    if (_reportsRafCallbacks.size > 0) {
      _reportsRafId = requestAnimationFrame(loop)
    }
    else {
      _reportsRafId = null
    }
  }

  _reportsRafId = requestAnimationFrame(loop)
}

/**
 * 报告页 RAF 动画循环
 * 用于背景渐变扫描等持续动画
 */
export function useReportsRaf(
  callback: (time: number) => void,
  active = true,
): void {
  const keyRef = useRef(Symbol())
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.RAF) || !active) {
      return
    }

    const key = keyRef.current
    _reportsRafCallbacks.set(key, time => savedCallback.current(time))
    startReportsRafLoop()

    return () => {
      _reportsRafCallbacks.delete(key)
    }
  }, [active])
}

/**
 * RAF 节流函数
 */
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

// ==================== DOM Batch Hooks ====================

let _reportsReadQueue: Array<() => void> = []
let _reportsWriteQueue: Array<() => void> = []
let _reportsBatchScheduled = false

function flushReportsBatch() {
  // 先读后写，避免强制重排
  const reads = _reportsReadQueue
  const writes = _reportsWriteQueue
  _reportsReadQueue = []
  _reportsWriteQueue = []
  _reportsBatchScheduled = false

  for (const read of reads) read()
  for (const write of writes) write()
}

/**
 * 报告页 DOM 批量操作
 */
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

// ==================== 清理 ====================

export function cleanupReports(): void {
  // 清理所有 interval
  for (const id of _reportsIntervals) {
    clearInterval(id)
  }
  _reportsIntervals.clear()

  // 清理 RAF
  if (_reportsRafId !== null) {
    cancelAnimationFrame(_reportsRafId)
    _reportsRafId = null
  }
  _reportsRafCallbacks.clear()

  // 清理 DOM batch
  _reportsReadQueue = []
  _reportsWriteQueue = []
  _reportsBatchScheduled = false
}
