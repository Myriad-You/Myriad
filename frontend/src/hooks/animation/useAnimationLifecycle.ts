/**
 * 动画资源生命周期管理 Hook
 *
 * 提供精确的动画资源控制：
 * 1. 动画开始前：不分配资源
 * 2. 动画进行中：分配 GPU 层和过渡
 * 3. 动画完成后：立即释放所有资源
 *
 * @module useAnimationLifecycle
 * @version 1.0
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { coordinator } from './coordinator'
import { AnimationPriority, AnimationState } from './types'

/** 动画生命周期阶段 */
export enum AnimationLifecyclePhase {
  /** 空闲 - 不分配任何资源 */
  IDLE = 'idle',
  /** 准备中 - 等待调度 */
  PREPARING = 'preparing',
  /** 活跃 - 正在播放动画 */
  ACTIVE = 'active',
  /** 完成 - 动画结束，资源已释放 */
  COMPLETED = 'completed',
}

/** 动画生命周期配置 */
export interface AnimationLifecycleOptions {
  /** 动画 ID（可选，自动生成） */
  id?: string
  /** 动画优先级 */
  priority?: AnimationPriority
  /** 延迟时间（毫秒） */
  delay?: number
  /** 持续时间（毫秒） */
  duration?: number
  /** 是否自动开始 */
  autoStart?: boolean
  /** 是否等待页面就绪 */
  waitForPage?: boolean
  /** 动画完成回调 */
  onComplete?: () => void
  /** 阶段变化回调 */
  onPhaseChange?: (phase: AnimationLifecyclePhase) => void
}

/** 动画生命周期返回值 */
export interface AnimationLifecycleResult {
  /** 当前阶段 */
  phase: AnimationLifecyclePhase
  /** 是否可以播放动画 */
  canAnimate: boolean
  /** 是否已完成（可用于条件渲染） */
  isComplete: boolean
  /** 是否空闲（未进入动画） */
  isIdle: boolean
  /** 手动开始动画 */
  start: () => void
  /** 手动完成动画 */
  complete: () => void
  /** 重置到空闲状态 */
  reset: () => void
  /** 动画 ID */
  animationId: string
  /** CSS 类名（根据阶段自动切换） */
  className: string
  /** 内联样式（根据阶段自动计算） */
  style: React.CSSProperties
}

let lifecycleIdCounter = 0

/**
 * 动画生命周期管理 Hook
 *
 * @example
 * ```tsx
 * function AnimatedCard() {
 *   const { canAnimate, isComplete, className, style } = useAnimationLifecycle({
 *     duration: 300,
 *     autoStart: true,
 *   });
 *
 *   return (
 *     <div
 *       className={className}
 *       style={{
 *         opacity: canAnimate ? 1 : 0,
 *         transform: canAnimate ? 'none' : 'translateY(20px)',
 *         // 完成后不设置 transition，释放资源
 *         transition: isComplete ? undefined : 'all 0.3s ease',
 *         ...style,
 *       }}
 *     >
 *       Content
 *     </div>
 *   );
 * }
 * ```
 */
