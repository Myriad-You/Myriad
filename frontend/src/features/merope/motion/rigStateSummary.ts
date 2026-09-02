import type {
  PerformanceBaseline,
  PerformanceCue,
  PerformanceDirective,
  PerformancePhase,
  RigBeatPhase,
  RigMusicEnergy,
  RigStateSummary,
} from '../../../services/agent/types'
import type { MeropeRigManifest } from '../rig/types'
import type { SingingSpectrumDrive } from '../singing/singingGroove'
import type { BehaviorSnapshot } from './behavior'
import type { MotionSourceId } from './channels'
import type { MotionRuntime } from './runtime'
import {
  PERFORMANCE_BASELINE_EXPRESSIONS,
  PERFORMANCE_POSTURES,
} from '../performanceContract'
import { hasAnime25DCapability } from '../rig/anime25dCapabilities'
import { MOTION_SOURCES } from './channels'

const EXPRESSIONS: readonly PerformanceBaseline['expression'][] =
  PERFORMANCE_BASELINE_EXPRESSIONS
const POSTURES: readonly PerformanceBaseline['posture'][] = PERFORMANCE_POSTURES
const PHASES: readonly (PerformancePhase | 'idle')[] = [
  'reaction',
  'delivery',
  'outcome',
  'proactive',
  'mood',
  'idle',
]
const BEHAVIOR_PHASES = [
  'planned',
  'preparing',
  'committed',
  'holding',
  'recovering',
  'complete',
  'rejected',
] as const
/** Mirrors `RIG_STATE_BEHAVIOR_FUNCTIONS`; locked to the producers by test. */
export const BEHAVIOR_FUNCTIONS = [
  'orient',
  'attend',
  'acknowledge',
  'uncertain',
  'prepareSpeech',
  'emphasize',
  'surprise',
  'celebrate',
  'relief',
  'entrain',
  'express',
] as const
const CAPABILITY_MAP = [
  ['blink', 'blink'],
  ['independent-eyes', 'independent-eyes'],
  ['dizzy-eye-variant', 'dizzy-eye'],
  ['squeeze-eye-variant', 'squeeze-eye'],
  ['cry-eye-variant', 'cry-eye'],
  ['silly-eye-variant', 'silly-eye'],
  ['lovestruck-heart-pupils', 'lovestruck'],
  ['lovestruck-face-effects', 'lovestruck'],
  ['cry-mouth-variant', 'cry-mouth'],
  ['maniac-mouth-variant', 'maniac-mouth'],
  ['silly-mouth-variant', 'silly-mouth'],
  ['mouth-shapes', 'mouth-shapes'],
] as const
const MAX_RECENT = 6
const MAX_ACTIVE_BEHAVIORS = 8
/**
 * A behavior's producer. `MOTION_SOURCES` is the wider lease vocabulary —
 * mood and ambient own channels without ever publishing a behavior, so an
 * active behavior claiming one of them is not a state this rig can reach.
 */
export const BEHAVIOR_SOURCES = ['performance', 'coSpeech', 'music'] as const

const BEHAVIOR_RESOURCES = [
  'face.mouth',
  'face.expression',
  'face.gaze',
  'body.head',
  'body.torso',
  'body.arm.left',
  'body.arm.right',
  'body.hand.left',
  'body.hand.right',
  'secondary.hair',
  'secondary.clothing',
  'secondary.bust',
] as const

export function semanticRigCapabilities(
  manifest: MeropeRigManifest | null | undefined,
): string[] {
  if (!manifest) return []
  const layers = [
    ...manifest.parts.map((part) => ({
      id: part.id,
      slot: part.slot,
      variant: part.variant,
    })),
    ...(manifest.anime25dPlayback?.layers.map((layer) => ({
      id: layer.name,
      role: layer.role,
      side:
        layer.side === 'L'
          ? ('left' as const)
          : layer.side === 'R'
            ? ('right' as const)
            : null,
    })) ?? []),
  ]
  const capabilities = new Set<string>(['head-body'])
  for (const [internal, semantic] of CAPABILITY_MAP) {
    if (hasAnime25DCapability(layers, internal)) capabilities.add(semantic)
  }
  return [...capabilities]
}

