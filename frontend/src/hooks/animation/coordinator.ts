/**
 * 动画协调器核心类
 *
 * 设计原则：
 * 1. 单一 RAF 循环，批量更新
 * 2. 页面级事件驱动，元素级队列调度
 * 3. Ref 存储状态，最小化渲染
 * 4. 支持优先级抢占和跳过
 * 5. 并发控制，限制同时运行的动画数量
 * 6. WeakRef 元素追踪，自动 GC 释放（可选）
 */

import type {
  AnimationConfig,
  AnimationListener,
  CoordinatorConfig,
  Unsubscribe,
} from './types'
import {
  AnimationPriority,
  AnimationState,
  DEFAULT_CONFIG,
} from './types'

/** IdleDeadline 类型（用于 requestIdleCallback） */
interface IdleDeadline {
  didTimeout: boolean
  timeRemaining: () => number
}

/** 动画槽位信息 */
interface AnimationSlot {
  id: string
  priority: AnimationPriority
  startTime: number
  duration: number
}

/** 等待队列项 */
interface WaitingItem {
  id: string
  priority: AnimationPriority
  delay: number
  index: number
  groupId?: string
  registeredAt: number
}

/** 🔧 WeakRef 元素追踪项 */
interface WeakRefEntry {
  ref: WeakRef<HTMLElement>
  animationId: string
}

class AnimationCoordinator {
  private config: CoordinatorConfig

  // 状态存储（非响应式）
  private states = new Map<string, AnimationState>()

  // 事件订阅
  private listeners = new Map<string, Set<AnimationListener>>()

  // ==================== 高效任务调度 ====================
  // 🔧 使用 MessageChannel 替代 setTimeout(0)，更高效的微任务调度
  private messageChannel: MessageChannel | null = null
  private pendingYieldCallbacks: Array<() => void> = []

  // ==================== 页面可见性优化 ====================
  // 🔧 页面是否可见，不可见时暂停非关键操作
  private isPageVisible: boolean = true
  private visibilityHandler: (() => void) | null = null

  // ==================== 时间戳缓存（减少 performance.now 调用）====================
  // 当前帧缓存的时间戳
  private cachedNow: number = 0
  // 时间戳是否有效（每帧开始时重置）
  private nowCacheValid: boolean = false

  // 页面就绪状态
  private currentPageId: string | null = null
  private pageReadyResolve: (() => void) | null = null
  private pageReadyPromise: Promise<void> | null = null
  private isPageReady = false

  // 页面就绪回调队列
  private pageReadyCallbacks = new Set<() => void>()

  // 批量更新（使用 microtask 而非 RAF）
  private pendingUpdates = new Set<string>()
  private isMicrotaskScheduled = false

  // ==================== 并发控制 ====================
  // 活动动画槽位
  private activeSlots = new Map<string, AnimationSlot>()
  // 等待队列（按优先级排序）
  private waitingQueue: WaitingItem[] = []
  // 上次启动时间（用于最小间隔控制）
  private lastStartTime = 0

  // ==================== 爆发模式 ====================
  // 爆发模式开始时间
  private burstStartTime: number = 0
  // 是否处于爆发模式
  private inBurstMode: boolean = false
  // 爆发模式持续时间（动态调整）
  private currentBurstDuration: number = 0

  // ==================== 超时清理 ====================
  // 动画超时时间(ms)，超时后自动释放槽位
  private readonly ANIMATION_TIMEOUT = 2000
  // 超时检查定时器
  private timeoutCheckerId: ReturnType<typeof setInterval> | null = null

  // 延迟队列
  private delayedQueue: Array<{ id: string, executeAt: number, priority: AnimationPriority }> = []
  private delayTimerId: ReturnType<typeof setTimeout> | null = null

  // ==================== 分片处理配置 ====================
  // 每批最大处理数，避免 Long Task
  private readonly BATCH_SIZE = 8

  // ==================== 帧率监控与管理 ====================
  /** 帧时间预算（ms） - 60fps 基准，会根据检测到的刷新率动态调整 */
  private frameBudget: number = 16
  /**
   * FPS 采样窗口大小 - 使用环形缓冲区
   * 64 是 2 的幂，位运算取模更快
   */
  private readonly FPS_SAMPLE_SIZE = 64
  /**
   * 低帧率阈值比例 - 相对于检测到的显示器刷新率
   * 🔧 优化：使用比例而非绝对值，适配任意刷新率
   * 当前帧率 < 显示器刷新率 * 0.75 时视为低帧率
   */
  private readonly LOW_FPS_RATIO = 0.75
  /**
   * FPS 更新间隔（ms）
   * 🔧 优化：延长到 1000ms，减少计算频率
   */
  private readonly FPS_UPDATE_INTERVAL = 1000

  /**
   * 帧时间环形缓冲区（避免 push/shift 开销）
   * 🔧 优化：预分配固定大小数组
   */
  private frameTimes: Float32Array = new Float32Array(64)
  /** 环形缓冲区写入指针 */
  private frameTimeIndex: number = 0
  /** 已记录的帧数（用于判断缓冲区是否填满） */
  private frameTimeCount: number = 0
  /** 当前帧率 */
  private currentFps: number = 60
  /** 是否处于低帧率模式 */
  private isLowFpsMode: boolean = false
  /** FPS 监控 RAF ID */
  private fpsMonitorRafId: number | null = null
  /** 上次帧时间 */
  private lastFrameTimestamp: number = 0
  /** 上次 FPS 更新时间 */
  private lastFpsUpdateTime: number = 0
  /** FPS 监控是否运行中 */
  private fpsMonitorRunning: boolean = false
  /** 卡顿计数 */
  private jankCount: number = 0
  /** 总帧数 */
  private totalFrames: number = 0
  /**
   * 🔧 累计帧时间（用于快速计算平均值）
   * 避免每次遍历整个数组
   */
  private frameTimeSum: number = 0
  /**
   * 🔧 检测到的显示器刷新率
   * 通过前几帧的最小帧时间推断
   */
  private detectedRefreshRate: number = 60
  /**
   * 🔧 刷新率检测阶段的最小帧时间
   * 用于推断显示器刷新率
   */
  private minFrameTime: number = Infinity
  /**
   * 🔧 刷新率检测是否完成
   */
  private refreshRateDetected: boolean = false
  /**
   * 🔧 低帧率阈值（动态计算）
   */
  private lowFpsThreshold: number = 45

  // DOM 批量读写队列
  private domReadQueue: (() => void)[] = []
  private domWriteQueue: (() => void)[] = []
  private domBatchScheduled: boolean = false

  // ==================== ResizeObserver 管理器 ====================
  /** 共享的 ResizeObserver 实例（单一观察者，多元素） */
  private sharedResizeObserver: ResizeObserver | null = null
  /** 尺寸回调映射：element -> callback */
  private resizeCallbacks = new WeakMap<Element, (entry: ResizeObserverEntry) => void>()
  /** 被观察元素集合（用于统计和清理） */
  private observedElements = new Set<Element>()
  /** 尺寸更新批次队列 */
  private resizeBatchQueue: Array<{ element: Element, entry: ResizeObserverEntry }> = []
  /** 尺寸批次是否已调度 */
  private resizeBatchScheduled: boolean = false
  /** 尺寸更新节流时间（ms） */
  private readonly RESIZE_THROTTLE_MS = 50
  /** 上次处理尺寸更新的时间戳 */
  private lastResizeProcessTime: number = 0
  /** 尺寸变化阈值（px），小于此值的变化将被忽略 */
  private readonly RESIZE_THRESHOLD_PX = 4
  /** 元素上次尺寸缓存 */
  private elementSizeCache = new WeakMap<Element, { width: number, height: number }>()

  // ==================== IntersectionObserver 管理器 ====================
  /** 共享的 IntersectionObserver 实例池（按配置分组） */
  private intersectionObservers = new Map<string, IntersectionObserver>()
  /** 可见性回调映射：element -> { callback, observerKey } */
  private intersectionCallbacks = new WeakMap<Element, {
    callback: (entry: IntersectionObserverEntry) => void
    observerKey: string
  }>()

