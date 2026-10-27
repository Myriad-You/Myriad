import type { BodyControl, BodyPose } from '../../../services/agent/types'
import type { FaceLife, FaceShift } from '../events/performanceContract'
import type { MotionChannelPolicy } from '../motion/policy'
import type { ResolvedScore } from '../motion/scoreTimeline'
import type { Anime25DDriver } from './driver'
import type { PerformanceExpressionOffset } from './performanceExpressionShared'
import { BODY_CONTROLS, faceLife, facePose, faceShift, sanitizeBodyPose } from '../events/performanceContract'
import { ActingField, mappedControlValue } from './actingField'
import { ARCS, carriesMomentum, shiftTiming } from './faceShift'
import { STANDING_IDLE_ROLL_SHARE } from './standing'

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
  musicNotes: 'accentNotes', questionMark: 'accentQuestion', sleepZ: 'accentSleep',
  sparkles: 'accentSparkle', blush: 'accentBlush',
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
  /** Its own pace and start after a change of face (shiftTiming), else the pose's. */
  omega?: number
  startAt?: number
  /** Its damping ratio, below 1 to go past the goal and back. */
  zeta?: number
  /** A wind-up: velocity given the other way as it sets off. */
  kick?: number
  /** The way it set off on, for the arc it bows another control by. */
  from?: number
  to?: number
  arc?: number
}

interface PendingShift {
  controls: Set<BodyControl>
  shift: FaceShift
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
  private readonly acting: ActingField
  constructor(private readonly random: () => number = Math.random) {
    this.acting = new ActingField(random)
  }

  /** A standing figure: the head tilts its looks sway with take their idle share (STANDING_IDLE_ROLL_SHARE). */
  private standing = false

  setStanding(standing: boolean): void { this.standing = standing }

  private lifeOf(face: string | null | undefined): FaceLife | null {
    const life = faceLife(face)
    return life && this.standing ? standingLife(life) : life
  }

  /** A change passing through looks on the way: the ones still ahead, where it ends, and when to turn next. */
  private route: { stops: string[]; pose: BodyPose | null; face: string | null; shift: string | null; at: number } | null = null
  /** A change of face to carry on from where she is, applied on the next step. */
  private shifting: PendingShift | null = null
  /** What the composition last gave each control, and how fast it was moving (driver units). */
  private readonly shown = new Map<BodyControl, { value: number; velocity: number }>()
  private stepTime = 0
  private shownTime = 0

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

  /**
   * The held pose; with `face`, also the look her face holds (null for none),
   * whose life plays over it. With `shift` (how she gets there, faceShifts)
   * the change starts from where every control is and how it is moving, not
   * from the old goals: nothing jumps, fades or plays out.
   */
  set(
    pose: BodyPose | null | undefined,
    time: number,
    face?: string | null,
    shift?: string | null,
    via?: readonly string[] | null,
    onTheWay = false,
  ): void {
    this.route = null
    // Some changes pass through other looks on the way: she heads for each in
    // turn and turns to the next before she gets there, so the way bends
    // through them instead of stopping; a look passed through plays no life.
    const stops = (via ?? []).filter((stop) => stop !== face && facePose(stop))
    if (stops.length) {
      const [first, ...rest] = stops
      this.set(facePose(first), time, null, shift ?? null, null, onTheWay)
      const way = faceShift(shift)
      this.route = { stops: rest, pose: pose ?? null, face: face ?? null, shift: shift ?? null, at: time + ((onTheWay ? 0 : way.pauseMs) + way.passMs) / 1000 }
      return
    }
    const dropped = face !== undefined ? this.acting.setFace(face, this.lifeOf(face), time) : []
    const next = sanitizeBodyPose(pose)
    const same = Boolean(next && this.pose && time < this.expiresAt && poseKey(next) === poseKey(this.pose))
    if (shift !== undefined && (!same || dropped.length)) {
      this.shiftFrom([...Object.keys(this.pose?.targets ?? {}), ...Object.keys(next?.targets ?? {}), ...dropped] as BodyControl[], shift, onTheWay)
    } else if (shift === undefined && !same) {
      for (const axis of this.axes.values()) {
        delete axis.omega; delete axis.startAt; delete axis.zeta; delete axis.kick; delete axis.from; delete axis.to; delete axis.arc
      }
    }
    if (same) return
    this.pose = next
    if (next) this.transitionSeconds = next.transitionMs / 1000
    this.expiresAt = next && next.holdMs > 0 ? time + next.holdMs / 1000 : Infinity
  }

  /** On the way through a look, she neither pauses again nor winds up again nor starts again. */
  private shiftFrom(controls: readonly BodyControl[], shift: string | null, onTheWay = false): void {
    const way = faceShift(shift)
    const pending = this.shifting ?? { controls: new Set<BodyControl>(), shift: way }
    for (const control of controls) pending.controls.add(control)
    pending.shift = onTheWay ? { ...way, pauseMs: 0, anticipation: 0, move: undefined } : way
    this.shifting = pending
  }

