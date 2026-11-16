import type { MotionRuntime } from '@myriad/figure/motion'
import type { RigStateSummary } from '@myriad/figure/performance'
import { captureRigStateSummary, createLiveMotionRuntime } from '@myriad/figure/motion'
import { figureChannel } from '../figureChannel'
import { getMusicMotionSource } from './musicSourceRuntime'
import { getProductionCoordinator } from './productionCoordinator'

const production: { current: MotionRuntime | null } = { current: null }

export function getProductionMotionRuntime(): MotionRuntime {
  if (!production.current) {
    production.current = createLiveMotionRuntime(
      getProductionCoordinator(),
      getMusicMotionSource(),
      figureChannel,
    )
  }
  return production.current
}

export function captureProductionRigStateSummary(): RigStateSummary {
  return captureRigStateSummary(getProductionMotionRuntime())
}
