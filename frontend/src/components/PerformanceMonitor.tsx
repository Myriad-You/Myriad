/**
 * 性能监控面板 (开发环境)
 * 实时显示FPS、内存使用、渲染时间、资源加载状态等
 * 🆕 增强功能：动效监控、布局重排检测、GPU层统计、JS动画监控
 * 🔧 优化：FPS 数据统一从 AnimationCoordinator 获取，避免重复 RAF 循环
 */

import { useCallback, useEffect, useState } from 'react'
import { configureAnimationCoordinator, coordinator, getFrameStats } from '../hooks/animation'
import { clearLyricsCache, clearPlaylistCache } from '../utils/musicPlayer'
import { globalResourceLoader } from '../utils/resourceLoader'
import './PerformanceMonitor.css'

interface PerformanceMetrics {
  fps: number
  memory?: {
    used: number
    limit: number
    usedPercent: number
  }
  renderTime: number
  longTasks: number
}

/** 动效元素信息 */
interface AnimationInfo {
  element: string
  selector: string
  animationName: string
  duration: string
  iterationCount: string
  state: 'running' | 'paused'
  isInfinite: boolean
  isExpensive: boolean
}

/** JS 动画信息 */
interface JsAnimationInfo {
  id: number
  type: 'raf' | 'interval' | 'timeout' | 'webAnimation' | 'framerMotion'
  name?: string
  target?: string
  duration?: number
  isActive: boolean
}

/** JS 动画统计 */
interface JsAnimationStats {
  activeRAFs: number
  activeIntervals: number
  activeTimeouts: number
  webAnimations: number
  webAnimationsRunning: number
  framerMotionElements: number
  totalJsAnimations: number
  rafCallsPerSecond: number
  webAnimationDetails: JsAnimationInfo[]
}

/** 动效统计 */
interface AnimationStats {
  total: number
  running: number
  paused: number
  infinite: number
  cssAnimations: AnimationInfo[]
  cssTransitions: number
  framerMotionElements: number
  willChangeElements: number
  transformElements: number
  expensiveAnimations: AnimationInfo[]
}

/** 布局性能统计 */
interface LayoutStats {
  reflows: number
  repaints: number
  styleRecalcs: number
  lastLayoutShift: number
}

/** 扩展选项卡类型 */
type TabType = 'overview' | 'animations' | 'js' | 'layout'

// ==================== JS 动画追踪器 ====================
/** 全局 JS 动画追踪器 */
interface AnimationTracker {
  rafIds: Set<number>
  intervalIds: Set<ReturnType<typeof setInterval>>
  timeoutIds: Set<ReturnType<typeof setTimeout>>
  rafCallCount: number
  lastRafCountReset: number
  isInstalled: boolean
}

const animationTracker: AnimationTracker = {
  rafIds: new Set(),
  intervalIds: new Set(),
  timeoutIds: new Set(),
  rafCallCount: 0,
  lastRafCountReset: Date.now(),
  isInstalled: false,
}

/** 安装 JS 动画追踪钩子 */
function installAnimationTracker() {
  if (animationTracker.isInstalled || typeof window === 'undefined')
    return

  // 保存原始函数
  const originalRAF = window.requestAnimationFrame.bind(window)
  const originalCAF = window.cancelAnimationFrame.bind(window)
  const originalSetInterval = window.setInterval.bind(window)
  const originalClearInterval = window.clearInterval.bind(window)
  const originalSetTimeout = window.setTimeout.bind(window)
  const originalClearTimeout = window.clearTimeout.bind(window);

  // 包装 requestAnimationFrame
  (window as Window).requestAnimationFrame = function (callback: FrameRequestCallback): number {
    const id = originalRAF((time: DOMHighResTimeStamp) => {
      animationTracker.rafCallCount++
      animationTracker.rafIds.delete(id)
      callback(time)
    })
    animationTracker.rafIds.add(id)
    return id
  };

  // 包装 cancelAnimationFrame
  (window as Window).cancelAnimationFrame = function (id: number): void {
    animationTracker.rafIds.delete(id)
    originalCAF(id)
  }

  // 注意：由于 TypeScript 类型系统的复杂性，我们只追踪 RAF
  // setInterval/setTimeout 在浏览器中返回 number，但类型定义可能不一致
  // 这里我们简化实现，只追踪 Web Animations API

  animationTracker.isInstalled = true
}

/** 获取元素简短选择器 */
function getShortSelector(el: Element | null): string {
  if (!el)
    return 'unknown'
  const tag = el.tagName?.toLowerCase() || 'unknown'
  const id = el.id ? `#${el.id}` : ''
  const classes = el.className && typeof el.className === 'string'
    ? `.${el.className.split(' ').filter(c => c && !c.startsWith('_')).slice(0, 1).join('.')}`
    : ''
  return `${tag}${id}${classes}`.slice(0, 25)
}