  setScore(score: Readonly<ResolvedScore>, time: number, nowMs: number): void {
    this.acting.setScore(score, time, nowMs)
  }

  setSpeaking(speaking: boolean): void {
    this.acting.setSpeaking(speaking)
  }

  /** Workbench: her face takes a look on its own, as from the director, without a bearing, by way of `via`. */
  preview(face: string | null, shift: string | null, time: number, via?: readonly string[]): void {
    this.set(face ? facePose(face) : null, time, face, shift, via)
  }

  /** The look her face holds, whose life plays over it while it holds; what it was doing settles from where it is. */
  setFace(face: string | null | undefined, time: number): void {
    const dropped = this.acting.setFace(face ?? null, this.lifeOf(face), time)
    if (dropped.length) this.shiftFrom(dropped, null)
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
    this.write(target, vocalizing, undefined, true)
  }

  /** Advance once; expression previews and final composition read the same state. */
  step(dt: number, time: number, target: Readonly<Anime25DDriver>): void {
    if (!(dt > 0) || !Number.isFinite(dt)) return
    if (this.route && time >= this.route.at) {
      const { stops, pose, face, shift } = this.route
      this.set(pose, time, face, shift, stops, true)
    }
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
      // Before its turn in a change of face, a control eases to a stop where it is.
      const waiting = axis.startAt !== undefined && time < axis.startAt
      if (!waiting && axis.kick) {
        axis.velocity += axis.kick
        axis.kick = 0
      }
      const rate = axis.omega ?? omega
      spring(axis, 'value', 'velocity', waiting ? axis.value : value, rate, dt, axis.zeta)
      spring(axis, 'weight', 'weightVelocity', active || waiting ? 1 : 0, rate, dt)
      axis.weight = Math.max(0, Math.min(1, axis.weight))
      if (!active && axis.weight < 0.0001) { this.axes.delete(key); continue }
    }
    this.stepTime = time
    if (this.shifting) this.carryOn(this.shifting, time, target)
    this.shifting = null
  }

  /**
   * A change of face from where she is: each control's held axis takes the
   * value the composition showed last, moving as fast as it was, so with the
   * last look's life gone the output does not move this frame; from there it
   * sets off toward its new goal (or back to her own pose) on its own pace
   * and turn (shiftTiming).
   */
  private carryOn(pending: Readonly<PendingShift>, time: number, target: Readonly<Anime25DDriver>): void {
    const elapsed = Math.max(0, time - this.shownTime)
    for (const key of pending.controls) {
      if (!this.capabilities.has(BODY_CONTROLS[key].capability)) continue
      const held = 1 - this.interruption(key)
      if (held < 0.02) continue
      const driver = BODY_CONTROL_DRIVERS[key]
      const base = target[driver]
      const shown = this.shown.get(key)
      const desired = shown ? shown.value + shown.velocity * elapsed : base
      // Undo what still lies over the axis: the remaining beats, then the moves.
      const sample = this.acting.values.get(key)
      const over = sample ? sample.weight * held : 0
      if (over > 0.98) continue
      const offset = mappedControlValue(key, this.acting.offsets.get(key) ?? 0) * held
      const under = (desired - offset - (sample?.value ?? 0) * over) / (1 - over)
      const axis = this.axes.get(key) ?? { value: base, velocity: 0, weight: 0, weightVelocity: 0 }
      axis.value = base + (under - base) / held
      axis.velocity = shown?.velocity ?? 0
      axis.weight = 1
      axis.weightVelocity = 0
      const goal = this.pose?.targets[key]
      const to = goal !== undefined ? mappedControlValue(key, goal) : base
      const timing = shiftTiming(pending.shift, key, axis.value, to)
      axis.omega = timing.omega
      axis.startAt = time + timing.delay
      if (carriesMomentum(key)) {
        axis.zeta = pending.shift.damping
        axis.kick = -pending.shift.anticipation * (to - axis.value) * timing.omega
        axis.from = axis.value
        axis.to = to
        axis.arc = pending.shift.arc * (this.random() < 0.5 ? -1 : 1)
      } else {
        delete axis.zeta; delete axis.kick; delete axis.from; delete axis.to; delete axis.arc
      }
      this.axes.set(key, axis)
    }
    if (pending.shift.move) this.acting.addMove(`shift:${time}`, pending.shift.move, time)
  }

  /** The lids are on their way open or shut: a blink on top would read as a flutter. */
  lidsMoving(): boolean {
    return (['eyeOpenLeft', 'eyeOpenRight'] as const).some((key) => Math.abs(this.shown.get(key)?.velocity ?? 0) > LIDS_MOVING)
  }

  /** The bow each control's way takes off a straight line, onto another control (ARCS). */
  private arcs(): Map<BodyControl, number> {
    const bows = new Map<BodyControl, number>()
    for (const [key, axis] of this.axes) {
      const arc = ARCS[key]
      if (!arc || axis.from === undefined || axis.to === undefined || !axis.arc) continue
      const way = axis.to - axis.from
      if (Math.abs(way) < 0.02) continue
      const done = Math.max(0, Math.min(1, (axis.value - axis.from) / way))
      const bow = Math.abs(way) * Math.abs(axis.arc) * arc.share * (arc.sided ? Math.sign(axis.arc) : 1) * Math.sin(Math.PI * done)
      bows.set(arc.onto, (bows.get(arc.onto) ?? 0) + bow * axis.weight)
    }
    return bows
  }

  /**
   * Compose into a target; expression preview and final output use separate targets.
   * What is `riding` (her breath, the beats of her voice) stays on top of a directed
   * axis instead of being held still with the rest: a held tilt still nods as she talks.
   */
  write(target: Anime25DDriver, vocalizing: boolean, riding?: Readonly<Partial<Record<keyof Anime25DDriver, number>>>, final = riding !== undefined): void {
    for (const [key, axis] of this.axes) {
      const driver = BODY_CONTROL_DRIVERS[key]
      if (this.policy?.[controlChannel(key)] === 'preview') continue
      // Articulation always follows actual speech/audio. Resting mouth directions
      // remain alive, so they can resume rather than restart after speech ends.
      if (vocalizing && isArticulation(key)) continue
      // A held goal is not an interaction lock: immediate contact reactions
      // temporarily take their existing performance share, then the goal resumes.
      const weight = this.heldWeight(key)
      target[driver] += (axis.value - target[driver] + (riding?.[driver] ?? 0)) * weight
      if (key === 'hairSway') target.physAmp += (axis.value * 0.5 - target.physAmp) * weight
    }
    // The director's beats, over the standing pose, each as strongly as it plays.
    for (const [key, sample] of this.acting.values) {
      if (vocalizing && isArticulation(key)) continue
      const driver = BODY_CONTROL_DRIVERS[key]
      const weight = sample.weight * (1 - this.interruption(key))
      target[driver] += (sample.value - target[driver] + (riding?.[driver] ?? 0)) * weight
      if (key === 'hairSway') target.physAmp += (sample.value * 0.5 - target.physAmp) * weight
    }
    for (const [key, offset] of this.acting.offsets) {
      const driver = BODY_CONTROL_DRIVERS[key]
      target[driver] += mappedControlValue(key, offset) * (1 - this.interruption(key))
      if (key === 'eyeOpenLeft' || key === 'eyeOpenRight') target[driver] = Math.max(0, Math.min(1, target[driver]))
    }
    for (const [key, bow] of this.arcs()) {
      if (!this.capabilities.has(BODY_CONTROLS[key].capability)) continue
      target[BODY_CONTROL_DRIVERS[key]] += mappedControlValue(key, bow) * (1 - this.interruption(key))
    }
    if (final) this.remember(target)
  }

  /** What the final composition shows, for a change of face to carry on from. */
  private remember(target: Readonly<Anime25DDriver>): void {
    const elapsed = this.stepTime - this.shownTime
    for (const key of Object.keys(BODY_CONTROL_DRIVERS) as BodyControl[]) {
      const value = target[BODY_CONTROL_DRIVERS[key]]
      const last = this.shown.get(key)
      const velocity = last && elapsed > 0 ? (value - last.value) / elapsed : 0
      this.shown.set(key, { value, velocity })
    }
    this.shownTime = this.stepTime
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

/** A damped spring toward `goal`, exactly over `dt`: critical without `zeta`, past the goal and back below 1. */
function spring(axis: Axis, position: 'value' | 'weight', velocity: 'velocity' | 'weightVelocity', goal: number, omega: number, dt: number, zeta = 1): void {
  const displacement = axis[position] - goal
  if (zeta >= 0.999) {
    const coefficient = axis[velocity] + omega * displacement
    const decay = Math.exp(-omega * dt)
    axis[position] = goal + (displacement + coefficient * dt) * decay
    axis[velocity] = (axis[velocity] - omega * coefficient * dt) * decay
    return
  }
  const damped = omega * Math.sqrt(1 - zeta * zeta)
  const along = (axis[velocity] + zeta * omega * displacement) / damped
  const decay = Math.exp(-zeta * omega * dt)
  const cos = Math.cos(damped * dt)
  const sin = Math.sin(damped * dt)
  axis[position] = goal + decay * (displacement * cos + along * sin)
  axis[velocity] = decay * ((along * damped - zeta * omega * displacement) * cos - (displacement * damped + zeta * omega * along) * sin)
}

/** Lids opening or closing faster than this (eyeOpen per second) are on their way somewhere. */
const LIDS_MOVING = 0.8

/**
 * A look's life on a standing figure: the head tilt it sways with, from side
 * to side, takes the idle roll share. A bust's gentle tilt tips a whole
 * figure off its upright, and swung to and fro it reads as slouching about.
 */
function standingLife(life: FaceLife): FaceLife {
  return {
    ...life,
    beats: life.beats.map((beat) => {
      const tilt = beat.pose?.targets.headTilt
      return beat.pose && tilt !== undefined
        ? { ...beat, pose: { ...beat.pose, targets: { ...beat.pose.targets, headTilt: tilt * STANDING_IDLE_ROLL_SHARE } } }
        : beat
    }),
  }
}
