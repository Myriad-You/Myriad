import type {
  AnimationConfig,
  AnimationListener,
  CoordinatorConfig,
  Unsubscribe,
} from './types'
import {
  batchRead as coreBatchRead,
  batchWrite as coreBatchWrite,
  isPageVisible as coreIsPageVisible,
  now as coreNow,
  refreshNow as coreRefreshNow,
  onVisibility,
  scheduleTask,
} from './core'
import { runIdleSlice } from './idleSlice'
import { AnimationPriority, AnimationState, DEFAULT_CONFIG } from './types'

interface IdleDeadline {
  didTimeout: boolean
  timeRemaining: () => number
}

interface AnimationSlot {
  id: string
  priority: AnimationPriority
  startTime: number
  duration: number
}

interface WaitingItem {
  id: string
  priority: AnimationPriority
  delay: number
  index: number
  groupId?: string
  registeredAt: number
}

class AnimationCoordinator {
  private config: CoordinatorConfig

  private states = new Map<string, AnimationState>()

  private listeners = new Map<string, Set<AnimationListener>>()

  private currentPageId: string | null = null
  private isPageReady = false

  private pageReadyCallbacks = new Set<() => void>()

  // 用 microtask 刷新，不用 RAF。
  private pendingUpdates = new Set<string>()
  private isMicrotaskScheduled = false

  private activeSlots = new Map<string, AnimationSlot>()

  private waitingQueue: WaitingItem[] = []

  /** 瞬时 activeSlots 常为 0；峰值才能证明槽位跑过。 */
  private peakActiveSlots = 0

  private totalScheduled = 0

  private totalAcquired = 0

  private burstStartTime: number = 0

  private inBurstMode: boolean = false

  private currentBurstDuration: number = 0

  /** 超时 2s 自动放槽。 */
  private readonly ANIMATION_TIMEOUT = 2000

  private timeoutCheckerId: ReturnType<typeof setInterval> | null = null

  private delayedQueue: Array<{
    id: string
    executeAt: number
    priority: AnimationPriority
  }> = []

  private delayTimerId: ReturnType<typeof setTimeout> | null = null

  /** 每片最多 8 个，避开 Long Task。 */
  private readonly BATCH_SIZE = 8

  private frameBudget: number = 16

  /** 2 的幂，环形下标用 & 63。 */
  private readonly FPS_SAMPLE_SIZE = 64

  /** 低帧率 = 检测刷新率 * 0.75。 */
  private readonly LOW_FPS_RATIO = 0.75

  private readonly FPS_UPDATE_INTERVAL = 1000

  private frameTimes: Float32Array = new Float32Array(64)

  private frameTimeIndex: number = 0

  private frameTimeCount: number = 0
  private currentFps: number = 60

  private isLowFpsMode: boolean = false

  private fpsMonitorRafId: number | null = null

  private lastFrameTimestamp: number = 0

  private lastFpsUpdateTime: number = 0

  private fpsMonitorRunning: boolean = false
  private totalFrames: number = 0

  private frameTimeSum: number = 0
  private detectedRefreshRate: number = 60
  private minFrameTime: number = Infinity
  private refreshRateDetected: boolean = false
  private lowFpsThreshold: number = 45

  private sharedResizeObserver: ResizeObserver | null = null

  private resizeCallbacks = new WeakMap<
    Element,
    (entry: ResizeObserverEntry) => void
  >()

  private observedElements = new Set<Element>()

  private resizeBatchQueue: Array<{
    element: Element
    entry: ResizeObserverEntry
  }> = []

  private resizeBatchScheduled: boolean = false

  private readonly RESIZE_THROTTLE_MS = 50

  private lastResizeProcessTime: number = 0

  /** 小于 4px 的尺寸变化忽略。 */
  private readonly RESIZE_THRESHOLD_PX = 4

  private elementSizeCache = new WeakMap<
    Element,
    { width: number; height: number }
  >()

  private idleTaskQueue: Array<{
    id: string
    task: () => void
    timeout?: number
    priority: number // 0=低, 1=中, 2=高
  }> = []

  private idleCallbackId: number | null = null

  private registeredIdleTasks = new Set<string>()

  private waitingQueueIndex = new Map<string, number>()

  // 升版本以丢掉页面就绪前已作废的回调。
  private scheduleVersions = new Map<string, number>()

