import type { MotionRuntime } from '@myriad/figure/motion/runtime'
import type { RigStateSummary } from '@myriad/figure/performance/types'
import { getRigMotionCoordinator } from '@myriad/figure/motion/coordinator'
import { captureRigStateSummary } from '@myriad/figure/motion/rigStateSummary'
import { createLiveMotionRuntime } from '@myriad/figure/motion/runtime'
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
