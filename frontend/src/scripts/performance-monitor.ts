/**
 * 前端性能监控工具
 * 监测页面加载性能、FPS、内存使用等指标
 * 🔧 优化：FPS 数据统一从 AnimationCoordinator 获取，避免重复 RAF 循环
 */

import { getFrameStats } from '../hooks/animation'

interface PerformanceMetrics {
  fcp: number // First Contentful Paint
  lcp: number // Largest Contentful Paint
  fid: number // First Input Delay
  cls: number // Cumulative Layout Shift
  ttfb: number // Time to First Byte
  fps: number // Frames Per Second
}

// 性能监控配置
const ENABLE_LOGGING = import.meta.env.DEV // 仅在开发环境启用日志
const FPS_WARNING_THRESHOLD = 30
const LONG_TASK_THRESHOLD = 50 // ms

class PerformanceMonitor {
  private metrics: Partial<PerformanceMetrics> = {}
  private fpsFrames: number[] = []
  private fpsIntervalId: ReturnType<typeof setInterval> | null = null

  constructor() {
    this.init()
  }

  private init() {
    if (typeof window === 'undefined')
      return

    // 监测Core Web Vitals
    this.measureWebVitals()

    // 开始FPS监测（从 coordinator 轮询）
    this.startFPSMonitor()

    // 监听页面可见性变化
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) {
        this.stopFPSMonitor()
      }
      else {
        this.startFPSMonitor()
      }
    })
  }

  /**
   * 测量Web Vitals指标
   */
  private measureWebVitals() {
    // 使用Performance Observer API
    if ('PerformanceObserver' in window) {
      // First Contentful Paint (FCP)
      try {
        const fcpObserver = new PerformanceObserver((entryList) => {
          for (const entry of entryList.getEntries()) {
            if (entry.name === 'first-contentful-paint') {
              this.metrics.fcp = entry.startTime
              if (ENABLE_LOGGING) {
                console.log(`✅ FCP: ${entry.startTime.toFixed(2)}ms`)
              }
            }
          }
        })
        fcpObserver.observe({ entryTypes: ['paint'] })
      }
      catch (e) {
        // FCP monitoring not supported
      }

      // Largest Contentful Paint (LCP)
      try {
        const lcpObserver = new PerformanceObserver((entryList) => {
          const entries = entryList.getEntries()
          const lastEntry = entries[entries.length - 1]
          this.metrics.lcp = lastEntry.startTime
          if (ENABLE_LOGGING) {
            console.log(`✅ LCP: ${lastEntry.startTime.toFixed(2)}ms`)
          }
        })
        lcpObserver.observe({ entryTypes: ['largest-contentful-paint'] })
      }
      catch (e) {
        // LCP monitoring not supported
      }

      // Cumulative Layout Shift (CLS)
      try {
        let clsValue = 0
        const clsObserver = new PerformanceObserver((entryList) => {
          for (const entry of entryList.getEntries()) {
            // @ts-ignore
            if (!entry.hadRecentInput) {
              // @ts-ignore
              clsValue += entry.value
              this.metrics.cls = clsValue
            }
          }
          if (ENABLE_LOGGING) {
            console.log(`✅ CLS: ${clsValue.toFixed(4)}`)
          }
        })
        clsObserver.observe({ entryTypes: ['layout-shift'] })
      }
      catch (e) {
        // CLS monitoring not supported
      }

      // First Input Delay (FID)
      try {
        const fidObserver = new PerformanceObserver((entryList) => {
          for (const entry of entryList.getEntries()) {
            // @ts-ignore
            this.metrics.fid = entry.processingStart - entry.startTime
            if (ENABLE_LOGGING) {
              console.log(`✅ FID: ${this.metrics.fid.toFixed(2)}ms`)
            }
          }
        })
        fidObserver.observe({ entryTypes: ['first-input'] })
      }
      catch (e) {
        // FID monitoring not supported
      }
    }

    // Time to First Byte (TTFB)
    if (performance.timing) {
      const ttfb = performance.timing.responseStart - performance.timing.requestStart
      this.metrics.ttfb = ttfb
      if (ENABLE_LOGGING) {
        console.log(`✅ TTFB: ${ttfb}ms`)
      }
    }
  }

  /**
   * 开始FPS监测（从 AnimationCoordinator 轮询）
   */
  private startFPSMonitor() {
    if (this.fpsIntervalId)
      return

    // 每秒从 coordinator 获取 FPS 数据
    this.fpsIntervalId = setInterval(() => {
      try {
        const stats = getFrameStats()
        const fps = stats.fps
        this.metrics.fps = fps
        this.fpsFrames.push(fps)

        // 只保留最近10秒的数据
        if (this.fpsFrames.length > 10) {
          this.fpsFrames.shift()
        }

        // FPS低于阈值时发出警告
        if (ENABLE_LOGGING && fps < FPS_WARNING_THRESHOLD) {
          console.warn(`⚠️ Low FPS detected: ${fps}`)
        }
      }
      catch {
        // coordinator 可能未初始化
      }
    }, 1000)
  }

  /**
   * 停止FPS监测
   */
  private stopFPSMonitor() {
    if (this.fpsIntervalId) {
      clearInterval(this.fpsIntervalId)
      this.fpsIntervalId = null
    }
  }

  /**
   * 获取平均FPS
   */
  public getAverageFPS(): number {
    if (this.fpsFrames.length === 0)
      return 0
    const sum = this.fpsFrames.reduce((a, b) => a + b, 0)
    return Math.round(sum / this.fpsFrames.length)
  }

  /**
   * 获取所有性能指标
   */
  public getMetrics(): Partial<PerformanceMetrics> {
    return {
      ...this.metrics,
      fps: this.getAverageFPS(),
    }
  }

  /**
   * 输出性能报告
   */
  public logReport() {
    if (!ENABLE_LOGGING)
      return

    console.group('📊 Performance Report')
    console.log('FCP (First Contentful Paint):', this.metrics.fcp?.toFixed(2), 'ms')
    console.log('LCP (Largest Contentful Paint):', this.metrics.lcp?.toFixed(2), 'ms')
    console.log('FID (First Input Delay):', this.metrics.fid?.toFixed(2), 'ms')
    console.log('CLS (Cumulative Layout Shift):', this.metrics.cls?.toFixed(4))
    console.log('TTFB (Time to First Byte):', this.metrics.ttfb, 'ms')
    console.log('Average FPS:', this.getAverageFPS())
    console.groupEnd()
  }

  /**
   * 检测长任务
   */
  public detectLongTasks() {
    if (!ENABLE_LOGGING || !('PerformanceObserver' in window))
      return

    try {
      const observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (entry.duration > LONG_TASK_THRESHOLD) {
            console.warn(`⚠️ Long task detected: ${entry.duration.toFixed(2)}ms`, entry)
          }
        }
      })
      observer.observe({ entryTypes: ['longtask'] })
    }
    catch (e) {
      // Long task monitoring not supported
    }
  }
}

// 创建单例实例
let monitorInstance: PerformanceMonitor | null = null

/**
 * 获取性能监控实例
 */
export function getPerformanceMonitor(): PerformanceMonitor {
  if (!monitorInstance && typeof window !== 'undefined') {
    monitorInstance = new PerformanceMonitor()
  }
  return monitorInstance!
}

/**
 * 快速检查性能
 */
export function quickPerformanceCheck() {
  if (typeof window === 'undefined')
    return

  const monitor = getPerformanceMonitor()

  // 延迟输出，确保有足够的数据
  setTimeout(() => {
    monitor.logReport()
  }, 3000)
}

// 自动启动性能监控（仅在开发环境）
if (typeof window !== 'undefined' && ENABLE_LOGGING) {
  console.log('🚀 Performance monitoring started')
  quickPerformanceCheck()

  // 监测长任务
  getPerformanceMonitor().detectLongTasks()
}
