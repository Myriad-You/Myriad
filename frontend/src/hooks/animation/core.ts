/**
 * 动画调度器核心 - 极简原子化版本
 *
 * 设计原则：
 * 1. 零开销抽象：未使用的功能不产生运行时开销
 * 2. 惰性初始化：只在首次调用时创建资源
 * 3. 页面级隔离：SPA 路由切换时自动清理
 * 4. 原子化 API：每个功能独立，可单独使用
 * 5. 🔧 页面级按需加载：只初始化当前页面需要的功能
 *
 * @module animation/core
 */

import { Feature, getFeatureList, hasFeature, PAGE_FEATURES } from './pageFeatures'

// ==================== 类型定义 ====================

/** 取消订阅函数 */
export type Unsubscribe = () => void

/** 页面上下文 ID */
let currentPageId: string | null = null

/** 当前页面启用的功能 */
let currentFeatures: number = 0

/** 是否处于活动状态 */
let isActive = true

// ==================== 惰性初始化标记 ====================

let visibilityInitialized = false
let messageChannelInitialized = false
let resizeObserverInitialized = false
let intersectionInitialized = false
let idleSchedulerInitialized = false

// ==================== 页面可见性模块（惰性） ====================

let _isPageVisible = true
let _visibilityHandler: (() => void) | null = null
const _visibilitySubscribers = new Set<(visible: boolean) => void>()

function initVisibility() {
  if (visibilityInitialized || typeof document === 'undefined')
    return
  // 检查当前页面是否需要此功能
  if (currentPageId && !hasFeature(currentPageId, Feature.Visibility)) {
    if (import.meta.env.DEV) {
      console.warn(`[Core] Visibility not enabled for page: ${currentPageId}`)
    }
  }
  visibilityInitialized = true

  _isPageVisible = !document.hidden
  _visibilityHandler = () => {
    _isPageVisible = !document.hidden
    // 直接遍历，避免创建临时数组
    for (const sub of _visibilitySubscribers) {
      try { sub(_isPageVisible) }
      catch {}
    }
  }
  document.addEventListener('visibilitychange', _visibilityHandler, { passive: true })
}

/** 订阅页面可见性 */
export function onVisibility(callback: (visible: boolean) => void): Unsubscribe {
  initVisibility()
  _visibilitySubscribers.add(callback)
  return () => { _visibilitySubscribers.delete(callback) }
}

/** 获取页面可见性 */
export function isPageVisible(): boolean {
  if (!visibilityInitialized)
    initVisibility()
  return _isPageVisible
}

// ==================== MessageChannel 模块（惰性） ====================

let _channel: MessageChannel | null = null
let _pendingCallbacks: Array<() => void> = []

