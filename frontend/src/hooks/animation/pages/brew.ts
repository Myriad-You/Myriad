/**
 * Brew 页专用调度器 Hooks
 *
 * 性能优化版本 (WebKit 优化):
 * - 使用 CSS 变量替代内联样式减少 style recalc
 * - 使用 will-change: transform, opacity 提示 GPU 加速
 * - 避免在动画过程中重复创建对象
 * - 使用 ref 追踪动画状态，避免不必要的 re-render
 *
 * @example
 * ```tsx
 * // 在 Brew.tsx 中
 * import { useBrewScheduler } from '@hooks/animation/pages/brew';
 *
 * function Brew() {
 *   useBrewScheduler();
 *   return <BrewContent />;
 * }
 *
 * // 在 BrewSourceGrid.tsx 中
 * import { useBrewCardStagger } from '@hooks/animation/pages/brew';
 *
 * function SourceCard({ index }) {
 *   const { canAnimate, animateClassName } = useBrewCardStagger(index, 'source');
 *   return (
 *     <div className={animateClassName}>...</div>
 *   );
 * }
 * ```
 */

import type { AnimationConfig } from '../../useAnimationLevel'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useAnimationLevel } from '../../useAnimationLevel'

const PAGE_ID = 'brew'

// 动画配置 - 减少延迟提升响应速度
const STAGGER_CONFIG = {
  source: {
    baseDelay: 25, // 订阅源卡片延迟 (降低)
    maxDelay: 200, // 最大延迟 (降低)
  },
  item: {
    baseDelay: 20, // 文章卡片延迟 (降低)
    maxDelay: 150,
  },
}

// 页面级别的动画状态追踪，避免卡片重复触发动画
let pageAnimationBatchId = 0

// ==================== 页面初始化 ====================

/**
 * Brew 页面调度器初始化 Hook
 * 在页面组件最顶层调用一次
 *
 * 注意：startPage('brew') 由 useRouteScheduler 统一调用
 * 这里只负责 Brew 特有的初始化逻辑（如动画批次 ID）
 */
export function useBrewScheduler(): void {
  useEffect(() => {
    // 每次页面挂载时增加批次 ID，让所有卡片知道这是新的一批动画
    // 注意：startPage 已由 useRouteScheduler 调用，这里不再重复
    pageAnimationBatchId++
  }, [])
}

// ==================== 动画配置 Hook ====================

// 缓存的动画配置，避免每次调用都创建新对象
const ANIM_CONFIG_CACHE = new Map<string, ReturnType<typeof useBrewAnimationConfig>>()

/**
 * 获取 Brew 专用的动画配置
 * 根据设备性能自动调整
 */
export function useBrewAnimationConfig(): AnimationConfig & {
  /** 是否启用交错动画 */
  enableStagger: boolean
  /** 是否启用悬浮效果 */
  enableHover: boolean
  /** 卡片动画时长 */
  cardDuration: number
  /** 阅读器动画时长 */
  readerDuration: number
} {
  const baseConfig = useAnimationLevel()

  return useMemo(() => {
    const cacheKey = baseConfig.level
    const cached = ANIM_CONFIG_CACHE.get(cacheKey)
    if (cached && cached.level === baseConfig.level) {
      return cached
    }

    const isNone = baseConfig.level === 'none'
    const isLight = baseConfig.level === 'light'

    const config = {
      ...baseConfig,
      enableStagger: !isNone,
      enableHover: !isNone,
      cardDuration: isNone ? 0 : isLight ? 120 : 200,
      readerDuration: isNone ? 0 : isLight ? 100 : 200,
    }

    ANIM_CONFIG_CACHE.set(cacheKey, config)
    return config
  }, [baseConfig])
}

// ==================== Stagger Animation Hook ====================

interface BrewStaggerResult {
  /** 是否可以开始动画 */
  canAnimate: boolean
  /** 延迟时间（毫秒） */
  delay: number
  /** 动画完成回调 */
  onComplete: () => void
  /** 动画配置 */
  animConfig: AnimationConfig
  /** CSS 样式（用于初始状态） */
  initialStyle: React.CSSProperties
  /** CSS 样式（用于动画后状态） */
  animateStyle: React.CSSProperties
}

// 预计算的静态样式，避免每次渲染创建新对象
const INITIAL_STYLE_HIDDEN: React.CSSProperties = {
  opacity: 0,
  transform: 'translateY(8px)',
}

const INITIAL_STYLE_VISIBLE: React.CSSProperties = {
  opacity: 1,
  transform: 'translateY(0)',
}

const EMPTY_STYLE: React.CSSProperties = {}

/**
 * Brew 卡片交错动画 Hook
 *
 * 性能优化版本：
 * - 使用 ref 追踪动画批次，避免重复动画
 * - 预计算静态样式对象
 * - 减少 state 更新次数
 *
 * @param index - 卡片在列表中的索引
 * @param type - 卡片类型：'source' 订阅源 | 'item' 文章
 */
