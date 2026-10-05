import type { BodyControl, BodyPose } from '../../../services/agent/types'
import type { MotionChannelPolicy } from '../motion/policy'
import type { ResolvedScore } from '../motion/scoreTimeline'
import type { Anime25DDriver } from './driver'
import type { PerformanceExpressionOffset } from './performanceExpressionShared'
import { BODY_CONTROLS, sanitizeBodyPose } from '../events/performanceContract'
import { ActingField, mappedControlValue } from './actingField'

// Semantic goals are mapped only here, never interpreted as arbitrary drivers.
export const BODY_CONTROL_DRIVERS = {
  headTurn: 'angleX', headNod: 'angleY', headTilt: 'angleZ',
  torsoTurn: 'torsoTurn', torsoLean: 'body', torsoRise: 'bodyLift', torsoPitch: 'bodyPitch',
  gazeHorizontal: 'eyeX', gazeVertical: 'eyeY',
  leftArmRaise: 'armRaiseL', rightArmRaise: 'armRaiseR', leftArmSwing: 'armSwingL', rightArmSwing: 'armSwingR',
  eyeOpenLeft: 'eyeOpenL', eyeOpenRight: 'eyeOpenR', eyeSmile: 'eyeSmile', eyeWide: 'eyeWide',
  browLift: 'brow', browTiltLeft: 'browAngL', browTiltRight: 'browAngR',
  mouthSmile: 'mouthForm', mouthOpen: 'mouthOpen', mouthWide: 'mouthWide', mouthRound: 'mouthRound',
  mouthNarrow: 'mouthNarrow', mouthSeal: 'mouthSeal',
  angry: 'anger', speechless: 'speechless', maniac: 'maniac', silly: 'silly', lovestruck: 'lovestruck',
  cry: 'eyeCry', dizzy: 'eyeDizzy', hairSway: 'fhAmp', chestSway: 'bust',
} as const satisfies Record<BodyControl, keyof Anime25DDriver>

/** These drivers are directed contributions to geometry's automatic posture. */
export const INDEPENDENT_BODY_CONTROLS = [
  'torsoTurn', 'torsoRise', 'torsoPitch',
  'leftArmRaise', 'rightArmRaise', 'leftArmSwing', 'rightArmSwing',
] as const satisfies readonly BodyControl[]
export type IndependentBodyControl = (typeof INDEPENDENT_BODY_CONTROLS)[number]

interface Axis {
  value: number
  velocity: number
  weight: number
  weightVelocity: number
}

/** Full target restatements with continuous retarget/release, on the player's clock. */
export class DirectedPoseController {
  private pose: BodyPose | null = null
  private expiresAt = Infinity
  private readonly axes = new Map<BodyControl, Axis>()
  private capabilities = new Set<string>()
  private transitionSeconds = 0.3
  private policy: MotionChannelPolicy | null = null
  private touchShare = 0
  private transientShare = 0
  private readonly transientAxes = new Set<BodyControl>()
  /** The director's scores over the standing pose, each beat by its own envelope. */
  private readonly acting = new ActingField()

  setPolicy(policy: MotionChannelPolicy): void { this.policy = policy }

  setTouchShare(share: number): void {
    this.touchShare = Number.isFinite(share) ? Math.max(0, Math.min(1, share)) : 0
  }

  /**
   * A short expression only borrows the axes it actually moves. The held
   * goals keep evolving underneath it and do not restart on recovery.
   */
  setTransientMotion(offset: Readonly<PerformanceExpressionOffset>, share: number): void {
    this.transientShare = Number.isFinite(share) ? Math.max(0, Math.min(1, share)) : 0
    this.transientAxes.clear()
    if (this.transientShare === 0) return
    for (const key of this.axes.keys()) {
      const source = transientOffsetKey(key)
      if (source && Math.abs(offset[source] ?? 0) > 1e-6) this.transientAxes.add(key)
    }
  }

  setCapabilities(capabilities: readonly string[]): void {
    this.capabilities = new Set(capabilities)
  }

  set(pose: BodyPose | null | undefined, time: number): void {
    const next = sanitizeBodyPose(pose)
    if (next && this.pose && time < this.expiresAt && poseKey(next) === poseKey(this.pose)) return
    this.pose = next
    if (next) this.transitionSeconds = next.transitionMs / 1000
    this.expiresAt = next && next.holdMs > 0 ? time + next.holdMs / 1000 : Infinity
  }

  setScore(score: Readonly<ResolvedScore>, time: number, nowMs: number): void {
    this.acting.setScore(score, time, nowMs)
  }

  setSpeaking(speaking: boolean): void {
    this.acting.setSpeaking(speaking)
  }

  /** How much directed motion, standing pose or score, drives a control now. */
  weight(control: BodyControl): number {
    return Math.max(
      this.heldWeight(control),
      (Math.max(this.acting.values.get(control)?.weight ?? 0, this.acting.moveWeights.get(control) ?? 0)) *
        (1 - this.interruption(control)),
    )
  }

  private heldWeight(control: BodyControl): number {
    return (this.axes.get(control)?.weight ?? 0) * (1 - this.interruption(control))
  }

  private interruption(control: BodyControl): number {
    if (this.policy?.[controlChannel(control)] === 'preview') return 1
    return this.policy?.[controlChannel(control)] === 'performance'
      ? Math.max(this.touchShare, this.transientAxes.has(control) ? this.transientShare : 0) : 0
  }

