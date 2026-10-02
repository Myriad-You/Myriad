import type { BodyLift } from './bodyLift'
import type { Anime25DPlaybackAnchors } from './types'

/**
 * What only a full figure does: it stands on a ground. Shifting its weight,
 * the hips move against the lean while the soles stay planted: the leg under
 * the weight straightens, the other relaxes at the knee and lifts its heel,
 * the pelvis tilts, and a skirt hung on it trails. A bust is cut off above
 * any ground and never builds one of these.
 */

/**
 * The hip shift at `body` = ±1, as a share of the leg length. The hips move
 * against the lean, so the weight stays over the feet.
 */
export const STANCE_SHIFT = 0.06

/** A skirt hem on the hips: a loose, slow pendulum that trails and then overshoots. */
const SKIRT_FREQUENCY = 1.3
const SKIRT_DAMPING = 0.35

/**
 * The free leg, the one without the weight: its knee bends in towards the
 * other, by this share of the leg length, and its heel lifts by this share
 * while the toes stay down. The knee and ankle sit this far down from the
 * hips; the free foot keeps this share of the knee's inward travel.
 */
const KNEE_IN = 0.05
const HEEL_LIFT = 0.024
const KNEE_AT = 0.45
const ANKLE_AT = 0.85
/** A found knee or ankle this far off the usual proportions is a misreading. */
const KNEE_RANGE = [0.25, 0.65] as const
const ANKLE_RANGE = [0.7, 0.97] as const
const FOOT_FOLLOW = 0.15
/** How much of the heel's lift the tip of the shoe keeps. */
const TOE_LIFT = 0.12
/** The pelvis tilts with the weight, the weight-bearing hip up, by this many radians. */
const PELVIS_TILT = 0.045
/** Weight passes from leg to leg like this: a soft spring that settles just past. */
const LEG_FREQUENCY = 1.8
const LEG_DAMPING = 0.75

/**
 * Standing idle, a figure settles on one leg, holds it a while, then moves its
 * weight over to the other: these holds, in seconds, and leans (as `body`),
 * alternating sides, then again. Each move takes this long.
 */
const WEIGHT_HOLDS = [7.5, 5.5, 9, 6.5, 8, 6] as const
const WEIGHT_LEANS = [0.55, 0.5, 0.45, 0.6, 0.42, 0.52] as const
const WEIGHT_MOVE = 1.4
const WEIGHT_CYCLE = WEIGHT_HOLDS.reduce((sum, hold) => sum + hold, 0)

function weightLean(index: number): number {
  const i = ((index % WEIGHT_HOLDS.length) + WEIGHT_HOLDS.length) % WEIGHT_HOLDS.length
  return (i % 2 === 0 ? 1 : -1) * WEIGHT_LEANS[i]
}

/** The idle lean of a standing figure at `time`, which shifts its weight. */
export function standingWeightShift(time: number): number {
  const t = Number.isFinite(time) ? ((time % WEIGHT_CYCLE) + WEIGHT_CYCLE) % WEIGHT_CYCLE : 0
  let start = 0
  for (let i = 0; i < WEIGHT_HOLDS.length; i++) {
    const end = start + WEIGHT_HOLDS[i]
    if (t < end || i === WEIGHT_HOLDS.length - 1) {
      const u = Math.max(0, Math.min(1, (t - start) / WEIGHT_MOVE))
      const eased = u * u * (3 - 2 * u)
      const from = weightLean(i - 1)
      return from + (weightLean(i) - from) * eased
    }
    start = end
  }
  return 0
}

/** How a layer of a standing figure follows its stance. */
export type StandingBinding =
  | {
      kind: 'skirt'
      /** The waist it hangs from and its hem. */
      topY: number
      bottomY: number
      /** The pelvis it tilts with turns about here. */
      centerX: number
    }
  | {
      kind: 'leg'
      side: 'L' | 'R'
      /** Towards the other leg: +1 for the leg on the left of the picture. */
      inward: 1 | -1
      hipY: number
      groundY: number
      /** Where this leg leaves the pelvis, and the pelvis' middle. */
      hipX: number
      centerX: number
      /**
       * The hip joint, knee and ankle, as shares of the way from `hipY` down
       * to the ground: found on the portrait, or the usual proportions.
       */
      hipAt: number
      kneeAt: number
      ankleAt: number
    }

/** The stance a standing figure's layers follow this frame. */
export interface Anime25DStandingFrame {
  /** How far the skirt hem trails the hips, in pixels. */
  skirtSwing: number
  /** Pelvis tilt in radians; positive raises the hip on the right of the picture. */
  pelvisTilt: number
  /** How bent each knee is, 0 straight to 1 fully relaxed. */
  bendL: number
  bendR: number
}