export function captureRigStateSummary(
  runtime: MotionRuntime,
  nowMs: number = currentNow(),
): RigStateSummary {
  const frame = runtime.frame(nowMs)
  const facts = runtime.summaryFacts()
  const performance = frame.performance?.directive ?? null
  const baseline = frame.bearing
  const behaviors = frame.behaviors
  const acting = resolveActing(performance, behaviors)
  const spectrum = frame.music?.spectrum ?? null
  const singing = Boolean(frame.music?.apply.writeGroove)
  const musicPlaying =
    singing || Boolean(frame.music && !frame.music.apply.release)
  const summary: RigStateSummary = {
    expression: allowExpression(baseline?.expression) ?? 'steady',
    posture: allowPosture(baseline?.posture) ?? 'neutral',
    acting,
    activeBehaviors: activeBehaviorSummaries(behaviors),
    owners: {
      mouth: allowOwner(frame.snapshot.owners.mouth),
      expression: allowOwner(frame.snapshot.owners.expression),
      gaze: allowOwner(frame.snapshot.owners.gaze),
      headBody: allowOwner(frame.snapshot.owners.headBody),
    },
    speaking: Boolean(frame.speech?.active),
    singing,
    musicPlaying,
    ...(musicPlaying
      ? { music: { energy: musicEnergy(spectrum), beat: beatPhase(spectrum) } }
      : {}),
    capabilities: facts.capabilities,
    recentIntents: facts.recentIntents,
    motionStyle: facts.motionStyle,
    pageVisible:
      typeof document === 'undefined'
        ? true
        : document.visibilityState !== 'hidden',
    faceVisible: facts.faceVisible,
  }
  return sanitizeRigStateSummary(summary) ?? summary
}

export function sanitizeRigStateSummary(
  value: unknown,
): RigStateSummary | null {
  if (!isRecord(value)) return null
  const acting = isRecord(value.acting) ? value.acting : {}
  const owners = isRecord(value.owners) ? value.owners : {}
  const music = isRecord(value.music) ? value.music : null
  const activeBehaviors = Array.isArray(value.activeBehaviors)
    ? value.activeBehaviors
        .map(sanitizeActiveBehavior)
        .filter(
          (behavior): behavior is RigStateSummary['activeBehaviors'][number] =>
            behavior !== null,
        )
        .slice(0, MAX_ACTIVE_BEHAVIORS)
    : []
  const capabilities = Array.isArray(value.capabilities)
    ? value.capabilities
        .filter((item): item is string => typeof item === 'string')
        .filter((item) =>
          CAPABILITY_MAP.some(
            ([, semantic]) => semantic === item || item === 'head-body',
          ),
        )
        .slice(0, 12)
    : []
  const recentIntents = Array.isArray(value.recentIntents)
    ? value.recentIntents
        .filter(
          (item): item is PerformanceCue['intent'] =>
            typeof item === 'string' && isCueIntent(item),
        )
        .slice(0, MAX_RECENT)
    : []
  return {
    expression: allowExpression(value.expression) ?? 'steady',
    posture: allowPosture(value.posture) ?? 'neutral',
    acting: {
      intent: isCueIntent(acting.intent) ? acting.intent : null,
      phase: PHASES.includes(acting.phase as PerformancePhase | 'idle')
        ? (acting.phase as PerformancePhase | 'idle')
        : 'idle',
      function: BEHAVIOR_FUNCTIONS.includes(
        acting.function as (typeof BEHAVIOR_FUNCTIONS)[number],
      )
        ? (acting.function as (typeof BEHAVIOR_FUNCTIONS)[number])
        : null,
      lifecycle: BEHAVIOR_PHASES.includes(
        acting.lifecycle as (typeof BEHAVIOR_PHASES)[number],
      )
        ? (acting.lifecycle as (typeof BEHAVIOR_PHASES)[number])
        : null,
      remainingMs: clampMs(acting.remainingMs),
    },
    activeBehaviors,
    owners: {
      mouth: allowOwner(owners.mouth),
      expression: allowOwner(owners.expression),
      gaze: allowOwner(owners.gaze),
      headBody: allowOwner(owners.headBody),
    },
    speaking: value.speaking === true,
    singing: value.singing === true,
    musicPlaying: value.musicPlaying === true,
    ...(music
      ? {
          music: {
            energy: allowEnergy(music.energy),
            beat: allowBeat(music.beat),
          },
        }
      : {}),
    capabilities,
    recentIntents,
    motionStyle:
      value.motionStyle === 'restrained' || value.motionStyle === 'open'
        ? value.motionStyle
        : 'even',
    pageVisible: value.pageVisible !== false,
    faceVisible: value.faceVisible !== false,
  }
}

