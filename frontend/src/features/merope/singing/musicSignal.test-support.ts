import type { MotionAudioFeatures } from '../../../utils/audioMotionAnalysis'
import type { MusicMotionSignal, MusicPhrase } from './musicSignal'

export function musicSignalAt(
  time: number,
  options: {
    bpm?: number
    confidence?: number
    audio?: MotionAudioFeatures | null
    phrase?: MusicPhrase | null
  } = {},
): MusicMotionSignal {
  const bpm = options.bpm ?? 120
  const position = (time * bpm) / 60
  return {
    sampleTimeSeconds: time,
    audio:
      options.audio === undefined
        ? { energy: 0.6, bass: 0.5, pulse: 0.6, presence: 0.5 }
        : options.audio,
    phrase: options.phrase ?? null,
    beatFrame: {
      bpm,
      confidence: options.confidence ?? (bpm > 0 ? 1 : 0),
      beatCount: Math.floor(position),
      beatPhase: position % 1,
      beatCrossed: false,
      onset: bpm > 0 && position % 1 < 0.035,
    },
  }
}