interface Spring {
  value: number
  velocity: number
}

function stepSpring(spring: Spring, target: number, dt: number, frequency: number, damping: number): void {
  const omega = 2 * Math.PI * frequency
  spring.velocity += (-omega * omega * (spring.value - target) - 2 * damping * omega * spring.velocity) * dt
  spring.value += spring.velocity * dt
}

function settle(spring: Spring, target: number): void {
  spring.value = target
  spring.velocity = 0
}

export class Anime25DStanding {
  readonly frame: Anime25DStandingFrame = { skirtSwing: 0, pelvisTilt: 0, bendL: 0, bendR: 0 }
  /** Where the skirt hem has got to, chasing the hips. */
  private readonly skirt: Spring = { value: 0, velocity: 0 }
  private readonly knees = { L: { value: 0, velocity: 0 }, R: { value: 0, velocity: 0 } }

  private constructor(
    readonly groundY: number,
    private readonly hipsY: number,
  ) {}

  /** A standing figure's stance, or none for a bust. */
  static of(anchors: Readonly<Anime25DPlaybackAnchors>): Anime25DStanding | null {
    return anchors.groundY === undefined
      ? null
      : new Anime25DStanding(anchors.groundY, anchors.bodyPivot.y)
  }

  /** How far the hips move at a lean of `body`. */
  shift(body: number): number {
    return -body * STANCE_SHIFT * (this.groundY - this.hipsY)
  }

  /**
   * Steps the stance after the lean. The hips go against it, so the weight
   * lands on the leg they move over: that leg straightens, the other relaxes.
   */
  step(dt: number, body: number, phys: boolean): Readonly<Anime25DStandingFrame> {
    const hips = this.shift(body)
    // +1 with the hips fully over the leg on the right of the picture.
    const weight = Math.max(-1, Math.min(1, -body))
    const free = { L: Math.max(0, weight), R: Math.max(0, -weight) }
    const frame = this.frame
    frame.pelvisTilt = weight * PELVIS_TILT
    if (!phys) {
      settle(this.skirt, hips)
      settle(this.knees.L, free.L)
      settle(this.knees.R, free.R)
    } else {
      stepSpring(this.skirt, hips, dt, SKIRT_FREQUENCY, SKIRT_DAMPING)
      stepSpring(this.knees.L, free.L, dt, LEG_FREQUENCY, LEG_DAMPING)
      stepSpring(this.knees.R, free.R, dt, LEG_FREQUENCY, LEG_DAMPING)
    }
    // The hem's lag behind the hips the stance field already carries it with.
    frame.skirtSwing = this.skirt.value - hips
    frame.bendL = Math.max(0, this.knees.L.value)
    frame.bendR = Math.max(0, this.knees.R.value)
    return frame
  }
}

/** The weight shift: a straight leg from the planted sole to the moved hip. */
export function applyBodyStance(point: { x: number; y: number }, field: Readonly<BodyLift>): void {
  const shift = field.stanceShift ?? 0
  const ground = field.groundY ?? field.lowerY
  if (!shift || !(ground > field.lowerY)) return
  point.x += shift * Math.max(0, Math.min(1, (ground - point.y) / (ground - field.lowerY)))
}

/** The shader's `applyBodyStance`. stance: hips (lowerY), ground, shift. */
export const BODY_STANCE_GLSL = `
vec2 bodyStance(vec2 p, vec3 stance) {
  if (stance.z == 0.0 || !(stance.y > stance.x)) return p;
  return vec2(p.x + stance.z * clamp((stance.y - p.y) / (stance.y - stance.x), 0.0, 1.0), p.y);
}
`

/**
 * A standing figure's skirt hangs on its hips and each leg stands on its own
 * sole; a bust's lower clothing is cut off with the frame.
 */
