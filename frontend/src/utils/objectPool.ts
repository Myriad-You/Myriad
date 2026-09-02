/**
 * 对象池管理器 - 通用对象复用工具
 *
 * 性能优化特性：
 * 1. 减少 GC 压力：复用对象而非频繁创建/销毁
 * 2. 预分配内存：提前创建常用对象
 * 3. 自动回收：空闲对象超时自动清理
 * 4. 泛型支持：适用于任何可复用对象类型
 *
 * @module objectPool
 * @version 1.0
 */

/** 对象池配置 */
export interface ObjectPoolConfig<T> {
  /** 创建新对象的工厂函数 */
  create: () => T
  /** 重置对象状态（可选，用于复用前清理） */
  reset?: (obj: T) => void
  /** 销毁对象（可选，用于清理资源） */
  destroy?: (obj: T) => void
  /** 初始池大小（默认 0） */
  initialSize?: number
  /** 最大池大小（默认 50） */
  maxSize?: number
  /** 空闲超时（毫秒，默认 60000，超时后销毁空闲对象） */
  idleTimeout?: number
}

/** 池化对象包装器 */
interface PooledObject<T> {
  obj: T
  lastUsed: number
}

/**
 * 通用对象池
 */
export class ObjectPool<T> {
  private pool: PooledObject<T>[] = []
  private config: Required<ObjectPoolConfig<T>>
  private cleanupTimer: ReturnType<typeof setTimeout> | null = null
  private activeCount = 0

  constructor(config: ObjectPoolConfig<T>) {
    this.config = {
      create: config.create,
      reset: config.reset ?? (() => {}),
      destroy: config.destroy ?? (() => {}),
      initialSize: config.initialSize ?? 0,
      maxSize: config.maxSize ?? 50,
      idleTimeout: config.idleTimeout ?? 60000,
    }

    // 预分配对象
    this.preallocate(this.config.initialSize)
  }

  /** 预分配对象 */
  private preallocate(count: number) {
    const toCreate = Math.min(count, this.config.maxSize - this.pool.length)
    for (let i = 0; i < toCreate; i++) {
      this.pool.push({
        obj: this.config.create(),
        lastUsed: Date.now(),
      })
    }
  }

  /** 启动空闲清理定时器 */
  private startCleanupTimer() {
    if (this.cleanupTimer) return
    if (this.config.idleTimeout <= 0) return
    // initialSize 是永久保底容量；只有额外对象才需要定时回收。
    if (this.pool.length <= this.config.initialSize) return

    this.cleanupTimer = setTimeout(() => {
      this.cleanupTimer = null
      this.cleanupIdle()
      this.startCleanupTimer()
    }, this.config.idleTimeout / 2)
  }

  /** 清理空闲对象 */
  private cleanupIdle() {
    const now = Date.now()
    const timeout = this.config.idleTimeout

    // 保留至少 initialSize 个对象
    const minKeep = this.config.initialSize

    // 从后往前遍历，移除超时的对象
    for (let i = this.pool.length - 1; i >= minKeep; i--) {
      const item = this.pool[i]
      if (now - item.lastUsed > timeout) {
        this.config.destroy(item.obj)
        this.pool.splice(i, 1)
      }
    }
  }

  /**
   * 从池中获取对象
   * 如果池为空，创建新对象
   */
  acquire(): T {
    this.activeCount++

    if (this.pool.length > 0) {
      const pooled = this.pool.pop()!
      this.config.reset(pooled.obj)
      return pooled.obj
    }

    return this.config.create()
  }

  /**
   * 将对象归还到池中
   * 如果池已满，销毁对象
   */
  release(obj: T): void {
    this.activeCount = Math.max(0, this.activeCount - 1)

    if (this.pool.length < this.config.maxSize) {
      this.pool.push({
        obj,
        lastUsed: Date.now(),
      })
      this.startCleanupTimer()
    } else {
      this.config.destroy(obj)
    }
  }

  /** 获取当前池大小 */
  get size(): number {
    return this.pool.length
  }

