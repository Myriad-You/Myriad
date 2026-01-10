/**
 * 统一资源加载管理系统
 * 用于管理各种资源的加载优先级和时机，避免阻塞关键内容
 */

export enum LoadPriority {
  CRITICAL = 0, // 关键资源：必须立即加载（用户认证、页面基础数据）
  HIGH = 1, // 高优先级：页面主要内容（天气、名言、壁纸）
  MEDIUM = 2, // 中优先级：次要内容（音乐歌单）
  LOW = 3, // 低优先级：预加载和缓存（下一首歌曲、图片预加载）
  IDLE = 4, // 空闲时加载：完全非关键资源
}

interface LoadTask {
  id: string
  priority: LoadPriority
  loader: () => Promise<void>
  timeout?: number
  retryCount?: number
}

interface LoaderConfig {
  maxConcurrent: number // 最大并发加载数
  idleDelay: number // 空闲加载延迟（ms）
  lowPriorityDelay: number // 低优先级延迟（ms）
  mediumPriorityDelay: number // 中优先级延迟（ms）
}

class ResourceLoader {
  private queue: LoadTask[] = []
  private activeLoads: Set<string> = new Set()
  private completedLoads: Set<string> = new Set()
  private failedLoads: Map<string, number> = new Map()
  private config: LoaderConfig
  private isPageLoaded = false
  private idleCallbackId: number | null = null

  constructor(config?: Partial<LoaderConfig>) {
    this.config = {
      maxConcurrent: 3,
      idleDelay: 2000,
      lowPriorityDelay: 1000,
      mediumPriorityDelay: 500,
      ...config,
    }

    // 监听页面加载完成
    if (document.readyState === 'complete') {
      this.isPageLoaded = true
    }
    else {
      window.addEventListener('load', () => {
        this.isPageLoaded = true
        this.processQueue()
      })
    }
  }

  /**
   * 添加加载任务
   */
  addTask(task: LoadTask): void {
    // 避免重复添加
    if (this.completedLoads.has(task.id) || this.activeLoads.has(task.id)) {
      return
    }

    // 检查是否在队列中
    const existingIndex = this.queue.findIndex(t => t.id === task.id)
    if (existingIndex !== -1) {
      // 如果新任务优先级更高，更新优先级
      if (task.priority < this.queue[existingIndex].priority) {
        this.queue[existingIndex] = task
        this.sortQueue()
      }
      return
    }

    this.queue.push(task)
    this.sortQueue()
    this.processQueue()
  }

  /**
   * 批量添加任务
   */
  addTasks(tasks: LoadTask[]): void {
    tasks.forEach(task => this.addTask(task))
  }

  /**
   * 取消任务
   */
  cancelTask(id: string): void {
    const index = this.queue.findIndex(t => t.id === id)
    if (index !== -1) {
      this.queue.splice(index, 1)
    }
  }

  /**
   * 清空指定优先级的任务
   */
  clearPriority(priority: LoadPriority): void {
    this.queue = this.queue.filter(t => t.priority !== priority)
  }

  /**
   * 按优先级排序队列
   */
  private sortQueue(): void {
    this.queue.sort((a, b) => a.priority - b.priority)
  }

  /**
   * 处理队列
   */
  private async processQueue(): Promise<void> {
    // 如果没有任务，返回
    if (this.queue.length === 0) {
      return
    }

    // 如果达到最大并发数，等待
    if (this.activeLoads.size >= this.config.maxConcurrent) {
      return
    }

    // 获取下一个任务
    const task = this.queue.shift()
    if (!task)
      return

    // 根据优先级决定是否延迟
    const delay = this.getDelayForPriority(task.priority)
    if (delay > 0) {
      // 重新加入队列，等待延迟
      await new Promise(resolve => setTimeout(resolve, delay))

      // 延迟后检查任务是否已被取消
      if (this.completedLoads.has(task.id) || this.activeLoads.has(task.id)) {
        this.processQueue()
        return
      }
    }

    // 执行任务
    this.executeTask(task)

    // 继续处理队列
    this.processQueue()
  }

  /**
   * 根据优先级获取延迟时间
   */
  private getDelayForPriority(priority: LoadPriority): number {
    // 关键和高优先级立即执行
    if (priority === LoadPriority.CRITICAL || priority === LoadPriority.HIGH) {
      return 0
    }

    // 中优先级：页面加载后才执行，或延迟执行
    if (priority === LoadPriority.MEDIUM) {
      return this.isPageLoaded ? 0 : this.config.mediumPriorityDelay
    }

    // 低优先级：页面加载后延迟执行
    if (priority === LoadPriority.LOW) {
      return this.isPageLoaded ? this.config.lowPriorityDelay : this.config.lowPriorityDelay * 2
    }

    // 空闲优先级：使用 requestIdleCallback 或长延迟
    if (priority === LoadPriority.IDLE) {
      return this.isPageLoaded ? this.config.idleDelay : this.config.idleDelay * 2
    }

    return 0
  }