  constructor(config: Partial<CoordinatorConfig> = {}) {
    this.config = { ...DEFAULT_CONFIG, ...config }

    onVisibility((visible) => {
      if (visible) {
        if (this.fpsMonitorRunning) {
          this.lastFrameTimestamp = performance.now()
          this.lastFpsUpdateTime = this.lastFrameTimestamp
          this.pumpFpsMonitor()
        }
        this.processWaitQueue()
        this.scheduleIdleCallback()
      } else {
        this.pauseFpsMonitorLoop()
      }
    })

    // 首屏爆发 10s。
    this.activateBurstMode(10000)
  }

  private startTimeoutChecker() {
    if (this.timeoutCheckerId !== null || this.activeSlots.size === 0) return

    this.timeoutCheckerId = setInterval(() => {
      if (typeof document !== 'undefined' && document.hidden) return
      this.cleanupTimedOutSlots()
    }, 1000)
  }

  private stopTimeoutChecker() {
    if (this.timeoutCheckerId === null) return

    clearInterval(this.timeoutCheckerId)
    this.timeoutCheckerId = null
  }

  private stopTimeoutCheckerIfIdle() {
    if (this.activeSlots.size === 0) this.stopTimeoutChecker()
  }

  private cleanupTimedOutSlots() {
    if (!coreIsPageVisible()) return

    if (this.activeSlots.size === 0) {
      this.stopTimeoutChecker()
      return
    }

    const now = coreRefreshNow()
    let hasTimedOut = false

    for (const [id, slot] of this.activeSlots) {
      if (now - slot.startTime > this.ANIMATION_TIMEOUT) {
        this.activeSlots.delete(id)
        this.states.set(id, AnimationState.COMPLETED)
        this.pendingUpdates.add(id)
        hasTimedOut = true
      }
    }

    if (hasTimedOut) {
      this.scheduleMicrotaskFlush()
      this.processWaitQueue()
      this.stopTimeoutCheckerIfIdle()
    }
  }

  private getMaxConcurrent(): number {
    let maxConcurrent = this.config.baseConcurrent

    if (this.inBurstMode) {
      const elapsed = coreNow() - this.burstStartTime
      if (elapsed < this.currentBurstDuration) {
        maxConcurrent = this.config.burstConcurrent
      } else {
        this.inBurstMode = false
      }
    }

    // 低帧率只降后续并发，不打断已开始的动画。
    return this.isLowFpsMode
      ? Math.max(2, Math.ceil(maxConcurrent / 2))
      : maxConcurrent
  }

  private checkAndTriggerBurst() {
    if (this.inBurstMode) return

    const totalQueued = this.waitingQueue.length + this.delayedQueue.length
    if (totalQueued > Math.ceil(this.config.baseConcurrent * 0.5)) {
      this.activateBurstMode(this.config.burstDuration)
    }
  }

  private activateBurstMode(duration: number) {
    this.inBurstMode = true
    this.burstStartTime = coreRefreshNow()
    this.currentBurstDuration = duration
  }

  updateConfig(config: Partial<CoordinatorConfig>) {
    this.config = { ...this.config, ...config }
  }

  startPageTransition(pageId: string) {
    if (this.currentPageId && this.currentPageId !== pageId) {
      this.cleanupPage(this.currentPageId)
    }

    this.currentPageId = pageId
    this.isPageReady = false

    // 页面切换爆发 10s。
    this.activateBurstMode(10000)
  }

  completePageTransition(pageId?: string): boolean {
    // 旧页迟到的 RAF 不得放行当前页。
    if (pageId && this.currentPageId !== pageId) return false

    this.isPageReady = true

    const callbackCount = this.pageReadyCallbacks.size
    if (callbackCount > 0) {
      if (callbackCount <= this.BATCH_SIZE) {
        // 先换 Set，回调才能再注册。
        const callbacks = this.pageReadyCallbacks
        this.pageReadyCallbacks = new Set()
        queueMicrotask(() => {
          for (const cb of callbacks) {
            cb()
          }
        })
      } else {
        const callbacks = Iterator.from(this.pageReadyCallbacks).toArray()
        this.pageReadyCallbacks.clear()

        let index = 0
        const processBatch = () => {
          const end = Math.min(index + this.BATCH_SIZE, callbacks.length)
          for (; index < end; index++) {
            callbacks[index]()
          }
          if (index < callbacks.length) {
            // scheduleTask 走 MessageChannel，不是 setTimeout。
            scheduleTask(processBatch)
          }
        }
        queueMicrotask(processBatch)
      }
    }

    this.processDelayedQueue()
    return true
  }