/** 获取 JS 动画统计 */
function getJsAnimationStats(): JsAnimationStats {
  const now = Date.now()
  const elapsed = (now - animationTracker.lastRafCountReset) / 1000
  const rafPerSecond = elapsed > 0 ? Math.round(animationTracker.rafCallCount / elapsed) : 0

  // 重置计数器（每秒）
  if (elapsed >= 1) {
    animationTracker.rafCallCount = 0
    animationTracker.lastRafCountReset = now
  }

  // 获取 Web Animations API 动画
  let webAnimations = 0
  let webAnimationsRunning = 0
  const webAnimationDetails: JsAnimationInfo[] = []

  try {
    const allAnimations = document.getAnimations?.() || []
    webAnimations = allAnimations.length

    // 获取前 10 个运行中的动画详情
    allAnimations
      .filter(a => a.playState === 'running')
      .slice(0, 10)
      .forEach((anim, index) => {
        const effect = anim.effect as KeyframeEffect | null
        const target = effect?.target as Element | null
        const timing = effect?.getTiming?.()

        webAnimationDetails.push({
          id: index,
          type: 'webAnimation',
          name: (anim as CSSAnimation).animationName || 'anonymous',
          target: getShortSelector(target),
          duration: typeof timing?.duration === 'number' ? timing.duration : undefined,
          isActive: true,
        })
      })

    webAnimationsRunning = allAnimations.filter(a => a.playState === 'running').length
  }
  catch {
    // getAnimations 可能不支持
  }

  // 检测 Framer Motion 元素（通过 data-framer-* 属性或 style 特征）
  // ⚠️ 优化：避免使用 getComputedStyle，改用内联 style 检测
  let framerMotionElements = 0
  try {
    // Framer Motion 会给动画元素添加内联 transform 和 transition style
    // 🔧 优化：限制最多检测 50 个元素，避免 Long Task
    const motionElements = document.querySelectorAll('[style*="transform"]')
    const maxCheck = Math.min(motionElements.length, 50)
    for (let i = 0; i < maxCheck; i++) {
      const htmlEl = motionElements[i] as HTMLElement
      const inlineStyle = htmlEl.style
      // 检查内联 style 而非 computedStyle，避免强制重排
      if (inlineStyle.transform && (
        inlineStyle.transform.includes('translateX')
        || inlineStyle.transform.includes('translateY')
        || inlineStyle.transform.includes('scale')
        || inlineStyle.transform.includes('rotate')
      )) {
        // 检查内联 transition（Framer Motion 会设置内联 transition）
        if (inlineStyle.transition && inlineStyle.transition !== 'none') {
          framerMotionElements++
        }
      }
    }
  }
  catch {
    // 忽略错误
  }

  return {
    activeRAFs: animationTracker.rafIds.size,
    activeIntervals: 0, // 简化：不追踪 interval
    activeTimeouts: 0, // 简化：不追踪 timeout
    webAnimations,
    webAnimationsRunning,
    framerMotionElements,
    totalJsAnimations: animationTracker.rafIds.size + webAnimationsRunning + framerMotionElements,
    rafCallsPerSecond: rafPerSecond,
    webAnimationDetails,
  }
}

/** 生成元素选择器描述 */
function getElementSelector(el: Element): string {
  const tag = el.tagName.toLowerCase()
  const id = el.id ? `#${el.id}` : ''
  const classes = el.className && typeof el.className === 'string'
    ? `.${el.className.split(' ').filter(c => c && !c.startsWith('_')).slice(0, 2).join('.')}`
    : ''
  return `${tag}${id}${classes}`.slice(0, 40)
}

/** 已知的优化过的动画名称（使用 transform/opacity，无需标记） */
const OPTIMIZED_ANIMATIONS = new Set([
  'spin',
  'blob',
  'fadeIn',
  'fadeInUp',
  'fadeOut',
  'scaleIn',
  'scaleOut',
  'slideInLeft',
  'slideInRight',
  'slideUp',
  'slideDown',
  'cardFadeIn',
  'toastSlideIn',
  'toastSlideOut',
  'overlayFadeIn',
  'pulse',
  'shimmer',
  'icon-sway',
  'icon-float',
  'icon-spin',
  'pet-walk-bounce',
  'fade-in-up',
  'slide-in-right',
  'scale-in',
  'perf-pulse',
  'perf-slide-in',
])

/** 检测动画是否可能造成性能问题 */
function isExpensiveAnimation(el: Element, style: CSSStyleDeclaration): boolean {
  const animationName = style.animationName || ''
  const willChange = style.willChange || ''
  const transform = style.transform || ''

  // 跳过已知的优化过的动画
  const animNames = animationName.split(',').map(n => n.trim())
  if (animNames.every(name => OPTIMIZED_ANIMATIONS.has(name))) {
    return false
  }

  // 检查是否使用了触发重排的属性名称
  const expensiveProps = ['width', 'height', 'top', 'left', 'right', 'bottom', 'margin', 'padding']
  const animatedProps = animationName.toLowerCase()
  if (expensiveProps.some(p => animatedProps.includes(p))) {
    return true
  }

  // 无限循环动画
  const isInfinite = style.animationIterationCount === 'infinite'

  // 检查是否使用了 GPU 优化
  const hasWillChange = willChange.includes('transform') || willChange.includes('opacity')
  const hasGpuHint = transform.includes('translateZ') || transform.includes('translate3d') || transform.includes('matrix')
  const isOptimized = hasWillChange || hasGpuHint

  // 长时间运行的动画（> 10秒）且未优化 - 可能是问题
  const duration = Number.parseFloat(style.animationDuration) || 0
  const isVeryLongRunning = duration > 10

  return isVeryLongRunning && !isOptimized && isInfinite
}

