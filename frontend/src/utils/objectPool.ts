export interface ObjectPoolConfig<T> {
  create: () => T
  reset?: (obj: T) => void
  destroy?: (obj: T) => void
  initialSize?: number
  maxSize?: number
  /** 60000ms */
  idleTimeout?: number
}

interface PooledObject<T> {
  obj: T
  lastUsed: number
}

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

    this.preallocate(this.config.initialSize)
  }

  private preallocate(count: number) {
    const toCreate = Math.min(count, this.config.maxSize - this.pool.length)
    for (let i = 0; i < toCreate; i++) {
      this.pool.push({
        obj: this.config.create(),
        lastUsed: Date.now(),
      })
    }
  }

  private startCleanupTimer() {
    if (this.cleanupTimer) return
    if (this.config.idleTimeout <= 0) return
    if (this.pool.length <= this.config.initialSize) return

    this.cleanupTimer = setTimeout(() => {
      this.cleanupTimer = null
      this.cleanupIdle()
      this.startCleanupTimer()
    }, this.config.idleTimeout / 2)
  }

  private cleanupIdle() {
    const now = Date.now()
    const timeout = this.config.idleTimeout

    const minKeep = this.config.initialSize

    for (let i = this.pool.length - 1; i >= minKeep; i--) {
      const item = this.pool[i]
      if (now - item.lastUsed > timeout) {
        this.config.destroy(item.obj)
        this.pool = this.pool.toSpliced(i, 1)
      }
    }
  }

  acquire(): T {
    this.activeCount++

    if (this.pool.length > 0) {
      const pooled = this.pool.pop()!
      this.config.reset(pooled.obj)
      return pooled.obj
    }

    return this.config.create()
  }

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

  get size(): number {
    return this.pool.length
  }

  get active(): number {
    return this.activeCount
  }

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

  destroy(): void {
    if (this.cleanupTimer) {
      clearTimeout(this.cleanupTimer)
      this.cleanupTimer = null
    }
    this.clear()
  }

  getStatus() {
    return {
      poolSize: this.pool.length,
      activeCount: this.activeCount,
      maxSize: this.config.maxSize,
      idleTimeout: this.config.idleTimeout,
    }
  }
}

export interface AnimationStateObject {
  id: string
  opacity: number
  transform: string
  isActive: boolean
  startTime: number
}

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

export interface TimerObject {
  id: ReturnType<typeof setTimeout> | null
  callback: (() => void) | null
  delay: number
}

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

export const globalPoolManager = new PoolManager()

globalPoolManager.register('animationState', animationStatePool)
globalPoolManager.register('timer', timerPool)

export interface ImageLoadObject {
  img: HTMLImageElement
  onLoad: ((e: Event) => void) | null
  onError: ((e: Event | string) => void) | null
}

export const imagePool = new ObjectPool<ImageLoadObject>({
  create: () => ({
    img: new Image(),
    onLoad: null,
    onError: null,
  }),
  reset: (obj) => {
    obj.img.onload = null
    obj.img.onerror = null
    obj.img.src = ''
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
  idleTimeout: 120000,
})

globalPoolManager.register('image', imagePool)

export interface CanvasPoolObject {
  canvas: HTMLCanvasElement
  ctx: CanvasRenderingContext2D | null
}

export const canvasPool = new ObjectPool<CanvasPoolObject>({
  create: () => {
    const canvas = document.createElement('canvas')
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    return { canvas, ctx }
  },
  reset: (obj) => {
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
  idleTimeout: 60000,
})

globalPoolManager.register('canvas', canvasPool)

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

    if (timeout > 0) {
      timeoutId = setTimeout(() => {
        if (resolved) return
        resolved = true
        img.src = ''
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