  /** 被观察元素集合（IntersectionObserver） */
  private intersectionObservedElements = new Set<Element>()
  /** 可见性批次队列 */
  private intersectionBatchQueue: IntersectionObserverEntry[] = []
  /** 可见性批次是否已调度 */
  private intersectionBatchScheduled: boolean = false

  // ==================== 页面可见性订阅管理 ====================
  /** 页面可见性变化回调集合 */
  private visibilitySubscribers = new Set<(isVisible: boolean) => void>()

  // ==================== 空闲任务调度器 ====================
  /** 空闲任务队列 */
  private idleTaskQueue: Array<{
    id: string
    task: () => void
    timeout?: number
    priority: number // 0=低, 1=中, 2=高
  }> = []

  /** 空闲任务回调 ID */
  private idleCallbackId: number | null = null
  /** 已注册的空闲任务 ID 集合（用于去重） */
  private registeredIdleTasks = new Set<string>()

  // ==================== 对象池（减少 GC 压力）====================
  // 🔧 等待队列 ID 索引，用于 O(1) 查找
  private waitingQueueIndex = new Map<string, number>()

  // ==================== WeakRef 元素追踪（内存优化）====================
  // 🔧 新增：使用 WeakRef 追踪元素，元素被 GC 时自动清理状态
  private elementRefs = new Map<string, WeakRefEntry>()
  // FinalizationRegistry 用于自动清理
  private finalizationRegistry: FinalizationRegistry<string> | null = null
  // WeakRef 清理检查间隔
  private weakRefCheckerId: ReturnType<typeof setInterval> | null = null

  constructor(config: Partial<CoordinatorConfig> = {}) {
    this.config = { ...DEFAULT_CONFIG, ...config }
    // 🔧 初始化 MessageChannel（比 setTimeout(0) 更高效）
    this.initMessageChannel()
    // 🔧 初始化页面可见性监听
    this.initVisibilityListener()
    // 🔧 初始化共享 ResizeObserver
    this.initSharedResizeObserver()
    // 启动超时检查器
    this.startTimeoutChecker()
    // 📌 初始化时立即进入超频模式，确保首屏动画流畅
    this.activateBurstMode(10000)
    // 🔧 初始化 FinalizationRegistry（如果浏览器支持）
    this.initFinalizationRegistry()
  }

  // ==================== MessageChannel 优化 ====================

  /**
   * 🔧 初始化 MessageChannel
   * MessageChannel 比 setTimeout(fn, 0) 更高效，因为：
   * 1. 不受最小 4ms 延迟限制
   * 2. 不会被浏览器节流
   * 3. 在微任务之后、下一个任务之前执行
   */
  private initMessageChannel() {
    if (typeof MessageChannel !== 'undefined') {
      this.messageChannel = new MessageChannel()
      this.messageChannel.port1.onmessage = () => {
        // 批量处理所有待执行的回调
        const callbacks = this.pendingYieldCallbacks
        this.pendingYieldCallbacks = []
        for (let i = 0; i < callbacks.length; i++) {
          callbacks[i]()
        }
      }
    }
  }

  /**
   * 🔧 高效的任务让出（替代 setTimeout(fn, 0)）
   */
  private scheduleYield(callback: () => void) {
    if (this.messageChannel) {
      this.pendingYieldCallbacks.push(callback)
      // 只在队列从空变非空时发送消息
      if (this.pendingYieldCallbacks.length === 1) {
        this.messageChannel.port2.postMessage(null)
      }
    }
    else {
      // 降级方案
      setTimeout(callback, 0)
    }
  }

  // ==================== 页面可见性优化 ====================

  /**
   * 🔧 初始化页面可见性监听
   * 页面不可见时暂停非关键操作，节省 CPU
   */
  private initVisibilityListener() {
    if (typeof document === 'undefined')
      return

    this.visibilityHandler = () => {
      this.isPageVisible = !document.hidden

      // 页面变为可见时，触发等待队列处理
      if (this.isPageVisible) {
        this.processWaitQueue()
      }

      // 🔧 通知所有可见性订阅者
      for (const subscriber of this.visibilitySubscribers) {
        try {
          subscriber(this.isPageVisible)
        }
        catch (e) {
          console.error('[Coordinator] Visibility subscriber error:', e)
        }
      }
    }

    document.addEventListener('visibilitychange', this.visibilityHandler)
  }

  /**
   * 🔧 订阅页面可见性变化
   * 组件可以使用此方法来响应页面可见性变化，无需各自添加事件监听器
   *
   * @param callback 可见性变化回调 (isVisible: boolean) => void
   * @returns 取消订阅函数
   *
   * @example
   * ```ts
   * // 在组件中使用
   * useEffect(() => {
   *   return coordinator.onVisibilityChange((isVisible) => {
   *     if (isVisible) startPolling();
   *     else stopPolling();
   *   });
   * }, []);
   * ```
   */
  onVisibilityChange(callback: (isVisible: boolean) => void): () => void {
    this.visibilitySubscribers.add(callback)
    return () => {
      this.visibilitySubscribers.delete(callback)
    }
  }

  /**
   * 🔧 获取当前页面可见性状态
   */
  getPageVisibility(): boolean {
    return this.isPageVisible
  }

  // ==================== 时间戳缓存优化 ====================

  /**
   * 🔧 获取缓存的时间戳
   * 同一帧内多次调用只执行一次 performance.now()
   * 大幅减少 performance.now() 的调用开销
   */
  private getNow(): number {
    if (!this.nowCacheValid) {
      this.cachedNow = performance.now()
      this.nowCacheValid = true
      // 在下一个微任务中失效缓存，确保下一帧获取新时间
      queueMicrotask(() => {
        this.nowCacheValid = false
      })
    }
    return this.cachedNow
  }

  /**
   * 🔧 强制刷新时间戳缓存
   * 用于需要精确时间的场景
   */
  private refreshNow(): number {
    this.cachedNow = performance.now()
    this.nowCacheValid = true
    return this.cachedNow
  }

  // ==================== WeakRef 元素追踪 ====================

  /**
   * 🔧 初始化 FinalizationRegistry
   * 用于在元素被 GC 时自动清理相关状态
   */
  private initFinalizationRegistry() {
    if (typeof FinalizationRegistry !== 'undefined') {
      this.finalizationRegistry = new FinalizationRegistry<string>((animationId) => {
        // 元素被 GC，清理相关状态
        this.cleanupAnimationState(animationId)
      })
    }

    // 备用方案：定期检查 WeakRef（用于不支持 FinalizationRegistry 的环境）
    this.startWeakRefChecker()
  }

  /**
   * 🔧 启动 WeakRef 定期检查器
   * 作为 FinalizationRegistry 的备用方案
   */
  private startWeakRefChecker() {
    if (this.weakRefCheckerId)
      return

    // 每 30 秒检查一次（低频率，避免性能影响）
    this.weakRefCheckerId = setInterval(() => {
      this.checkAndCleanupWeakRefs()
    }, 30000)
  }

  /**
   * 🔧 检查并清理已失效的 WeakRef
   */
  private checkAndCleanupWeakRefs() {
    if (this.elementRefs.size === 0)
      return

    const toCleanup: string[] = []

    for (const [id, entry] of this.elementRefs) {
      const element = entry.ref.deref()
      if (!element) {
        // WeakRef 已失效，元素已被 GC
        toCleanup.push(id)
      }
    }

    // 批量清理
    for (const id of toCleanup) {
      this.cleanupAnimationState(id)
    }
  }

  /**
   * 🔧 注册元素引用（可选 API）
   * 允许组件注册 DOM 元素，实现自动内存管理
   *
   * @param animationId 动画 ID
   * @param element DOM 元素
   */
  registerElement(animationId: string, element: HTMLElement) {
    // 清理旧引用
    const oldEntry = this.elementRefs.get(animationId)
    if (oldEntry && this.finalizationRegistry) {
      // 无法取消注册旧元素，但新注册会覆盖
    }

    // 创建新的 WeakRef
    const ref = new WeakRef(element)
    this.elementRefs.set(animationId, { ref, animationId })

    // 注册到 FinalizationRegistry
    if (this.finalizationRegistry) {
      this.finalizationRegistry.register(element, animationId)
    }
  }

