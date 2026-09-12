import type { MotionAudioFeatures } from '../../../utils/audioMotionAnalysis'
import type { BeatFrame } from './beatClock'

export const MUSIC_MODES = ['listen', 'hum', 'sing', 'settle'] as const
export type MusicMode = (typeof MUSIC_MODES)[number]

export interface MusicPhrase {
  start: number
  end: number
  confidence: number
}

/** Live evidence only. */
export interface MusicMotionSignal {
  sampleTimeSeconds: number
  audio: Readonly<MotionAudioFeatures> | null
  beatFrame: Readonly<BeatFrame>
  phrase: Readonly<MusicPhrase> | null
}

export function isMusicMode(value: string): value is MusicMode {
  return (MUSIC_MODES as readonly string[]).includes(value)
}
