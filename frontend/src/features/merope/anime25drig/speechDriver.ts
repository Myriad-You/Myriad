import type { SpeechArticulation } from '../rig/articulation'
import type { Anime25DDriver } from './player'

type EnergyDriverPatch = Pick<Anime25DDriver, 'mouthOpen' | 'talk'>
type ArticulationDriverPatch = Pick<
  Anime25DDriver,
  'mouthOpen' | 'mouthForm' | 'talk'
>

/**
 * External audio/text articulation is an authored signal. Keep preview speech
 * disabled so the player never combines it with an unrelated random mouth.
 */
export function speechEnergyDriverPatch(
  energy: number | null,
): EnergyDriverPatch {
  return {
    mouthOpen: energy == null ? 0 : clamp01(energy),
    talk: false,
  }
}

export function speechArticulationDriverPatch(
  articulation: SpeechArticulation,
  baselineMouthForm = 0,
): ArticulationDriverPatch {
  const shape =
    articulation.viseme === 'closed' || articulation.viseme === 'rest'
      ? 0
      : articulation.viseme === 'wide'
        ? 1
        : articulation.viseme === 'round'
          ? 0.68
          : 0.55
  return {
    mouthOpen: clamp01(shape * finiteOrZero(articulation.amount)),
    mouthForm:
      articulation.viseme === 'wide'
        ? 0.25
        : articulation.viseme === 'round'
          ? -0.2
          : finiteOrZero(baselineMouthForm),
    talk: false,
  }
}

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, finiteOrZero(value)))
}

function finiteOrZero(value: number): number {
  return Number.isFinite(value) ? value : 0
}