export function useAnimationLifecycle(
  options: AnimationLifecycleOptions = {},
): AnimationLifecycleResult {
  const {
    id: providedId,
    priority = AnimationPriority.ELEMENT,
    delay = 0,
    duration = 300,
    autoStart = true,
    waitForPage = true,
    onComplete,
    onPhaseChange,
  } = options

  // 生成稳定的 ID
  const idRef = useRef<string>(providedId || '')
  if (!idRef.current) {
    idRef.current = `lifecycle-${++lifecycleIdCounter}`
  }
  const animationId = idRef.current

  // 阶段状态
  const [phase, setPhase] = useState<AnimationLifecyclePhase>(
    AnimationLifecyclePhase.IDLE,
  )

  const mountedRef = useRef(true)
  const startTimeRef = useRef<number>(0)
  const completionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // 清理定时器
  const clearCompletionTimer = useCallback(() => {
    if (completionTimerRef.current) {
      clearTimeout(completionTimerRef.current)
      completionTimerRef.current = null
    }
  }, [])

  // 更新阶段
  const updatePhase = useCallback((newPhase: AnimationLifecyclePhase) => {
    setPhase((prev) => {
      if (prev !== newPhase) {
        onPhaseChange?.(newPhase)
        return newPhase
      }
      return prev
    })
  }, [onPhaseChange])

  // 完成动画
  const complete = useCallback(() => {
    clearCompletionTimer()
    updatePhase(AnimationLifecyclePhase.COMPLETED)
    coordinator.markCompleted(animationId)
    onComplete?.()
  }, [animationId, clearCompletionTimer, updatePhase, onComplete])

  // 开始动画
  const start = useCallback(() => {
    if (phase !== AnimationLifecyclePhase.IDLE)
      return

    updatePhase(AnimationLifecyclePhase.PREPARING)

    // 调度动画
    coordinator.schedule({
      id: animationId,
      priority,
      delay,
    })

    // 订阅状态变化
    const unsubscribe = coordinator.subscribe(animationId, (state) => {
      if (!mountedRef.current)
        return

      if (state === AnimationState.READY || state === AnimationState.RUNNING) {
        startTimeRef.current = performance.now()
        updatePhase(AnimationLifecyclePhase.ACTIVE)

        // 设置完成定时器
        clearCompletionTimer()
        completionTimerRef.current = setTimeout(() => {
          if (mountedRef.current) {
            complete()
          }
        }, duration)
      }
      else if (state === AnimationState.SKIPPED) {
        // 被跳过，直接完成
        complete()
      }
    })

    // 返回清理函数（存储到 ref 以便后续清理）
    return unsubscribe
  }, [phase, animationId, priority, delay, duration, updatePhase, clearCompletionTimer, complete])

  // 重置
  const reset = useCallback(() => {
    clearCompletionTimer()
    updatePhase(AnimationLifecyclePhase.IDLE)
    // 注意：不清理协调器状态，让它自然过期
  }, [clearCompletionTimer, updatePhase])

  // 自动开始
  useEffect(() => {
    mountedRef.current = true

    if (autoStart) {
      // 如果需要等待页面，注册回调
      if (waitForPage) {
        const unsub = coordinator.onPageReady(() => {
          if (mountedRef.current) {
            start()
          }
        })
        return () => {
          unsub()
          mountedRef.current = false
          clearCompletionTimer()
        }
      }
      else {
        start()
      }
    }

    return () => {
      mountedRef.current = false
      clearCompletionTimer()
    }
  }, [autoStart, waitForPage, start, clearCompletionTimer])

  // 计算派生状态
  const canAnimate = phase === AnimationLifecyclePhase.ACTIVE
    || phase === AnimationLifecyclePhase.COMPLETED
  const isComplete = phase === AnimationLifecyclePhase.COMPLETED
  const isIdle = phase === AnimationLifecyclePhase.IDLE

  // 计算 CSS 类名
  const className = useMemo(() => {
    switch (phase) {
      case AnimationLifecyclePhase.IDLE:
        return 'anim-waiting'
      case AnimationLifecyclePhase.PREPARING:
        return 'anim-prepare'
      case AnimationLifecyclePhase.ACTIVE:
        return 'anim-active'
      case AnimationLifecyclePhase.COMPLETED:
        return 'anim-done'
      default:
        return ''
    }
  }, [phase])

  // 计算内联样式
  const style = useMemo((): React.CSSProperties => {
    switch (phase) {
      case AnimationLifecyclePhase.IDLE:
        return {
          opacity: 0,
          visibility: 'hidden',
        }
      case AnimationLifecyclePhase.PREPARING:
        return {
          opacity: 0,
          willChange: 'opacity, transform',
        }
      case AnimationLifecyclePhase.ACTIVE:
        return {
          willChange: 'opacity, transform',
        }
      case AnimationLifecyclePhase.COMPLETED:
        // 完成后不设置任何动画相关属性
        return {}
      default:
        return {}
    }
  }, [phase])

  return {
    phase,
    canAnimate,
    isComplete,
    isIdle,
    start,
    complete,
    reset,
    animationId,
    className,
    style,
  }
}

/**
 * 批量动画生命周期管理
 * 用于列表等需要交错动画的场景
 */