  onPageReady(callback: () => void): Unsubscribe {
    if (this.isPageReady) {
      let active = true
      queueMicrotask(() => {
        if (active) callback()
      })
      return () => {
        active = false
      }
    }

    this.pageReadyCallbacks.add(callback)
    return () => this.pageReadyCallbacks.delete(callback)
  }

  getPageReadyState(): boolean {
    return this.isPageReady
  }

  private cleanupPage(_pageId: string) {
    this.pageReadyCallbacks.clear()

    this.states.clear()
    this.listeners.clear()
    this.pendingUpdates.clear()
    this.isMicrotaskScheduled = false

    this.delayedQueue.length = 0
    if (this.delayTimerId) {
      clearTimeout(this.delayTimerId)
      this.delayTimerId = null
    }

    this.activeSlots.clear()
    this.waitingQueue.length = 0
    this.waitingQueueIndex.clear()
    this.scheduleVersions.clear()
    this.stopTimeoutChecker()
  }

  schedule(config: AnimationConfig): AnimationState {
    const { id, priority, delay = 0, index = 0, groupId } = config
    this.totalScheduled++
    const scheduleVersion = (this.scheduleVersions.get(id) ?? 0) + 1
    this.scheduleVersions.set(id, scheduleVersion)

    this.removeQueuedAnimation(id)
    if (this.activeSlots.has(id)) {
      this.releaseSlot(id)
      this.processWaitQueue()
    }

    // PAGE 不占槽。
    if (priority === AnimationPriority.PAGE) {
      this.states.set(id, AnimationState.READY)
      return AnimationState.READY
    }

    if (!this.isPageReady) {
      this.states.set(id, AnimationState.WAITING)

      this.onPageReady(() => {
        if (
          this.scheduleVersions.get(id) !== scheduleVersion ||
          this.states.get(id) !== AnimationState.WAITING
        ) {
          return
        }
        this.scheduleAfterPageReady(id, delay, index, groupId, priority)
      })

      return AnimationState.WAITING
    }

    return this.scheduleAfterPageReady(id, delay, index, groupId, priority)
  }

  private scheduleAfterPageReady(
    id: string,
    delay: number,
    index: number,
    groupId?: string,
    priority: AnimationPriority = AnimationPriority.COMPONENT,
  ): AnimationState {
    const staggerDelay = groupId ? index * this.config.defaultStaggerDelay : 0
    const totalDelay = delay + staggerDelay

    if (totalDelay > 0) {
      this.states.set(id, AnimationState.SCHEDULED)
      this.addToDelayedQueue(id, totalDelay, priority)
      return AnimationState.SCHEDULED
    }

    return this.tryAcquireSlot(id, priority)
  }

  private tryAcquireSlot(
    id: string,
    priority: AnimationPriority,
  ): AnimationState {
    const maxConcurrent = this.getMaxConcurrent()

    if (this.activeSlots.size < maxConcurrent) {
      this.acquireSlot(id, priority)
      this.markReady(id)
      return AnimationState.READY
    }

    if (this.canPreempt(priority)) {
      this.preemptLowestPriority(id, priority)
      return AnimationState.READY
    }

    this.addToWaitQueue(id, priority)
    return AnimationState.SCHEDULED
  }

  private acquireSlot(id: string, priority: AnimationPriority) {
    const now = coreNow()
    this.activeSlots.set(id, {
      id,
      priority,
      startTime: now,
      duration: 0,
    })
    this.totalAcquired++
    if (this.activeSlots.size > this.peakActiveSlots) {
      this.peakActiveSlots = this.activeSlots.size
    }
    this.startTimeoutChecker()
  }

  private canPreempt(priority: AnimationPriority): boolean {
    // 数值越小优先级越高；仅 SECTION 及以上可抢占。
    if (priority > AnimationPriority.SECTION) return false

    if (this.activeSlots.size === 0) return false

    for (const slot of this.activeSlots.values()) {
      if (slot.priority > priority) {
        return true
      }
    }
    return false
  }

  private preemptLowestPriority(id: string, priority: AnimationPriority) {
    let lowestPriority = -1
    let victimSlot: AnimationSlot | null = null

    for (const slot of this.activeSlots.values()) {
      if (slot.priority > lowestPriority) {
        lowestPriority = slot.priority
        victimSlot = slot
      }
    }

    if (victimSlot) {
      const victimId = victimSlot.id

      this.releaseSlot(victimId)
      this.skip(victimId)

      this.acquireSlot(id, priority)
      this.markReady(id)
    }
  }

