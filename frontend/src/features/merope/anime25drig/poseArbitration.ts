import type { MotionChannelPolicy } from '../motion/policy'
import type { Anime25DBehaviorMotionSample } from './behaviorMotion'
import type { PoseOccupancy } from './poseOccupancy'
import {
  allowsAmbientMotion,
  allowsCoSpeechExpression,
  allowsCoSpeechHead,
} from '../motion/policy'

/**
 * Where the coordinator's lease table meets the rig.
 *
 * The channel table in `motion/channels.ts` decided who owns what, and the
 * player then wrote every source into one driver in a fixed order anyway — so
 * ownership described the rig without governing it. `music` sat in the
 * expression priority list as a rule nothing exercised, and three policy
 * predicates had no callers at all.
 *
 * This resolves one weight per (source, channel) from ownership and occupancy
 * together: ownership says who is entitled to the channel, occupancy says how
 * much of it the situation wants. Nothing downstream improvises a scale.
 */

/** Pose channels the compositor arbitrates, grouped by lease. */
export const POSE_CHANNEL_OF_KEY = {
  angleX: 'headBody',
  angleY: 'headBody',
  angleZ: 'headBody',
  body: 'headBody',
  armY: 'headBody',
  armPos: 'headBody',
  eyeX: 'gaze',
  eyeY: 'gaze',
  brow: 'expression',
} as const

export type PoseKey = keyof typeof POSE_CHANNEL_OF_KEY
export type PoseChannel = (typeof POSE_CHANNEL_OF_KEY)[PoseKey]

export const POSE_KEYS = Object.keys(POSE_CHANNEL_OF_KEY) as PoseKey[]

export interface PoseChannelWeights {
  gaze: number
  headBody: number
  expression: number
}

/**
 * A source that has lost a channel is attenuated, not silenced.
 *
 * Occupancy's own rule is that weights tilt rather than exclusive-zero living
 * sources; a hard ownership gate would make the character freeze the instant a
 * plan landed. Losing the lease means stop competing, not stop breathing.
 */
export const UNOWNED_POSE_KEEP = 0.25

export interface PoseGate {
  /** Idle drift: slow head/eye wander. */
  ambient: PoseChannelWeights
  /** Autonomous fidgeting between beats. */
  random: PoseChannelWeights
  /** Music groove. */
  groove: PoseChannelWeights
  /** The constrained thinking loop. */
  thinking: PoseChannelWeights
  /**
   * The director's own plan. It writes eyes as well as brows — `think` looks
   * away — so it answers to the gaze lease like anything else.
   */
  performance: PoseChannelWeights
  /** Staged sticker motion, including renderer-local geometry. */
  stylized: PoseChannelWeights
  /** Brow/eye accents and head nods that ride the voice. */
  coSpeech: PoseChannelWeights
  speechMouth: number
  grooveMouth: number
}

export interface PoseGateScales {
  /** Director attention: a focused baseline quiets ambient motion. */
  performance: number
  /** Sticker faces hold the pose; secondary motion steps back. */
  stylized: number
  /** A large random action damps the ambient drift underneath it. */
  randomAmbient: number
}

const GATE_SOURCES = [
  'ambient',
  'random',
  'groove',
  'thinking',
  'performance',
  'stylized',
  'coSpeech',
] as const
const GATE_CHANNELS = ['gaze', 'headBody', 'expression'] as const
const GATE_ATTACK_FREQUENCY_HZ = 8
const GATE_RELEASE_FREQUENCY_HZ = 4.5

/**
 * Velocity-preserving ownership transition.
 *
 * Lease winners are discrete, but a body cannot change contribution weight in
 * one frame. An exact critically damped step keeps position and velocity
 * continuous while converging quickly enough for interactive reactions.
 */
export class PoseGateController {
  private readonly output = zeroPoseGate()
  private readonly velocity = zeroPoseGate()
  private initialized = false