  /**
   * 🔧 取消元素注册
   *
   * @param animationId 动画 ID
   */
  unregisterElement(animationId: string) {
    const entry = this.elementRefs.get(animationId)
    if (entry) {
      // 无法从 FinalizationRegistry 取消注册，但删除 elementRefs 条目
      this.elementRefs.delete(animationId)
    }
  }

  /**
   * 🔧 检查元素是否仍然存在
   *
   * @param animationId 动画 ID
   * @returns 元素是否存在
   */
  isElementAlive(animationId: string): boolean {
    const entry = this.elementRefs.get(animationId)
    if (!entry)
      return true // 未注册元素，假设存在

    const element = entry.ref.deref()
    return element !== undefined
  }

  /**
   * 🔧 清理动画状态（内部使用）- 优化版：使用索引 Map
   */
  private cleanupAnimationState(animationId: string) {
    // 从 elementRefs 移除
    this.elementRefs.delete(animationId)

    // 清理状态
    this.states.delete(animationId)
    this.listeners.delete(animationId)
    this.pendingUpdates.delete(animationId)

    // 释放槽位
    if (this.activeSlots.has(animationId)) {
      this.activeSlots.delete(animationId)
      this.processWaitQueue()
    }

    // 🔧 使用索引 Map O(1) 查找并移除
    const waitIdx = this.waitingQueueIndex.get(animationId)
    if (waitIdx !== undefined) {
      this.waitingQueue.splice(waitIdx, 1)
      this.waitingQueueIndex.delete(animationId)
      this.rebuildWaitingQueueIndex(waitIdx)
    }

    // 从延迟队列移除（仍用 findIndex，因为延迟队列通常较小）
    const delayIdx = this.delayedQueue.findIndex(item => item.id === animationId)
    if (delayIdx !== -1) {
      this.delayedQueue.splice(delayIdx, 1)
    }
  }

  // ==================== 超时清理 ====================

  /**
   * 启动超时检查器
   * 定期检查并清理超时的动画槽位
   * 🔧 优化：使用 requestIdleCallback 在浏览器空闲时执行，减少主线程开销
   */
  private startTimeoutChecker() {
    if (this.timeoutCheckerId)
      return

    // 🔧 优先使用 requestIdleCallback，降低调度开销
    if (typeof requestIdleCallback !== 'undefined') {
      const scheduleIdleCheck = () => {
        this.timeoutCheckerId = requestIdleCallback(
          (deadline) => {
            // 只在有空闲时间且页面可见时执行
            if (deadline.timeRemaining() > 0
              && (typeof document === 'undefined' || !document.hidden)) {
              this.cleanupTimedOutSlots()
            }
            // 继续调度下一次检查
            if (this.timeoutCheckerId) {
              scheduleIdleCheck()
            }
          },
          { timeout: 2000 }, // 最多延迟 2 秒
        ) as unknown as ReturnType<typeof setInterval>
      }
      scheduleIdleCheck()
    }
    else {
      // 降级方案：使用 setInterval
      this.timeoutCheckerId = setInterval(() => {
        if (typeof document !== 'undefined' && document.hidden) {
          return
        }
        this.cleanupTimedOutSlots()
      }, 1000)
    }
  }

  /**
   * 清理超时的槽位 - 优化版：使用缓存时间戳，减少数组创建
   */
  private cleanupTimedOutSlots() {
    // 🔧 页面不可见时跳过清理（节省 CPU）
    if (!this.isPageVisible)
      return

    // 快速返回：无活动槽位
    if (this.activeSlots.size === 0)
      return

    const now = this.refreshNow() // 使用刷新的时间戳确保准确
    let hasTimedOut = false

    for (const [id, slot] of this.activeSlots) {
      // 检查是否超时
      if (now - slot.startTime > this.ANIMATION_TIMEOUT) {
        this.activeSlots.delete(id)
        this.states.set(id, AnimationState.COMPLETED)
        hasTimedOut = true
      }
    }

    // 如果有释放，处理等待队列
    if (hasTimedOut) {
      this.processWaitQueue()
    }
  }

  // ==================== 动态并发控制 ====================

  /**
   * 获取当前最大并发数 - 优化版：使用缓存时间戳减少计算开销
   */
  private getMaxConcurrent(): number {
    // 快速路径：非爆发模式
    if (!this.inBurstMode) {
      return this.config.baseConcurrent
    }

    // 爆发模式：检查是否到期
    const elapsed = this.getNow() - this.burstStartTime
    if (elapsed < this.currentBurstDuration) {
      return this.config.burstConcurrent
    }

    // 爆发模式到期
    this.inBurstMode = false
    return this.config.baseConcurrent
  }

  /**
   * 检查并触发爆发模式（仅在需要时调用）
   */
  private checkAndTriggerBurst() {
    if (this.inBurstMode)
      return

    const totalQueued = this.waitingQueue.length + this.delayedQueue.length
    if (totalQueued > Math.ceil(this.config.baseConcurrent * 0.5)) {
      this.activateBurstMode(this.config.burstDuration)
    }
  }

  /**
   * 激活爆发模式
   * @param duration 爆发持续时间
   */
  private activateBurstMode(duration: number) {
    this.inBurstMode = true
    this.burstStartTime = this.refreshNow()
    this.currentBurstDuration = duration
  }

  // ==================== 配置 ====================

  updateConfig(config: Partial<CoordinatorConfig>) {
    this.config = { ...this.config, ...config }
  }

  // ==================== 页面级（事件驱动）====================

  /**
   * 开始页面过渡
   */
  startPageTransition(pageId: string) {
    // 清理旧页面状态
    if (this.currentPageId && this.currentPageId !== pageId) {
      this.cleanupPage(this.currentPageId)
    }

    this.currentPageId = pageId
    this.isPageReady = false

    // 激活爆发模式（页面切换时使用更长的持续时间 10s）
    this.activateBurstMode(10000)

    // 创建新的 Promise
    this.pageReadyPromise = new Promise((resolve) => {
      this.pageReadyResolve = resolve
    })
  }

  /**
   * 完成页面过渡 - 分片版：避免 Long Task
   * 🔧 优化：避免创建临时数组，直接迭代 Set
   */
  completePageTransition() {
    this.isPageReady = true

    // 解析 Promise
    this.pageReadyResolve?.()
    this.pageReadyResolve = null

    // 触发所有等待的回调（分片处理）
    const callbackCount = this.pageReadyCallbacks.size
    if (callbackCount > 0) {
      // 小批量直接处理（避免创建数组）
      if (callbackCount <= this.BATCH_SIZE) {
        // 先保存引用再清空，防止回调中添加新回调
        const callbacks = this.pageReadyCallbacks
        this.pageReadyCallbacks = new Set()
        queueMicrotask(() => {
          for (const cb of callbacks) {
            cb()
          }
        })
      }
      else {
        // 大批量：需要转数组以支持分片索引
        const callbacks = Array.from(this.pageReadyCallbacks)
        this.pageReadyCallbacks.clear()

        let index = 0
        const processBatch = () => {
          const end = Math.min(index + this.BATCH_SIZE, callbacks.length)
          for (; index < end; index++) {
            callbacks[index]()
          }
          if (index < callbacks.length) {
            // 🔧 使用 MessageChannel 替代 setTimeout
            this.scheduleYield(processBatch)
          }
        }
        queueMicrotask(processBatch)
      }
    }

    // 处理延迟队列
    this.processDelayedQueue()
  }

  /**
   * 等待页面就绪
   */
  async waitForPage(): Promise<void> {
    if (this.isPageReady) {
      return Promise.resolve()
    }
    return this.pageReadyPromise ?? Promise.resolve()
  }

