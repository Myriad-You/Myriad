import type { RefObject } from 'react'
import type { RigMotionPort } from '../rig/motionPort'
import type { MeropeActivity } from '../types'
import type { LiveFaceConsumer, MotionRuntime } from './runtime'
import { useEffect, useLayoutEffect, useRef } from 'react'
import { setLiveFaceVisible } from '../faceVisible'
import { markTurnTrace, noteTurnTraceDrop } from '../turnTrace'
import { useRigSingingLifecycle } from '../useRigSingingLifecycle'
import { applyMotionFrame, createMotionApplyState } from './applyFrame'
import { createPreviewMotionRuntime } from './runtime'
import { getProductionMotionRuntime } from './runtimeHost'

export interface RigMotionLifecycleOptions {
  mood?: number
  arousal?: number
  activity?: MeropeActivity
  capabilities?: readonly string[]
  ready?: boolean
  /** Higher wins only when two visible production faces disagree. */
  priority?: number
}

function useMotionRuntimeConsumer(
  runtime: MotionRuntime,
  rigRef: RefObject<RigMotionPort | null>,
  options: RigMotionLifecycleOptions = {},
  liveFace = false,
): void {
  const mood = options.mood ?? 70
  const arousal = options.arousal ?? 48
  const activity = options.activity ?? 'idle'
  const capabilityKey = options.capabilities?.join(',') ?? ''
  const ready = options.ready ?? true
  const priority = options.priority ?? 0
  const liveConsumerRef = useRef<LiveFaceConsumer | null>(null)
  const liveStateRef = useRef({
    mood,
    arousal,
    activity,
    capabilities: capabilityKey ? capabilityKey.split(',') : [],
    ready,
    priority,
  })
  liveStateRef.current = {
    mood,
    arousal,
    activity,
    capabilities: capabilityKey ? capabilityKey.split(',') : [],
    ready,
    priority,
  }

  useEffect(() => {
    const release = runtime.retain()
    if (liveFace) {
      liveConsumerRef.current = runtime.attachLiveFaceConsumer(
        liveStateRef.current,
      )
      setLiveFaceVisible(runtime.summaryFacts().faceVisible)
    }
    return () => {
      liveConsumerRef.current?.release()
      liveConsumerRef.current = null
      release()
      if (liveFace) {
        setLiveFaceVisible(
          getProductionMotionRuntime().summaryFacts().faceVisible,
        )
      }
    }
  }, [runtime, liveFace])

  useEffect(() => {
    if (liveFace) {
      liveConsumerRef.current?.update(liveStateRef.current)
      setLiveFaceVisible(runtime.summaryFacts().faceVisible)
      return
    }
    runtime.mood.set(mood, activity, arousal)
    runtime.setCapabilities(capabilityKey ? capabilityKey.split(',') : [])
  }, [
    runtime,
    liveFace,
    mood,
    activity,
    arousal,
    capabilityKey,
    ready,
    priority,
  ])

  useLayoutEffect(() => {
    if (!ready) return undefined
    const state = createMotionApplyState()
    return runtime.subscribe((frame) => {
      const rig = rigRef.current
      if (!rig) return
      applyMotionFrame(rig, frame, state, (feedback) => {
        if (liveFace && feedback.behaviorId.includes(':cue-')) {
          if (feedback.result === 'accepted') {
            markTurnTrace('performance_applied', {
              behavior: feedback.behaviorId,
            })
          } else {
            noteTurnTraceDrop('behavior_rejected')
          }
        }
        runtime.reportBehaviorRealizer(
          feedback.planId,
          feedback.behaviorId,
          feedback.result,
          feedback.atMs,
          feedback.reason,
        )
      })
    })
  }, [runtime, rigRef, liveFace, ready])
}

export function useRigMotionLifecycle(
  rigRef: RefObject<RigMotionPort | null>,
  options: RigMotionLifecycleOptions = {},
): void {
  useRigSingingLifecycle(options.ready ?? true)
  useMotionRuntimeConsumer(getProductionMotionRuntime(), rigRef, options, true)
}

export function useRigPreviewMotionLifecycle(
  rigRef: RefObject<RigMotionPort | null>,
  options: RigMotionLifecycleOptions = {},
): void {
  const runtimeRef = useRef<MotionRuntime | null>(null)
  if (runtimeRef.current === null) {
    runtimeRef.current = createPreviewMotionRuntime()
  }
  useMotionRuntimeConsumer(runtimeRef.current, rigRef, options)
}
