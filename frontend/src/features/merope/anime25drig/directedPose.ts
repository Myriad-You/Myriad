import type { BodyControl, BodyPose, ScoreMove } from '../../../services/agent/types'
import type { MotionChannelPolicy } from '../motion/policy'
import type { ResolvedScore } from '../motion/scoreTimeline'
import type { Anime25DDriver } from './driver'
import type { PerformanceExpressionOffset } from './performanceExpressionShared'
import { BODY_CONTROLS, sanitizeBodyPose } from '../events/performanceContract'
import { scoreMoveOffsets, scoreMoveSeconds } from './scoreMoves'

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

/** One control's target from one beat of the score. */
interface Keyframe {
  id: string
  scoreId: number
  at: number
  value: number
  transitionSeconds: number
  until: number
}

interface ScheduledMove {
  id: string
  scoreId: number
  start: number
  move: ScoreMove
}

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
  /**
   * The score over the standing pose: per control, targets that take over
   * from their beat until a later beat revises them or they run out, and
   * moves that go and come back over whatever is held.
   */
  private readonly keyframes = new Map<BodyControl, Keyframe[]>()
  private moves: ScheduledMove[] = []
  private scoreId = 0
  private readonly moveOffsets = new Map<BodyControl, number>()
  private readonly moveWeights = new Map<BodyControl, number>()

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

  /**
   * Beats of the current score, moved from the clock (`nowMs`) onto the
   * player's (`time`). A new score drops what an older one had not begun;
   * anything under way finishes. A beat already known is only moved while it
   * has not started.
   */
  setScore(score: Readonly<ResolvedScore>, time: number, nowMs: number): void {
    const scoreId = score.id
    const beats = score.beats.map((beat) => ({ ...beat, at: time + (beat.atMs - nowMs) / 1000 }))
    if (scoreId !== this.scoreId) {
      for (const [key, frames] of this.keyframes) this.keyframes.set(key, frames.filter((frame) => frame.at <= time))
      this.moves = this.moves.filter((move) => move.start <= time)
      this.scoreId = scoreId
    }
    for (const beat of beats) {
      if (beat.pose) {
        const transitionSeconds = beat.pose.transitionMs / 1000
        const until = beat.pose.holdMs > 0 ? beat.at + beat.pose.holdMs / 1000 : Infinity
        for (const [key, value] of Object.entries(beat.pose.targets) as Array<[BodyControl, number]>) {
          const frames = this.keyframes.get(key) ?? []
          const known = frames.find((frame) => frame.id === beat.id)
          if (known) {
            if (known.at > time) Object.assign(known, { at: beat.at, until })
          } else {
            frames.push({ id: beat.id, scoreId, at: beat.at, value: mappedValue(key, value), transitionSeconds, until })
          }
          frames.sort((a, b) => a.at - b.at)
          this.keyframes.set(key, frames)
        }
      }
      if (beat.move) {
        const known = this.moves.find((move) => move.id === beat.id)
        if (known) {
          if (known.start > time) known.start = beat.at
        } else {
          this.moves.push({ id: beat.id, scoreId, start: beat.at, move: beat.move })
        }
      }
    }
  }

  /** How much directed motion, held or moving, drives a control now. */
  weight(control: BodyControl): number {
    if (this.policy?.[controlChannel(control)] === 'preview') return 0
    const interruption = this.policy?.[controlChannel(control)] === 'performance'
      ? Math.max(this.touchShare, this.transientAxes.has(control) ? this.transientShare : 0) : 0
    return Math.max(this.axes.get(control)?.weight ?? 0, this.moveWeights.get(control) ?? 0) * (1 - interruption)
  }

  /** The score's current target for a control, if a beat holds it now. */
  private scoreGoal(key: BodyControl, time: number): Keyframe | null {
    const frames = this.keyframes.get(key)
    if (!frames) return null
    let current: Keyframe | null = null
    for (const frame of frames) {
      if (frame.at > time) break
      current = frame
    }
    // Keep only the frame in force and those to come.
    const kept = frames.filter((frame) => frame === current || frame.at > time)
    if (kept.length !== frames.length) {
      if (kept.length === 0) this.keyframes.delete(key)
      else this.keyframes.set(key, kept)
    }
    return current && time < current.until ? current : null
  }

  private stepMoves(time: number): void {
    this.moveOffsets.clear()
    this.moveWeights.clear()
    this.moves = this.moves.filter((scheduled) => time < scheduled.start + scoreMoveSeconds(scheduled.move))
    const offsets: Partial<Record<BodyControl, number>> = {}
    for (const scheduled of this.moves) {
      if (time < scheduled.start) continue
      for (const key of Object.keys(offsets) as BodyControl[]) delete offsets[key]
      const envelope = scoreMoveOffsets(scheduled.move, time - scheduled.start, offsets)
      for (const [key, offset] of Object.entries(offsets) as Array<[BodyControl, number]>) {
        if (!this.capabilities.has(BODY_CONTROLS[key].capability)) continue
        this.moveOffsets.set(key, (this.moveOffsets.get(key) ?? 0) + offset)
        this.moveWeights.set(key, Math.max(this.moveWeights.get(key) ?? 0, envelope))
      }
    }
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
    const goals = this.pose?.targets ?? {}
    const scored = new Map<BodyControl, Keyframe>()
    for (const key of this.keyframes.keys()) {
      const goal = this.scoreGoal(key, time)
      if (goal) scored.set(key, goal)
    }
    for (const key of [...Object.keys(goals) as BodyControl[], ...scored.keys()]) {
      if (!this.axes.has(key) && this.capabilities.has(BODY_CONTROLS[key].capability)) {
        this.axes.set(key, { value: target[BODY_CONTROL_DRIVERS[key]], velocity: 0, weight: 0, weightVelocity: 0 })
      }
    }
    this.stepMoves(time)
    for (const [key, axis] of this.axes) {
      const driver = BODY_CONTROL_DRIVERS[key]
      if (this.policy?.[controlChannel(key)] === 'preview') continue
      const allowed = this.capabilities.has(BODY_CONTROLS[key].capability)
      // A beat of the score takes over from the standing pose while it holds.
      const beat = allowed ? scored.get(key) : undefined
      const goal = allowed ? goals[key] : undefined
      const active = beat !== undefined || goal !== undefined
      const value = beat ? beat.value : goal !== undefined ? mappedValue(key, goal) : target[driver]
      const omega = 6 / (beat ? beat.transitionSeconds : this.transitionSeconds)
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
      if (vocalizing && (key === 'maniac' || ['mouthOpen', 'mouthWide', 'mouthRound', 'mouthNarrow', 'mouthSeal'].includes(key))) continue
      // A held goal is not an interaction lock: immediate contact reactions
      // temporarily take their existing performance share, then the goal resumes.
      const weight = this.heldWeight(key)
      target[driver] += (axis.value - target[driver]) * weight
      if (key === 'hairSway') target.physAmp += (axis.value * 0.5 - target.physAmp) * weight
    }
    // Moves go and come back over whatever is held.
    for (const [key, offset] of this.moveOffsets) {
      if (this.policy?.[controlChannel(key)] === 'preview') continue
      const interruption = this.policy?.[controlChannel(key)] === 'performance'
        ? Math.max(this.touchShare, this.transientAxes.has(key) ? this.transientShare : 0) : 0
      const driver = BODY_CONTROL_DRIVERS[key]
      target[driver] += mappedValue(key, offset) * (1 - interruption)
      if (key === 'eyeOpenLeft' || key === 'eyeOpenRight') target[driver] = Math.max(0, Math.min(1, target[driver]))
    }
  }

  /** How much a held goal (standing pose or score) drives its control now. */
  private heldWeight(control: BodyControl): number {
    if (this.policy?.[controlChannel(control)] === 'preview') return 0
    const interruption = this.policy?.[controlChannel(control)] === 'performance'
      ? Math.max(this.touchShare, this.transientAxes.has(control) ? this.transientShare : 0) : 0
    return (this.axes.get(control)?.weight ?? 0) * (1 - interruption)
  }
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

function mappedValue(key: BodyControl, value: number): number {
  // Semantic up is positive; the renderer's image-space Y points down.
  if (key === 'gazeVertical') return -value
  if (key === 'hairSway') return value * 1.5
  if (key === 'chestSway') return value * 4
  return value
}

function spring(axis: Axis, position: 'value' | 'weight', velocity: 'velocity' | 'weightVelocity', goal: number, omega: number, dt: number): void {
  const displacement = axis[position] - goal
  const coefficient = axis[velocity] + omega * displacement
  const decay = Math.exp(-omega * dt)
  axis[position] = goal + (displacement + coefficient * dt) * decay
  axis[velocity] = (axis[velocity] - omega * coefficient * dt) * decay
}
