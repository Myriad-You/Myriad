import type {
  PerformanceCue,
  PerformanceDirective,
} from '../../../services/agent/types'
import type { BehaviorSnapshot } from './behavior'
import type { BehaviorResource } from './behaviorResources'
import type { MotionChannel } from './channels'
import {
  performanceCueChannels,
  performanceCueDefinition,
} from '../anime25drig/performanceCueDefinitions'
import { cueDurationMs } from '../anime25drig/performanceMotion'
import { resourcesConflict } from './behaviorResources'
import { channelPriority } from './channels'

export type ReactionDecisionReason =
  'selected' | 'habituated' | 'resource-busy' | 'retimed'

export interface ReactionDecision {
  intent: PerformanceCue['intent']
  reason: ReactionDecisionReason
  atMs: number
}

export interface ReactionSelection {
  directive: PerformanceDirective
  decisions: readonly ReactionDecision[]
}

interface ReactionMemory {
  scope?: string
  intent: PerformanceCue['intent']
  intensity: number
  selectedAtMs: number
}

const MAX_REACTION_MEMORY = 32
const RECOVERY_GAP_MS = 90

/**
 * Deterministic reaction policy between semantic selection and scheduling.
 * It adds human-like refractory periods and respects concurrent body activity;
 * it never emits poses and remains renderer neutral.
 */
export class HumanReactionPolicy {
  private readonly memory: ReactionMemory[] = []

  select(
    directive: PerformanceDirective,
    active: readonly BehaviorSnapshot[],
    nowMs: number,
    scope?: string,
    generation = 0,
  ): ReactionSelection {
    const selected: PerformanceCue[] = []
    const decisions: ReactionDecision[] = []
    for (const original of directive.plan.cues) {
      const cue = { ...original }
      let retimed = false
      const definition = performanceCueDefinition(cue.intent)
      const resources = definition.resources
      const blocking = active.filter(
        (behavior) =>
          isLive(behavior) && resourcesOverlap(resources, behavior.resources),
      )
      if (
        cue.interrupt === 'if-lower' &&
        blocking.some(
          (behavior) =>
            !acceptsPerformanceHandoff(
              directive,
              behavior,
              scope,
              generation,
              cue.intent,
            ) && blocksIfLower(performanceCueChannels(cue.intent), behavior),
        )
      ) {
        decisions.push({
          intent: cue.intent,
          reason: 'resource-busy',
          atMs: cue.atMs,
        })
        continue
      }
      if (this.isHabituated(cue, nowMs, scope)) {
        decisions.push({
          intent: cue.intent,
          reason: 'habituated',
          atMs: cue.atMs,
        })
        continue
      }
      if (cue.interrupt === 'queue') {
        const waitMs = blocking.reduce(
          (maximum, behavior) => Math.max(maximum, behavior.remainingMs ?? 0),
          0,
        )
        if (waitMs > cue.atMs) {
          cue.atMs = Math.min(5_000, waitMs + RECOVERY_GAP_MS)
          retimed = true
          decisions.push({
            intent: cue.intent,
            reason: 'retimed',
            atMs: cue.atMs,
          })
        }
      }
      if (!retimed) {
        decisions.push({
          intent: cue.intent,
          reason: 'selected',
          atMs: cue.atMs,
        })
      }
      selected.push(cue)
      this.remember(cue, nowMs + cue.atMs, scope)
    }
    return {
      directive: {
        ...directive,
        plan: { ...directive.plan, cues: selected },
      },
      decisions,
    }
  }

  timeline(): readonly ReactionMemory[] {
    return this.memory
  }

  private isHabituated(
    cue: PerformanceCue,
    nowMs: number,
    scope?: string,
  ): boolean {
    const recent = [...this.memory]
      .reverse()
      .find(
        (entry) =>
          entry.intent === cue.intent && (!scope || entry.scope === scope),
      )
    if (!recent) return false
    const elapsed = nowMs + cue.atMs - recent.selectedAtMs
    return (
      elapsed >= 0 &&
      elapsed < refractoryMs(cue) &&
      cue.intensity <= recent.intensity + 0.18
    )
  }

  private remember(
    cue: PerformanceCue,
    selectedAtMs: number,
    scope?: string,
  ): void {
    this.memory.push({
      ...(scope ? { scope } : {}),
      intent: cue.intent,
      intensity: cue.intensity,
      selectedAtMs,
    })
    if (this.memory.length > MAX_REACTION_MEMORY) this.memory.shift()
  }
}

/**
 * Same-turn beats can be refined; acknowledgement yields to delivery. A newer
 * live Chat generation supersedes its predecessor. The shared scheduler owns
 * recovery, so admission never restarts or directly clears a pose.
 */
function acceptsPerformanceHandoff(
  directive: PerformanceDirective,
  behavior: BehaviorSnapshot,
  scope: string | undefined,
  generation: number,
  intent: PerformanceCue['intent'],
): boolean {
  if (
    !scope ||
    behavior.source !== 'performance' ||
    behavior.form.family !== 'performance-cue'
  ) {
    return false
  }
  const previous = behavior.form.parameters
  // A new live Chat generation replaces its predecessor, with scheduler-owned
  // recovery. A mood revision is affect state, never proof of turn identity.
  if (
    generation > 0 &&
    typeof previous?.generation === 'number' &&
    previous.generation > 0 &&
    previous.generation < generation
  ) {
    return true
  }
  return (
    previous?.performanceScope === scope &&
    (behavior.form.id === intent ||
      ((directive.phase === 'delivery' || directive.phase === 'outcome') &&
        previous?.phase === 'reaction'))
  )
}

function refractoryMs(cue: PerformanceCue): number {
  const definition = performanceCueDefinition(cue.intent)
  const body = definition.resources.some((resource) =>
    resource.startsWith('body.'),
  )
  const secondary = definition.resources.some((resource) =>
    resource.startsWith('secondary.'),
  )
  return Math.max(
    cueDurationMs(cue) * 0.55,
    secondary ? 1_800 : body ? 1_200 : 760,
  )
}

function isLive(behavior: BehaviorSnapshot): boolean {
  return (
    behavior.phase !== 'complete' &&
    behavior.phase !== 'rejected' &&
    behavior.phase !== 'recovering'
  )
}

function resourcesOverlap(
  left: readonly BehaviorResource[],
  right: readonly BehaviorResource[],
): boolean {
  return left.some((a) => right.some((b) => resourcesConflict(a, b)))
}

function blocksIfLower(
  cueChannels: readonly MotionChannel[],
  behavior: BehaviorSnapshot,
): boolean {
  for (const channel of cueChannels) {
    if (!behavior.channels.includes(channel)) continue
    if (
      channelPriority(channel, behavior.source) >=
      channelPriority(channel, 'performance')
    ) {
      return true
    }
  }
  return false
}