  /** 获取活跃对象数量 */
  get active(): number {
    return this.activeCount
  }

  /** 清空池 */
  clear(): void {
    if (this.cleanupTimer) {
      clearTimeout(this.cleanupTimer)
      this.cleanupTimer = null
    }
    for (const item of this.pool) {
      this.config.destroy(item.obj)
    }
    this.pool = []
    this.activeCount = 0
  }

  /** 销毁池 */
  destroy(): void {
    if (this.cleanupTimer) {
      clearTimeout(this.cleanupTimer)
      this.cleanupTimer = null
    }
    this.clear()
  }

  /** 获取池状态（调试用） */
  getStatus() {
    return {
      poolSize: this.pool.length,
      activeCount: this.activeCount,
      maxSize: this.config.maxSize,
      idleTimeout: this.config.idleTimeout,
    }
  }
}

// 预定义的对象池

/** 动画状态对象 */
export interface AnimationStateObject {
  id: string
  opacity: number
  transform: string
  isActive: boolean
  startTime: number
}

/**
 * 创建动画状态对象池
 * 用于管理大量动画元素的状态
 */
export const animationStatePool = new ObjectPool<AnimationStateObject>({
  create: () => ({
    id: '',
    opacity: 0,
    transform: '',
    isActive: false,
    startTime: 0,
  }),
  reset: (obj) => {
    obj.id = ''
    obj.opacity = 0
    obj.transform = ''
    obj.isActive = false
    obj.startTime = 0
  },
  initialSize: 10,
  maxSize: 100,
  idleTimeout: 30000,
})

/** 定时器对象 */
export interface TimerObject {
  id: ReturnType<typeof setTimeout> | null
  callback: (() => void) | null
  delay: number
}

/**
 * 创建定时器对象池
 * 用于减少 setTimeout 创建开销
 */
export const timerPool = new ObjectPool<TimerObject>({
  create: () => ({
    id: null,
    callback: null,
    delay: 0,
  }),
  reset: (obj) => {
    if (obj.id !== null) {
      clearTimeout(obj.id)
    }
    obj.id = null
    obj.callback = null
    obj.delay = 0
  },
  destroy: (obj) => {
    if (obj.id !== null) {
      clearTimeout(obj.id)
    }
  },
  initialSize: 5,
  maxSize: 30,
  idleTimeout: 60000,
})

// 工具函数

/**
 * 批量对象池管理器
 * 用于管理多个相关对象池
 */
export class PoolManager {
  private pools = new Map<string, ObjectPool<any>>()

  register<T>(name: string, pool: ObjectPool<T>): void {
    this.pools.set(name, pool)
  }

  get<T>(name: string): ObjectPool<T> | undefined {
    return this.pools.get(name)
  }

  getStatus(): Record<string, ReturnType<ObjectPool<any>['getStatus']>> {
    const status: Record<string, ReturnType<ObjectPool<any>['getStatus']>> = {}
    for (const [name, pool] of this.pools) {
      status[name] = pool.getStatus()
    }
    return status
  }

  clearAll(): void {
    for (const pool of this.pools.values()) {
      pool.clear()
    }
  }

  destroyAll(): void {
    for (const pool of this.pools.values()) {
      pool.destroy()
    }
    this.pools.clear()
  }
}

/** 全局池管理器实例 */
export const globalPoolManager = new PoolManager()

// 注册预定义池
globalPoolManager.register('animationState', animationStatePool)
globalPoolManager.register('timer', timerPool)

// Image 对象池 - 用于图片预加载

/** 图片加载对象 */
export interface ImageLoadObject {
  img: HTMLImageElement
  onLoad: ((e: Event) => void) | null
  onError: ((e: Event | string) => void) | null
}

/**
 * Image 对象池
 * 用于减少 new Image() 的 GC 开销
 */
