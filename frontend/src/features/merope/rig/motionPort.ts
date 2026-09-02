import type { RigBearing } from '../motion/bearing'
import type { BehaviorPlan, BehaviorRealizerReport } from '../motion/behavior'
import type { MotionChannelPolicy } from '../motion/policy'
import type { SingingSpectrumDrive } from '../singing/singingGroove'
import type { SpeechProsodyPlan } from '../speech/prosody'
import type { MeropeActivity } from '../types'
import type { SpeechArticulation } from './articulation'

/** Renderer-neutral commands consumed by every production face. */
export interface RigMotionPort {
  setMotionPolicy: (policy: MotionChannelPolicy) => void
  setBearing: (bearing: RigBearing | null) => void
  setMood: (mood: number, activity: MeropeActivity) => void
  setSpeechActive: (active: boolean) => void
  setAutoSpeech: (active: boolean) => void
  setSpeechEnergy: (energy: number | null) => void
  setSpeechArticulation: (articulation: SpeechArticulation) => void
  setSpeechProsody: (prosody: SpeechProsodyPlan | null) => void
  enqueueSpeechText: (text: string, locale?: string) => void
  setSinging: (active: boolean) => void
  setSingingTrack: (trackId: string | null) => void
  setSingingSpectrum: (drive: SingingSpectrumDrive | null) => void
  playBehaviorPlan: (plan: BehaviorPlan) => readonly BehaviorRealizerReport[]
  stopBehaviorPlan: (planId?: string) => void
}
