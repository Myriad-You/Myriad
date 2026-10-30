import type performanceContract from '@myriad/core/shared/merope_performance_contract.json'

export type BodyControl = keyof typeof performanceContract.bodyControls
export interface BodyPose {
  targets: Partial<Record<BodyControl, number>>
  transitionMs: number
  /** Zero keeps the pose until revised or explicitly released. */
  holdMs: number
}

export type RigMotionStyle = 'restrained' | 'even' | 'open'
export type RigMusicEnergy = 'quiet' | 'soft' | 'present' | 'strong'
export type RigBeatPhase = 'rest' | 'downbeat' | 'pulse' | 'hold'
export type RigBehaviorFunction =
  | 'orient'
  | 'attend'
  | 'acknowledge'
  | 'uncertain'
  | 'prepareSpeech'
  | 'emphasize'
  | 'surprise'
  | 'celebrate'
  | 'relief'
  | 'entrain'
  | 'express'
export type RigBehaviorPhase =
  | 'planned'
  | 'preparing'
  | 'committed'
  | 'holding'
  | 'recovering'
  | 'complete'
  | 'rejected'

export interface RigStateSummary {
  expression: PerformanceBaseline['expression'] | 'steady'
  posture: PerformanceBaseline['posture']
  acting: {
    intent: PerformanceCue['intent'] | null
    phase: PerformancePhase | 'idle'
    function: RigBehaviorFunction | null
    lifecycle: RigBehaviorPhase | null
    remainingMs: number
  }
  activeBehaviors: {
    function: RigBehaviorFunction
    lifecycle: RigBehaviorPhase
    source: string
    resources: string[]
    remainingMs: number
  }[]
  owners: {
    mouth: string
    expression: string
    gaze: string
    headBody: string
  }
  speaking: boolean
  singing: boolean
  musicPlaying: boolean
  music?: {
    energy: RigMusicEnergy
    beat: RigBeatPhase
  }
  capabilities: string[]
  recentIntents: PerformanceCue['intent'][]
  motionStyle: RigMotionStyle
  pageVisible: boolean
  faceVisible: boolean
  /** The look her face holds now (facePresets), for the director to change from. */
  face?: string
}

export type PerformancePhase =
  'reaction' | 'delivery' | 'outcome' | 'proactive' | 'mood' | 'presence'

export interface PerformanceBaseline {
  expression: 'withdrawn' | 'subdued' | 'steady' | 'warm' | 'tense'
  posture: 'closed' | 'neutral' | 'open'
  motionEnergy: number
  attention: number
  pose?: BodyPose
  /** The contract's look her face holds (facePresets), whose life the body plays. */
  face?: string
  /** How she gets from the look before to this one (faceShifts); naturally without. */
  shift?: string
  /** Looks passed through on the way, in order and without stopping (facePresets names, or 'plain'). */
  via?: string[]
}

export interface PerformanceCue {
  intent:
    | 'greet'
    | 'respond'
    | 'question'
    | 'delight'
    | 'emphasize'
    | 'listen'
    | 'notify'
    | 'think'
    | 'dizzy'
    | 'cry'
    | 'angry'
    | 'speechless'
    | 'maniac'
    | 'silly'
    | 'lovestruck'
  atMs: number
  intensity: number
  tempo: number
  fadeInMs: number
  fadeOutMs: number
  holdMs?: number
  interrupt: 'replace' | 'queue' | 'if-lower'
}

export interface PerformanceDirective {
  phrases?: SpeechPhrase[]
  /** Beats on the reply's words or on time: acting that unfolds beat by beat. */
  score?: ScoreBeat[]
  /** Standing acting only: the score plays again each this many milliseconds. */
  loopMs?: number
  phase: PerformancePhase
  moodRevision: number
  motionStyle: RigMotionStyle
  plan: {
    baseline?: PerformanceBaseline
    cues: PerformanceCue[]
  }
}

export type ScoreMoveKind =
  | 'nod' | 'shake' | 'glance' | 'blink' | 'wink' | 'beat' | 'shrug' | 'sigh' | 'bounce' | 'startle'
export type ScoreSide = 'left' | 'right' | 'both'
export type ScoreDirection =
  | 'left' | 'right' | 'up' | 'down' | 'up-left' | 'up-right' | 'down-left' | 'down-right'

/** A movement that goes and comes back by itself, such as a nod. */
export interface ScoreMove {
  kind: ScoreMoveKind
  side?: ScoreSide
  direction?: ScoreDirection
  amount: number
  count: number
  tempo: number
}

/**
 * One beat of the director's score: on words of the reply as they are
 * spoken (`text`, shifted by `offsetMs`), or `atMs` after the score arrives.
 */
export interface ScoreBeat {
  text?: string
  atMs: number
  offsetMs: number
  /** Targets reached from this moment and held until revised or `holdMs` passes. */
  pose?: BodyPose
  move?: ScoreMove
}

export interface SpeechPhrase {
  text: string
  intent:
    'ask' | 'hesitate' | 'tease' | 'explain' | 'check-in' | 'laugh' | 'none'
}

export type MoodBandName = 'floor' | 'sad' | 'tense' | 'calm' | 'excited'

export interface MoodTransition {
  before: number
  after: number
  arousalBefore?: number
  arousalAfter?: number
  bandBefore: MoodBandName
  bandAfter: MoodBandName
  delta: number
  cause: string
  revision: number
}
