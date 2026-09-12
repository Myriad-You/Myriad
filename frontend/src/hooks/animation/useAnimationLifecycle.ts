import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { coordinator } from './coordinator'

import { AnimationPriority, AnimationState } from './types'

export enum AnimationLifecyclePhase {
  IDLE = 'idle',
  PREPARING = 'preparing',
  ACTIVE = 'active',
  COMPLETED = 'completed',
}

export interface AnimationLifecycleOptions {
  id?: string
  priority?: AnimationPriority
  delay?: number
  duration?: number
  autoStart?: boolean
  waitForPage?: boolean
  onComplete?: () => void
  onPhaseChange?: (phase: AnimationLifecyclePhase) => void
}

export interface AnimationLifecycleResult {
  phase: AnimationLifecyclePhase
  canAnimate: boolean
  isComplete: boolean
  isIdle: boolean
  start: () => void
  complete: () => void
  reset: () => void
  animationId: string
  className: string
  style: React.CSSProperties
}

let lifecycleIdCounter = 0

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

  const idRef = useRef<string>(providedId || '')
  if (!idRef.current) {
    idRef.current = `lifecycle-${++lifecycleIdCounter}`
  }
  const animationId = idRef.current

  const [phase, setPhase] = useState<AnimationLifecyclePhase>(
    AnimationLifecyclePhase.IDLE,
  )

  const mountedRef = useRef(true)
  const startTimeRef = useRef<number>(0)
  const completionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const clearCompletionTimer = useCallback(() => {
    if (completionTimerRef.current) {
      clearTimeout(completionTimerRef.current)
      completionTimerRef.current = null
    }
  }, [])

  const updatePhase = useCallback(
    (newPhase: AnimationLifecyclePhase) => {
      setPhase((prev) => {
        if (prev !== newPhase) {
          onPhaseChange?.(newPhase)
          return newPhase
        }
        return prev
      })
    },
    [onPhaseChange],
  )

  const complete = useCallback(() => {
    clearCompletionTimer()
    updatePhase(AnimationLifecyclePhase.COMPLETED)
    coordinator.markCompleted(animationId)
    onComplete?.()
  }, [animationId, clearCompletionTimer, updatePhase, onComplete])

  const start = useCallback(() => {
    if (phase !== AnimationLifecyclePhase.IDLE) return

    updatePhase(AnimationLifecyclePhase.PREPARING)

    coordinator.schedule({
      id: animationId,
      priority,
      delay,
    })

    const unsubscribe = coordinator.subscribe(animationId, (state) => {
      if (!mountedRef.current) return

      if (state === AnimationState.READY || state === AnimationState.RUNNING) {
        startTimeRef.current = performance.now()
        updatePhase(AnimationLifecyclePhase.ACTIVE)

        clearCompletionTimer()
        completionTimerRef.current = setTimeout(() => {
          if (mountedRef.current) {
            complete()
          }
        }, duration)
      } else if (state === AnimationState.SKIPPED) {
        complete()
      }
    })

    return unsubscribe
  }, [
    phase,
    animationId,
    priority,
    delay,
    duration,
    updatePhase,
    clearCompletionTimer,
    complete,
  ])

  const reset = useCallback(() => {
    clearCompletionTimer()
    // 不清理协调器状态，让它自然过期。
    updatePhase(AnimationLifecyclePhase.IDLE)
  }, [clearCompletionTimer, updatePhase])

  useEffect(() => {
    mountedRef.current = true

    if (autoStart) {
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
      } else {
        start()
      }
    }

    return () => {
      mountedRef.current = false
      clearCompletionTimer()
    }
  }, [autoStart, waitForPage, start, clearCompletionTimer])

  const canAnimate =
    phase === AnimationLifecyclePhase.ACTIVE ||
    phase === AnimationLifecyclePhase.COMPLETED
  const isComplete = phase === AnimationLifecyclePhase.COMPLETED
  const isIdle = phase === AnimationLifecyclePhase.IDLE

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

export interface BatchAnimationOptions {
  count: number

  staggerDelay?: number

  duration?: number

  autoStart?: boolean
  groupId?: string
}

export interface BatchAnimationResult {
  getItemState: (index: number) => {
    canAnimate: boolean
    isComplete: boolean
    className: string
  }

  allComplete: boolean

  completedCount: number

  startAll: () => void

  resetAll: () => void
}

export function useBatchAnimationLifecycle(
  options: BatchAnimationOptions,
): BatchAnimationResult {
  const { count, staggerDelay = 50, duration = 300, autoStart = true } = options

  const [completedSet, setCompletedSet] = useState<Set<number>>(new Set())
  const [activeSet, setActiveSet] = useState<Set<number>>(new Set())
  const [started, setStarted] = useState(false)

  const mountedRef = useRef(true)
  const timersRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(
    new Map(),
  )

  const clearAllTimers = useCallback(() => {
    for (const timer of timersRef.current.values()) {
      clearTimeout(timer)
    }
    timersRef.current.clear()
  }, [])

  const startAll = useCallback(() => {
    if (started) return
    setStarted(true)

    for (let i = 0; i < count; i++) {
      const itemDelay = i * staggerDelay

      const activateTimer = setTimeout(() => {
        if (!mountedRef.current) return
        setActiveSet((prev) => new Set(prev).add(i))

        const completeTimer = setTimeout(() => {
          if (!mountedRef.current) return
          setCompletedSet((prev) => new Set(prev).add(i))
        }, duration)

        timersRef.current.set(i * 2 + 1, completeTimer)
      }, itemDelay)

      timersRef.current.set(i * 2, activateTimer)
    }
  }, [started, count, staggerDelay, duration])

  const resetAll = useCallback(() => {
    clearAllTimers()
    setCompletedSet(new Set())
    setActiveSet(new Set())
    setStarted(false)
  }, [clearAllTimers])

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

  const getItemState = useCallback(
    (index: number) => {
      const isActive = activeSet.has(index)
      const isComplete = completedSet.has(index)

      let className = 'anim-waiting'
      if (isComplete) {
        className = 'anim-done'
      } else if (isActive) {
        className = 'anim-active'
      }

      return {
        canAnimate: isActive || isComplete,
        isComplete,
        className,
      }
    },
    [activeSet, completedSet],
  )

  return {
    getItemState,
    allComplete: completedSet.size >= count,
    completedCount: completedSet.size,
    startAll,
    resetAll,
  }
}

export default useAnimationLifecycle
