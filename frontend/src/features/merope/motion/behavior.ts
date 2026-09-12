import type {
  RigBehaviorFunction,
  RigBehaviorPhase,
} from '../../../services/agent/types'
import type { BehaviorResource } from './behaviorResources'
import type { MotionChannel, MotionSourceId } from './channels'

export type BehaviorKind =
  'oneShot' | 'state' | 'rhythmic' | 'tracking' | 'secondary'

export type BehaviorFunction = RigBehaviorFunction
export type BehaviorPhase = RigBehaviorPhase

export interface TimePeg {
  id: string
  atMs: number
  revision: number
  confidence?: number
}

export interface BehaviorTiming {
  start: string
  ready: string
  strokeStart: string
  strokePeak: string
  strokeEnd: string
  relax: string | null
  end: string | null
}

export interface BehaviorQuality {
  extent: number
  tempo: number
  power: number
  fluidity: number
  directness: number
  rebound: number
  asymmetry: number
  density: number
}

export interface BehaviorForm {
  family: string
  id: string
  parameters?: Readonly<Record<string, string | number | boolean>>
}

export interface ScheduledBehavior {
  id: string
  function: BehaviorFunction
  kind: BehaviorKind
  source: MotionSourceId
  resources: readonly BehaviorResource[]
  channels: readonly MotionChannel[]
  timing: BehaviorTiming
  anticipation?: string
  form: BehaviorForm
  intensity: number
  quality?: Partial<BehaviorQuality>
  confidence?: number
}

export interface BehaviorPlan {
  id: string
  originMs: number
  metadata?: Readonly<Record<string, string | number | boolean>>
  pegs: readonly TimePeg[]
  behaviors: readonly ScheduledBehavior[]
}

export interface BehaviorRealizerReport {
  behaviorId: string
  result: 'accepted' | 'rejected'
  atMs: number
  reason?: 'unsupported-form' | 'invalid-timing' | 'superseded'
}

export interface BehaviorSnapshot {
  id: string
  function: BehaviorFunction
  kind: BehaviorKind
  source: MotionSourceId
  resources: readonly BehaviorResource[]
  channels: readonly MotionChannel[]
  form: BehaviorForm
  phase: BehaviorPhase
  startedAtMs: number
  readyAtMs: number
  strokeStartAtMs: number
  strokePeakAtMs: number
  strokeEndAtMs: number
  relaxAtMs: number | null
  endsAtMs: number | null
  remainingMs: number | null
  anticipatedAtMs?: number
  anticipationConfidence?: number
}

export type BehaviorFeedbackType =
  'scheduled' | 'phase' | 'retimed' | 'interrupted' | 'accepted' | 'rejected'

export interface BehaviorFeedback {
  type: BehaviorFeedbackType
  behaviorId: string
  atMs: number
  phase: BehaviorPhase
  from?: BehaviorPhase
  pegId?: string
  reason?: BehaviorRealizerReport['reason']
}
