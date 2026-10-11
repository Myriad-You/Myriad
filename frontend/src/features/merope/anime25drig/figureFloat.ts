import type { BoneTransform, FigurePose, FigureRest } from './figureSkeleton'
import { compose, FIGURE_LEAN, setRotationAbout, setSegment } from './figureSkeleton'

/**
 * A figure that floats instead of standing, the way a VTuber's full body is
 * often rigged: the upper body leads, swaying as the chest goes, and what is
 * below hangs from it. The hips trail the chest on a spring and overshoot, so
 * the spine bends; each leg is a pendulum on its hip, swung by how the hips
 * move; the knee of the leg the sway leaves behind bends, its shin seen
 * shorter as it folds back and its foot drawn up. Nothing is planted.
 */

/** The chest's sway at `body` = ±1, as a share of the leg span. */
const FLOAT_SWAY = 0.05
/** The hips trail the chest like this (Hz, damping ratio). */
const HIP_FREQUENCY = 1.0
const HIP_DAMPING = 0.4
/** Each leg swings on its hip like this, within this many radians. */
const LEG_FREQUENCY = 0.9
const LEG_DAMPING = 0.3
const LEG_REACH = 0.3
/** A knee bends this far at most, as the share its shin shortens by, and turns its foot out this much. */
const KNEE_FOLD = 0.35
const KNEE_TURN = 0.15
const KNEE_FREQUENCY = 1.6
const KNEE_DAMPING = 0.6

interface Spring {
  value: number
  velocity: number
}

function spring(): Spring {
  return { value: 0, velocity: 0 }
}

/** Steps a spring towards `target` with an extra push; returns its acceleration. */
function stepSpring(s: Spring, target: number, push: number, dt: number, frequency: number, damping: number): number {
  const omega = 2 * Math.PI * frequency
  const acceleration = -omega * omega * (s.value - target) - 2 * damping * omega * s.velocity + push
  s.velocity += acceleration * dt
  s.value += s.velocity * dt
  return acceleration
}

/** The floating figure's springs. */
export class FigureFloat {
  private readonly hips = spring()
  private readonly legs = { L: spring(), R: spring() }
  private readonly knees = { L: spring(), R: spring() }
  private settled = false

  constructor(private readonly rest: Readonly<FigureRest>) {}

  private chestX(body: number): number {
    return Math.max(-1, Math.min(1, body)) * FLOAT_SWAY * this.rest.legSpan
  }

  /** Steps the hips, legs and knees after the chest moved to its sway at `body`. */
  step(dt: number, body: number, dynamic: boolean): void {
    const chest = this.chestX(body)
    const sway = Math.max(-1, Math.min(1, body))
    // The leg the sway leaves behind folds back at the knee.
    const fold = { L: Math.max(0, sway), R: Math.max(0, -sway) }
    if (!dynamic || !this.settled || !(dt > 0)) {
      this.hips.value = chest
      this.hips.velocity = 0
      for (const side of ['L', 'R'] as const) {
        this.legs[side].value = 0
        this.legs[side].velocity = 0
        this.knees[side].value = fold[side]
        this.knees[side].velocity = 0
      }
      this.settled = true
      return
    }
    const hipAcceleration = stepSpring(this.hips, chest, 0, dt, HIP_FREQUENCY, HIP_DAMPING)
    const legLength = Math.max(1, this.rest.legSpan)
    for (const side of ['L', 'R'] as const) {
      // A pendulum on a moving pivot: the hips' acceleration swings it the other way.
      const leg = this.legs[side]
      stepSpring(leg, 0, -hipAcceleration / legLength, dt, LEG_FREQUENCY, LEG_DAMPING)
      leg.value = Math.max(-LEG_REACH, Math.min(LEG_REACH, leg.value))
      stepSpring(this.knees[side], fold[side], 0, dt, KNEE_FREQUENCY, KNEE_DAMPING)
    }
  }

  /** Writes the bones of the floating figure at a sway of `body`. */
  pose(body: number, out: FigurePose): FigurePose {
    const rest = this.rest
    const chestX = this.chestX(body)
    // The chest sways and leans; the spine below it bends to where the hips
    // trail: at rest, plumb under the chest, whatever its lean.
    const lean = Math.max(-1, Math.min(1, body)) * FIGURE_LEAN
    setRotationAbout(out.chest, lean, rest.neckBase.x, rest.neckBase.y)
    out.chest.tx += chestX
    const spine = Math.max(1, rest.hips.y - rest.waist.y)
    const leanedHips = chestX - (rest.hips.y - rest.neckBase.y) * Math.sin(lean)
    const bend = -(this.hips.value - leanedHips) / spine
    setRotationAbout(scratch, bend, rest.waist.x, rest.waist.y)
    compose(out.abdomen, out.chest, scratch)
    Object.assign(out.pelvis, out.abdomen)
    out.chestAngle = Math.atan2(out.chest.b, out.chest.a)
    for (const side of ['L', 'R'] as const) {
      const leg = rest.legs[side]
      const left = side === 'L'
      const thigh = left ? out.thighL : out.thighR
      const shin = left ? out.shinL : out.shinR
      const foot = left ? out.footL : out.footR
      setRotationAbout(scratch, this.legs[side].value, leg.hip.x, leg.hip.y)
      compose(thigh, out.pelvis, scratch)
      // The shin folds back: seen shorter from the knee, its foot turned out a little.
      const fold = Math.max(0, Math.min(1, this.knees[side].value))
      const outward = left ? -1 : 1
      const dx = (leg.ankle.x - leg.knee.x) * (1 - KNEE_FOLD * fold)
      const dy = (leg.ankle.y - leg.knee.y) * (1 - KNEE_FOLD * fold)
      const turn = outward * KNEE_TURN * fold
      ankleAt.x = leg.knee.x + dx * Math.cos(turn) - dy * Math.sin(turn)
      ankleAt.y = leg.knee.y + dx * Math.sin(turn) + dy * Math.cos(turn)
      setSegment(scratch, leg.knee, leg.ankle, leg.knee, ankleAt)
      compose(shin, thigh, scratch)
      Object.assign(foot, shin)
    }
    return out
  }
}

const scratch: BoneTransform = { a: 1, b: 0, c: 0, d: 1, tx: 0, ty: 0 }
const ankleAt = { x: 0, y: 0 }
