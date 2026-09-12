import { useCallback, useEffect, useReducer, useRef } from 'react'
import { coordinator } from '../coordinator'
import { isPageVisible, onVisibility, registerPageCleanup } from '../core'

import { AnimationPriority, AnimationState } from '../types'

const _PAGE_ID = 'tapp'
const BASE_STAGGER_DELAY = 60

let staggerIdCounter = 0

export function useTappScheduler(): void {
  useEffect(() => {
    return () => {
      staggerIdCounter = 0
    }
  }, [])
}

interface TappStaggerResult {
  canAnimate: boolean
  onComplete: () => void
}

interface TappStaggerOptions {
  baseDelay?: number
  enabled?: boolean
}

export function useTappStagger(
  index: number,
  options: TappStaggerOptions = {},
): TappStaggerResult {
  const { baseDelay = BASE_STAGGER_DELAY, enabled = true } = options

  const idRef = useRef<string>('')
  if (!idRef.current) {
    idRef.current = `tapp-card-${++staggerIdCounter}`
  }
  const id = idRef.current

  // 延迟只由协调器执行；Motion 收到 READY 后立即播放。
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

  const onComplete = useCallback(() => {
    if (
      stateRef.current === AnimationState.READY ||
      stateRef.current === AnimationState.RUNNING
    ) {
      stateRef.current = AnimationState.COMPLETED
      coordinator.markCompleted(id)
    }
  }, [id])

  useEffect(() => {
    if (!enabled) {
      stateRef.current = AnimationState.COMPLETED
      forceUpdate()
      return
    }

    // 已经以无动画模式显示过的卡片不在偏好切换后重播入场。
    if (stateRef.current === AnimationState.COMPLETED) return

    if (scheduledRef.current) return
    scheduledRef.current = true

    coordinator.schedule({
      id,
      priority: AnimationPriority.COMPONENT,
      delay: coordinatedDelay,
    })

    const unsubscribe = coordinator.subscribe(id, (state) => {
      stateRef.current = state

      if (state === AnimationState.READY) {
        stateRef.current = AnimationState.RUNNING
        coordinator.markRunning(id)
      }

      if (state === AnimationState.READY || state === AnimationState.SKIPPED) {
        forceUpdate()
      }
    })

    return () => {
      if (
        stateRef.current !== AnimationState.COMPLETED &&
        stateRef.current !== AnimationState.SKIPPED
      ) {
        coordinator.skip(id)
      }
      unsubscribe()
      scheduledRef.current = false
    }
  }, [coordinatedDelay, enabled, id])

  return { canAnimate, onComplete }
}

export function useTappVisibility(): boolean {
  const [visible, setVisible] = useReducer(
    () => isPageVisible(),
    isPageVisible(),
  )

  useEffect(() => {
    return onVisibility(() => {
      setVisible()
    })
  }, [])

  return visible
}

export function cleanupTapp(): void {
  staggerIdCounter = 0
}

registerPageCleanup(_PAGE_ID, cleanupTapp)