function initMessageChannel() {
  if (messageChannelInitialized)
    return
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

/** 高效任务让出（比 setTimeout(0) 快） */
export function scheduleTask(callback: () => void): void {
  initMessageChannel()
  if (_channel) {
    _pendingCallbacks.push(callback)
    if (_pendingCallbacks.length === 1) {
      _channel.port2.postMessage(null)
    }
  }
  else {
    setTimeout(callback, 0)
  }
}

/** 让出主线程并返回 Promise */
export function yieldToMain(): Promise<void> {
  return new Promise(resolve => scheduleTask(resolve))
}

// ==================== 时间戳缓存（微优化） ====================

let _cachedNow = 0
let _nowValid = false

/** 获取缓存的时间戳（同一帧内复用） */
export function now(): number {
  if (!_nowValid) {
    _cachedNow = performance.now()
    _nowValid = true
    queueMicrotask(() => { _nowValid = false })
  }
  return _cachedNow
}

/** 强制刷新时间戳 */
export function refreshNow(): number {
  _cachedNow = performance.now()
  _nowValid = true
  return _cachedNow
}

// ==================== ResizeObserver 模块（惰性） ====================

let _resizeObserver: ResizeObserver | null = null
const _resizeCallbacks = new WeakMap<Element, (entry: ResizeObserverEntry) => void>()
const _resizeElements = new Set<Element>()
let _resizeBatch: ResizeObserverEntry[] = []
let _resizeScheduled = false
const RESIZE_THROTTLE = 50
let _lastResizeTime = 0

function initResizeObserver() {
  if (resizeObserverInitialized || typeof ResizeObserver === 'undefined')
    return
  resizeObserverInitialized = true

  _resizeObserver = new ResizeObserver((entries) => {
    if (!_isPageVisible)
      return

    const nowTime = performance.now()
    // 节流
    if (nowTime - _lastResizeTime < RESIZE_THROTTLE) {
      _resizeBatch.push(...entries)
      if (!_resizeScheduled) {
        _resizeScheduled = true
        setTimeout(flushResizeBatch, RESIZE_THROTTLE)
      }
      return
    }

    _lastResizeTime = nowTime
    // 直接处理
    for (const entry of entries) {
      const cb = _resizeCallbacks.get(entry.target)
      if (cb)
        cb(entry)
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
    if (cb)
      cb(entry)
  }
}

/** 观察元素尺寸变化 */
export function observeResize(
  element: Element,
  callback: (entry: ResizeObserverEntry) => void,
): Unsubscribe {
  initResizeObserver()
  if (!_resizeObserver)
    return () => {}

  _resizeCallbacks.set(element, callback)
  _resizeElements.add(element)
  _resizeObserver.observe(element)

  return () => {
    _resizeCallbacks.delete(element)
    _resizeElements.delete(element)
    _resizeObserver?.unobserve(element)
  }
}

// ==================== IntersectionObserver 模块（惰性） ====================

const _intersectionObservers = new Map<string, IntersectionObserver>()
const _intersectionCallbacks = new WeakMap<Element, {
  callback: (entry: IntersectionObserverEntry) => void
  key: string
}>()
const _intersectionElements = new Set<Element>()

function getIntersectionKey(threshold: number, rootMargin: string): string {
  return `${threshold}:${rootMargin}`
}

function getOrCreateIntersectionObserver(threshold: number, rootMargin: string): IntersectionObserver {
  const key = getIntersectionKey(threshold, rootMargin)
  let observer = _intersectionObservers.get(key)

  if (!observer) {
    observer = new IntersectionObserver(
      (entries) => {
        if (!_isPageVisible)
          return
        for (const entry of entries) {
          const info = _intersectionCallbacks.get(entry.target)
          if (info)
            info.callback(entry)
        }
      },
      { threshold, rootMargin },
    )
    _intersectionObservers.set(key, observer)
    intersectionInitialized = true
  }

  return observer
}

/** 观察元素可见性 */
export function observeIntersection(
  element: Element,
  callback: (entry: IntersectionObserverEntry) => void,
  options: { threshold?: number, rootMargin?: string } = {},
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

// ==================== 空闲任务调度模块（惰性） ====================

interface IdleTask {
  id: string
  task: () => void
  priority: number
}

const _idleTasks: IdleTask[] = []
let _idleCallbackId: number | null = null
const _registeredTasks = new Set<string>()

function scheduleIdleRun() {
  if (_idleCallbackId !== null || _idleTasks.length === 0 || !_isPageVisible)
    return

  const run = typeof requestIdleCallback !== 'undefined'
    ? requestIdleCallback
    : (cb: IdleRequestCallback) => setTimeout(() => cb({ didTimeout: false, timeRemaining: () => 50 }), 1)

  _idleCallbackId = run((deadline) => {
    _idleCallbackId = null

    while (_idleTasks.length > 0 && (deadline.timeRemaining() > 2 || deadline.didTimeout)) {
      const task = _idleTasks.shift()!
      _registeredTasks.delete(task.id)
      try { task.task() }
      catch {}
    }

    if (_idleTasks.length > 0)
      scheduleIdleRun()
  }, { timeout: 2000 }) as number

  idleSchedulerInitialized = true
}

/** 调度空闲任务 */
export function scheduleIdle(
  id: string,
  task: () => void,
  priority: 'low' | 'normal' | 'high' = 'normal',
): Unsubscribe {
  // 去重
  if (_registeredTasks.has(id)) {
    return () => cancelIdle(id)
  }

  const p = priority === 'high' ? 2 : priority === 'normal' ? 1 : 0
  _idleTasks.push({ id, task, priority: p })
  _registeredTasks.add(id)

  // 按优先级排序（简单插入排序，因为通常队列很短）
  for (let i = _idleTasks.length - 1; i > 0; i--) {
    if (_idleTasks[i].priority > _idleTasks[i - 1].priority) {
      [_idleTasks[i], _idleTasks[i - 1]] = [_idleTasks[i - 1], _idleTasks[i]]
    }
    else {
      break
    }
  }

  scheduleIdleRun()
  return () => cancelIdle(id)
}

/** 取消空闲任务 */
export function cancelIdle(id: string): boolean {
  const idx = _idleTasks.findIndex(t => t.id === id)
  if (idx !== -1) {
    _idleTasks.splice(idx, 1)
    _registeredTasks.delete(id)
    return true
  }
  return false
}

// ==================== DOM 批量读写（原子化） ====================

let _reads: Array<() => void> = []
let _writes: Array<() => void> = []
let _domBatchScheduled = false

function flushDomBatch() {
  _domBatchScheduled = false

  // 先执行所有读取
  const reads = _reads
  _reads = []
  for (const r of reads) {
    try { r() }
    catch {}
  }

  // 再执行所有写入
  const writes = _writes
  _writes = []
  for (const w of writes) {
    try { w() }
    catch {}
  }
}

/** 批量 DOM 读取 */
export function batchRead(callback: () => void): void {
  _reads.push(callback)
  if (!_domBatchScheduled) {
    _domBatchScheduled = true
    requestAnimationFrame(flushDomBatch)
  }
}

/** 批量 DOM 写入 */
export function batchWrite(callback: () => void): void {
  _writes.push(callback)
  if (!_domBatchScheduled) {
    _domBatchScheduled = true
    requestAnimationFrame(flushDomBatch)
  }
}

// ==================== 页面生命周期（SPA 优化） ====================

/**
 * 开始新页面（SPA 路由切换时调用）
 * 清理旧页面资源，根据页面配置初始化所需功能
 */
export function startPage(pageId: string): void {
  if (currentPageId === pageId)
    return

  // 清理旧页面
  if (currentPageId) {
    cleanupPage()
  }

  currentPageId = pageId
  currentFeatures = PAGE_FEATURES[pageId] ?? 0
  isActive = true

  // 根据页面配置预初始化必要模块
  // 这确保了只有当前页面需要的功能才会初始化
  if (hasFeature(pageId, Feature.Visibility) && !visibilityInitialized) {
    initVisibility()
  }
}

/** 清理当前页面资源 */
function cleanupPage(): void {
  // 清理空闲任务
  _idleTasks.length = 0
  _registeredTasks.clear()
  if (_idleCallbackId !== null) {
    if (typeof cancelIdleCallback !== 'undefined') {
      cancelIdleCallback(_idleCallbackId)
    }
    _idleCallbackId = null
  }

  // 清理 DOM 批量队列
  _reads.length = 0
  _writes.length = 0
  _domBatchScheduled = false

  // 清理 resize 批量队列
  _resizeBatch.length = 0
  _resizeScheduled = false

  // MessageChannel 待处理回调
  _pendingCallbacks.length = 0
}

/**
 * 暂停调度器（如后台标签页）
 */
export function pause(): void {
  isActive = false
}

/**
 * 恢复调度器
 */
export function resume(): void {
  isActive = true
  scheduleIdleRun()
}

/** 获取当前页面 ID */
export function getCurrentPageId(): string | null {
  return currentPageId
}

/** 是否处于活动状态 */
export function isSchedulerActive(): boolean {
  return isActive && _isPageVisible
}

// ==================== 统计信息（调试用） ====================

/** 获取调度器状态 */
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

// ==================== 销毁（仅测试用） ====================

export function destroy(): void {
  cleanupPage()

  // 清理可见性
  if (_visibilityHandler && typeof document !== 'undefined') {
    document.removeEventListener('visibilitychange', _visibilityHandler)
    _visibilityHandler = null
  }
  _visibilitySubscribers.clear()
  visibilityInitialized = false

  // 清理 MessageChannel
  if (_channel) {
    _channel.port1.close()
    _channel.port2.close()
    _channel = null
  }
  messageChannelInitialized = false

  // 清理 ResizeObserver
  if (_resizeObserver) {
    _resizeObserver.disconnect()
    _resizeObserver = null
  }
  _resizeElements.clear()
  resizeObserverInitialized = false

  // 清理 IntersectionObserver
  for (const obs of _intersectionObservers.values()) {
    obs.disconnect()
  }
  _intersectionObservers.clear()
  _intersectionElements.clear()
  intersectionInitialized = false

  currentPageId = null
  currentFeatures = 0
  isActive = true
}