  private addToWaitQueue(id: string, priority: AnimationPriority) {
    if (this.waitingQueueIndex.has(id)) {
      return
    }

    this.states.set(id, AnimationState.SCHEDULED)

    const item: WaitingItem = {
      id,
      priority,
      delay: 0,
      index: 0,
      registeredAt: coreNow(),
    }

    const queueLen = this.waitingQueue.length
    if (
      queueLen === 0 ||
      this.waitingQueue[queueLen - 1].priority <= priority
    ) {
      this.waitingQueueIndex.set(id, queueLen)
      this.waitingQueue.push(item)
      return
    }

    let left = 0
    let right = queueLen
    while (left < right) {
      const mid = (left + right) >>> 1
      if (this.waitingQueue[mid].priority <= priority) {
        left = mid + 1
      } else {
        right = mid
      }
    }
    this.waitingQueue = this.waitingQueue.toSpliced(left, 0, item)

    this.rebuildWaitingQueueIndex(left)
  }

  private rebuildWaitingQueueIndex(fromIndex: number = 0) {
    for (let i = fromIndex; i < this.waitingQueue.length; i++) {
      this.waitingQueueIndex.set(this.waitingQueue[i].id, i)
    }
  }

  private removeQueuedAnimation(id: string) {
    const waitIndex = this.waitingQueueIndex.get(id)
    if (waitIndex !== undefined) {
      this.waitingQueue = this.waitingQueue.toSpliced(waitIndex, 1)
      this.waitingQueueIndex.delete(id)
      this.rebuildWaitingQueueIndex(waitIndex)
    }

    const previousFirstId = this.delayedQueue[0]?.id
    this.delayedQueue = this.delayedQueue.filter((item) => item.id !== id)

    if (previousFirstId === id && this.delayTimerId) {
      clearTimeout(this.delayTimerId)
      this.delayTimerId = null
      this.scheduleNextDelay()
    }
  }

  private releaseSlot(id: string) {
    this.activeSlots.delete(id)
  }

  private processWaitQueue() {
    if (this.waitingQueue.length === 0) return

    this.checkAndTriggerBurst()

    const maxConcurrent = this.getMaxConcurrent()
    let processed = 0

    while (
      this.waitingQueue.length > 0 &&
      this.activeSlots.size < maxConcurrent &&
      processed < this.BATCH_SIZE
    ) {
      const next = this.waitingQueue.shift()
      if (next) {
        this.waitingQueueIndex.delete(next.id)
        this.acquireSlot(next.id, next.priority)
        this.markReady(next.id)
        processed++
      }
    }

    if (processed > 0 && this.waitingQueue.length > 0) {
      this.rebuildWaitingQueueIndex(0)
    }

    if (this.waitingQueue.length > 0 && this.activeSlots.size < maxConcurrent) {
      // scheduleTask 走 MessageChannel，不是 setTimeout。
      scheduleTask(() => this.processWaitQueue())
    }
  }

  private addToDelayedQueue(
    id: string,
    delay: number,
    priority: AnimationPriority = AnimationPriority.COMPONENT,
  ) {
    const executeAt = coreNow() + delay
    const item = { id, executeAt, priority }

    const queueLen = this.delayedQueue.length
    if (
      queueLen === 0 ||
      this.delayedQueue[queueLen - 1].executeAt <= executeAt
    ) {
      this.delayedQueue.push(item)
      this.scheduleNextDelay()
      return
    }

    let left = 0
    let right = queueLen
    while (left < right) {
      const mid = (left + right) >>> 1
      if (this.delayedQueue[mid].executeAt <= executeAt) {
        left = mid + 1
      } else {
        right = mid
      }
    }
    this.delayedQueue = this.delayedQueue.toSpliced(left, 0, item)

    this.scheduleNextDelay()
  }

  private scheduleNextDelay() {
    if (this.delayTimerId || this.delayedQueue.length === 0) return

    const next = this.delayedQueue[0]
    const wait = Math.max(0, next.executeAt - coreNow())

    this.delayTimerId = setTimeout(() => {
      this.delayTimerId = null
      this.processDelayedQueue()
    }, wait)
  }

  private processDelayedQueue() {
    const now = coreRefreshNow()

    while (this.delayedQueue.length > 0) {
      const next = this.delayedQueue[0]
      if (next.executeAt > now) break

      this.delayedQueue.shift()

      this.tryAcquireSlot(next.id, next.priority)
    }

    this.scheduleNextDelay()
  }

  private markReady(id: string) {
    this.states.set(id, AnimationState.READY)
    this.pendingUpdates.add(id)
    this.scheduleMicrotaskFlush()
  }