/** Furthest along first; a running behavior outranks a merely scheduled one. */
const LIFECYCLE_RANK: Record<BehaviorSnapshot['phase'], number> = {
  holding: 0,
  committed: 1,
  recovering: 2,
  preparing: 3,
  planned: 4,
  complete: 5,
  rejected: 6,
}

/**
 * What the director needs to decide with, not the first eight behaviors.
 *
 * The prompt tells the model two things about this list: do not repeat a
 * function that is already in flight, and drop a cue whose resources are busy.
 * Both are questions about *which* functions and resources are live, not about
 * how many times one of them recurs — so one entry per source and function is
 * the whole signal, and the twelfth queued speech accent adds nothing.
 *
 * Taking the earliest eight instead put a whole utterance of prosody accents
 * in front of the director: seven identical `emphasize` rows, most of them not
 * yet started, telling a model under a no-repeat rule to stop choosing the one
 * cue that reads as emphasis. The kept instance is the one furthest along,
 * because that is the one whose `remainingMs` says when the resource frees.
 */
function activeBehaviorSummaries(
  behaviors: readonly BehaviorSnapshot[],
): RigStateSummary['activeBehaviors'] {
  const live = behaviors
    .filter(
      (behavior) =>
        behavior.phase !== 'complete' && behavior.phase !== 'rejected',
    )
    .sort(
      (left, right) =>
        LIFECYCLE_RANK[left.phase] - LIFECYCLE_RANK[right.phase] ||
        left.startedAtMs - right.startedAtMs,
    )
  const kept = new Map<string, BehaviorSnapshot>()
  for (const behavior of live) {
    const key = `${behavior.source}:${behavior.function}`
    if (!kept.has(key)) kept.set(key, behavior)
  }
  return [...kept.values()]
    .slice(0, MAX_ACTIVE_BEHAVIORS)
    .map((behavior) => ({
      function: behavior.function,
      lifecycle: behavior.phase,
      source: behavior.source,
      resources: [...behavior.resources],
      remainingMs: clampMs(behavior.remainingMs),
    }))
}

function sanitizeActiveBehavior(
  value: unknown,
): RigStateSummary['activeBehaviors'][number] | null {
  if (!isRecord(value)) return null
  if (
    !BEHAVIOR_FUNCTIONS.includes(
      value.function as (typeof BEHAVIOR_FUNCTIONS)[number],
    ) ||
    !BEHAVIOR_PHASES.includes(
      value.lifecycle as (typeof BEHAVIOR_PHASES)[number],
    ) ||
    !BEHAVIOR_SOURCES.includes(
      value.source as (typeof BEHAVIOR_SOURCES)[number],
    )
  ) {
    return null
  }
  const resources = Array.isArray(value.resources)
    ? value.resources
        .filter((resource): resource is (typeof BEHAVIOR_RESOURCES)[number] =>
          BEHAVIOR_RESOURCES.includes(
            resource as (typeof BEHAVIOR_RESOURCES)[number],
          ),
        )
        .slice(0, 8)
    : []
  return {
    function:
      value.function as RigStateSummary['activeBehaviors'][number]['function'],
    lifecycle:
      value.lifecycle as RigStateSummary['activeBehaviors'][number]['lifecycle'],
    source: value.source as string,
    resources,
    remainingMs: clampMs(value.remainingMs),
  }
}