export default function PerformanceMonitor() {
  const [metrics, setMetrics] = useState<PerformanceMetrics>({
    fps: 0,
    renderTime: 0,
    longTasks: 0,
  })
  const [resourceStats, setResourceStats] = useState(globalResourceLoader.getStats())
  const [isExpanded, setIsExpanded] = useState(false)
  const [showCacheCleared, setShowCacheCleared] = useState(false)
  const [activeTab, setActiveTab] = useState<TabType>('overview')
  const [animationStats, setAnimationStats] = useState<AnimationStats>({
    total: 0,
    running: 0,
    paused: 0,
    infinite: 0,
    cssAnimations: [],
    cssTransitions: 0,
    framerMotionElements: 0,
    willChangeElements: 0,
    transformElements: 0,
    expensiveAnimations: [],
  })
  const [jsAnimationStats, setJsAnimationStats] = useState<JsAnimationStats>({
    activeRAFs: 0,
    activeIntervals: 0,
    activeTimeouts: 0,
    webAnimations: 0,
    webAnimationsRunning: 0,
    framerMotionElements: 0,
    totalJsAnimations: 0,
    rafCallsPerSecond: 0,
    webAnimationDetails: [],
  })
  const [layoutStats, setLayoutStats] = useState<LayoutStats>({
    reflows: 0,
    repaints: 0,
    styleRecalcs: 0,
    lastLayoutShift: 0,
  })
  const [highlightAnimations, setHighlightAnimations] = useState(false)
  const [pauseAllAnimations, setPauseAllAnimations] = useState(false)

  // 安装 JS 动画追踪器
  useEffect(() => {
    installAnimationTracker()
  }, [])

  // JS 动画统计更新 - 🔧 降低更新频率减少 Long Tasks
  useEffect(() => {
    if (isExpanded && activeTab === 'js') {
      const updateJsStats = () => {
        setJsAnimationStats(getJsAnimationStats())
      }
      updateJsStats()
      // 🔧 从 500ms 改为 1000ms，减少 querySelectorAll 调用频率
      const interval = setInterval(updateJsStats, 1000)
      return () => clearInterval(interval)
    }
  }, [isExpanded, activeTab])

  /** 扫描页面动效 - 使用 Web Animations API 避免强制重排 */
  const scanAnimations = useCallback(() => {
    // ⚠️ 优化：使用 Web Animations API 代替遍历所有元素调用 getComputedStyle
    // 这避免了在扫描时触发强制重排（215ms+ 的性能损失）
    const animations: AnimationInfo[] = []
    const expensiveAnimations: AnimationInfo[] = []
    let running = 0
    let paused = 0
    let infinite = 0
    let cssTransitions = 0
    let framerMotionElements = 0
    let willChangeElements = 0
    let transformElements = 0

    // 排除性能监控面板自身
    const monitorPanel = document.querySelector('[data-perf-monitor]')

    try {
      // 使用 Web Animations API 获取所有动画（不触发重排）
      const allAnimations = document.getAnimations?.() || []

      allAnimations.forEach((anim) => {
        const effect = anim.effect as KeyframeEffect | null
        const target = effect?.target as Element | null

        // 跳过监控面板内的元素
        if (!target || monitorPanel?.contains(target))
          return

        const timing = effect?.getTiming?.()
        const isRunning = anim.playState === 'running'
        const isInfiniteAnim = timing?.iterations === Infinity
        const animName = (anim as CSSAnimation).animationName || 'anonymous'
        const durationMs = typeof timing?.duration === 'number' ? timing.duration : 0

        // 检测是否可能造成性能问题
        const isExpensive = isInfiniteAnim && durationMs > 10000
          && !OPTIMIZED_ANIMATIONS.has(animName)

        const info: AnimationInfo = {
          element: target.tagName?.toLowerCase() || 'unknown',
          selector: getElementSelector(target),
          animationName: animName,
          duration: `${durationMs}ms`,
          iterationCount: isInfiniteAnim ? 'infinite' : String(timing?.iterations || 1),
          state: isRunning ? 'running' : 'paused',
          isInfinite: isInfiniteAnim,
          isExpensive,
        }

        animations.push(info)

        if (isRunning)
          running++
        else paused++
        if (isInfiniteAnim)
          infinite++
        if (isExpensive)
          expensiveAnimations.push(info)
      })
    }
    catch {
      // getAnimations 可能不支持
    }

    // 🔧 优化：使用更高效的采样策略，减少 DOM 查询次数
    // 只检测有内联 style 且包含动画相关属性的元素
    try {
      // 使用更具体的选择器减少匹配元素数量
      // 只匹配可能包含动画的元素（有 transform 或 transition 的内联样式）
      const transformElements_list = document.querySelectorAll('[style*="transform"]')
      const transitionElements_list = document.querySelectorAll('[style*="transition"]')
      const willChangeElements_list = document.querySelectorAll('[style*="will-change"]')

      // 直接计数，避免重复遍历
      let transformCount = 0
      let transitionCount = 0
      let willChangeCount = 0
      let framerCount = 0

      // 限制每种类型最多检测 30 个，总共最多 90 次 DOM 访问
      const maxPerType = 30

      for (let i = 0; i < Math.min(transformElements_list.length, maxPerType); i++) {
        const el = transformElements_list[i] as HTMLElement
        if (monitorPanel?.contains(el))
          continue
        transformCount++
        // Framer Motion 检测：有 transform 且有 transition 的元素
        if (el.style.transition && el.style.transition !== 'none') {
          framerCount++
        }
      }

      for (let i = 0; i < Math.min(transitionElements_list.length, maxPerType); i++) {
        const el = transitionElements_list[i] as HTMLElement
        if (monitorPanel?.contains(el))
          continue
        transitionCount++
      }

      for (let i = 0; i < Math.min(willChangeElements_list.length, maxPerType); i++) {
        const el = willChangeElements_list[i] as HTMLElement
        if (monitorPanel?.contains(el))
          continue
        if (el.style.willChange && el.style.willChange !== 'auto') {
          willChangeCount++
        }
      }

      transformElements = transformCount
      cssTransitions = transitionCount
      willChangeElements = willChangeCount
      framerMotionElements = framerCount
    }
    catch {
      // 忽略错误
    }

    setAnimationStats({
      total: animations.length,
      running,
      paused,
      infinite,
      cssAnimations: animations.slice(0, 20), // 只保留前20个
      cssTransitions,
      framerMotionElements,
      willChangeElements,
      transformElements,
      expensiveAnimations: expensiveAnimations.slice(0, 10),
    })
  }, [])

  // FPS监控 - 🔧 统一从 AnimationCoordinator 获取，无需额外 RAF 循环
  useEffect(() => {
    // 从调度器获取 FPS 数据
    const updateFromCoordinator = () => {
      const stats = getFrameStats()
      setMetrics(prev => ({ ...prev, fps: stats.fps }))
    }

    // 收缩状态：低频率轮询（每 2 秒）
    // 展开状态：高频率轮询（每 500ms）
    const interval = isExpanded ? 500 : 2000

    // 立即更新一次
    updateFromCoordinator()

    const intervalId = setInterval(updateFromCoordinator, interval)

    return () => {
      clearInterval(intervalId)
    }
  }, [isExpanded])

  // 内存监控 - 只在展开时运行，减少收缩状态性能开销
  useEffect(() => {
    if (!isExpanded)
      return

    const checkMemory = () => {
      if ('memory' in performance) {
        const mem = (performance as any).memory
        setMetrics(prev => ({
          ...prev,
          memory: {
            used: Math.round(mem.usedJSHeapSize / 1048576),
            limit: Math.round(mem.jsHeapSizeLimit / 1048576),
            usedPercent: Math.round((mem.usedJSHeapSize / mem.jsHeapSizeLimit) * 100),
          },
        }))
      }
    }

    const interval = setInterval(checkMemory, 2000)
    checkMemory()

    return () => clearInterval(interval)
  }, [isExpanded])

  // Long Tasks监控 - 只在展开时运行
  useEffect(() => {
    if (!isExpanded)
      return

    if ('PerformanceObserver' in window) {
      try {
        const observer = new PerformanceObserver((list) => {
          const entries = list.getEntries()
          setMetrics(prev => ({
            ...prev,
            longTasks: prev.longTasks + entries.length,
          }))
        })

        observer.observe({ entryTypes: ['longtask'] })

        return () => observer.disconnect()
      }
      catch {
        // longtask可能不被支持
      }
    }
  }, [isExpanded])

  // Layout Shift 监控 - 只在展开时运行
  useEffect(() => {
    if (!isExpanded)
      return

    if ('PerformanceObserver' in window) {
      try {
        const observer = new PerformanceObserver((list) => {
          const entries = list.getEntries()
          entries.forEach((entry: any) => {
            if (entry.value) {
              setLayoutStats(prev => ({
                ...prev,
                lastLayoutShift: entry.value,
                reflows: prev.reflows + 1,
              }))
            }
          })
        })

        observer.observe({ type: 'layout-shift', buffered: true })

        return () => observer.disconnect()
      }
      catch {
        // layout-shift可能不被支持
      }
    }
  }, [isExpanded])

  // 渲染时间监控
  useEffect(() => {
    if ('PerformanceObserver' in window) {
      try {
        const observer = new PerformanceObserver((list) => {
          const entries = list.getEntries()
          entries.forEach((entry: any) => {
            if (entry.name === 'first-contentful-paint') {
              setMetrics(prev => ({
                ...prev,
                renderTime: Math.round(entry.startTime),
              }))
            }
          })
        })

        observer.observe({ entryTypes: ['paint'] })

        return () => observer.disconnect()
      }
      catch {
        // paint可能不被支持
      }
    }
  }, [])

  // 资源加载监控 - 收缩状态下降低频率减少性能开销
  useEffect(() => {
    const updateInterval = isExpanded ? 1000 : 5000 // 收缩时 5 秒更新一次

    const interval = setInterval(() => {
      setResourceStats(globalResourceLoader.getStats())
    }, updateInterval)

    // 立即更新一次
    setResourceStats(globalResourceLoader.getStats())

    return () => clearInterval(interval)
  }, [isExpanded])

  // 动效扫描（展开时或切换到动效标签页时）- 🔧 降低扫描频率
  useEffect(() => {
    if (isExpanded && activeTab === 'animations') {
      scanAnimations()
      // 🔧 从 2 秒改为 3 秒，减少 Long Task 风险
      const interval = setInterval(scanAnimations, 3000)
      return () => clearInterval(interval)
    }
  }, [isExpanded, activeTab, scanAnimations])

  // 高亮动效元素
  useEffect(() => {
    if (highlightAnimations) {
      const style = document.createElement('style')
      style.id = 'perf-highlight-animations'
      style.textContent = `
        *[style*="animation"], *[class*="animate"] {
          outline: 2px solid #ff6b6b !important;
          outline-offset: 2px !important;
        }
        *[style*="transition"] {
          outline: 2px solid #4ecdc4 !important;
          outline-offset: 2px !important;
        }
        *[style*="transform"]:not([style*="animation"]) {
          outline: 2px solid #ffe66d !important;
          outline-offset: 2px !important;
        }
      `
      document.head.appendChild(style)

      return () => {
        document.getElementById('perf-highlight-animations')?.remove()
      }
    }
  }, [highlightAnimations])

  // 暂停所有动画
  useEffect(() => {
    if (pauseAllAnimations) {
      const style = document.createElement('style')
      style.id = 'perf-pause-animations'
      style.textContent = `
        *, *::before, *::after {
          animation-play-state: paused !important;
          transition: none !important;
        }
      `
      document.head.appendChild(style)

      return () => {
        document.getElementById('perf-pause-animations')?.remove()
      }
    }
  }, [pauseAllAnimations])

  // 快捷键
  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'M') {
        e.preventDefault()
        setIsExpanded(prev => !prev)
      }
      // Ctrl+Shift+A 切换动效高亮
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'A') {
        e.preventDefault()
        setHighlightAnimations(prev => !prev)
      }
    }

    window.addEventListener('keydown', handleKeyPress)
    return () => window.removeEventListener('keydown', handleKeyPress)
  }, [])

  const getFPSColor = (fps: number) => {
    if (fps >= 55)
      return 'text-green-400'
    if (fps >= 30)
      return 'text-yellow-400'
    return 'text-red-400'
  }

  const getMemoryColor = (percent?: number) => {
    if (!percent)
      return 'text-gray-400'
    if (percent < 70)
      return 'text-green-400'
    if (percent < 85)
      return 'text-yellow-400'
    return 'text-red-400'
  }

  const hasActivity = resourceStats.queued > 0 || resourceStats.active > 0
  const hasAnimationIssues = animationStats.expensiveAnimations.length > 0 || animationStats.infinite > 3

  return (
    <div
      data-perf-monitor
      className="fixed bottom-4 right-4 z-[9999] bg-black/90 backdrop-blur-md rounded-lg shadow-xl text-white text-xs font-mono"
      style={{ maxWidth: isExpanded ? '360px' : '280px' }}
    >
      {/* 紧凑模式 */}
      <div className="p-3">
        <div className="flex items-center justify-between mb-2">
          <div className="flex items-center gap-2">
            <span className="font-bold">⚡ 性能</span>
            <span className={`font-bold ${getFPSColor(metrics.fps)}`}>
              {metrics.fps}
              {' '}
              FPS
            </span>
            {hasAnimationIssues && (
              <span className="text-orange-400 animate-pulse" title="检测到可能影响性能的动效">
                ⚠️
              </span>
            )}
          </div>
          <button
            onClick={() => setIsExpanded(!isExpanded)}
            className="text-gray-400 hover:text-white transition-colors"
            title={isExpanded ? '收起 (Ctrl+Shift+M)' : '展开 (Ctrl+Shift+M)'}
          >
            {isExpanded ? '▼' : '▲'}
          </button>
        </div>

        {/* 紧凑信息 */}
        <div className="flex items-center gap-2 text-[10px] flex-wrap">
          <span className="text-gray-400">资源:</span>
          <span className={hasActivity ? 'text-yellow-400' : 'text-green-400'}>
            {resourceStats.queued}
            Q
          </span>
          <span className={resourceStats.active > 0 ? 'text-blue-400' : 'text-gray-500'}>
            {resourceStats.active}
            A
          </span>
          <span className="text-gray-500">
            {resourceStats.completed}
            ✓
          </span>
          {resourceStats.failed > 0 && (
            <span className="text-red-400">
              {resourceStats.failed}
              ✗
            </span>
          )}
          <span className="text-gray-500">|</span>
          <span className="text-gray-400">动效:</span>
          <span className={animationStats.running > 0 ? 'text-cyan-400' : 'text-gray-500'}>
            {animationStats.running}
            ▶
          </span>
          <span className={animationStats.infinite > 0 ? 'text-purple-400' : 'text-gray-500'}>
            {animationStats.infinite}
            ∞
          </span>
        </div>

        {/* 内存进度条 */}
        {metrics.memory && (
          <div className="mt-2">
            <div className="flex justify-between items-center text-[10px] mb-1">
              <span className="text-gray-400">内存</span>
              <span className={getMemoryColor(metrics.memory.usedPercent)}>
                {metrics.memory.usedPercent}
                %
              </span>
            </div>
            <div className="w-full bg-gray-700 rounded-full h-1 overflow-hidden">
              <div
                className={`h-full transition-all ${
                  metrics.memory.usedPercent < 70
                    ? 'bg-green-400'
                    : metrics.memory.usedPercent < 85
                      ? 'bg-yellow-400'
                      : 'bg-red-400'
                }`}
                data-width={metrics.memory.usedPercent}
              />
            </div>
          </div>
        )}
      </div>

      {/* 展开模式 */}
      {isExpanded && (
        <div className="border-t border-white/10">
          {/* 选项卡 */}
          <div className="flex border-b border-white/10">
            {(['overview', 'animations', 'js', 'layout'] as TabType[]).map(tab => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab)}
                className={`flex-1 py-2 px-2 text-[10px] transition-colors ${
                  activeTab === tab
                    ? 'bg-white/10 text-white'
                    : 'text-gray-400 hover:text-white'
                }`}
              >
                {tab === 'overview' && '📊 概览'}
                {tab === 'animations' && `🎬 CSS`}
                {tab === 'js' && `⚡ JS`}
                {tab === 'layout' && '📐 布局'}
              </button>
            ))}
          </div>

          <div className="p-3 space-y-3 max-h-[400px] overflow-y-auto">
            {/* 概览标签页 */}
            {activeTab === 'overview' && (
              <>
                <div>
                  <div className="font-bold mb-2 text-gray-300">性能指标</div>
                  <div className="space-y-1.5 text-[11px]">
                    {metrics.memory && (
                      <div className="flex justify-between">
                        <span className="text-gray-400">内存:</span>
                        <span className={getMemoryColor(metrics.memory.usedPercent)}>
                          {metrics.memory.used}
                          {' '}
                          /
                          {metrics.memory.limit}
                          {' '}
                          MB
                        </span>
                      </div>
                    )}
                    {metrics.renderTime > 0 && (
                      <div className="flex justify-between">
                        <span className="text-gray-400">FCP:</span>
                        <span className="text-blue-400">
                          {metrics.renderTime}
                          {' '}
                          ms
                        </span>
                      </div>
                    )}
                    {metrics.longTasks > 0 && (
                      <div className="flex justify-between">
                        <span className="text-gray-400">Long Tasks:</span>
                        <span className="text-orange-400">{metrics.longTasks}</span>
                      </div>
                    )}
                    <div className="flex justify-between">
                      <span className="text-gray-400">活跃动效:</span>
                      <span className={animationStats.running > 5 ? 'text-orange-400' : 'text-cyan-400'}>
                        {animationStats.running}
                        {' '}
                        个
                      </span>
                    </div>
                  </div>
                </div>

                <div>
                  <div className="font-bold mb-2 text-gray-300">资源队列</div>
                  <div className="grid grid-cols-2 gap-1.5 text-[10px]">
                    {Object.entries(resourceStats.queuedByPriority).map(([priority, count]) => (
                      <div key={priority} className="flex justify-between bg-white/5 rounded px-2 py-1">
                        <span className="text-gray-400">
                          {priority}
                          :
                        </span>
                        <span className={count > 0 ? 'text-yellow-400 font-bold' : 'text-gray-500'}>
                          {count}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>

                <div>
                  <div className="font-bold mb-2 text-gray-300">操作</div>
                  <div className="grid grid-cols-2 gap-1.5">
                    <button
                      className="px-2 py-1 bg-red-500/20 hover:bg-red-500/30 rounded text-[10px] transition-colors disabled:opacity-30"
                      onClick={() => {
                        globalResourceLoader.clear()
                        setResourceStats(globalResourceLoader.getStats())
                      }}
                      disabled={!hasActivity}
                    >
                      🗑️ 清空
                    </button>
                    <button
                      className="px-2 py-1 bg-blue-500/20 hover:bg-blue-500/30 rounded text-[10px] transition-colors"
                      onClick={() => {
                        globalResourceLoader.reset()
                        setResourceStats(globalResourceLoader.getStats())
                      }}
                    >
                      🔄 重置
                    </button>
                    <button
                      className="px-2 py-1 bg-purple-500/20 hover:bg-purple-500/30 rounded text-[10px] transition-colors col-span-2"
                      onClick={() => {
                        clearPlaylistCache()
                        clearLyricsCache()
                        setShowCacheCleared(true)
                        setTimeout(() => setShowCacheCleared(false), 2000)
                      }}
                    >
                      🗑️ 清除缓存
                    </button>
                  </div>
                  {showCacheCleared && (
                    <div className="mt-1.5 text-center text-green-400 text-[10px]">
                      ✓ 缓存已清除
                    </div>
                  )}
                </div>
              </>
            )}

            {/* 动效标签页 */}
            {activeTab === 'animations' && (
              <>
                {/* 动效概览 */}
                <div>
                  <div className="font-bold mb-2 text-gray-300">动效统计</div>
                  <div className="grid grid-cols-3 gap-1.5 text-[10px]">
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">CSS动画</div>
                      <div className={animationStats.total > 0 ? 'text-cyan-400 font-bold' : 'text-gray-500'}>
                        {animationStats.total}
                      </div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">运行中</div>
                      <div className={animationStats.running > 0 ? 'text-green-400 font-bold' : 'text-gray-500'}>
                        {animationStats.running}
                      </div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">无限循环</div>
                      <div className={animationStats.infinite > 3 ? 'text-orange-400 font-bold' : 'text-purple-400'}>
                        {animationStats.infinite}
                      </div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">过渡</div>
                      <div className="text-blue-400">{animationStats.cssTransitions}</div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">will-change</div>
                      <div className={animationStats.willChangeElements > 10 ? 'text-yellow-400' : 'text-gray-400'}>
                        {animationStats.willChangeElements}
                      </div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">transform</div>
                      <div className="text-gray-400">{animationStats.transformElements}</div>
                    </div>
                  </div>
                </div>

                {/* 可能有问题的动效 */}
                {animationStats.expensiveAnimations.length > 0 && (
                  <div>
                    <div className="font-bold mb-2 text-orange-400">⚠️ 可能影响性能</div>
                    <div className="space-y-1 text-[10px] max-h-[100px] overflow-y-auto">
                      {animationStats.expensiveAnimations.map((anim, i) => (
                        <div key={i} className="bg-orange-500/10 border border-orange-500/30 rounded px-2 py-1">
                          <div className="text-orange-300 font-medium truncate" title={anim.selector}>
                            {anim.selector}
                          </div>
                          <div className="text-orange-200/70 text-[9px]">
                            {anim.animationName}
                            {' '}
                            ·
                            {anim.duration}
                            {anim.isInfinite && ' · ∞'}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* 所有动效列表 */}
                <div>
                  <div className="font-bold mb-2 text-gray-300">
                    活跃动效
                    {' '}
                    {animationStats.cssAnimations.length > 0 && `(${animationStats.cssAnimations.length})`}
                  </div>
                  {animationStats.cssAnimations.length > 0
                    ? (
                        <div className="space-y-1 text-[10px] max-h-[150px] overflow-y-auto">
                          {animationStats.cssAnimations.map((anim, i) => (
                            <div
                              key={i}
                              className={`bg-white/5 rounded px-2 py-1 ${
                                anim.state === 'running' ? 'border-l-2 border-green-400' : 'border-l-2 border-gray-600'
                              }`}
                            >
                              <div className="flex items-center justify-between">
                                <span className="text-gray-300 truncate flex-1" title={anim.selector}>
                                  {anim.selector}
                                </span>
                                <span className={anim.state === 'running' ? 'text-green-400' : 'text-gray-500'}>
                                  {anim.state === 'running' ? '▶' : '⏸'}
                                </span>
                              </div>
                              <div className="text-gray-500 text-[9px]">
                                {anim.animationName}
                                {' '}
                                ·
                                {anim.duration}
                                {anim.isInfinite && <span className="text-purple-400 ml-1">∞</span>}
                              </div>
                            </div>
                          ))}
                        </div>
                      )
                    : (
                        <div className="text-gray-500 text-[10px] text-center py-2">
                          暂无活跃的 CSS 动画
                        </div>
                      )}
                </div>

                {/* 动效调试工具 */}
                <div>
                  <div className="font-bold mb-2 text-gray-300">调试工具</div>
                  <div className="grid grid-cols-2 gap-1.5">
                    <button
                      onClick={() => setHighlightAnimations(!highlightAnimations)}
                      className={`px-2 py-1.5 rounded text-[10px] transition-colors ${
                        highlightAnimations
                          ? 'bg-yellow-500/30 text-yellow-300 border border-yellow-500/50'
                          : 'bg-white/5 hover:bg-white/10 text-gray-300'
                      }`}
                    >
                      {highlightAnimations ? '🔍 高亮中' : '🔍 高亮动效'}
                    </button>
                    <button
                      onClick={() => setPauseAllAnimations(!pauseAllAnimations)}
                      className={`px-2 py-1.5 rounded text-[10px] transition-colors ${
                        pauseAllAnimations
                          ? 'bg-red-500/30 text-red-300 border border-red-500/50'
                          : 'bg-white/5 hover:bg-white/10 text-gray-300'
                      }`}
                    >
                      {pauseAllAnimations ? '▶️ 恢复' : '⏸️ 暂停全部'}
                    </button>
                    <button
                      onClick={scanAnimations}
                      className="px-2 py-1.5 bg-white/5 hover:bg-white/10 rounded text-[10px] transition-colors text-gray-300 col-span-2"
                    >
                      🔄 刷新扫描
                    </button>
                  </div>
                  <div className="text-[9px] text-gray-500 mt-2">
                    提示：Ctrl+Shift+A 切换高亮
                  </div>
                </div>
              </>
            )}

            {/* JS 动画标签页 */}
            {activeTab === 'js' && (
              <>
                {/* JS 动画概览 */}
                <div>
                  <div className="font-bold mb-2 text-gray-300">JS 动画统计</div>
                  <div className="grid grid-cols-3 gap-1.5 text-[10px]">
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">RAF 活跃</div>
                      <div className={jsAnimationStats.activeRAFs > 3 ? 'text-orange-400 font-bold' : 'text-cyan-400'}>
                        {jsAnimationStats.activeRAFs}
                      </div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">RAF/秒</div>
                      <div className={jsAnimationStats.rafCallsPerSecond > 120 ? 'text-orange-400 font-bold' : 'text-gray-400'}>
                        {jsAnimationStats.rafCallsPerSecond}
                      </div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">Web Anim</div>
                      <div className={jsAnimationStats.webAnimationsRunning > 5 ? 'text-yellow-400 font-bold' : 'text-green-400'}>
                        {jsAnimationStats.webAnimationsRunning}
                        /
                        {jsAnimationStats.webAnimations}
                      </div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                      <div className="text-gray-400">Framer</div>
                      <div className={jsAnimationStats.framerMotionElements > 5 ? 'text-purple-400 font-bold' : 'text-purple-400'}>
                        {jsAnimationStats.framerMotionElements}
                      </div>
                    </div>
                    <div className="bg-white/5 rounded px-2 py-1.5 text-center col-span-2">
                      <div className="text-gray-400">JS 动画总计</div>
                      <div className={jsAnimationStats.totalJsAnimations > 10 ? 'text-orange-400 font-bold' : 'text-green-400'}>
                        {jsAnimationStats.totalJsAnimations}
                      </div>
                    </div>
                  </div>
                </div>

                {/* 运行中的 Web Animations 列表 */}
                {jsAnimationStats.webAnimationDetails.length > 0 && (
                  <div>
                    <div className="font-bold mb-2 text-gray-300">
                      运行中的动画 (
                      {jsAnimationStats.webAnimationDetails.length}
                      )
                    </div>
                    <div className="space-y-1 text-[10px] max-h-[100px] overflow-y-auto">
                      {jsAnimationStats.webAnimationDetails.map(anim => (
                        <div
                          key={anim.id}
                          className="bg-white/5 rounded px-2 py-1 border-l-2 border-green-400"
                        >
                          <div className="flex items-center justify-between">
                            <span className="text-gray-300 truncate flex-1" title={anim.target}>
                              {anim.target}
                            </span>
                            <span className="text-green-400 text-[9px]">▶</span>
                          </div>
                          <div className="text-gray-500 text-[9px]">
                            {anim.name}
                            {anim.duration && (
                              <span className="ml-1">
                                ·
                                {anim.duration}
                                ms
                              </span>
                            )}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* 性能警告 */}
                {(jsAnimationStats.activeRAFs > 5
                  || jsAnimationStats.rafCallsPerSecond > 180
                  || jsAnimationStats.totalJsAnimations > 15) && (
                  <div>
                    <div className="font-bold mb-2 text-orange-400">⚠️ 潜在问题</div>
                    <div className="space-y-1 text-[10px]">
                      {jsAnimationStats.activeRAFs > 5 && (
                        <div className="bg-orange-500/10 text-orange-300 rounded px-2 py-1">
                          同时运行
                          {' '}
                          {jsAnimationStats.activeRAFs}
                          {' '}
                          个 RAF，考虑合并动画循环
                        </div>
                      )}
                      {jsAnimationStats.rafCallsPerSecond > 180 && (
                        <div className="bg-red-500/10 text-red-300 rounded px-2 py-1">
                          RAF 调用频率过高 (
                          {jsAnimationStats.rafCallsPerSecond}
                          /s)，可能存在递归问题
                        </div>
                      )}
                      {jsAnimationStats.totalJsAnimations > 15 && (
                        <div className="bg-yellow-500/10 text-yellow-300 rounded px-2 py-1">
                          JS 动画过多 (
                          {jsAnimationStats.totalJsAnimations}
                          )，注意性能影响
                        </div>
                      )}
                    </div>
                  </div>
                )}

                {/* 指标说明 */}
                <div>
                  <div className="font-bold mb-2 text-gray-300">指标说明</div>
                  <div className="space-y-1 text-[10px] text-gray-400">
                    <div className="flex items-start gap-1">
                      <span className="text-cyan-400 shrink-0">RAF:</span>
                      <span>requestAnimationFrame 回调</span>
                    </div>
                    <div className="flex items-start gap-1">
                      <span className="text-green-400 shrink-0">Web Anim:</span>
                      <span>Web Animations API (CSS/WAAPI)</span>
                    </div>
                    <div className="flex items-start gap-1">
                      <span className="text-purple-400 shrink-0">Framer:</span>
                      <span>Framer Motion 过渡元素</span>
                    </div>
                  </div>
                  <div className="text-[9px] text-gray-500 mt-2">
                    理想 RAF/秒 ≈ 60 (匹配刷新率)
                  </div>
                </div>

                {/* 动画调度器状态 */}
                <div>
                  <div className="font-bold mb-2 text-gray-300">🎬 动画协调器</div>
                  {(() => {
                    const status = coordinator.getConcurrencyStatus()
                    return (
                      <div className="space-y-2">
                        <div className="grid grid-cols-3 gap-1.5 text-[10px]">
                          <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                            <div className="text-gray-400">活跃</div>
                            <div className={status.activeSlots >= status.maxConcurrent ? 'text-orange-400 font-bold' : 'text-cyan-400'}>
                              {status.activeSlots}
                            </div>
                          </div>
                          <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                            <div className="text-gray-400">队列</div>
                            <div className={status.waitingQueue > 5 ? 'text-yellow-400 font-bold' : 'text-gray-400'}>
                              {status.waitingQueue}
                            </div>
                          </div>
                          <div className="bg-white/5 rounded px-2 py-1.5 text-center">
                            <div className="text-gray-400">上限</div>
                            <div className={status.inBurstMode ? 'text-green-400' : 'text-gray-400'}>
                              {status.maxConcurrent}
                              {status.inBurstMode && ' 🚀'}
                            </div>
                          </div>
                        </div>
                        <div className="flex gap-1">
                          <button
                            onClick={() => configureAnimationCoordinator({ baseConcurrent: 6, burstConcurrent: 16 })}
                            className="flex-1 px-2 py-1 bg-orange-500/20 hover:bg-orange-500/30 rounded text-[9px] text-orange-300 transition-colors"
                          >
                            节能 (6/16)
                          </button>
                          <button
                            onClick={() => configureAnimationCoordinator({ baseConcurrent: 12, burstConcurrent: 32 })}
                            className="flex-1 px-2 py-1 bg-white/5 hover:bg-white/10 rounded text-[9px] text-gray-300 transition-colors"
                          >
                            默认 (12/32)
                          </button>
                          <button
                            onClick={() => configureAnimationCoordinator({ baseConcurrent: 24, burstConcurrent: 48 })}
                            className="flex-1 px-2 py-1 bg-green-500/20 hover:bg-green-500/30 rounded text-[9px] text-green-300 transition-colors"
                          >
                            性能 (24/48)
                          </button>
                        </div>
                      </div>
                    )
                  })()}
                </div>

                {/* 优化建议 */}
                <div>
                  <div className="font-bold mb-2 text-gray-300">快速优化</div>
                  <div className="space-y-1.5 text-[10px]">
                    {jsAnimationStats.activeRAFs > 3 && (
                      <div className="bg-purple-500/10 text-purple-300 rounded px-2 py-1">
                        💡 使用单一 RAF 循环管理多个动画
                      </div>
                    )}
                    {jsAnimationStats.framerMotionElements > 10 && (
                      <div className="bg-blue-500/10 text-blue-300 rounded px-2 py-1">
                        💡 为 Framer Motion 元素添加 layout=
                        {false}
                      </div>
                    )}
                    {jsAnimationStats.webAnimationsRunning > 10 && (
                      <div className="bg-orange-500/10 text-orange-300 rounded px-2 py-1">
                        💡 使用 IntersectionObserver 暂停视口外动画
                      </div>
                    )}
                    {jsAnimationStats.totalJsAnimations <= 10 && jsAnimationStats.rafCallsPerSecond <= 120 && (
                      <div className="bg-green-500/10 text-green-300 rounded px-2 py-1">
                        ✅ JS 动画性能良好
                      </div>
                    )}
                  </div>
                </div>
              </>
            )}

            {/* 布局标签页 */}
            {activeTab === 'layout' && (
              <>
                <div>
                  <div className="font-bold mb-2 text-gray-300">布局性能</div>
                  <div className="space-y-1.5 text-[11px]">
                    <div className="flex justify-between">
                      <span className="text-gray-400">布局偏移 (CLS):</span>
                      <span className={layoutStats.lastLayoutShift > 0.1 ? 'text-orange-400' : 'text-green-400'}>
                        {layoutStats.lastLayoutShift.toFixed(4)}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-400">重排次数:</span>
                      <span className={layoutStats.reflows > 10 ? 'text-yellow-400' : 'text-gray-400'}>
                        {layoutStats.reflows}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-400">Long Tasks:</span>
                      <span className={metrics.longTasks > 5 ? 'text-red-400' : 'text-gray-400'}>
                        {metrics.longTasks}
                      </span>
                    </div>
                  </div>
                </div>

                <div>
                  <div className="font-bold mb-2 text-gray-300">GPU 层统计</div>
                  <div className="space-y-1.5 text-[11px]">
                    <div className="flex justify-between">
                      <span className="text-gray-400">will-change 元素:</span>
                      <span className={animationStats.willChangeElements > 20 ? 'text-orange-400' : 'text-gray-400'}>
                        {animationStats.willChangeElements}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-400">transform 元素:</span>
                      <span className="text-gray-400">{animationStats.transformElements}</span>
                    </div>
                  </div>
                  {animationStats.willChangeElements > 20 && (
                    <div className="mt-2 text-[9px] text-orange-400 bg-orange-500/10 rounded px-2 py-1">
                      ⚠️ will-change 过多可能导致内存问题
                    </div>
                  )}
                </div>

                <div>
                  <div className="font-bold mb-2 text-gray-300">优化建议</div>
                  <div className="space-y-1 text-[10px]">
                    {metrics.longTasks > 5 && (
                      <div className="bg-red-500/10 text-red-300 rounded px-2 py-1">
                        💡 考虑使用 Web Worker 处理耗时任务
                      </div>
                    )}
                    {animationStats.infinite > 5 && (
                      <div className="bg-orange-500/10 text-orange-300 rounded px-2 py-1">
                        💡 减少无限循环动画，使用 IntersectionObserver 按需播放
                      </div>
                    )}
                    {animationStats.willChangeElements > 20 && (
                      <div className="bg-yellow-500/10 text-yellow-300 rounded px-2 py-1">
                        💡 减少 will-change 使用，仅在动画时添加
                      </div>
                    )}
                    {layoutStats.lastLayoutShift > 0.1 && (
                      <div className="bg-orange-500/10 text-orange-300 rounded px-2 py-1">
                        💡 为图片/动态内容预留空间避免布局偏移
                      </div>
                    )}
                    {metrics.longTasks <= 5
                      && animationStats.infinite <= 5
                      && animationStats.willChangeElements <= 20
                      && layoutStats.lastLayoutShift <= 0.1 && (
                      <div className="bg-green-500/10 text-green-300 rounded px-2 py-1">
                        ✅ 当前页面性能良好
                      </div>
                    )}
                  </div>
                </div>
              </>
            )}
          </div>

          <div className="text-[9px] text-gray-500 text-center py-2 border-t border-white/10">
            按 Ctrl+Shift+M 切换面板
          </div>
        </div>
      )}
    </div>
  )
}