  sample(deltaSeconds: number, target: Readonly<PoseGate>): Readonly<PoseGate> {
    if (!this.initialized) {
      this.initialized = true
      copyPoseGate(this.output, target)
      return this.output
    }
    const dt = clamp(deltaSeconds, 0, 0.05)
    for (const source of GATE_SOURCES) {
      for (const channel of GATE_CHANNELS) {
        const stepped = stepCritical(
          this.output[source][channel],
          this.velocity[source][channel],
          target[source][channel],
          dt,
        )
        this.output[source][channel] = stepped.value
        this.velocity[source][channel] = stepped.velocity
      }
    }
    for (const mouth of ['speechMouth', 'grooveMouth'] as const) {
      const stepped = stepCritical(
        this.output[mouth],
        this.velocity[mouth],
        target[mouth],
        dt,
      )
      this.output[mouth] = stepped.value
      this.velocity[mouth] = stepped.velocity
    }
    return this.output
  }
}

export function resolvePoseGate(
  policy: MotionChannelPolicy,
  occupancy: Readonly<PoseOccupancy>,
  scales: PoseGateScales,
): PoseGate {
  const ambientScale = occupancy.glance * scales.performance * scales.stylized
  return {
    ambient: idleClassWeights(policy, ambientScale * scales.randomAmbient),
    random: idleClassWeights(
      policy,
      occupancy.random * scales.performance * scales.stylized,
    ),
    groove: musicWeights(policy, occupancy.groove),
    thinking: idleClassWeights(policy, occupancy.thinking),
    performance: performanceWeights(policy),
    stylized: stylizedWeights(policy),
    coSpeech: {
      gaze: 0,
      expression:
        occupancy.coSpeech *
        ownership(policy.expression, allowsCoSpeechExpression),
      headBody:
        occupancy.coSpeech * ownership(policy.headBody, allowsCoSpeechHead),
    },
    speechMouth: occupancy.speechMouth,
    grooveMouth: occupancy.grooveMouth,
  }
}

/** Ambient, random action and thinking are the rig's own idle behaviour. */
function idleClassWeights(
  policy: MotionChannelPolicy,
  amount: number,
): PoseChannelWeights {
  return {
    gaze: amount * ownership(policy.gaze, allowsAmbientMotion),
    headBody: amount * ownership(policy.headBody, allowsAmbientMotion),
    expression: amount * ownership(policy.expression, allowsAmbientMotion),
  }
}

/** Preview and live performance publish through the same directed-pose path. */
function performanceWeights(policy: MotionChannelPolicy): PoseChannelWeights {
  return {
    gaze: ownership(policy.gaze, ownedByPerformance),
    headBody: ownership(policy.headBody, ownedByPerformance),
    expression: ownership(policy.expression, ownedByPerformance),
  }
}

function stylizedWeights(policy: MotionChannelPolicy): PoseChannelWeights {
  return {
    gaze: ownership(policy.gaze, ownedByStylizedExpression),
    headBody: ownership(policy.headBody, ownedByStylizedExpression),
    expression: ownership(policy.expression, ownedByStylizedExpression),
  }
}

function ownedByPerformance(owner: MotionChannelPolicy['mouth']): boolean {
  return owner === 'performance'
}

function ownedByStylizedExpression(
  owner: MotionChannelPolicy['mouth'],
): boolean {
  return ownedByPerformance(owner) || owner === 'preview'
}

function musicWeights(
  policy: MotionChannelPolicy,
  amount: number,
): PoseChannelWeights {
  return {
    gaze: 0,
    headBody: amount * ownership(policy.headBody, ownedByMusic),
    expression: 0,
  }
}

function ownedByMusic(owner: MotionChannelPolicy['mouth']): boolean {
  return owner === 'music'
}

/**
 * Preview is the one genuinely exclusive owner — the workbench must show
 * exactly what it drives, with nothing living underneath it.
 */
