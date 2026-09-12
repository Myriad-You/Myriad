import { useCallback, useEffect, useReducer, useRef } from 'react'
import { coordinator } from '../coordinator'
import { AnimationPriority, AnimationState } from '../types'
import { brewMotionQuiet } from './brewMotion'

/** 与 settings-motion `--sm-stagger` 同值。 */
export const BREW_TAG_STAGGER_MS = 26

/** 与 settings-motion `--sm-dur-slow` 同值。 */
export const BREW_TAG_ENTER_MS = 320

/** 与 settings-motion `--sm-dur-base` 同值。 */
export const BREW_TAG_EXIT_MS = 220

let brewTagIdCounter = 0

export function resetBrewTagIds(): void {
  brewTagIdCounter = 0
}

export function brewTagQuiet(): boolean {
  return brewMotionQuiet()
}

export function brewTagDelay(index: number, quiet = false): number {
  if (quiet || index <= 0) return 0
  return index * BREW_TAG_STAGGER_MS
}

interface BrewTagMotionOptions {
  enabled?: boolean
  role?: 'enter' | 'exit' | 'in'
  chipId?: string
}

interface BrewTagMotionResult {
  canAnimate: boolean
  onComplete: () => void
}

/** 按 role 占槽。enter 播完不重来；切到 exit 另开一条。 */
export function useBrewTag(
  index: number,
  options: BrewTagMotionOptions = {},
): BrewTagMotionResult {
  const quiet = brewTagQuiet()
  const role = options.role ?? 'enter'
  const enabled = (options.enabled ?? true) && !quiet && role !== 'in'
  const instanceRef = useRef(0)
  if (!instanceRef.current) instanceRef.current = ++brewTagIdCounter
  const id = `brew-tag-${role}-${options.chipId ?? instanceRef.current}`

  const stateRef = useRef<AnimationState>(
    enabled ? AnimationState.WAITING : AnimationState.COMPLETED,
  )
  const scheduledRef = useRef('')
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

    if (scheduledRef.current === id) return
    scheduledRef.current = id
    stateRef.current = AnimationState.WAITING

    const apply = (state: AnimationState) => {
      stateRef.current = state
      if (state === AnimationState.READY) {
        stateRef.current = AnimationState.RUNNING
        coordinator.markRunning(id)
      }
      if (state === AnimationState.READY || state === AnimationState.SKIPPED) {
        forceUpdate()
      }
    }

    const unsubscribe = coordinator.subscribe(id, apply)
    apply(
      coordinator.schedule({
        id,
        priority: AnimationPriority.SECTION,
        delay: brewTagDelay(index, false),
      }),
    )

    return () => {
      if (
        stateRef.current !== AnimationState.COMPLETED &&
        stateRef.current !== AnimationState.SKIPPED
      ) {
        coordinator.skip(id)
      }
      unsubscribe()
      if (scheduledRef.current === id) scheduledRef.current = ''
    }
  }, [enabled, id, index])

  return { canAnimate, onComplete }
}