  /**
   * 执行任务
   */
  private async executeTask(task: LoadTask): Promise<void> {
    this.activeLoads.add(task.id)

    try {
      // 设置超时
      const timeoutPromise = task.timeout
        ? new Promise<void>((_, reject) =>
            setTimeout(() => reject(new Error('Task timeout')), task.timeout),
          )
        : null

      // 执行加载
      if (timeoutPromise) {
        await Promise.race([task.loader(), timeoutPromise])
      }
      else {
        await task.loader()
      }

      // 标记完成
      this.completedLoads.add(task.id)
      this.failedLoads.delete(task.id)
    }
    catch (error) {
      console.warn(`Resource load failed for task ${task.id}:`, error)

      // 记录失败次数
      const failCount = (this.failedLoads.get(task.id) || 0) + 1
      this.failedLoads.set(task.id, failCount)

      // 如果允许重试且未超过重试次数，重新加入队列
      if (task.retryCount && failCount < task.retryCount) {
        // 降低优先级重试
        this.queue.push({
          ...task,
          priority: Math.min(task.priority + 1, LoadPriority.IDLE) as LoadPriority,
        })
        this.sortQueue()
      }
    }
    finally {
      this.activeLoads.delete(task.id)
      // 继续处理下一个任务
      this.processQueue()
    }
  }

  /**
   * 使用 requestIdleCallback 执行空闲任务
   */
  scheduleIdleTask(id: string, loader: () => Promise<void>): void {
    if (typeof window.requestIdleCallback !== 'undefined') {
      this.idleCallbackId = window.requestIdleCallback(() => {
        this.addTask({
          id,
          priority: LoadPriority.IDLE,
          loader,
        })
      })
    }
    else {
      // 降级方案：使用 setTimeout
      setTimeout(() => {
        this.addTask({
          id,
          priority: LoadPriority.IDLE,
          loader,
        })
      }, this.config.idleDelay)
    }
  }

  /**
   * 等待关键资源加载完成
   */
  async waitForCritical(): Promise<void> {
    // 等待所有关键和高优先级任务完成
    while (
      this.queue.some(t => t.priority <= LoadPriority.HIGH)
      || Array.from(this.activeLoads).some((id) => {
        const task = this.queue.find(t => t.id === id)
        return task && task.priority <= LoadPriority.HIGH
      })
    ) {
      await new Promise(resolve => setTimeout(resolve, 100))
    }
  }

  /**
   * 获取加载统计
   */
  getStats() {
    // 性能优化：单次遍历替代多次 filter
    const queuedByPriority = {
      critical: 0,
      high: 0,
      medium: 0,
      low: 0,
      idle: 0,
    }

    for (const task of this.queue) {
      switch (task.priority) {
        case LoadPriority.CRITICAL: queuedByPriority.critical++; break
        case LoadPriority.HIGH: queuedByPriority.high++; break
        case LoadPriority.MEDIUM: queuedByPriority.medium++; break
        case LoadPriority.LOW: queuedByPriority.low++; break
        case LoadPriority.IDLE: queuedByPriority.idle++; break
      }
    }

    return {
      queued: this.queue.length,
      active: this.activeLoads.size,
      completed: this.completedLoads.size,
      failed: this.failedLoads.size,
      queuedByPriority,
    }
  }

  /**
   * 清空所有任务
   */
  clear(): void {
    this.queue = []
    this.activeLoads.clear()
    if (this.idleCallbackId !== null) {
      window.cancelIdleCallback(this.idleCallbackId)
      this.idleCallbackId = null
    }
  }

  /**
   * 重置加载器
   */
  reset(): void {
    this.clear()
    this.completedLoads.clear()
    this.failedLoads.clear()
  }
}

// 全局资源加载器实例
export const globalResourceLoader = new ResourceLoader({
  maxConcurrent: 3,
  idleDelay: 2000,
  lowPriorityDelay: 1500,
  mediumPriorityDelay: 800,
})

// 开发环境下的调试工具
if (import.meta.env.DEV) {
  // 将资源加载器暴露到全局，方便调试
  (window as any).__resourceLoader = globalResourceLoader

  // 调试统计 - 仅在有活动任务时处理
  setInterval(() => {
    // 保持间隔检查以便调试时可运行断点
    globalResourceLoader.getStats()
  }, 10000)
}

// 便捷方法
export const loadResource = {
  /**
   * 加载关键资源
   */
  critical: (id: string, loader: () => Promise<void>) => {
    globalResourceLoader.addTask({ id, priority: LoadPriority.CRITICAL, loader })
  },

  /**
   * 加载高优先级资源
   */
  high: (id: string, loader: () => Promise<void>) => {
    globalResourceLoader.addTask({ id, priority: LoadPriority.HIGH, loader })
  },

  /**
   * 加载中优先级资源
   */
  medium: (id: string, loader: () => Promise<void>) => {
    globalResourceLoader.addTask({ id, priority: LoadPriority.MEDIUM, loader })
  },

  /**
   * 加载低优先级资源
   */
  low: (id: string, loader: () => Promise<void>) => {
    globalResourceLoader.addTask({ id, priority: LoadPriority.LOW, loader, retryCount: 2 })
  },

  /**
   * 空闲时加载
   */
  idle: (id: string, loader: () => Promise<void>) => {
    globalResourceLoader.scheduleIdleTask(id, loader)
  },
}
