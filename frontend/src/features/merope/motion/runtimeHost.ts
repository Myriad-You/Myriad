import type { MotionRuntime } from '@myriad/figure/motion'
import type { RigStateSummary } from '@myriad/figure/performance'
import { captureRigStateSummary, createLiveMotionRuntime, getRigMotionCoordinator } from '@myriad/figure/motion'
import { getMusicMotionSource } from './musicSourceRuntime'

const production: { current: MotionRuntime | null } = { current: null }

export function getProductionMotionRuntime(): MotionRuntime {
  if (!production.current) {
    production.current = createLiveMotionRuntime(
      getRigMotionCoordinator(),
      getMusicMotionSource(),
    )
  }
  return production.current
}

export function captureProductionRigStateSummary(): RigStateSummary {
  return captureRigStateSummary(getProductionMotionRuntime())
}