  apply(dt: number, time: number, target: Anime25DDriver, vocalizing: boolean): void {
    if (!(dt > 0) || !Number.isFinite(dt)) return
    this.step(dt, time, target)
    this.write(target, vocalizing)
  }

  /** Advance once; expression previews and final composition read the same state. */
  step(dt: number, time: number, target: Readonly<Anime25DDriver>): void {
    if (!(dt > 0) || !Number.isFinite(dt)) return
    if (time >= this.expiresAt) this.pose = null
    this.acting.step(dt, time, (control) => this.capabilities.has(BODY_CONTROLS[control].capability))
    const goals = this.pose?.targets ?? {}
    for (const key of Object.keys(goals) as BodyControl[]) {
      if (!this.axes.has(key) && this.capabilities.has(BODY_CONTROLS[key].capability)) {
        this.axes.set(key, { value: target[BODY_CONTROL_DRIVERS[key]], velocity: 0, weight: 0, weightVelocity: 0 })
      }
    }
    const omega = 6 / this.transitionSeconds
    for (const [key, axis] of this.axes) {
      const driver = BODY_CONTROL_DRIVERS[key]
      if (this.policy?.[controlChannel(key)] === 'preview') continue
      const allowed = this.capabilities.has(BODY_CONTROLS[key].capability)
      const goal = allowed ? goals[key] : undefined
      const active = goal !== undefined
      const value = active ? mappedControlValue(key, goal) : target[driver]
      spring(axis, 'value', 'velocity', value, omega, dt)
      spring(axis, 'weight', 'weightVelocity', active ? 1 : 0, omega, dt)
      axis.weight = Math.max(0, Math.min(1, axis.weight))
      if (!active && axis.weight < 0.0001) { this.axes.delete(key); continue }
    }
  }

  /** Compose into a target; expression preview and final output use separate targets. */
  write(target: Anime25DDriver, vocalizing: boolean): void {
    for (const [key, axis] of this.axes) {
      const driver = BODY_CONTROL_DRIVERS[key]
      if (this.policy?.[controlChannel(key)] === 'preview') continue
      // Articulation always follows actual speech/audio. Resting mouth directions
      // remain alive, so they can resume rather than restart after speech ends.
      if (vocalizing && isArticulation(key)) continue
      // A held goal is not an interaction lock: immediate contact reactions
      // temporarily take their existing performance share, then the goal resumes.
      const weight = this.heldWeight(key)
      target[driver] += (axis.value - target[driver]) * weight
      if (key === 'hairSway') target.physAmp += (axis.value * 0.5 - target.physAmp) * weight
    }
    // The director's beats, over the standing pose, each as strongly as it plays.
    for (const [key, sample] of this.acting.values) {
      if (vocalizing && isArticulation(key)) continue
      const weight = sample.weight * (1 - this.interruption(key))
      target[BODY_CONTROL_DRIVERS[key]] += (sample.value - target[BODY_CONTROL_DRIVERS[key]]) * weight
    }
    for (const [key, offset] of this.acting.offsets) {
      const driver = BODY_CONTROL_DRIVERS[key]
      target[driver] += mappedControlValue(key, offset) * (1 - this.interruption(key))
      if (key === 'eyeOpenLeft' || key === 'eyeOpenRight') target[driver] = Math.max(0, Math.min(1, target[driver]))
    }
  }
}

/** Speech keeps the mouth while she vocalizes. */
function isArticulation(key: BodyControl): boolean {
  return key === 'maniac' || ['mouthOpen', 'mouthWide', 'mouthRound', 'mouthNarrow', 'mouthSeal'].includes(key)
}

function poseKey(pose: BodyPose): string {
  return JSON.stringify([pose.transitionMs, pose.holdMs, Object.entries(pose.targets).sort(([a], [b]) => a.localeCompare(b))])
}

function controlChannel(key: BodyControl): keyof MotionChannelPolicy {
  if (key.startsWith('gaze')) return 'gaze'
  if (['mouthOpen', 'mouthWide', 'mouthRound', 'mouthNarrow', 'mouthSeal'].includes(key)) return 'mouth'
  if (key.startsWith('head') || key.startsWith('torso') || key.includes('Arm') || key.endsWith('Sway')) return 'headBody'
  return 'expression'
}

function transientOffsetKey(key: BodyControl): keyof PerformanceExpressionOffset | null {
  if (key === 'eyeOpenLeft' || key === 'eyeOpenRight') return 'eyeOpen'
  if (key === 'browTiltLeft' || key === 'browTiltRight') return 'browAngSym'
  if (key === 'leftArmRaise' || key === 'rightArmRaise') return 'armY'
  if (key === 'leftArmSwing' || key === 'rightArmSwing') return 'armPos'
  if (['headTurn', 'torsoTurn', 'torsoRise', 'torsoPitch', 'hairSway',
    'mouthOpen', 'mouthWide', 'mouthRound', 'mouthNarrow', 'mouthSeal'].includes(key)) { return null
}
  return BODY_CONTROL_DRIVERS[key] as keyof PerformanceExpressionOffset
}

function spring(axis: Axis, position: 'value' | 'weight', velocity: 'velocity' | 'weightVelocity', goal: number, omega: number, dt: number): void {
  const displacement = axis[position] - goal
  const coefficient = axis[velocity] + omega * displacement
  const decay = Math.exp(-omega * dt)
  axis[position] = goal + (displacement + coefficient * dt) * decay
  axis[velocity] = (axis[velocity] - omega * coefficient * dt) * decay
}
