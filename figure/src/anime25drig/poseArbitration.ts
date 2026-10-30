import type { MotionChannelPolicy } from '../motion/policy'
import type { Anime25DBehaviorMotionSample } from './behaviorMotion'
import type { PoseOccupancy } from './poseOccupancy'
import {
  allowsAmbientMotion,
  allowsCoSpeechExpression,
  allowsCoSpeechHead,
} from '../motion/policy'

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

export const UNOWNED_POSE_KEEP = 0.25

export interface PoseGate {
  ambient: PoseChannelWeights
  random: PoseChannelWeights
  groove: PoseChannelWeights
  thinking: PoseChannelWeights
  performance: PoseChannelWeights
  stylized: PoseChannelWeights
  coSpeech: PoseChannelWeights
  speechMouth: number
  grooveMouth: number
}

export interface PoseGateScales {
  touch?: number
  performance: number
  stylized: number
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

/** Lease winners are discrete, but a body cannot change contribution weight in one frame. */
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
  const touch = clamp(scales.touch ?? 0, 0, 1)
  const share = (owner: MotionChannelPolicy['headBody'], keep: number, normal: number) =>
    owner === 'performance' ? normal + (keep - normal) * touch : normal
  return {
    ambient: idleClassWeights(policy, ambientScale * scales.randomAmbient),
    random: idleClassWeights(
      policy,
      occupancy.random * scales.performance * scales.stylized,
    ),
    groove: { ...musicWeights(policy, occupancy.groove),
      headBody: occupancy.groove * share(policy.headBody, 0.75, ownership(policy.headBody, ownedByMusic)) },
    thinking: idleClassWeights(policy, occupancy.thinking),
    performance: performanceWeights(policy),
    stylized: stylizedWeights(policy),
    coSpeech: {
      gaze: 0,
      expression:
        occupancy.coSpeech *
        share(policy.expression, 0.85, ownership(policy.expression, allowsCoSpeechExpression)),
      headBody:
        occupancy.coSpeech * share(policy.headBody, 0.65, ownership(policy.headBody, allowsCoSpeechHead)),
    },
    speechMouth: occupancy.speechMouth,
    grooveMouth: occupancy.grooveMouth,
  }
}

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
  // Their overlap is a smooth crossfade, not an extra wait before the reaction becomes visible.
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

export function behaviorMotionScale(extent: number, power: number): number {
  if (extent <= 0) return 1
  return Math.min(1, extent * (0.82 + power * 0.18))
}

export function applyBehaviorMotionGate(
  gate: PoseGate,
  motion: Readonly<Omit<Anime25DBehaviorMotionSample, 'coSpeechGesture'>>,
): PoseGate {
  const coSpeech = behaviorMotionScale(motion.coSpeech, motion.coSpeechPower)
  const music = behaviorMotionScale(motion.music, motion.musicPower)
  gate.coSpeech.gaze *= coSpeech
  gate.coSpeech.headBody *= coSpeech
  gate.coSpeech.expression *= coSpeech
  gate.groove.gaze *= music
  gate.groove.headBody *= music
  gate.groove.expression *= music
  if (motion.music > 0 && motion.musicMode === 'settle') {
    // Attentive stilling is a body decision, not a frozen face.
    gate.ambient.headBody *= 0.18
    gate.random.headBody *= 0.18
  }
  return gate
}