  /**
   * 注册页面就绪回调
   */
  onPageReady(callback: () => void): Unsubscribe {
    if (this.isPageReady) {
      queueMicrotask(callback)
      return () => {}
    }

    this.pageReadyCallbacks.add(callback)
    return () => this.pageReadyCallbacks.delete(callback)
  }

  /**
   * 获取页面就绪状态
   */
  getPageReadyState(): boolean {
    return this.isPageReady
  }

  /**
   * 清理页面状态 - 优化版：直接清空而非迭代
   */
  private cleanupPage(_pageId: string) {
    // 直接清空所有状态，避免迭代开销
    this.states.clear()
    this.listeners.clear()
    this.pendingUpdates.clear()
    this.isMicrotaskScheduled = false

    // 清理延迟队列
    this.delayedQueue.length = 0
    if (this.delayTimerId) {
      clearTimeout(this.delayTimerId)
      this.delayTimerId = null
    }

    // 清理并发控制队列
    this.activeSlots.clear()
    this.waitingQueue.length = 0
    this.waitingQueueIndex.clear()
  }

  // ==================== 元素级（批量调度）====================

  /**
   * 调度动画
   */
  schedule(config: AnimationConfig): AnimationState {
    const { id, priority, delay = 0, index = 0, groupId } = config

    // 页面级：立即就绪（不受并发限制）
    if (priority === AnimationPriority.PAGE) {
      this.states.set(id, AnimationState.READY)
      return AnimationState.READY
    }

    // 非页面级：检查页面就绪状态
    if (!this.isPageReady) {
      this.states.set(id, AnimationState.WAITING)

      // 注册页面就绪回调
      this.onPageReady(() => {
        this.scheduleAfterPageReady(id, delay, index, groupId, priority)
      })

      return AnimationState.WAITING
    }

    // 页面已就绪，直接调度
    return this.scheduleAfterPageReady(id, delay, index, groupId, priority)
  }

  /**
   * 页面就绪后调度
   */
  private scheduleAfterPageReady(
    id: string,
    delay: number,
    index: number,
    groupId?: string,
    priority: AnimationPriority = AnimationPriority.COMPONENT,
  ): AnimationState {
    // 计算交错延迟
    const staggerDelay = groupId ? index * this.config.defaultStaggerDelay : 0
    const totalDelay = delay + staggerDelay

    if (totalDelay > 0) {
      // 加入延迟队列
      this.states.set(id, AnimationState.SCHEDULED)
      this.addToDelayedQueue(id, totalDelay, priority)
      return AnimationState.SCHEDULED
    }

    // 无延迟，尝试获取槽位
    return this.tryAcquireSlot(id, priority)
  }

  /**
   * 尝试获取并发槽位
   */
  private tryAcquireSlot(id: string, priority: AnimationPriority): AnimationState {
    const maxConcurrent = this.getMaxConcurrent()

    // 检查是否有可用槽位
    if (this.activeSlots.size < maxConcurrent) {
      // 有空闲槽位，直接获取
      this.acquireSlot(id, priority)
      this.markReady(id)
      return AnimationState.READY
    }

    // 槽位已满，检查是否可以抢占
    if (this.canPreempt(priority)) {
      // 抢占最低优先级的槽位
      this.preemptLowestPriority(id, priority)
      return AnimationState.READY
    }

    // 加入等待队列
    this.addToWaitQueue(id, priority)
    return AnimationState.SCHEDULED
  }

  /**
   * 获取槽位 - 优化版：使用缓存时间戳
   */
  private acquireSlot(id: string, priority: AnimationPriority) {
    const now = this.getNow()
    this.activeSlots.set(id, {
      id,
      priority,
      startTime: now,
      duration: 0,
    })
    this.lastStartTime = now
  }

  /**
   * 检查是否可以抢占 - 优化版：添加快速返回
   */
  private canPreempt(priority: AnimationPriority): boolean {
    // 只有高优先级可以抢占（数值越小优先级越高）
    if (priority > AnimationPriority.SECTION)
      return false

    // 快速返回：没有活动槽位
    if (this.activeSlots.size === 0)
      return false

    // 检查是否有可抢占的低优先级项
    for (const slot of this.activeSlots.values()) {
      if (slot.priority > priority) {
        return true
      }
    }
    return false
  }