export const imagePool = new ObjectPool<ImageLoadObject>({
  create: () => ({
    img: new Image(),
    onLoad: null,
    onError: null,
  }),
  reset: (obj) => {
    // 清除事件监听器
    obj.img.onload = null
    obj.img.onerror = null
    // 清除 src 停止加载
    obj.img.src = ''
    // 重置属性
    obj.img.crossOrigin = null
    obj.onLoad = null
    obj.onError = null
  },
  destroy: (obj) => {
    obj.img.onload = null
    obj.img.onerror = null
    obj.img.src = ''
  },
  initialSize: 3,
  maxSize: 10,
  idleTimeout: 120000, // 2分钟
})

globalPoolManager.register('image', imagePool)

// Canvas 对象池 - 用于图片处理（颜色提取等）

/** Canvas 加载对象 */
export interface CanvasPoolObject {
  canvas: HTMLCanvasElement
  ctx: CanvasRenderingContext2D | null
}

/**
 * Canvas 对象池
 * 用于减少 document.createElement('canvas') 的开销
 */
export const canvasPool = new ObjectPool<CanvasPoolObject>({
  create: () => {
    const canvas = document.createElement('canvas')
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    return { canvas, ctx }
  },
  reset: (obj) => {
    // 重置 canvas 大小会清空内容
    obj.canvas.width = 0
    obj.canvas.height = 0
  },
  destroy: (obj) => {
    obj.canvas.width = 0
    obj.canvas.height = 0
    obj.ctx = null
  },
  initialSize: 1,
  maxSize: 3,
  idleTimeout: 60000, // 1分钟
})

globalPoolManager.register('canvas', canvasPool)

/**
 * 使用池化的 canvas 进行图片处理
 *
 * @param width canvas 宽度
 * @param height canvas 高度
 * @param processor 处理函数，接收 ctx 和 canvas
 * @returns 处理结果
 *
 * @example
 * ```ts
 * const imageData = await withPooledCanvas(100, 100, (ctx, canvas) => {
 *   ctx.drawImage(img, 0, 0, 100, 100);
 *   return ctx.getImageData(0, 0, 100, 100);
 * });
 * ```
 */
export function withPooledCanvas<T>(
  width: number,
  height: number,
  processor: (ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement) => T,
): T {
  const pooled = canvasPool.acquire()
  const { canvas, ctx } = pooled

  if (!ctx) {
    canvasPool.release(pooled)
    throw new Error('Failed to get canvas context')
  }

  try {
    canvas.width = width
    canvas.height = height
    return processor(ctx, canvas)
  } finally {
    canvasPool.release(pooled)
  }
}

/**
 * 使用池化的图片加载
 *
 * @param src 图片 URL
 * @param options 加载选项
 * @returns Promise<boolean> 加载是否成功
 *
 * @example
 * ```ts
 * const success = await loadImagePooled('https://example.com/image.jpg');
 * ```
 */
export function loadImagePooled(
  src: string,
  options: {
    crossOrigin?: string | null
    timeout?: number
  } = {},
): Promise<boolean> {
  const { crossOrigin = null, timeout = 15000 } = options

  return new Promise((resolve) => {
    const pooled = imagePool.acquire()
    const { img } = pooled

    let timeoutId: ReturnType<typeof setTimeout> | null = null
    let resolved = false

    const cleanup = () => {
      if (timeoutId) {
        clearTimeout(timeoutId)
        timeoutId = null
      }
      imagePool.release(pooled)
    }

    const handleLoad = () => {
      if (resolved) return
      resolved = true
      cleanup()
      resolve(true)
    }

    const handleError = () => {
      if (resolved) return
      resolved = true
      cleanup()
      resolve(false)
    }

    // 设置超时
    if (timeout > 0) {
      timeoutId = setTimeout(() => {
        if (resolved) return
        resolved = true
        img.src = '' // 停止加载
        cleanup()
        resolve(false)
      }, timeout)
    }

    img.onload = handleLoad
    img.onerror = handleError

    if (crossOrigin !== null) {
      img.crossOrigin = crossOrigin
    }

    img.src = src
  })
}

export default ObjectPool
