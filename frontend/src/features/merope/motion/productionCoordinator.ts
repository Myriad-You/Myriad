import { RigMotionCoordinator } from '@myriad/figure/motion'
import { figureChannel } from '../figureChannel'

const production: { current: RigMotionCoordinator | null } = { current: null }

/** Who holds which of the live figure's motion channels; previews keep their own. */
export function getProductionCoordinator(): RigMotionCoordinator {
  production.current ??= new RigMotionCoordinator(figureChannel.trace)
  return production.current
}