  /** 槽位在 schedule 时已占，这里只标 running。 */
  markRunning(id: string) {
    this.states.set(id, AnimationState.RUNNING)
    this.pendingUpdates.add(id)
    this.scheduleMicrotaskFlush()
  }

  markCompleted(id: string) {
    this.states.set(id, AnimationState.COMPLETED)
    this.removeQueuedAnimation(id)

    if (this.activeSlots.has(id)) {
      this.releaseSlot(id)

      this.processWaitQueue()
    }

    this.pendingUpdates.add(id)
    this.scheduleMicrotaskFlush()
    this.stopTimeoutCheckerIfIdle()
  }

  skip(id: string) {
    this.scheduleVersions.set(id, (this.scheduleVersions.get(id) ?? 0) + 1)
    this.states.set(id, AnimationState.SKIPPED)
    this.removeQueuedAnimation(id)

    if (this.activeSlots.has(id)) {
      this.releaseSlot(id)
      this.processWaitQueue()
    }

    this.pendingUpdates.add(id)
    this.scheduleMicrotaskFlush()
    this.stopTimeoutCheckerIfIdle()
  }

  /** microtask 刷新不计入 RAF。 */
  private scheduleMicrotaskFlush() {
    if (this.isMicrotaskScheduled) return
    this.isMicrotaskScheduled = true

    queueMicrotask(() => {
      this.isMicrotaskScheduled = false
      this.flushPendingUpdates()
    })
  }

  private flushPendingUpdates() {
    if (this.pendingUpdates.size === 0) return

    if (this.pendingUpdates.size <= this.BATCH_SIZE) {
      for (const id of this.pendingUpdates) {
        const state = this.states.get(id)
        if (state) {
          this.notify(id, state)
        }
      }
      this.pendingUpdates.clear()
      return
    }

    const ids = Iterator.from(this.pendingUpdates).toArray()
    this.pendingUpdates.clear()

    let index = 0
    const processBatch = () => {
      const end = Math.min(index + this.BATCH_SIZE, ids.length)
      for (; index < end; index++) {
        const id = ids[index]
        const state = this.states.get(id)
        if (state) {
          this.notify(id, state)
        }
      }

      if (index < ids.length) {
        // scheduleTask 走 MessageChannel，不是 setTimeout。
        scheduleTask(processBatch)
      }
    }

    processBatch()
  }

  subscribe(id: string, callback: AnimationListener): Unsubscribe {
    let listenerSet = this.listeners.get(id)
    if (!listenerSet) {
      listenerSet = new Set()
      this.listeners.set(id, listenerSet)
    }
    listenerSet.add(callback)

    const state = this.states.get(id)
    if (state) {
      queueMicrotask(() => {
        if (this.listeners.get(id)?.has(callback)) callback(state)
      })
    }

    return () => {
      const listeners = this.listeners.get(id)
      if (listeners) {
        listeners.delete(callback)
        if (listeners.size === 0) {
          this.listeners.delete(id)

          // 最后一个监听者离开时连 state 一起清。
          this.states.delete(id)
          this.pendingUpdates.delete(id)
        }
      }
    }
  }

  private notify(id: string, state: AnimationState) {
    const listeners = this.listeners.get(id)
    if (listeners) {
      for (const cb of listeners) {
        cb(state)
      }
    }
  }

  getState(id: string): AnimationState | undefined {
    return this.states.get(id)
  }

  getStaggerDelay(index: number, baseDelay?: number): number {
    return index * (baseDelay ?? this.config.defaultStaggerDelay)
  }

  /** 瞬时槽位几乎总是 0；看 peak / totals。 */
  getConcurrencyStatus() {
    const maxConcurrent = this.getMaxConcurrent()
    const totalQueued = this.waitingQueue.length + this.delayedQueue.length
    const pressureThreshold = Math.ceil(this.config.baseConcurrent * 0.5)

    return {
      activeSlots: this.activeSlots.size,
      maxConcurrent,
      baseConcurrent: this.config.baseConcurrent,
      burstConcurrent: this.config.burstConcurrent,
      inBurstMode: this.inBurstMode,
      burstTimeRemaining: this.inBurstMode
        ? Math.max(
            0,
            this.currentBurstDuration - (coreNow() - this.burstStartTime),
          )
        : 0,
      waitingQueue: this.waitingQueue.length,
      delayedQueue: this.delayedQueue.length,
      totalQueued,
      queuePressure: totalQueued > pressureThreshold,
      load: this.activeSlots.size / maxConcurrent,

      peakActiveSlots: this.peakActiveSlots,

      totalScheduled: this.totalScheduled,

      totalAcquired: this.totalAcquired,

      pageReady: this.isPageReady,
      currentPageId: this.currentPageId,

      statesSize: this.states.size,
    }
  }

