import type { SpeechArticulation, SpeechViseme } from '../rig/articulation'
import type { SingingCue } from './singingTimeline'

const REST_ARTICULATION: SpeechArticulation = {
  energy: 0,
  viseme: 'rest',
  amount: 0,
}

const HUMMING_THRESHOLD = 0.08
const HUMMING_OPEN_THRESHOLD = 0.45
const DEFAULT_VOICED_AMOUNT = 0.72

export function sampleSingingCue(
  cues: readonly SingingCue[],
  time: number,
): SingingCue | null {
  if (cues.length === 0 || !Number.isFinite(time)) return null
  let low = 0
  let high = cues.length - 1
  let found = -1
  while (low <= high) {
    const mid = (low + high) >> 1
    if (cues[mid].start <= time) {
      found = mid
      low = mid + 1
    } else {
      high = mid - 1
    }
  }
  if (found < 0) return null
  const cue = cues[found]
  return time < cue.end ? cue : null
}

/** Mid/presence bands; skip kick/bass so drums do not chew the mouth. */
export function singingVocalEnergy(bands: readonly number[]): number {
  if (bands.length === 0) return 0
  const mid = unit(bands[1])
  const presence = unit(bands[2])
  const treble = unit(bands[3])
  return unit(mid * 0.45 + presence * 0.35 + treble * 0.2)
}

export function singingArticulation(input: {
  cue: SingingCue | null
  energy: number | null
  humming: boolean
}): SpeechArticulation {
  const energy =
    input.energy == null || !Number.isFinite(input.energy)
      ? null
      : unit(input.energy)

  if (input.humming) return hummingArticulation(energy)

  const cue = input.cue
  if (!cue || cue.viseme === 'rest') return REST_ARTICULATION
  if (cue.viseme === 'closed') {
    return { energy: energy ?? 0, viseme: 'closed', amount: 1 }
  }

  const voiced = energy == null ? DEFAULT_VOICED_AMOUNT : mix(0.28, 1, energy)
  const amount = unit((cue.emphasis ? 1 : 0.86) * voiced)
  return {
    energy: energy ?? voiced,
    viseme: cue.viseme,
    amount,
  }
}

function hummingArticulation(energy: number | null): SpeechArticulation {
  if (energy == null || energy < HUMMING_THRESHOLD) return REST_ARTICULATION
  const viseme: SpeechViseme =
    energy >= HUMMING_OPEN_THRESHOLD ? 'open' : 'narrow'
  return {
    energy,
    viseme,
    amount: mix(0.22, 0.82, energy),
  }
}

function unit(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(0, Math.min(1, value))
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * unit(amount)
}

export function restSingingArticulation(): SpeechArticulation {
  return REST_ARTICULATION
}