function ownership(
  owner: MotionChannelPolicy['mouth'],
  allows: (owner: MotionChannelPolicy['mouth']) => boolean,
): number {
  if (allows(owner)) return 1
  return owner === 'preview' ? 0 : UNOWNED_POSE_KEEP
}

export function poseChannelWeight(
  weights: Readonly<PoseChannelWeights>,
  key: PoseKey,
): number {
  return weights[POSE_CHANNEL_OF_KEY[key]]
}

function zeroPoseGate(): PoseGate {
  const weights = (): PoseChannelWeights => ({
    gaze: 0,
    headBody: 0,
    expression: 0,
  })
  return {
    ambient: weights(),
    random: weights(),
    groove: weights(),
    thinking: weights(),
    performance: weights(),
    stylized: weights(),
    coSpeech: weights(),
    speechMouth: 0,
    grooveMouth: 0,
  }
}

function copyPoseGate(target: PoseGate, source: Readonly<PoseGate>): void {
  for (const layer of GATE_SOURCES) {
    for (const channel of GATE_CHANNELS) {
      target[layer][channel] = source[layer][channel]
    }
  }
  target.speechMouth = source.speechMouth
  target.grooveMouth = source.grooveMouth
}

function stepCritical(
  value: number,
  velocity: number,
  target: number,
  dt: number,
): { value: number; velocity: number } {
  if (dt <= 0) return { value, velocity }
  // Bring the newly entitled source in quickly while the previous source
  // releases at the calmer rate. Their overlap is a smooth crossfade, not an
  // extra wait before the reaction becomes visible.
  const frequency =
    target > value ? GATE_ATTACK_FREQUENCY_HZ : GATE_RELEASE_FREQUENCY_HZ
  const omega = Math.PI * 2 * frequency
  const displacement = value - target
  const coefficient = velocity + omega * displacement
  const decay = Math.exp(-omega * dt)
  const nextValue = target + (displacement + coefficient * dt) * decay
  const nextVelocity = (velocity - omega * coefficient * dt) * decay
  return {
    value: clamp(nextValue, 0, 1),
    velocity: nextValue < 0 || nextValue > 1 ? 0 : nextVelocity,
  }
}

function clamp(value: number, minimum: number, maximum: number): number {
  if (!Number.isFinite(value)) return minimum
  return Math.max(minimum, Math.min(maximum, value))
}

/**
 * Behavior units modulate how big a motion is; they do not decide whether it
 * exists. Occupancy already resolved who owns the channel — multiplying by a
 * missing unit makes it a second, independent kill switch, and then any hiccup
 * upstream (a plan that ends before the mouth does, a rejected claim, a surface
 * with no music source) reads as a talking head on a frozen body.
 *
 * "The mouth still moves" is not a defence: the mouth moving while the body is
 * dead is the exact symptom this returns 1 to prevent.
 *
 * The ceiling is 1 because this scales a weight, and a weight above 1 lets one
 * source write past its authored offset into the shared accumulator. A strong
 * unit therefore opens its channel fully and stops there. That costs nothing:
 * quality already reaches the pose generators it belongs to — the co-speech
 * controller and the groove take `coSpeechQuality` and `musicQuality` directly
 * — so a boost here would scale the same extent and power a second time.
 */
export function behaviorMotionScale(extent: number, power: number): number {
  if (extent <= 0) return 1
  return Math.min(1, extent * (0.82 + power * 0.18))
}

export function applyBehaviorMotionGate(
  gate: PoseGate,
  motion: Readonly<Anime25DBehaviorMotionSample>,
): PoseGate {
  const coSpeech = behaviorMotionScale(motion.coSpeech, motion.coSpeechPower)
  const music = behaviorMotionScale(motion.music, motion.musicPower)
  gate.coSpeech.gaze *= coSpeech
  gate.coSpeech.headBody *= coSpeech
  gate.coSpeech.expression *= coSpeech
  gate.groove.gaze *= music
  gate.groove.headBody *= music
  gate.groove.expression *= music
  return gate
}