  resetConcurrencyStats() {
    this.peakActiveSlots = this.activeSlots.size
    this.totalScheduled = 0
    this.totalAcquired = 0
  }

  reset() {
    this.states.clear()
    this.listeners.clear()
    this.pendingUpdates.clear()
    this.pageReadyCallbacks.clear()
    this.delayedQueue.length = 0
    this.isMicrotaskScheduled = false

    this.activeSlots.clear()
    this.waitingQueue.length = 0
    this.waitingQueueIndex.clear()
    this.scheduleVersions.clear()
    this.stopTimeoutChecker()

    this.inBurstMode = false
    this.burstStartTime = 0
    this.currentBurstDuration = 0

    if (this.delayTimerId) {
      clearTimeout(this.delayTimerId)
      this.delayTimerId = null
    }

    this.isPageReady = false
    this.currentPageId = null
  }

  startFpsMonitor(): void {
    if (!this.fpsMonitorRunning) {
      this.fpsMonitorRunning = true
      this.lastFrameTimestamp = performance.now()
      this.lastFpsUpdateTime = this.lastFrameTimestamp
      this.frameTimeIndex = 0
      this.frameTimeCount = 0
      this.frameTimeSum = 0
      this.frameTimes.fill(0)
      this.totalFrames = 0
      this.minFrameTime = Infinity
      this.refreshRateDetected = false
    }
    this.pumpFpsMonitor()
  }

  private pauseFpsMonitorLoop(): void {
    if (this.fpsMonitorRafId !== null) {
      cancelAnimationFrame(this.fpsMonitorRafId)
      this.fpsMonitorRafId = null
    }
  }

  private pumpFpsMonitor(): void {
    if (!this.fpsMonitorRunning || this.fpsMonitorRafId !== null) return
    if (!coreIsPageVisible()) return
    this.lastFrameTimestamp = performance.now()
    this.fpsMonitorRafId = requestAnimationFrame((timestamp) => {
      this.measureFps(timestamp)
    })
  }

  private measureFps(timestamp: number): void {
    if (!this.fpsMonitorRunning) return
    if (!coreIsPageVisible()) {
      this.fpsMonitorRafId = null
      return
    }

    const frameTime = timestamp - this.lastFrameTimestamp
    this.lastFrameTimestamp = timestamp
    this.totalFrames++

    if (!this.refreshRateDetected && this.totalFrames <= 30) {
      // 前 30 帧忽略 <4ms 的测量噪声。
      if (frameTime > 4 && frameTime < this.minFrameTime) {
        this.minFrameTime = frameTime
      }

      if (this.totalFrames === 30 && this.minFrameTime < Infinity) {
        this.refreshRateDetected = true

        const inferredRate = Math.round(1000 / this.minFrameTime)

        this.detectedRefreshRate = this.snapToCommonRefreshRate(inferredRate)

        this.frameBudget = 1000 / this.detectedRefreshRate
        this.lowFpsThreshold = Math.round(
          this.detectedRefreshRate * this.LOW_FPS_RATIO,
        )

        this.currentFps = this.detectedRefreshRate
      }
    }

    const idx = this.frameTimeIndex
    const oldValue = this.frameTimes[idx]
    this.frameTimes[idx] = frameTime

    this.frameTimeIndex = (idx + 1) & 63

    if (this.frameTimeCount < this.FPS_SAMPLE_SIZE) {
      this.frameTimeCount++
      this.frameTimeSum += frameTime
    } else {
      this.frameTimeSum = this.frameTimeSum - oldValue + frameTime
    }

    const timeSinceUpdate = timestamp - this.lastFpsUpdateTime
    if (
      timeSinceUpdate >= this.FPS_UPDATE_INTERVAL &&
      this.frameTimeCount >= 10
    ) {
      this.lastFpsUpdateTime = timestamp

      const avgFrameTime = this.frameTimeSum / this.frameTimeCount

      const rawFps = 1000 / avgFrameTime

      // FPS：80% 新值 + 20% 旧值。
      this.currentFps = Math.round(rawFps * 0.8 + this.currentFps * 0.2)

      this.isLowFpsMode = this.currentFps < this.lowFpsThreshold
    }

    this.fpsMonitorRafId = requestAnimationFrame((next) => {
      this.measureFps(next)
    })
  }

