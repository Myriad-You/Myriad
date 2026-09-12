import { getProductionMotionRuntime } from '../motion/runtimeHost'
import { Anime25DBodyAdapter } from './anime25dAdapter'
import { LocalPerceptionAdapter } from './perceptionAdapter'

const production: { current: Anime25DBodyAdapter | null } = { current: null }

export function getProductionBody(): Anime25DBodyAdapter {
  production.current ??= new Anime25DBodyAdapter(getProductionMotionRuntime())
  return production.current
}

export function getLocalPerception(): LocalPerceptionAdapter {
  return new LocalPerceptionAdapter()
}
