import type { PerformanceDirective } from '../../../services/agent/types'
import type { SpeechArticulation } from '../rig/articulation'
import type { SpeechProsodyPlan } from '../speech/prosody'
import type { MeropeActivity } from '../types'
import type { RigBearing } from './bearing'
import type { BehaviorPlan, BehaviorSnapshot } from './behavior'
import type { MotionSnapshot } from './coordinator'
import type { SingingFrame } from './musicSource'

export interface SpeechTextChunk {
  seq: number
  text: string
  locale?: string
}

export interface SpeechIntent {
  active: boolean
  autoSpeech: boolean
  energy: number | null
  articulation: SpeechArticulation | null
  prosody: SpeechProsodyPlan | null
  behaviorPlan: BehaviorPlan | null
  behaviors: readonly BehaviorSnapshot[]
  queuedText: readonly SpeechTextChunk[]
}

export interface PerformanceIntent {
  directive: PerformanceDirective | null
  startedAtMs: number
  motionIntentId?: string | null
  generation?: number
  behaviorPlan?: BehaviorPlan | null
  behaviors?: readonly BehaviorSnapshot[]
}

export interface MoodIntent {
  mood: number
  arousal: number
  activity: MeropeActivity
}

export interface MotionFrame {
  snapshot: MotionSnapshot
  bearing: RigBearing | null
  speech: SpeechIntent | null
  performance: PerformanceIntent | null
  music: SingingFrame | null
  mood: MoodIntent | null
  behaviorPlan: BehaviorPlan | null
  behaviorRevision: number
  behaviors: readonly BehaviorSnapshot[]
}
