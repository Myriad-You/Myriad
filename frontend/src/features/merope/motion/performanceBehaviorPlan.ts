import type {
  PerformanceCue,
  PerformanceDirective,
} from '../../../services/agent/types'
import type {
  BehaviorFunction,
  BehaviorPlan,
  ScheduledBehavior,
  TimePeg,
} from './behavior'
import {
  performanceCueChannels,
  performanceCueDefinition,
} from '../anime25drig/performanceCueDefinitions'
import {
  authoredCueEnvelope,
  scheduleBodyCues,
} from '../anime25drig/performanceMotion'

/** Stable across directives, so a refinement reconciles against the floor. */
export const PERFORMANCE_BEHAVIOR_PLAN_ID = 'performance'

/**
 * Compiles a Strict-Lite directive into the renderer-neutral behavior
 * protocol. Bodies realize only this plan; the directive does not bypass the
 * shared scheduler.
 */
export function compilePerformanceBehaviorPlan(
  directive: PerformanceDirective,
  originMs: number,
  planId: string,
  scope?: string,
  generation = 0,
): BehaviorPlan {
  const pegs: TimePeg[] = []
  const behaviors: ScheduledBehavior[] = []
  const scheduled = scheduleBodyCues(directive.plan.cues, originMs)

  // A behavior id names the beat, not the delivery that carried it.
  //
  // Two directives reach this in one round: the deterministic floor the moment
  // the round opens, and the Lite refinement whenever it lands. Keying on a
  // fresh per-publish id made every refinement a full removal plus a full
  // addition, so the scheduler's reconcile path — retime the pegs, keep the
  // lifecycle — could never fire for the one producer that publishes twice.
  // Counting occurrences of an intent instead means a refinement that repeats
  // a beat continues it (and may strengthen it mid-flight), while a refinement
  // that chose differently still recovers the old beat and schedules the new.
  const occurrences = new Map<string, number>()

  scheduled.forEach((item) => {
    const ordinal = occurrences.get(item.cue.intent) ?? 0
    occurrences.set(item.cue.intent, ordinal + 1)
    const prefix = `${planId}${scope ? `:${scope}` : ''}:cue-${item.cue.intent}-${ordinal}`
    const envelope = authoredCueEnvelope(item.cue)
    const strokeAt = Math.min(
      item.endMs,
      item.startMs + envelope.fadeIn * 1_000,
    )
    const preparationMs = Math.max(0, strokeAt - item.startMs)
    const readyAt = item.startMs + preparationMs * 0.46
    const strokeStartAt = item.startMs + preparationMs * 0.72
    const relaxAt = Math.max(strokeAt, item.endMs - envelope.fadeOut * 1_000)
    const strokeEndAt = Math.min(relaxAt, strokeAt + 80)
    pegs.push(
      peg(`${prefix}:start`, item.startMs),
      peg(`${prefix}:ready`, readyAt),
      peg(`${prefix}:stroke-start`, strokeStartAt),
      peg(`${prefix}:stroke-peak`, strokeAt),
      peg(`${prefix}:stroke-end`, strokeEndAt),
      peg(`${prefix}:relax`, relaxAt),
      peg(`${prefix}:end`, item.endMs),
    )
    behaviors.push({
      id: prefix,
      function: cueFunction(item.cue.intent),
      kind: 'oneShot',
      source: 'performance',
      resources: performanceCueDefinition(item.cue.intent).resources,
      channels: performanceCueChannels(item.cue.intent),
      timing: {
        start: `${prefix}:start`,
        ready: `${prefix}:ready`,
        strokeStart: `${prefix}:stroke-start`,
        strokePeak: `${prefix}:stroke-peak`,
        strokeEnd: `${prefix}:stroke-end`,
        relax: `${prefix}:relax`,
        end: `${prefix}:end`,
      },
      form: {
        family: 'performance-cue',
        id: item.cue.intent,
        parameters: {
          tempo: item.cue.tempo,
          phase: directive.phase,
          moodRevision: directive.moodRevision,
          ...(scope ? { performanceScope: scope, generation } : {}),
        },
      },
      intensity: clamp(item.cue.intensity, 0.2, 1.4),
      quality: cueQuality(item.cue, directive.plan.baseline?.motionEnergy ?? 1),
      confidence: 1,
    })
  })

  return {
    id: planId,
    originMs,
    metadata: {
      phase: directive.phase,
      moodRevision: directive.moodRevision,
    },
    pegs,
    behaviors,
  }
}

function cueQuality(cue: PerformanceCue, motionEnergy: number) {
  const forceful =
    cue.intent === 'emphasize' ||
    cue.intent === 'angry' ||
    cue.intent === 'maniac'
  const buoyant =
    cue.intent === 'delight' ||
    cue.intent === 'greet' ||
    cue.intent === 'maniac'
  const energy = clamp(motionEnergy, 0.2, 1.4)
  const extentEnergy = 0.72 + energy * 0.32
  const powerEnergy = 0.76 + energy * 0.28
  return {
    extent: clamp((0.78 + cue.intensity * 0.32) * extentEnergy, 0.68, 1.4),
    tempo: clamp(cue.tempo, 0.5, 1.6),
    power: clamp(
      ((forceful ? 0.92 : 0.7) + cue.intensity * 0.22) * powerEnergy,
      0.58,
      1.4,
    ),
    fluidity: forceful ? 0.58 : 0.82,
    directness: forceful ? 0.9 : 0.72,
    rebound: buoyant ? 0.68 : 0.36,
    asymmetry: cue.intent === 'question' || cue.intent === 'think' ? 0.46 : 0.2,
    density: 1,
  }
}

function cueFunction(intent: PerformanceCue['intent']): BehaviorFunction {
  switch (intent) {
    case 'greet':
    case 'notify':
      return 'orient'
    case 'respond':
      return 'acknowledge'
    case 'question':
      return 'uncertain'
    case 'delight':
      return 'celebrate'
    case 'emphasize':
      return 'emphasize'
    case 'listen':
      return 'attend'
    case 'think':
      return 'prepareSpeech'
    case 'speechless':
      return 'surprise'
    case 'cry':
      return 'relief'
    default:
      return 'express'
  }
}

function peg(id: string, atMs: number): TimePeg {
  return { id, atMs: Math.max(0, atMs), revision: 0 }
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
