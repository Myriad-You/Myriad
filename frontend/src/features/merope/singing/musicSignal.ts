import type { MotionAudioFeatures } from '../../../utils/audioMotionAnalysis'
import type { BeatFrame } from './beatClock'

export const MUSIC_MODES = ['listen', 'hum', 'sing', 'settle'] as const
export type MusicMode = (typeof MUSIC_MODES)[number]

export interface MusicPhrase {
  start: number
  end: number
  /** Word timings are stronger evidence than an estimated line ending. */
  confidence: number
}

/** Live evidence only. The scheduled music form decides how the body responds. */
export interface MusicMotionSignal {
  sampleTimeSeconds: number
  /** Null is unavailable analysis, zero energy is measured silence. */
  audio: Readonly<MotionAudioFeatures> | null
  beatFrame: Readonly<BeatFrame>
  /** Active, imminent, or just-ended phrase on the media clock. */
  phrase: Readonly<MusicPhrase> | null
}

export function isMusicMode(value: string): value is MusicMode {
  return (MUSIC_MODES as readonly string[]).includes(value)
}
