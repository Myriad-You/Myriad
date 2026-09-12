import { useEffect, useReducer, useRef } from 'react'
import { coordinator } from './coordinator'
import { AnimationPriority, AnimationState } from './types'

interface UseStaggerAnimationOptions {
  groupId: string
  index: number
  baseDelay?: number
  waitForPage?: boolean
  enabled?: boolean
}

interface StaggerAnimationResult {
  canAnimate: boolean
  onComplete: () => void
}

let staggerIdCounter = 0

export function useStaggerAnimation(
  options: UseStaggerAnimationOptions,
): StaggerAnimationResult {
  const {
    groupId,
    index,
    baseDelay,
    waitForPage = true,
    enabled = true,
  } = options

  const idRef = useRef<string>('')
  if (!idRef.current) {
    idRef.current = `stagger-${groupId}-${++staggerIdCounter}`
  }
  const id = idRef.current

  // 延迟只由协调器执行，避免与 Motion 重复等待。
  const coordinatedDelay = coordinator.getStaggerDelay(index, baseDelay)

  const stateRef = useRef<AnimationState>(
    enabled ? AnimationState.WAITING : AnimationState.COMPLETED,
  )

  const scheduledRef = useRef(false)
  const [, forceUpdate] = useReducer((x) => x + 1, 0)

  const canAnimate =
    stateRef.current === AnimationState.READY ||
    stateRef.current === AnimationState.RUNNING ||
    stateRef.current === AnimationState.COMPLETED

  useEffect(() => {
    if (!enabled) {
      stateRef.current = AnimationState.COMPLETED
      forceUpdate()
      return
    }

    // 已经直接显示过的元素不在偏好切换后重播入场。
    if (stateRef.current === AnimationState.COMPLETED) return

    if (!waitForPage) {
      stateRef.current = AnimationState.READY
      forceUpdate()
      return
    }

    if (scheduledRef.current) {
      return
    }
    scheduledRef.current = true

    // 显式 delay 只交一次；不再同时交 groupId/index。
    coordinator.schedule({
      id,
      priority: AnimationPriority.ELEMENT,
      delay: coordinatedDelay,
    })

    const unsubscribe = coordinator.subscribe(id, (state) => {
      const prevState = stateRef.current
      stateRef.current = state

      if (state === AnimationState.READY) {
        stateRef.current = AnimationState.RUNNING
        coordinator.markRunning(id)
      }

      if (prevState !== state && state === AnimationState.READY) {
        forceUpdate()
      }
    })

    return () => {
      unsubscribe()
      scheduledRef.current = false
      if (stateRef.current !== AnimationState.COMPLETED) {
        coordinator.skip(id)
        stateRef.current = AnimationState.SKIPPED
      }
    }
  }, [coordinatedDelay, enabled, id, waitForPage])

  const onComplete = () => {
    if (
      stateRef.current === AnimationState.READY ||
      stateRef.current === AnimationState.RUNNING
    ) {
      coordinator.markCompleted(id)
      stateRef.current = AnimationState.COMPLETED
    }
  }

  return {
    canAnimate,
    onComplete,
  }
}

export default useStaggerAnimation
