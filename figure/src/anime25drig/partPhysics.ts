import type { Anime25DPart, Anime25DPartProfile } from './types'
import { partOfLayer } from './partSplit'

/**
 * How a part swings, from what it is made of: the tuning of a hanging
 * pendulum (earwearPhysics) and of a skirt's trail (standing). A rigger sets
 * these by eye per part; here the import's vision inventory (rig_part_analysis)
 * says how stiff and how heavy each part is, and the swing follows.
 */
export interface PendulumTuning {
  /** Scales the pendulum's spring (ω²): stiffer parts swing back faster. */
  stiffness: number
  /** Damping as a share of √(ω²): heavy, stiff parts settle sooner. */
  damping: number
  /** Scales how far it may swing. */
  reach: number
}

/** An earring's tuning when nothing else is known. */
export const PENDULUM_DEFAULT: Readonly<PendulumTuning> = { stiffness: 1, damping: 0.65, reach: 1 }

/** The part drawn alone in `layer` (or cut out to it, partSplit), if the inventory named one that moves. */
export function movingPartIn(profile: Anime25DPartProfile | undefined, layer: string): Anime25DPart | null {
  const cut = /^part:\d+:/.test(layer) ? partOfLayer(profile, layer) : null
  const part = cut ?? profile?.parts.find((found) => found.layer === layer && found.motion !== 'rigid') ?? null
  return part && part.motion !== 'rigid' ? part : null
}

/** A hanging part's pendulum tuning. */
export function pendulumTuning(part: Readonly<Anime25DPart>): PendulumTuning {
  const flutter = part.motion === 'flutter'
  return {
    // Limp, light things swing slow and wide; stiff ones quick and short.
    stiffness: (0.55 + 0.9 * part.stiffness) * (flutter ? 0.7 : 1),
    damping: (flutter ? 0.3 : 0.4) + 0.35 * part.stiffness + 0.15 * part.weight,
    reach: (1.25 - 0.5 * part.stiffness) * (flutter ? 1.3 : 1),
  }
}

/** A skirt's trail: its spring's frequency (Hz) and damping. */
export function skirtTuning(part: Readonly<Anime25DPart> | null): { frequency: number; damping: number } | null {
  if (!part || part.motion === 'rigid') return null
  return {
    frequency: 1.3 * (0.8 + 0.45 * part.stiffness) * (part.motion === 'flutter' ? 0.85 : 1),
    damping: 0.22 + 0.3 * part.stiffness + 0.12 * (1 - part.weight),
  }
}