  /**
   * 抢占最低优先级槽位 - 优化版：减少重复查找
   */
  private preemptLowestPriority(id: string, priority: AnimationPriority) {
    // 找到最低优先级的槽位
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
      // 跳过被抢占的动画
      this.releaseSlot(victimId)
      this.skip(victimId)

      // 获取槽位
      this.acquireSlot(id, priority)
      this.markReady(id)
    }
  }

  /**
   * 添加到等待队列 - 优化版：使用索引 Map 快速检查重复
   */
  private addToWaitQueue(id: string, priority: AnimationPriority) {
    // 🔧 使用索引 Map O(1) 检查是否已在队列中
    if (this.waitingQueueIndex.has(id)) {
      return
    }

    this.states.set(id, AnimationState.SCHEDULED)

    const item: WaitingItem = {
      id,
      priority,
      delay: 0,
      index: 0,
      registeredAt: this.getNow(),
    }

    // 🔧 快速路径：队列为空或应该插入末尾
    const queueLen = this.waitingQueue.length
    if (queueLen === 0 || this.waitingQueue[queueLen - 1].priority <= priority) {
      this.waitingQueueIndex.set(id, queueLen)
      this.waitingQueue.push(item)
      return
    }

    // 二分查找插入位置（优先级数值小的在前）
    let left = 0
    let right = queueLen
    while (left < right) {
      const mid = (left + right) >>> 1
      if (this.waitingQueue[mid].priority <= priority) {
        left = mid + 1
      }
      else {
        right = mid
      }
    }
    this.waitingQueue.splice(left, 0, item)

    // 🔧 更新索引（插入位置及之后的所有项）
    this.rebuildWaitingQueueIndex(left)
  }

  /**
   * 🔧 重建等待队列索引（从指定位置开始）
   */
  private rebuildWaitingQueueIndex(fromIndex: number = 0) {
    for (let i = fromIndex; i < this.waitingQueue.length; i++) {
      this.waitingQueueIndex.set(this.waitingQueue[i].id, i)
    }
  }

  /**
   * 释放槽位
   */
  private releaseSlot(id: string) {
    this.activeSlots.delete(id)
  }

  /**
   * 处理等待队列 - 优化版：批量处理并更新索引
   */
  private processWaitQueue() {
    // 快速返回：无等待项
    if (this.waitingQueue.length === 0)
      return

    // 队列有压力时触发爆发模式
    this.checkAndTriggerBurst()

    const maxConcurrent = this.getMaxConcurrent()
    let processed = 0

    while (
      this.waitingQueue.length > 0
      && this.activeSlots.size < maxConcurrent
      && processed < this.BATCH_SIZE
    ) {
      const next = this.waitingQueue.shift()
      if (next) {
        // 🔧 从索引中移除
        this.waitingQueueIndex.delete(next.id)
        this.acquireSlot(next.id, next.priority)
        this.markReady(next.id)
        processed++
      }
    }

    // 🔧 如果有移除，重建索引
    if (processed > 0 && this.waitingQueue.length > 0) {
      this.rebuildWaitingQueueIndex(0)
    }

    // 如果还有剩余且有槽位，延迟继续处理
    if (this.waitingQueue.length > 0 && this.activeSlots.size < maxConcurrent) {
      // 🔧 使用 MessageChannel 替代 setTimeout
      this.scheduleYield(() => this.processWaitQueue())
    }
  }

  /**
   * 添加到延迟队列 - 优化版：二分插入避免排序，使用缓存时间戳
   */
  private addToDelayedQueue(id: string, delay: number, priority: AnimationPriority = AnimationPriority.COMPONENT) {
    const executeAt = this.getNow() + delay
    const item = { id, executeAt, priority }

    // 🔧 快速路径：队列为空或应该插入末尾
    const queueLen = this.delayedQueue.length
    if (queueLen === 0 || this.delayedQueue[queueLen - 1].executeAt <= executeAt) {
      this.delayedQueue.push(item)
      this.scheduleNextDelay()
      return
    }

    // 二分查找插入位置（按执行时间升序）
    let left = 0
    let right = queueLen
    while (left < right) {
      const mid = (left + right) >>> 1
      if (this.delayedQueue[mid].executeAt <= executeAt) {
        left = mid + 1
      }
      else {
        right = mid
      }
    }
    this.delayedQueue.splice(left, 0, item)

    // 调度下一个
    this.scheduleNextDelay()
  }

  /**
   * 调度下一个延迟项
   */
  private scheduleNextDelay() {
    if (this.delayTimerId || this.delayedQueue.length === 0)
      return

    const next = this.delayedQueue[0]
    const wait = Math.max(0, next.executeAt - this.getNow())

    this.delayTimerId = setTimeout(() => {
      this.delayTimerId = null
      this.processDelayedQueue()
    }, wait)
  }

  /**
   * 处理延迟队列 - 优化版：使用刷新的时间戳确保精确
   */
  private processDelayedQueue() {
    const now = this.refreshNow()

    // 处理所有到期项
    while (this.delayedQueue.length > 0) {
      const next = this.delayedQueue[0]
      if (next.executeAt > now)
        break

      this.delayedQueue.shift()
      // 到期后尝试获取槽位
      this.tryAcquireSlot(next.id, next.priority)
    }

    // 调度下一个
    this.scheduleNextDelay()
  }

  /**
   * 标记为就绪 - 优化版：使用 microtask 批处理
   */
  private markReady(id: string) {
    this.states.set(id, AnimationState.READY)
    this.pendingUpdates.add(id)
    this.scheduleMicrotaskFlush()
  }

  /**
   * 标记动画开始（外部调用，表示动画真正开始播放）
   * 注意：槽位应该已经在 schedule 时获取
   */
  markRunning(id: string) {
    this.states.set(id, AnimationState.RUNNING)
  }

  /**
   * 标记动画完成
   */
  markCompleted(id: string) {
    this.states.set(id, AnimationState.COMPLETED)

    // 释放槽位
    if (this.activeSlots.has(id)) {
      this.releaseSlot(id)

      // 处理等待队列中的下一个
      this.processWaitQueue()
    }
  }

  /**
   * 跳过动画
   */
  skip(id: string) {
    this.states.set(id, AnimationState.SKIPPED)

    // 释放槽位（如果有）
    if (this.activeSlots.has(id)) {
      this.releaseSlot(id)
      this.processWaitQueue()
    }

    this.pendingUpdates.add(id)
    this.scheduleMicrotaskFlush()
  }

  // ==================== 帧率监控 ====================

  // ==================== 批量更新（microtask 替代 RAF）====================

  /**
   * 调度 microtask 刷新
   * microtask 在当前任务结束后立即执行，不会计入 RAF 计数
   */
  private scheduleMicrotaskFlush() {
    if (this.isMicrotaskScheduled)
      return
    this.isMicrotaskScheduled = true

    queueMicrotask(() => {
      this.isMicrotaskScheduled = false
      this.flushPendingUpdates()
    })
  }

  /**
   * 刷新待更新项 - 分片版：避免 Long Task
   */
  private flushPendingUpdates() {
    if (this.pendingUpdates.size === 0)
      return

    // 小批量直接处理
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

    // 大批量分片处理
    const ids = Array.from(this.pendingUpdates)
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
        // 🔧 使用 MessageChannel 替代 setTimeout
        this.scheduleYield(processBatch)
      }
    }

    processBatch()
  }

  // ==================== 订阅 ====================

  /**
   * 订阅状态变化 - 优化版：减少 Map 查找
   */
  subscribe(id: string, callback: AnimationListener): Unsubscribe {
    let listenerSet = this.listeners.get(id)
    if (!listenerSet) {
      listenerSet = new Set()
      this.listeners.set(id, listenerSet)
    }
    listenerSet.add(callback)

    // 如果已有状态，立即通知（microtask）
    const state = this.states.get(id)
    if (state) {
      queueMicrotask(() => callback(state))
    }

    return () => {
      const listeners = this.listeners.get(id)
      if (listeners) {
        listeners.delete(callback)
        if (listeners.size === 0) {
          this.listeners.delete(id)
          // 🔧 修复：同时清理废弃的状态，避免内存泄漏
          this.states.delete(id)
          this.pendingUpdates.delete(id)
        }
      }
    }
  }

  /**
   * 通知监听器 - 优化版：直接迭代避免闭包
   */
  private notify(id: string, state: AnimationState) {
    const listeners = this.listeners.get(id)
    if (listeners) {
      for (const cb of listeners) {
        cb(state)
      }
    }
  }

  /**
   * 获取状态
   */
  getState(id: string): AnimationState | undefined {
    return this.states.get(id)
  }

  // ==================== 工具方法 ====================

  /**
   * 计算交错延迟
   */
  getStaggerDelay(index: number, baseDelay?: number): number {
    return index * (baseDelay ?? this.config.defaultStaggerDelay)
  }

  /**
   * 获取活动动画数（占用槽位数）
   */
  getActiveCount(): number {
    return this.activeSlots.size
  }

  /**
   * 获取负载（0-1）- 优化版：避免除零
   */
  getLoad(): number {
    const max = this.inBurstMode ? this.config.burstConcurrent : this.config.baseConcurrent
    return this.activeSlots.size / max
  }

  /**
   * 获取等待队列长度
   */
  getWaitingCount(): number {
    return this.waitingQueue.length
  }

  /**
   * 🔧 新增：获取内存使用状态（用于监控内存泄漏）
   */
  getMemoryStatus() {
    return {
      statesSize: this.states.size,
      listenersSize: this.listeners.size,
      pendingUpdatesSize: this.pendingUpdates.size,
      pageReadyCallbacksSize: this.pageReadyCallbacks.size,
      activeSlotsSize: this.activeSlots.size,
      waitingQueueLength: this.waitingQueue.length,
      waitingQueueIndexSize: this.waitingQueueIndex.size,
      delayedQueueLength: this.delayedQueue.length,
      // 🔧 新增：WeakRef 追踪的元素数量
      trackedElementsCount: this.elementRefs.size,
      // 🔧 新增：待执行的 yield 回调数量
      pendingYieldCallbacksCount: this.pendingYieldCallbacks.length,
      // 🔧 新增：页面可见性状态
      isPageVisible: this.isPageVisible,
      // 总计：超过 500 可能有泄漏
      totalEntries: this.states.size + this.listeners.size + this.activeSlots.size
        + this.waitingQueue.length + this.delayedQueue.length
        + this.elementRefs.size,
    }
  }

  /**
   * 获取并发状态（用于调试）
   */
  getConcurrencyStatus() {
    const maxConcurrent = this.getMaxConcurrent()
    const totalQueued = this.waitingQueue.length + this.delayedQueue.length
    const pressureThreshold = Math.ceil(this.config.baseConcurrent * 0.5)

    return {
      activeSlots: this.activeSlots.size,
      maxConcurrent,
      inBurstMode: this.inBurstMode,
      burstTimeRemaining: this.inBurstMode
        ? Math.max(0, this.currentBurstDuration - (this.getNow() - this.burstStartTime))
        : 0,
      waitingQueue: this.waitingQueue.length,
      delayedQueue: this.delayedQueue.length,
      totalQueued,
      queuePressure: totalQueued > pressureThreshold,
      load: this.activeSlots.size / maxConcurrent,
    }
  }

  /**
   * 重置协调器
   */
  reset() {
    this.states.clear()
    this.listeners.clear()
    this.pendingUpdates.clear()
    this.pageReadyCallbacks.clear()
    this.delayedQueue.length = 0
    this.isMicrotaskScheduled = false

    // 清理并发控制
    this.activeSlots.clear()
    this.waitingQueue.length = 0
    this.waitingQueueIndex.clear()

    // 重置爆发模式
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

  /**
   * 手动激活爆发模式（用于首次加载）
   */
  triggerBurst() {
    this.activateBurstMode(10000) // 首次加载也使用10s
  }

  // ==================== 帧率监控 API ====================

  /**
   * 启动 FPS 监控
   * 用于检测帧率下降并自动降级
   *
   * 🔧 优化策略：
   * 1. 自动检测显示器刷新率（通过最小帧时间推断）
   * 2. 使用环形缓冲区避免数组操作开销
   * 3. 使用累加器快速计算平均值（O(1) vs O(n)）
   * 4. 延长更新间隔减少计算频率
   * 5. 使用加权移动平均提高稳定性
   * 6. 低帧率阈值相对于检测到的刷新率动态计算
   */
  startFpsMonitor(): void {
    if (this.fpsMonitorRunning)
      return
    this.fpsMonitorRunning = true
    this.lastFrameTimestamp = performance.now()
    this.lastFpsUpdateTime = this.lastFrameTimestamp

    // 重置环形缓冲区
    this.frameTimeIndex = 0
    this.frameTimeCount = 0
    this.frameTimeSum = 0
    this.frameTimes.fill(0)

    // 重置刷新率检测状态
    this.minFrameTime = Infinity
    this.refreshRateDetected = false

    const measureFps = (timestamp: number) => {
      if (!this.fpsMonitorRunning)
        return

      const frameTime = timestamp - this.lastFrameTimestamp
      this.lastFrameTimestamp = timestamp
      this.totalFrames++

      // 🔧 刷新率检测阶段（前 30 帧）
      // 通过最小帧时间推断显示器刷新率
      if (!this.refreshRateDetected && this.totalFrames <= 30) {
        // 过滤掉异常短的帧时间（< 4ms，可能是测量误差）
        if (frameTime > 4 && frameTime < this.minFrameTime) {
          this.minFrameTime = frameTime
        }

        // 30 帧后确定刷新率
        if (this.totalFrames === 30 && this.minFrameTime < Infinity) {
          this.refreshRateDetected = true
          // 根据最小帧时间推断刷新率
          // 添加小余量避免边界问题
          const inferredRate = Math.round(1000 / this.minFrameTime)
          // 对齐到常见刷新率: 60, 72, 75, 90, 120, 144, 165, 240, 360
          this.detectedRefreshRate = this.snapToCommonRefreshRate(inferredRate)
          // 动态计算帧时间预算和低帧率阈值
          this.frameBudget = 1000 / this.detectedRefreshRate
          this.lowFpsThreshold = Math.round(this.detectedRefreshRate * this.LOW_FPS_RATIO)
          // 初始 FPS 设为检测到的刷新率
          this.currentFps = this.detectedRefreshRate
        }
      }

      // 🔧 环形缓冲区写入（避免 push/shift）
      const idx = this.frameTimeIndex
      const oldValue = this.frameTimes[idx]
      this.frameTimes[idx] = frameTime
      // 位运算取模（64 = 2^6，所以 & 63 等价于 % 64）
      this.frameTimeIndex = (idx + 1) & 63

      // 🔧 增量更新累加器（O(1) 复杂度）
      if (this.frameTimeCount < this.FPS_SAMPLE_SIZE) {
        this.frameTimeCount++
        this.frameTimeSum += frameTime
      }
      else {
        // 缓冲区已满，减去被覆盖的旧值，加上新值
        this.frameTimeSum = this.frameTimeSum - oldValue + frameTime
      }

      // 检测卡顿（帧时间 > 2倍帧预算算卡顿）
      if (frameTime > this.frameBudget * 2) {
        this.jankCount++
      }

      // 每 1000ms 更新一次 FPS 显示值
      const timeSinceUpdate = timestamp - this.lastFpsUpdateTime
      if (timeSinceUpdate >= this.FPS_UPDATE_INTERVAL && this.frameTimeCount >= 10) {
        this.lastFpsUpdateTime = timestamp

        // 🔧 使用累加器直接计算平均帧时间（O(1)）
        const avgFrameTime = this.frameTimeSum / this.frameTimeCount

        // 🔧 平滑处理：与上一次 FPS 做加权平均，避免抖动
        const rawFps = 1000 / avgFrameTime
        // 80% 新值 + 20% 旧值，提高稳定性
        this.currentFps = Math.round(rawFps * 0.8 + this.currentFps * 0.2)

        // 🔧 使用动态计算的阈值
        this.isLowFpsMode = this.currentFps < this.lowFpsThreshold
      }

      this.fpsMonitorRafId = requestAnimationFrame(measureFps)
    }

    this.fpsMonitorRafId = requestAnimationFrame(measureFps)
  }

  /**
   * 🔧 将推断的刷新率对齐到常见值
   * 避免因测量误差导致的奇怪数值
   */
  private snapToCommonRefreshRate(inferredRate: number): number {
    // 常见刷新率列表
    const commonRates = [60, 72, 75, 90, 120, 144, 165, 240, 360]

    // 找到最接近的常见刷新率
    let closest = commonRates[0]
    let minDiff = Math.abs(inferredRate - closest)

    for (const rate of commonRates) {
      const diff = Math.abs(inferredRate - rate)
      if (diff < minDiff) {
        minDiff = diff
        closest = rate
      }
    }

    // 如果差距太大（>10%），使用原始推断值
    if (minDiff > inferredRate * 0.1) {
      return inferredRate
    }

    return closest
  }

  /**
   * 停止 FPS 监控
   */
  stopFpsMonitor(): void {
    this.fpsMonitorRunning = false
    if (this.fpsMonitorRafId !== null) {
      cancelAnimationFrame(this.fpsMonitorRafId)
      this.fpsMonitorRafId = null
    }
  }

  /**
   * 获取当前 FPS
   */
  getFps(): number {
    return this.currentFps
  }

  /**
   * 是否处于低帧率模式
   */
  isLowFps(): boolean {
    return this.isLowFpsMode
  }

  /**
   * 获取帧率统计信息
   * 用于性能监控面板，统一从调度器获取数据
   */
  getFrameStats(): {
    fps: number
    avgFrameTime: number
    isLowFps: boolean
    jankCount: number
    totalFrames: number
    isMonitoring: boolean
    sampleCount: number
    /** 🔧 检测到的显示器刷新率 */
    detectedRefreshRate: number
    /** 🔧 动态计算的低帧率阈值 */
    lowFpsThreshold: number
    /** 🔧 刷新率检测是否完成 */
    refreshRateDetected: boolean
  } {
    // 🔧 使用累加器直接计算，O(1) 复杂度
    const avgFrameTime = this.frameTimeCount > 0
      ? this.frameTimeSum / this.frameTimeCount
      : 16

    return {
      fps: this.currentFps,
      avgFrameTime,
      isLowFps: this.isLowFpsMode,
      jankCount: this.jankCount,
      totalFrames: this.totalFrames,
      isMonitoring: this.fpsMonitorRunning,
      sampleCount: this.frameTimeCount,
      detectedRefreshRate: this.detectedRefreshRate,
      lowFpsThreshold: this.lowFpsThreshold,
      refreshRateDetected: this.refreshRateDetected,
    }
  }

  /**
   * 获取检测到的显示器刷新率
   */
  getDetectedRefreshRate(): number {
    return this.detectedRefreshRate
  }

  /**
   * 重置帧率统计（可选）
   */
  resetFrameStats(): void {
    this.frameTimes.fill(0)
    this.frameTimeIndex = 0
    this.frameTimeCount = 0
    this.frameTimeSum = 0
    this.jankCount = 0
    this.totalFrames = 0
    this.currentFps = this.detectedRefreshRate // 重置为检测到的刷新率
    this.isLowFpsMode = false
    // 注意：不重置 detectedRefreshRate，保留之前的检测结果
  }

  // ==================== DOM 批量操作 ====================

  /**
   * 批量 DOM 读取
   * 将读取操作收集到队列，在下一帧统一执行，避免强制重排
   */
  batchRead(callback: () => void): void {
    this.domReadQueue.push(callback)
    this.scheduleDomBatch()
  }

  /**
   * 批量 DOM 写入
   * 将写入操作收集到队列，在读取之后统一执行
   */
  batchWrite(callback: () => void): void {
    this.domWriteQueue.push(callback)
    this.scheduleDomBatch()
  }

  /**
   * 调度 DOM 批量处理
   */
  private scheduleDomBatch(): void {
    if (this.domBatchScheduled)
      return
    this.domBatchScheduled = true

    requestAnimationFrame(() => {
      this.domBatchScheduled = false
      this.flushDomBatch()
    })
  }

  /**
   * 执行 DOM 批量操作
   * 先读后写，避免布局抖动
   */
  private flushDomBatch(): void {
    // 先执行所有读取
    const reads = this.domReadQueue
    this.domReadQueue = []
    for (const read of reads) {
      try { read() }
      catch (e) { console.error('DOM read error:', e) }
    }

    // 再执行所有写入
    const writes = this.domWriteQueue
    this.domWriteQueue = []
    for (const write of writes) {
      try { write() }
      catch (e) { console.error('DOM write error:', e) }
    }
  }

  /**
   * 让出主线程
   * 用于长任务中断
   */
  yieldToMain(): Promise<void> {
    return new Promise((resolve) => {
      this.scheduleYield(resolve)
    })
  }

  /**
   * 检查是否应该让出主线程
   */
  shouldYield(): boolean {
    return this.isLowFpsMode || this.waitingQueue.length > 10
  }

  // ==================== ResizeObserver 管理 ====================

  /**
   * 初始化共享 ResizeObserver
   * 使用单一 Observer 观察所有元素，比每个元素一个 Observer 更高效
   */
  private initSharedResizeObserver(): void {
    if (typeof ResizeObserver === 'undefined')
      return

    this.sharedResizeObserver = new ResizeObserver((entries) => {
      // 页面不可见时跳过处理
      if (!this.isPageVisible)
        return

      // 批量收集变化
      for (const entry of entries) {
        const callback = this.resizeCallbacks.get(entry.target)
        if (callback) {
          // 检查尺寸变化是否超过阈值
          const { width, height } = entry.contentRect
          const cached = this.elementSizeCache.get(entry.target)

          if (cached) {
            const widthDiff = Math.abs(cached.width - width)
            const heightDiff = Math.abs(cached.height - height)

            // 小于阈值的变化忽略
            if (widthDiff < this.RESIZE_THRESHOLD_PX && heightDiff < this.RESIZE_THRESHOLD_PX) {
              continue
            }
          }

          // 更新缓存
          this.elementSizeCache.set(entry.target, { width, height })

          // 加入批次队列
          this.resizeBatchQueue.push({ element: entry.target, entry })
        }
      }

      // 调度批量处理
      this.scheduleResizeBatch()
    })
  }

  /**
   * 调度 resize 批量处理
   * 使用节流避免过于频繁的更新
   */
  private scheduleResizeBatch(): void {
    if (this.resizeBatchScheduled || this.resizeBatchQueue.length === 0)
      return

    const now = this.getNow()
    const timeSinceLastProcess = now - this.lastResizeProcessTime

    if (timeSinceLastProcess >= this.RESIZE_THROTTLE_MS) {
      // 足够时间了，立即处理
      this.resizeBatchScheduled = true
      requestAnimationFrame(() => {
        this.flushResizeBatch()
      })
    }
    else {
      // 延迟到节流时间后处理
      this.resizeBatchScheduled = true
      setTimeout(() => {
        requestAnimationFrame(() => {
          this.flushResizeBatch()
        })
      }, this.RESIZE_THROTTLE_MS - timeSinceLastProcess)
    }
  }

  /**
   * 执行 resize 批量回调
   */
  private flushResizeBatch(): void {
    this.resizeBatchScheduled = false
    this.lastResizeProcessTime = this.getNow()

    // 取出所有待处理项
    const batch = this.resizeBatchQueue
    this.resizeBatchQueue = []

    // 批量执行回调
    for (const { element, entry } of batch) {
      const callback = this.resizeCallbacks.get(element)
      if (callback) {
        try {
          callback(entry)
        }
        catch (e) {
          console.error('ResizeObserver callback error:', e)
        }
      }
    }
  }

  /**
   * 观察元素尺寸变化
   * @param element 要观察的元素
   * @param callback 尺寸变化回调（接收 ResizeObserverEntry）
   * @param options 观察选项
   * @returns 取消观察函数
   */
  observeResize(
    element: Element,
    callback: (entry: ResizeObserverEntry) => void,
    options?: { immediate?: boolean },
  ): () => void {
    if (!this.sharedResizeObserver || !element) {
      return () => {}
    }

    // 注册回调
    this.resizeCallbacks.set(element, callback)
    this.observedElements.add(element)

    // 开始观察
    this.sharedResizeObserver.observe(element, { box: 'border-box' })

    // 立即执行一次测量（可选）
    if (options?.immediate) {
      requestAnimationFrame(() => {
        if (!element.isConnected)
          return
        const rect = element.getBoundingClientRect()
        // 更新缓存
        this.elementSizeCache.set(element, { width: rect.width, height: rect.height })
        // 创建模拟的 entry
        const fakeEntry = {
          target: element,
          contentRect: rect,
          borderBoxSize: [{ inlineSize: rect.width, blockSize: rect.height }],
          contentBoxSize: [{ inlineSize: rect.width, blockSize: rect.height }],
          devicePixelContentBoxSize: [{ inlineSize: rect.width, blockSize: rect.height }],
        } as ResizeObserverEntry
        callback(fakeEntry)
      })
    }

    // 返回取消函数
    return () => {
      this.unobserveResize(element)
    }
  }

  /**
   * 取消观察元素
   */
  unobserveResize(element: Element): void {
    if (!this.sharedResizeObserver)
      return

    this.sharedResizeObserver.unobserve(element)
    this.resizeCallbacks.delete(element)
    this.observedElements.delete(element)
    this.elementSizeCache.delete(element)

    // 从批次队列中移除该元素的待处理项
    this.resizeBatchQueue = this.resizeBatchQueue.filter(item => item.element !== element)
  }

  /**
   * 获取观察中的元素数量
   */
  getObservedElementCount(): number {
    return this.observedElements.size
  }

  /**
   * 获取元素的缓存尺寸（无需触发重排）
   */
  getCachedSize(element: Element): { width: number, height: number } | null {
    return this.elementSizeCache.get(element) || null
  }

  // ==================== IntersectionObserver 管理 ====================

  /**
   * 获取或创建 IntersectionObserver
   * 相同配置的元素共享同一个 Observer
   */
  private getIntersectionObserver(threshold: number, rootMargin: string): IntersectionObserver {
    const key = `${threshold}:${rootMargin}`

    let observer = this.intersectionObservers.get(key)
    if (!observer) {
      observer = new IntersectionObserver(
        (entries) => {
          // 页面不可见时跳过
          if (!this.isPageVisible)
            return

          // 批量收集
          for (const entry of entries) {
            this.intersectionBatchQueue.push(entry)
          }

          // 调度批量处理
          this.scheduleIntersectionBatch()
        },
        { threshold, rootMargin },
      )
      this.intersectionObservers.set(key, observer)
    }

    return observer
  }

  /**
   * 调度 IntersectionObserver 批量处理
   */
  private scheduleIntersectionBatch(): void {
    if (this.intersectionBatchScheduled || this.intersectionBatchQueue.length === 0)
      return

    this.intersectionBatchScheduled = true

    // 使用 microtask 批量处理，比 RAF 更快响应
    queueMicrotask(() => {
      this.flushIntersectionBatch()
    })
  }

  /**
   * 执行 IntersectionObserver 批量回调
   */
  private flushIntersectionBatch(): void {
    this.intersectionBatchScheduled = false

    const batch = this.intersectionBatchQueue
    this.intersectionBatchQueue = []

    for (const entry of batch) {
      const info = this.intersectionCallbacks.get(entry.target)
      if (info) {
        try {
          info.callback(entry)
        }
        catch (e) {
          console.error('IntersectionObserver callback error:', e)
        }
      }
    }
  }

  /**
   * 观察元素可见性变化
   * @param element 要观察的元素
   * @param callback 可见性变化回调
   * @param options 观察选项
   * @returns 取消观察函数
   */
  observeIntersection(
    element: Element,
    callback: (entry: IntersectionObserverEntry) => void,
    options?: { threshold?: number, rootMargin?: string },
  ): () => void {
    if (!element)
      return () => {}

    const threshold = options?.threshold ?? 0
    const rootMargin = options?.rootMargin ?? '0px'
    const observerKey = `${threshold}:${rootMargin}`

    const observer = this.getIntersectionObserver(threshold, rootMargin)

    // 注册回调
    this.intersectionCallbacks.set(element, { callback, observerKey })
    this.intersectionObservedElements.add(element)

    // 开始观察
    observer.observe(element)

    // 返回取消函数
    return () => {
      this.unobserveIntersection(element)
    }
  }

  /**
   * 取消观察元素可见性
   */
  unobserveIntersection(element: Element): void {
    const info = this.intersectionCallbacks.get(element)
    if (!info)
      return

    const observer = this.intersectionObservers.get(info.observerKey)
    if (observer) {
      observer.unobserve(element)
    }

    this.intersectionCallbacks.delete(element)
    this.intersectionObservedElements.delete(element)

    // 从批次队列中移除
    this.intersectionBatchQueue = this.intersectionBatchQueue.filter(
      entry => entry.target !== element,
    )
  }

  /**
   * 获取观察中的元素数量（IntersectionObserver）
   */
  getIntersectionObservedCount(): number {
    return this.intersectionObservedElements.size
  }

  /**
   * 获取 IntersectionObserver 实例数量
   */
  getIntersectionObserverCount(): number {
    return this.intersectionObservers.size
  }

  // ==================== 空闲任务调度器 ====================

  /**
   * 🔧 调度空闲任务
   * 在主线程空闲时执行非关键任务，避免阻塞用户交互
   *
   * @param id 任务唯一标识（用于去重和取消）
   * @param task 要执行的任务
   * @param options 配置选项
   * @returns 取消任务的函数
   *
   * @example
   * ```ts
   * // 调度一个空闲任务
   * const cancel = coordinator.scheduleIdleTask('prefetch-data', () => {
   *   prefetchNextPageData();
   * }, { timeout: 2000, priority: 'low' });
   *
   * // 取消任务
   * cancel();
   * ```
   */
  scheduleIdleTask(
    id: string,
    task: () => void,
    options: {
      timeout?: number
      priority?: 'low' | 'normal' | 'high'
      dedupe?: boolean // 是否去重，默认 true
    } = {},
  ): () => void {
    const { timeout, priority = 'normal', dedupe = true } = options

    // 去重检查
    if (dedupe && this.registeredIdleTasks.has(id)) {
      return () => this.cancelIdleTask(id)
    }

    const priorityValue = priority === 'high' ? 2 : priority === 'normal' ? 1 : 0

    this.idleTaskQueue.push({ id, task, timeout, priority: priorityValue })
    this.registeredIdleTasks.add(id)

    // 按优先级排序（高优先级在前）
    this.idleTaskQueue.sort((a, b) => b.priority - a.priority)

    // 调度空闲回调
    this.scheduleIdleCallback()

    return () => this.cancelIdleTask(id)
  }

  /**
   * 🔧 取消空闲任务
   */
  cancelIdleTask(id: string): boolean {
    const index = this.idleTaskQueue.findIndex(t => t.id === id)
    if (index !== -1) {
      this.idleTaskQueue.splice(index, 1)
      this.registeredIdleTasks.delete(id)
      return true
    }
    return false
  }

  /**
   * 🔧 调度 requestIdleCallback
   */
  private scheduleIdleCallback() {
    if (this.idleCallbackId !== null || this.idleTaskQueue.length === 0)
      return

    // 页面不可见时暂停
    if (!this.isPageVisible)
      return

    const scheduleIdle = typeof requestIdleCallback !== 'undefined'
      ? requestIdleCallback
      : (cb: IdleRequestCallback) => setTimeout(() => cb({
          didTimeout: false,
          timeRemaining: () => 50,
        }), 1)

    // 获取最高优先级任务的 timeout
    const highestPriorityTask = this.idleTaskQueue[0]

    this.idleCallbackId = scheduleIdle(
      (deadline: IdleDeadline) => {
        this.idleCallbackId = null
        this.processIdleTasks(deadline)
      },
      highestPriorityTask?.timeout ? { timeout: highestPriorityTask.timeout } : undefined,
    ) as number
  }

  /**
   * 🔧 处理空闲任务
   */
  private processIdleTasks(deadline: IdleDeadline) {
    // 在时间允许内尽可能多地处理任务
    while (
      this.idleTaskQueue.length > 0
      && (deadline.timeRemaining() > 5 || deadline.didTimeout)
    ) {
      const taskInfo = this.idleTaskQueue.shift()
      if (taskInfo) {
        this.registeredIdleTasks.delete(taskInfo.id)
        try {
          taskInfo.task()
        }
        catch (e) {
          console.error(`[Coordinator] Idle task "${taskInfo.id}" error:`, e)
        }
      }
    }

    // 如果还有任务，继续调度
    if (this.idleTaskQueue.length > 0) {
      this.scheduleIdleCallback()
    }
  }

  /**
   * 🔧 获取待处理的空闲任务数量
   */
  getIdleTaskCount(): number {
    return this.idleTaskQueue.length
  }

  /**
   * 销毁协调器（清理定时器）
   */
  destroy() {
    this.reset()
    // 停止 FPS 监控
    this.stopFpsMonitor()
    if (this.timeoutCheckerId) {
      clearInterval(this.timeoutCheckerId)
      this.timeoutCheckerId = null
    }
    // 🔧 清理 WeakRef 检查器
    if (this.weakRefCheckerId) {
      clearInterval(this.weakRefCheckerId)
      this.weakRefCheckerId = null
    }
    // 🔧 清理 MessageChannel
    if (this.messageChannel) {
      this.messageChannel.port1.close()
      this.messageChannel.port2.close()
      this.messageChannel = null
    }
    this.pendingYieldCallbacks.length = 0
    // 🔧 移除 visibility 监听器
    if (this.visibilityHandler && typeof document !== 'undefined') {
      document.removeEventListener('visibilitychange', this.visibilityHandler)
      this.visibilityHandler = null
    }
    // 🔧 清理可见性订阅者
    this.visibilitySubscribers.clear()
    // 清理 elementRefs
    this.elementRefs.clear()
    // 清理 DOM 批量队列
    this.domReadQueue.length = 0
    this.domWriteQueue.length = 0
    // 🔧 清理 ResizeObserver
    if (this.sharedResizeObserver) {
      this.sharedResizeObserver.disconnect()
      this.sharedResizeObserver = null
    }
    this.observedElements.clear()
    this.resizeBatchQueue.length = 0
    // 🔧 清理 IntersectionObserver
    for (const observer of this.intersectionObservers.values()) {
      observer.disconnect()
    }
    this.intersectionObservers.clear()
    this.intersectionObservedElements.clear()
    this.intersectionBatchQueue.length = 0
    // 🔧 清理空闲任务
    if (this.idleCallbackId !== null) {
      if (typeof cancelIdleCallback !== 'undefined') {
        cancelIdleCallback(this.idleCallbackId)
      }
      else {
        clearTimeout(this.idleCallbackId)
      }
      this.idleCallbackId = null
    }
    this.idleTaskQueue.length = 0
    this.registeredIdleTasks.clear()
  }
}

// 单例导出
export const coordinator = new AnimationCoordinator()

// 注意：构造函数中已自动激活超频模式，无需手动调用 triggerBurst

export default AnimationCoordinator