  private snapToCommonRefreshRate(inferredRate: number): number {
    const commonRates = [60, 72, 75, 90, 120, 144, 165, 240, 360]

    let closest = commonRates[0]
    let minDiff = Math.abs(inferredRate - closest)

    for (const rate of commonRates) {
      const diff = Math.abs(inferredRate - rate)
      if (diff < minDiff) {
        minDiff = diff
        closest = rate
      }
    }

    // 对齐误差 >10% 时保留原始推断。
    if (minDiff > inferredRate * 0.1) {
      return inferredRate
    }

    return closest
  }

  stopFpsMonitor(): void {
    this.fpsMonitorRunning = false
    if (this.fpsMonitorRafId !== null) {
      cancelAnimationFrame(this.fpsMonitorRafId)
      this.fpsMonitorRafId = null
    }
  }

  isLowFps(): boolean {
    return this.isLowFpsMode
  }

  /** 卡顿看 64 帧窗口，不再用会话累计。 */
  getFrameStats(): {
    fps: number
    avgFrameTime: number
    isLowFps: boolean

    maxFrameMs: number

    p95FrameMs: number

    jankRatio: number

    jankThresholdMs: number
    totalFrames: number
    isMonitoring: boolean
    sampleCount: number

    detectedRefreshRate: number

    lowFpsThreshold: number

    refreshRateDetected: boolean
  } {
    const avgFrameTime =
      this.frameTimeCount > 0 ? this.frameTimeSum / this.frameTimeCount : 16

    let maxFrameMs = 0
    let jankFrames = 0
    const jankThresholdMs = this.frameBudget * 2
    const n = this.frameTimeCount

    const samples: number[] = []
    for (let i = 0; i < n; i++) {
      const t = this.frameTimes[i]

      if (t <= 0) continue
      samples.push(t)
      if (t > maxFrameMs) maxFrameMs = t
      if (t > jankThresholdMs) jankFrames++
    }

    const sampleN = samples.length
    const jankRatio = sampleN > 0 ? jankFrames / sampleN : 0
    let p95FrameMs = 0
    if (sampleN > 0) {
      const ranked = samples.toSorted((a, b) => a - b)

      const idx = Math.min(
        sampleN - 1,
        // nearest-rank P95：ceil(0.95*n)-1。
        Math.max(0, Math.ceil(sampleN * 0.95) - 1),
      )
      p95FrameMs = ranked[idx]
    }

    return {
      fps: this.currentFps,
      avgFrameTime,
      isLowFps: this.isLowFpsMode,
      maxFrameMs: Math.round(maxFrameMs * 10) / 10,
      p95FrameMs: Math.round(p95FrameMs * 10) / 10,
      jankRatio,
      jankThresholdMs: Math.round(jankThresholdMs * 10) / 10,
      totalFrames: this.totalFrames,
      isMonitoring: this.fpsMonitorRunning,
      sampleCount: this.frameTimeCount,
      detectedRefreshRate: this.detectedRefreshRate,
      lowFpsThreshold: this.lowFpsThreshold,
      refreshRateDetected: this.refreshRateDetected,
    }
  }

  batchRead(callback: () => void): void {
    coreBatchRead(callback)
  }

  batchWrite(callback: () => void): void {
    coreBatchWrite(callback)
  }

  private initSharedResizeObserver(): void {
    if (typeof ResizeObserver === 'undefined') return

    this.sharedResizeObserver = new ResizeObserver((entries) => {
      if (!coreIsPageVisible()) return

      for (const entry of entries) {
        const callback = this.resizeCallbacks.get(entry.target)
        if (callback) {
          const { width, height } = entry.contentRect
          const cached = this.elementSizeCache.get(entry.target)

          if (cached) {
            const widthDiff = Math.abs(cached.width - width)
            const heightDiff = Math.abs(cached.height - height)

            if (
              widthDiff < this.RESIZE_THRESHOLD_PX &&
              heightDiff < this.RESIZE_THRESHOLD_PX
            ) {
              continue
            }
          }

          this.elementSizeCache.set(entry.target, { width, height })

          this.resizeBatchQueue.push({ element: entry.target, entry })
        }
      }

      this.scheduleResizeBatch()
    })
  }

  private scheduleResizeBatch(): void {
    if (this.resizeBatchScheduled || this.resizeBatchQueue.length === 0) return

    const now = coreNow()
    const timeSinceLastProcess = now - this.lastResizeProcessTime

    if (timeSinceLastProcess >= this.RESIZE_THROTTLE_MS) {
      this.resizeBatchScheduled = true
      requestAnimationFrame(() => {
        this.flushResizeBatch()
      })
    } else {
      this.resizeBatchScheduled = true
      setTimeout(() => {
        requestAnimationFrame(() => {
          this.flushResizeBatch()
        })
      }, this.RESIZE_THROTTLE_MS - timeSinceLastProcess)
    }
  }