export function useBrewCardStagger(
  index: number,
  type: 'source' | 'item' = 'source',
): BrewStaggerResult {
  const animConfig = useAnimationLevel()
  const config = STAGGER_CONFIG[type]

  // 动画级别为 none 时直接显示
  const isDisabled = animConfig.level === 'none'

  // 使用 ref 追踪已处理的动画批次，避免重复动画
  const lastBatchIdRef = useRef(0)
  const hasAnimatedRef = useRef(false)

  // 计算延迟
  const delay = useMemo(() => {
    if (isDisabled)
      return 0
    return Math.min(index * config.baseDelay, config.maxDelay)
  }, [index, config.baseDelay, config.maxDelay, isDisabled])

  // 状态：是否可以开始动画
  const [canAnimate, setCanAnimate] = useState(() => {
    // 如果已经动画过且是同一批次，直接显示
    if (hasAnimatedRef.current && lastBatchIdRef.current === pageAnimationBatchId) {
      return true
    }
    return isDisabled
  })

  // 动画完成回调
  const onComplete = useCallback(() => {
    hasAnimatedRef.current = true
  }, [])

  useEffect(() => {
    // 动画禁用时直接完成
    if (isDisabled) {
      setCanAnimate(true)
      hasAnimatedRef.current = true
      return
    }

    // 检查是否是新的动画批次
    if (lastBatchIdRef.current === pageAnimationBatchId && hasAnimatedRef.current) {
      // 同一批次且已动画过，直接显示
      setCanAnimate(true)
      return
    }

    // 更新批次 ID
    lastBatchIdRef.current = pageAnimationBatchId

    // 使用单个 RAF 来批量处理，减少回流
    let timeoutId: ReturnType<typeof setTimeout>

    // 直接使用 setTimeout，避免 RAF 嵌套
    timeoutId = setTimeout(() => {
      setCanAnimate(true)
      hasAnimatedRef.current = true
    }, delay)

    return () => {
      clearTimeout(timeoutId)
    }
  }, [delay, isDisabled])

  // 使用预计算的样式，避免每次渲染创建新对象
  const animateStyle = useMemo<React.CSSProperties>(() => {
    if (isDisabled)
      return EMPTY_STYLE

    // 使用 CSS transition 类名而不是内联样式来优化性能
    const duration = animConfig.level === 'light' ? 150 : 250

    if (canAnimate) {
      return {
        ...INITIAL_STYLE_VISIBLE,
        transition: `opacity ${duration}ms ease-out, transform ${duration}ms ease-out`,
      }
    }

    return {
      ...INITIAL_STYLE_HIDDEN,
      transition: `opacity ${duration}ms ease-out, transform ${duration}ms ease-out`,
    }
  }, [isDisabled, canAnimate, animConfig.level])

  // initialStyle 保持稳定引用
  const initialStyle = isDisabled ? EMPTY_STYLE : INITIAL_STYLE_HIDDEN

  return {
    canAnimate,
    delay,
    onComplete,
    animConfig,
    initialStyle,
    animateStyle,
  }
}

// ==================== 导出动画预设 ====================

/**
 * Brew 卡片动画预设
 * 用于 framer-motion 或 CSS 动画
 */
export const brewAnimationPresets = {
  /** 卡片入场动画 */
  cardEnter: {
    initial: { opacity: 0, y: 16 },
    animate: { opacity: 1, y: 0 },
    exit: { opacity: 0, y: -8 },
  },
  /** 阅读器入场动画 - WebKit 优化：移除 scale，仅用 opacity + translateY */
  readerEnter: {
    initial: { opacity: 0, y: 40 },
    animate: { opacity: 1, y: 0 },
    exit: { opacity: 0, y: 40 },
  },
  /** 淡入淡出 */
  fade: {
    initial: { opacity: 0 },
    animate: { opacity: 1 },
    exit: { opacity: 0 },
  },
} as const

/**
 * 获取动画 transition 配置
 */
export function getBrewTransition(
  animConfig: AnimationConfig,
  type: 'card' | 'reader' | 'fade' = 'card',
): { duration: number, ease: [number, number, number, number] } {
  const durations = {
    card: animConfig.level === 'none' ? 0 : animConfig.level === 'light' ? 0.15 : 0.25,
    reader: animConfig.level === 'none' ? 0 : animConfig.level === 'light' ? 0.2 : 0.4,
    fade: animConfig.level === 'none' ? 0 : animConfig.level === 'light' ? 0.1 : 0.2,
  }

  return {
    duration: durations[type],
    ease: [0.16, 1, 0.3, 1], // spring-like easing
  }
}

// ==================== 页面清理 ====================

/**
 * 清理 Brew 页面资源
 * 在路由离开时调用，重置动画批次状态
 */
export function cleanupBrew(): void {
  // 重置动画批次，下次进入页面时重新触发入场动画
  // 注意：不重置 pageAnimationBatchId，让它持续递增
  // 这样每次进入页面都是新的批次，卡片会重新动画
  ANIM_CONFIG_CACHE.clear()
}