export function bindStanding(
  baseRole: string,
  layer: { x: number; y: number; w: number; h: number; side?: string | null; group?: string | null },
  anchors: Readonly<Anime25DPlaybackAnchors>,
): StandingBinding | null {
  const group = layer.group ?? null
  const groundY = anchors.groundY
  if (groundY === undefined) return null
  const centerX = anchors.bodyPivot.x
  if (baseRole === 'bottomwear') {
    return { kind: 'skirt', topY: layer.y, bottomY: layer.y + layer.h, centerX }
  }
  const hipY = anchors.bodyPivot.y
  const middleX = layer.x + layer.w / 2
  const limb = baseRole === 'legwear' || baseRole === 'footwear'
  // Anything else drawn on a lower leg, such as a buckle recovered from the
  // portrait, goes with the leg it sits on.
  const onLowerLeg =
    !limb &&
    group === 'body' &&
    !NOT_ON_LEGS.has(baseRole) &&
    (layer.y + layer.h / 2 - hipY) / Math.max(1, groundY - hipY) > ON_LOWER_LEG
  const side =
    limb && (layer.side === 'L' || layer.side === 'R')
      ? layer.side
      : onLowerLeg
        ? middleX < centerX
          ? 'L'
          : 'R'
        : null
  if (!side) return null
  return {
    kind: 'leg',
    side,
    inward: side === 'L' ? 1 : -1,
    hipY,
    groundY,
    hipX: middleX,
    centerX,
    ...legJoints(anchors, side, hipY, groundY),
  }
}

/** This leg's joints from the skeleton found on the portrait, when they make sense. */
function legJoints(
  anchors: Readonly<Anime25DPlaybackAnchors>,
  side: 'L' | 'R',
  hipY: number,
  groundY: number,
): { hipAt: number; kneeAt: number; ankleAt: number } {
  const usual = { hipAt: 0, kneeAt: KNEE_AT, ankleAt: ANKLE_AT }
  const joints = anchors.skeleton?.joints
  const knee = joints?.[`knee${side}`]
  const ankle = joints?.[`ankle${side}`]
  if (!knee || !ankle) return usual
  const span = Math.max(1, groundY - hipY)
  const kneeAt = (knee.y - hipY) / span
  const ankleAt = (ankle.y - hipY) / span
  if (
    !(kneeAt >= KNEE_RANGE[0] && kneeAt <= KNEE_RANGE[1]) ||
    !(ankleAt >= ANKLE_RANGE[0] && ankleAt <= ANKLE_RANGE[1])
  ) {
    return usual
  }
  // The hip joint sits below where the leg's drawing starts, under the skirt.
  const hip = joints?.[`hip${side}`]
  const hipAt = hip ? Math.max(0, Math.min(kneeAt * 0.6, (hip.y - hipY) / span)) : 0
  return { hipAt, kneeAt, ankleAt }
}

/** Below this share of the leg, a loose part is on a lower leg, clear of hands and hem. */
const ON_LOWER_LEG = 0.45
/** Parts that hang from the shoulders or hips, never from a leg. */
const NOT_ON_LEGS = new Set(['handwear', 'topwear', 'bottomwear', 'neckwear', 'hair'])

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, value))
}

/** Moves one rest point of a standing figure's layer with its stance. */
export function applyStanding(
  point: { x: number; y: number },
  binding: Readonly<StandingBinding>,
  restX: number,
  restY: number,
  frame: Readonly<Anime25DStandingFrame>,
): void {
  if (binding.kind === 'skirt') {
    // Held at the waist, freest at the hem, which tilts with the pelvis.
    const along = clamp01((restY - binding.topY) / Math.max(1, binding.bottomY - binding.topY))
    point.x += frame.skirtSwing * along * along
    point.y -= (restX - binding.centerX) * frame.pelvisTilt * along
    return
  }
  const span = Math.max(1, binding.groundY - binding.hipY)
  const t = clamp01((restY - binding.hipY) / span)
  // The top of the leg rides the tilted pelvis; the sole stays on the ground.
  point.y -= (binding.hipX - binding.centerX) * frame.pelvisTilt * (1 - t)
  const bend = binding.side === 'L' ? frame.bendL : frame.bendR
  if (!(bend > 0)) return
  const { hipAt, kneeAt, ankleAt } = binding
  // Thigh and shin stay straight, from the hip joint to a knee that moves
  // inwards and on to the foot.
  const knee =
    t <= hipAt
      ? 0
      : t < kneeAt
        ? (t - hipAt) / (kneeAt - hipAt)
        : 1 - (1 - FOOT_FOLLOW) * (t - kneeAt) / (1 - kneeAt)
  point.x += binding.inward * bend * KNEE_IN * span * knee
  // The shin rises with the heel; the shoe tips forward onto its toes.
  const lift =
    t <= kneeAt
      ? 0
      : t < ankleAt
        ? (t - kneeAt) / (ankleAt - kneeAt)
        : 1 - (1 - TOE_LIFT) * (t - ankleAt) / (1 - ankleAt)
  point.y -= bend * HEEL_LIFT * span * lift
}
