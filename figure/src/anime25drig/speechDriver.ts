import type { SpeechArticulation } from '../rig/articulation'
import type { Anime25DDriver } from './driver'

type EnergyDriverPatch = Pick<
  Anime25DDriver,
  | 'mouthOpen'
  | 'mouthWide'
  | 'mouthRound'
  | 'mouthNarrow'
  | 'mouthSeal'
  | 'talk'
>
/** Keep preview speech disabled so the player never combines it with an unrelated random mouth. */
export function speechEnergyDriverPatch(
  energy: number | null,
): EnergyDriverPatch {
  return {
    mouthOpen: energy == null ? 0 : clamp01(energy),
    mouthWide: 0,
    mouthRound: 0,
    mouthNarrow: 0,
    mouthSeal: 0,
    talk: false,
  }
}

export function speechArticulationDriverPatch(
  articulation: SpeechArticulation,
): EnergyDriverPatch {
  const amount = clamp01(articulation.amount)
  const openness =
    articulation.viseme === 'closed' || articulation.viseme === 'rest'
      ? 0
      : articulation.viseme === 'wide'
        ? 0.54
        : articulation.viseme === 'round'
          ? 0.64
          : articulation.viseme === 'narrow'
            ? 0.34
            : 0.78
  return {
    mouthOpen: openness * amount,
    mouthWide: articulation.viseme === 'wide' ? amount : 0,
    mouthRound: articulation.viseme === 'round' ? amount : 0,
    mouthNarrow: articulation.viseme === 'narrow' ? amount : 0,
    mouthSeal: articulation.viseme === 'closed' ? amount : 0,
    talk: false,
  }
}

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, finiteOrZero(value)))
}

function finiteOrZero(value: number): number {
  return Number.isFinite(value) ? value : 0
}