  private flushResizeBatch(): void {
    this.resizeBatchScheduled = false
    this.lastResizeProcessTime = coreNow()

    const batch = this.resizeBatchQueue
    this.resizeBatchQueue = []

    for (const { element, entry } of batch) {
      const callback = this.resizeCallbacks.get(element)
      if (callback) {
        try {
          callback(entry)
        } catch (e) {
          console.error('ResizeObserver callback error:', e)
        }
      }
    }
  }

  observeResize(
    element: Element,
    callback: (entry: ResizeObserverEntry) => void,
    _options?: { immediate?: boolean },
  ): () => void {
    if (!element) {
      return () => {}
    }
    if (!this.sharedResizeObserver) this.initSharedResizeObserver()
    if (!this.sharedResizeObserver) return () => {}

    this.resizeCallbacks.set(element, callback)
    this.observedElements.add(element)

    this.sharedResizeObserver.observe(element, { box: 'border-box' })
    // `immediate` used to synthesize a ResizeObserverEntry via
    // getBoundingClientRect (forced reflow). Callers that need a first size
    // already measure themselves; the observer still delivers the next frame.

    return () => {
      this.unobserveResize(element)
    }
  }

  unobserveResize(element: Element): void {
    if (!this.sharedResizeObserver) return

    this.sharedResizeObserver.unobserve(element)
    this.resizeCallbacks.delete(element)
    this.observedElements.delete(element)
    this.elementSizeCache.delete(element)

    this.resizeBatchQueue = this.resizeBatchQueue.filter(
      (item) => item.element !== element,
    )
  }

  getCachedSize(element: Element): { width: number; height: number } | null {
    return this.elementSizeCache.get(element) ?? null
  }

  scheduleIdleTask(
    id: string,
    task: () => void,
    options: {
      timeout?: number
      priority?: 'low' | 'normal' | 'high'
      dedupe?: boolean
    } = {},
  ): () => void {
    const { timeout, priority = 'normal', dedupe = true } = options

    if (dedupe && this.registeredIdleTasks.has(id)) {
      return () => this.cancelIdleTask(id)
    }

    const priorityValue =
      priority === 'high' ? 2 : priority === 'normal' ? 1 : 0

    this.idleTaskQueue.push({ id, task, timeout, priority: priorityValue })
    this.registeredIdleTasks.add(id)

    this.idleTaskQueue = this.idleTaskQueue.toSorted(
      (a, b) => b.priority - a.priority,
    )

    this.scheduleIdleCallback()

    return () => this.cancelIdleTask(id)
  }

  cancelIdleTask(id: string): boolean {
    const index = this.idleTaskQueue.findIndex((t) => t.id === id)
    if (index !== -1) {
      this.idleTaskQueue = this.idleTaskQueue.toSpliced(index, 1)
      this.registeredIdleTasks.delete(id)
      return true
    }
    return false
  }

  private scheduleIdleCallback() {
    if (this.idleCallbackId !== null || this.idleTaskQueue.length === 0) return

    if (!coreIsPageVisible()) return

    const scheduleIdle =
      typeof requestIdleCallback !== 'undefined'
        ? requestIdleCallback
        : (cb: IdleRequestCallback) =>
            setTimeout(
              () =>
                cb({
                  didTimeout: false,
                  timeRemaining: () => 50,
                }),
              1,
            )

    const highestPriorityTask = this.idleTaskQueue[0]

    this.idleCallbackId = scheduleIdle(
      (deadline: IdleDeadline) => {
        this.idleCallbackId = null
        this.processIdleTasks(deadline)
      },
      highestPriorityTask?.timeout
        ? { timeout: highestPriorityTask.timeout }
        : undefined,
    ) as number
  }

  private processIdleTasks(deadline: IdleDeadline) {
    runIdleSlice(
      this.idleTaskQueue,
      deadline,
      (taskInfo) => {
        this.registeredIdleTasks.delete(taskInfo.id)
        try {
          taskInfo.task()
        } catch (e) {
          console.error(`[Coordinator] Idle task "${taskInfo.id}" error:`, e)
        }
      },
      5,
    )

    if (this.idleTaskQueue.length > 0) {
      this.scheduleIdleCallback()
    }
  }
}

export const coordinator = new AnimationCoordinator()