function resolveActing(
  directive: PerformanceDirective | null,
  behaviors: readonly BehaviorSnapshot[],
): RigStateSummary['acting'] {
  const phase =
    directive && PHASES.includes(directive.phase) ? directive.phase : 'idle'
  const cues = behaviors.filter(
    (behavior) => behavior.form.family === 'performance-cue',
  )
  const live = behaviors.filter(
    (behavior) =>
      behavior.phase !== 'planned' &&
      behavior.phase !== 'complete' &&
      behavior.phase !== 'rejected',
  )
  const active =
    live.find((behavior) => behavior.form.family === 'performance-cue') ??
    live.find(
      (behavior) =>
        behavior.form.family === 'co-speech' && behavior.form.id === 'accent',
    ) ??
    live.find((behavior) => behavior.source === 'coSpeech') ??
    live.find((behavior) => behavior.source === 'performance') ??
    live.find((behavior) => behavior.source === 'music') ??
    live[0]
  if (!active && cues.length === 0) {
    return {
      intent: null,
      phase,
      function: null,
      lifecycle: null,
      remainingMs: 0,
    }
  }
  const timed = cues.length > 0 ? cues : active ? [active] : []
  const remainingMs = timed.reduce(
    (remaining, behavior) => Math.max(remaining, behavior.remainingMs ?? 0),
    0,
  )
  const intent =
    active?.form.family === 'performance-cue' ? active.form.id : null
  return {
    intent: intent !== null && isCueIntent(intent) ? intent : null,
    phase,
    function: active?.function ?? null,
    lifecycle: active?.phase ?? null,
    remainingMs,
  }
}

function musicEnergy(spectrum: SingingSpectrumDrive | null): RigMusicEnergy {
  const amount = Math.max(spectrum?.vocal ?? 0, spectrum?.beat ?? 0)
  if (amount >= 0.6) return 'strong'
  if (amount >= 0.3) return 'present'
  if (amount >= 0.12) return 'soft'
  return 'quiet'
}

function beatPhase(spectrum: SingingSpectrumDrive | null): RigBeatPhase {
  if (!spectrum || spectrum.beat < 0.08) return 'rest'
  if (spectrum.beat >= 0.55) return 'downbeat'
  if (spectrum.vocal >= 0.4) return 'pulse'
  return 'hold'
}

function allowExpression(
  value: unknown,
): PerformanceBaseline['expression'] | null {
  return EXPRESSIONS.includes(value as PerformanceBaseline['expression'])
    ? (value as PerformanceBaseline['expression'])
    : null
}

function allowPosture(value: unknown): PerformanceBaseline['posture'] | null {
  return POSTURES.includes(value as PerformanceBaseline['posture'])
    ? (value as PerformanceBaseline['posture'])
    : null
}

function allowOwner(value: unknown): MotionSourceId {
  return MOTION_SOURCES.includes(value as MotionSourceId)
    ? (value as MotionSourceId)
    : 'idle'
}

function isCueIntent(value: unknown): value is PerformanceCue['intent'] {
  return (
    typeof value === 'string' &&
    [
      'greet',
      'respond',
      'question',
      'delight',
      'emphasize',
      'listen',
      'notify',
      'think',
      'dizzy',
      'cry',
      'angry',
      'speechless',
      'maniac',
      'silly',
      'lovestruck',
    ].includes(value)
  )
}

function allowEnergy(value: unknown): RigMusicEnergy {
  return value === 'soft' || value === 'present' || value === 'strong'
    ? value
    : 'quiet'
}

function allowBeat(value: unknown): RigBeatPhase {
  return value === 'downbeat' || value === 'pulse' || value === 'hold'
    ? value
    : 'rest'
}

function clampMs(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.max(0, Math.min(12_000, Math.round(value)))
    : 0
}

function currentNow(): number {
  return typeof performance === 'undefined' ? Date.now() : performance.now()
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