export interface BatchAnimationOptions {
  /** 动画数量 */
  count: number
  /** 交错延迟（毫秒） */
  staggerDelay?: number
  /** 单个动画持续时间（毫秒） */
  duration?: number
  /** 是否自动开始 */
  autoStart?: boolean
  /** 分组 ID */
  groupId?: string
}

export interface BatchAnimationResult {
  /** 获取指定索引的动画状态 */
  getItemState: (index: number) => {
    canAnimate: boolean
    isComplete: boolean
    className: string
  }
  /** 所有动画是否完成 */
  allComplete: boolean
  /** 已完成的动画数量 */
  completedCount: number
  /** 开始所有动画 */
  startAll: () => void
  /** 重置所有动画 */
  resetAll: () => void
}

/**
 * 批量动画管理 Hook
 *
 * @example
 * ```tsx
 * function AnimatedList({ items }) {
 *   const { getItemState, allComplete } = useBatchAnimationLifecycle({
 *     count: items.length,
 *     staggerDelay: 50,
 *     duration: 300,
 *   });
 *
 *   return items.map((item, index) => {
 *     const { canAnimate, className } = getItemState(index);
 *     return (
 *       <div
 *         key={item.id}
 *         className={className}
 *         style={{ opacity: canAnimate ? 1 : 0 }}
 *       >
 *         {item.content}
 *       </div>
 *     );
 *   });
 * }
 * ```
 */
export function useBatchAnimationLifecycle(
  options: BatchAnimationOptions,
): BatchAnimationResult {
  const {
    count,
    staggerDelay = 50,
    duration = 300,
    autoStart = true,
    groupId = 'batch',
  } = options

  // 存储每个元素的完成状态
  const [completedSet, setCompletedSet] = useState<Set<number>>(new Set())
  const [activeSet, setActiveSet] = useState<Set<number>>(new Set())
  const [started, setStarted] = useState(false)

  const mountedRef = useRef(true)
  const timersRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map())

  // 清理所有定时器
  const clearAllTimers = useCallback(() => {
    for (const timer of timersRef.current.values()) {
      clearTimeout(timer)
    }
    timersRef.current.clear()
  }, [])

  // 开始所有动画
  const startAll = useCallback(() => {
    if (started)
      return
    setStarted(true)

    for (let i = 0; i < count; i++) {
      const itemDelay = i * staggerDelay

      // 延迟激活
      const activateTimer = setTimeout(() => {
        if (!mountedRef.current)
          return
        setActiveSet(prev => new Set(prev).add(i))

        // 延迟完成
        const completeTimer = setTimeout(() => {
          if (!mountedRef.current)
            return
          setCompletedSet(prev => new Set(prev).add(i))
        }, duration)

        timersRef.current.set(i * 2 + 1, completeTimer)
      }, itemDelay)

      timersRef.current.set(i * 2, activateTimer)
    }
  }, [started, count, staggerDelay, duration])

  // 重置所有
  const resetAll = useCallback(() => {
    clearAllTimers()
    setCompletedSet(new Set())
    setActiveSet(new Set())
    setStarted(false)
  }, [clearAllTimers])

  // 自动开始
  useEffect(() => {
    mountedRef.current = true

    if (autoStart) {
      const unsub = coordinator.onPageReady(() => {
        if (mountedRef.current) {
          startAll()
        }
      })
      return () => {
        unsub()
        mountedRef.current = false
        clearAllTimers()
      }
    }

    return () => {
      mountedRef.current = false
      clearAllTimers()
    }
  }, [autoStart, startAll, clearAllTimers])

  // 获取单个元素状态
  const getItemState = useCallback((index: number) => {
    const isActive = activeSet.has(index)
    const isComplete = completedSet.has(index)

    let className = 'anim-waiting'
    if (isComplete) {
      className = 'anim-done'
    }
    else if (isActive) {
      className = 'anim-active'
    }

    return {
      canAnimate: isActive || isComplete,
      isComplete,
      className,
    }
  }, [activeSet, completedSet])

  return {
    getItemState,
    allComplete: completedSet.size >= count,
    completedCount: completedSet.size,
    startAll,
    resetAll,
  }
}

export default useAnimationLifecycle
