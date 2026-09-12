import type { ElementAnimationOptions } from './types'
import { useCallback, useEffect, useReducer, useRef } from 'react'
import { coordinator } from './coordinator'
import { AnimationPriority, AnimationState } from './types'

interface UseElementAnimationResult {
  canAnimate: boolean
  isAnimating: boolean
  onComplete: () => void
}

let elementIdCounter = 0

export function useElementAnimation(
  options: ElementAnimationOptions = {},
): UseElementAnimationResult {
  const { groupId, index = 0, staggerDelay, waitForPage = true } = options

  const idRef = useRef<string>('')
  if (!idRef.current) {
    idRef.current = `element-${++elementIdCounter}`
  }
  const id = idRef.current

  // 自定义交错手动算 delay，避免协调器再叠一层。
  const hasCustomStagger = typeof staggerDelay === 'number'
  const computedDelay = hasCustomStagger ? index * (staggerDelay ?? 0) : 0
  const effectiveGroupId = hasCustomStagger ? undefined : groupId

  const stateRef = useRef<AnimationState>(AnimationState.WAITING)

  const scheduledRef = useRef(false)
  const [, forceUpdate] = useReducer((x) => x + 1, 0)

  const canAnimate =
    stateRef.current === AnimationState.READY ||
    stateRef.current === AnimationState.RUNNING
  const isAnimating = stateRef.current === AnimationState.RUNNING

  useEffect(() => {
    if (!waitForPage) {
      stateRef.current = AnimationState.READY
      forceUpdate()
      return
    }

    if (scheduledRef.current) {
      return
    }
    scheduledRef.current = true

    coordinator.schedule({
      id,
      priority: AnimationPriority.ELEMENT,
      groupId: effectiveGroupId,
      index,
      delay: hasCustomStagger ? computedDelay : 0,
    })

    const unsubscribe = coordinator.subscribe(id, (state) => {
      const prevState = stateRef.current
      stateRef.current = state

      if (
        prevState !== state &&
        (state === AnimationState.READY || state === AnimationState.SKIPPED)
      ) {
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
  }, [
    id,
    effectiveGroupId,
    hasCustomStagger,
    computedDelay,
    index,
    waitForPage,
  ])

  const onComplete = useCallback(() => {
    coordinator.markCompleted(id)
    stateRef.current = AnimationState.COMPLETED
  }, [id])

  return {
    canAnimate,
    isAnimating,
    onComplete,
  }
}

export default useElementAnimation
