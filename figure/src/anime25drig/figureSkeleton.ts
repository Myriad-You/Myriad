import type { PartSwayBinding } from './partSway'
import type { Anime25DStandingFrame, StandingBinding } from './standing'
import type { Anime25DPartProfile, Anime25DPlaybackAnchors } from './types'
import { FigureFloat } from './figureFloat'
import { applyPartSways, bindPartSways, PartSways } from './partSway'
import { applySkirtTrail } from './standing'

/**
 * A standing figure's bones, as a Live2D rig nests its rotation deformers:
 * pelvis, abdomen and chest up the spine, thigh, shin and foot down each
 * leg. Everything drawn rides a bone or a blend of neighbouring bones; what
 * a layer does on its own (a keyed turn, a sleeve's swing, a hem's trail) is
 * done first, at rest, and the bones then carry it.
 *
 * The figure stands: its soles stay where they were drawn and it sways from
 * them as one, the hips moving and tilting over them and the upper body
 * leaning on a little further the same way. A leg
 * that has to reach less far bends its knee towards the viewer: seen from
 * the front, thigh and shin shorten and the knee drifts inwards. A free leg
 * rises onto its toes.
 *
 * A bust has no ground and builds none of this.
 */

/** A 2D affine transform, column-major like a layer transform: p' = (a x + c y + tx, b x + d y + ty). */
export interface BoneTransform {
  a: number
  b: number
  c: number
  d: number
  tx: number
  ty: number
}

interface Point {
  x: number
  y: number
}

interface LegRest {
  hip: Point
  knee: Point
  ankle: Point
  toe: Point
  /** Towards the other leg: +1 for the leg on the left of the picture. */
  inward: 1 | -1
}

/** The bones' joints as drawn, in canvas pixels. */
export interface FigureRest {
  /** Between the hips, where the legs meet the pelvis. */
  hips: Point
  /** Where the abdomen bends into the chest. */
  waist: Point
  /** The base of the neck: what the head and arms hang from. */
  neckBase: Point
  groundY: number
  /** From the hips down to the soles. */
  legSpan: number
  legs: { L: LegRest; R: LegRest }
}

/** Every bone's world transform this frame. */
export interface FigurePose {
  pelvis: BoneTransform
  abdomen: BoneTransform
  chest: BoneTransform
  thighL: BoneTransform
  shinL: BoneTransform
  footL: BoneTransform
  thighR: BoneTransform
  shinR: BoneTransform
  footR: BoneTransform
  /** The chest's world angle: which way is down for what hangs from it. */
  chestAngle: number
}

/** What drives the bones this frame. */
export interface FigurePoseInput {
  /** The sway, −1…1: the hips and, further, the shoulders go this way over the planted feet. */
  body: number
  /** Pelvis tilt in radians; positive raises the hip on the right of the picture. */
  pelvisTilt: number
  /** How relaxed each leg is, 0 bearing weight to 1 free: its heel lifts. */
  bendL: number
  bendR: number
}

/** How a figure holds itself: standing on its soles, or floating with its legs hanging. */
export type FigureStance = 'ground' | 'float'

/** How a layer rides the bones. */
export type FigureSkin =
  | { kind: 'pelvis' }
  | { kind: 'torso' }
  | { kind: 'chest' }
  | { kind: 'leg'; side: 'L' | 'R' }

/**
 * The upper body's lean over the hips at `body` = ±1, in radians: it goes
 * on the way the hips went, so the figure moves more the higher up it is.
 */
export const FIGURE_LEAN = 0.14
/** The hip shift at `body` = ±1, as a share of the leg span, the way of the sway. */
export const FIGURE_HIP_SHIFT = 0.2
/**
 * A free leg's heel lifts by this share of the leg span; its toes stay down.
 * At the rig's full range the free knee comes well in, as a drawn
 * contrapposto has it; much more and the shoe leaves the ground.
 */
const HEEL_LIFT = 0.1
/** The most a bent leg's thigh and shin are drawn longer, its knee drifting in. */
const LEG_STRETCH = 1.05
/** The most a free foot is drawn taller, rising onto its toes. */
const FOOT_STRETCH = 1.2
/**
 * A knee bent towards the viewer drifts inwards by this share of the thigh
 * for each unit of `sqrt(1 − (reach / drawn reach)²)`. It stays clear of the
 * other leg: a free knee carried in over it reads as legs twisted round each
 * other, not as the weight on one foot.
 */
const KNEE_DRIFT = 0.25
/** Of the lean, the abdomen takes this much; the chest the rest. */
const ABDOMEN_SHARE = 0.45
/** The usual knee and ankle, as shares of the way from the hips to the ground. */
const KNEE_AT = 0.45
const ANKLE_AT = 0.85
/** A found knee or ankle this far off the usual proportions is a misreading. */
const KNEE_RANGE = [0.25, 0.65] as const
const ANKLE_RANGE = [0.7, 0.97] as const
/** Without found hips, each hip joint sits this share of the leg span off the middle. */
const HIP_HALF_WIDTH = 0.09
/** The waist, as a share of the way up from the hips to the neck. */
const WAIST_AT = 0.45
/**
 * Bones blend across each joint over this share of the bones meeting there.
 * The knee's blend is wide enough that a bent knee rounds instead of creasing.
 */
const JOINT_BLEND = 0.18
const KNEE_BLEND = 0.21

const IDENTITY: Readonly<BoneTransform> = { a: 1, b: 0, c: 0, d: 1, tx: 0, ty: 0 }

/** The figure's rest skeleton from its anchors, or none for a bust. */
export function figureRest(anchors: Readonly<Anime25DPlaybackAnchors>): FigureRest | null {
  const groundY = anchors.groundY
  if (groundY === undefined) return null
  const hips = { x: anchors.bodyPivot.x, y: anchors.bodyPivot.y }
  const legSpan = Math.max(1, groundY - hips.y)
  const neckBase = { x: anchors.neckPivot.x, y: anchors.neckBottom }
  const waist = {
    x: hips.x + (neckBase.x - hips.x) * WAIST_AT,
    y: hips.y + (neckBase.y - hips.y) * WAIST_AT,
  }
  const joints = anchors.skeleton?.joints ?? {}
  const leg = (side: 'L' | 'R'): LegRest => {
    const outward = side === 'L' ? -1 : 1
    const hipJoint = joints[`hip${side}`]
    const hip = hipJoint
      ? { x: hipJoint.x, y: Math.max(hips.y, Math.min(hipJoint.y, hips.y + legSpan * 0.2)) }
      : { x: hips.x + outward * HIP_HALF_WIDTH * legSpan, y: hips.y }
    const found = (name: 'knee' | 'ankle', range: readonly [number, number]) => {
      const joint = joints[`${name}${side}`]
      if (!joint) return null
      const at = (joint.y - hips.y) / legSpan
      return at >= range[0] && at <= range[1] ? { x: joint.x, y: joint.y } : null
    }
    const kneeFound = found('knee', KNEE_RANGE)
    const ankleFound = found('ankle', ANKLE_RANGE)
    const both = kneeFound && ankleFound
    const knee = both ? kneeFound : { x: hip.x, y: hips.y + KNEE_AT * legSpan }
    const ankle = both ? ankleFound : { x: hip.x, y: hips.y + ANKLE_AT * legSpan }
    const toeJoint = joints[`bigToe${side}`]
    const toe = toeJoint && toeJoint.y > ankle.y ? { x: toeJoint.x, y: toeJoint.y } : { x: ankle.x, y: groundY }
    return { hip, knee, ankle, toe, inward: side === 'L' ? 1 : -1 }
  }
  return { hips, waist, neckBase, groundY, legSpan, legs: { L: leg('L'), R: leg('R') } }
}

export function createFigurePose(): FigurePose {
  const bone = () => ({ ...IDENTITY })
  return {
    pelvis: bone(),
    abdomen: bone(),
    chest: bone(),
    thighL: bone(),
    shinL: bone(),
    footL: bone(),
    thighR: bone(),
    shinR: bone(),
    footR: bone(),
    chestAngle: 0,
  }
}

export function setRotationAbout(out: BoneTransform, angle: number, x: number, y: number): BoneTransform {
  const c = Math.cos(angle)
  const s = Math.sin(angle)
  out.a = c
  out.b = s
  out.c = -s
  out.d = c
  out.tx = x - (c * x - s * y)
  out.ty = y - (s * x + c * y)
  return out
}

/** out = p ∘ q. `out` may be `q`, not `p`. */
export function compose(out: BoneTransform, p: Readonly<BoneTransform>, q: Readonly<BoneTransform>): BoneTransform {
  const a = p.a * q.a + p.c * q.b
  const b = p.b * q.a + p.d * q.b
  const c = p.a * q.c + p.c * q.d
  const d = p.b * q.c + p.d * q.d
  const tx = p.a * q.tx + p.c * q.ty + p.tx
  const ty = p.b * q.tx + p.d * q.ty + p.ty
  out.a = a
  out.b = b
  out.c = c
  out.d = d
  out.tx = tx
  out.ty = ty
  return out
}

function apply(t: Readonly<BoneTransform>, x: number, y: number, out: Point): Point {
  out.x = t.a * x + t.c * y + t.tx
  out.y = t.b * x + t.d * y + t.ty
  return out
}

/**
 * The transform taking the drawn segment p→q onto from→to: turned to its new
 * direction and stretched along its own length only, so its width holds.
 */
export function setSegment(out: BoneTransform, p: Point, q: Point, from: Point, to: Point): BoneTransform {
  const rx = q.x - p.x
  const ry = q.y - p.y
  const restLength = Math.max(1e-6, Math.hypot(rx, ry))
  const ux = rx / restLength
  const uy = ry / restLength
  const length = Math.hypot(to.x - from.x, to.y - from.y)
  const k = length / restLength
  const angle = Math.atan2(to.y - from.y, to.x - from.x) - Math.atan2(ry, rx)
  const cos = Math.cos(angle)
  const sin = Math.sin(angle)
  // Stretch along u: S = I + (k − 1) u uᵀ; then turn: R S.
  const s00 = 1 + (k - 1) * ux * ux
  const s01 = (k - 1) * ux * uy
  const s11 = 1 + (k - 1) * uy * uy
  out.a = cos * s00 - sin * s01
  out.b = sin * s00 + cos * s01
  out.c = cos * s01 - sin * s11
  out.d = sin * s01 + cos * s11
  out.tx = from.x - (out.a * p.x + out.c * p.y)
  out.ty = from.y - (out.b * p.x + out.d * p.y)
  return out
}

const scratch = {
  rotation: { ...IDENTITY },
  hip: { x: 0, y: 0 },
  knee: { x: 0, y: 0 },
  ankle: { x: 0, y: 0 },
  lifted: { x: 0, y: 0 },
}

function drawnReach(leg: Readonly<LegRest>): number {
  return Math.hypot(leg.ankle.x - leg.hip.x, leg.ankle.y - leg.hip.y)
}

/**
 * The knee for a hip and ankle: the drawn leg turned onto them and shortened
 * to fit, the knee where it was along the way and drifting inwards as the
 * leg bends towards the viewer. Whether it bent.
 */
function solveKnee(leg: Readonly<LegRest>, hip: Point, ankle: Point, out: Point): boolean {
  const restX = leg.ankle.x - leg.hip.x
  const restY = leg.ankle.y - leg.hip.y
  const restReach = Math.max(1e-6, Math.hypot(restX, restY))
  const dx = ankle.x - hip.x
  const dy = ankle.y - hip.y
  const reach = Math.hypot(dx, dy)
  const scale = reach / restReach
  const angle = Math.atan2(dy, dx) - Math.atan2(restY, restX)
  const cos = Math.cos(angle)
  const sin = Math.sin(angle)
  const kx = (leg.knee.x - leg.hip.x) * scale
  const ky = (leg.knee.y - leg.hip.y) * scale
  out.x = hip.x + kx * cos - ky * sin
  out.y = hip.y + kx * sin + ky * cos
  const bent = Math.sqrt(Math.max(0, 1 - scale * scale))
  if (bent > 0) {
    const thigh = Math.hypot(leg.knee.x - leg.hip.x, leg.knee.y - leg.hip.y)
    out.x += leg.inward * KNEE_DRIFT * thigh * bent
  }
  return bent > 0
}

/**
 * Moves a bent knee along its leg, keeping how far it drifted in, so the
 * thigh and the shin come out shortened alike: seen bending towards the
 * viewer both foreshorten, and one drawn longer than the other reads as a
 * thinner or a thicker leg. Neither is drawn longer than it is; a knee that
 * drifted too far in for that comes back out to where both are full length.
 */
function balanceKnee(leg: Readonly<LegRest>, hip: Point, ankle: Point, knee: Point): void {
  const thigh = Math.hypot(leg.knee.x - leg.hip.x, leg.knee.y - leg.hip.y)
  const shin = Math.hypot(leg.ankle.x - leg.knee.x, leg.ankle.y - leg.knee.y)
  const dx = ankle.x - hip.x
  const dy = ankle.y - hip.y
  const reach = Math.hypot(dx, dy)
  if (!(thigh > 0 && shin > 0 && reach > 0)) return
  const ux = dx / reach
  const uy = dy / reach
  // The knee's drift off the line from the hip to the ankle, kept.
  let off = (knee.x - hip.x) * -uy + (knee.y - hip.y) * ux
  // Along the line, where the thigh and the shin are shortened alike.
  const unequal = (along: number) => Math.hypot(along, off) / thigh - Math.hypot(reach - along, off) / shin
  let low = 0
  let high = reach
  for (let i = 0; i < 60; i++) {
    const mid = (low + high) / 2
    if (unequal(mid) < 0) low = mid
    else high = mid
  }
  let along = (low + high) / 2
  if (Math.hypot(along, off) > thigh * LEG_STRETCH) {
    const t = thigh * LEG_STRETCH
    const k = shin * LEG_STRETCH
    along = reach >= t + k ? (reach * t) / (t + k) : (reach * reach + t * t - k * k) / (2 * reach)
    off = Math.sign(off) * Math.sqrt(Math.max(0, t * t - along * along))
  }
  knee.x = hip.x + ux * along - uy * off
  knee.y = hip.y + uy * along + ux * off
}

/** Writes this frame's bones for a figure standing on its ground. */
export function solveFigurePose(rest: Readonly<FigureRest>, input: Readonly<FigurePoseInput>, out: FigurePose): FigurePose {
  const body = Math.max(-1, Math.min(1, input.body))
  const shiftX = body * FIGURE_HIP_SHIFT * rest.legSpan
  const tilt = input.pelvisTilt
  const lifts = { L: Math.max(0, input.bendL) * HEEL_LIFT * rest.legSpan, R: Math.max(0, input.bendR) * HEEL_LIFT * rest.legSpan }
  // The pelvis tilts about the hips and moves over the feet. A leg reaches
  // no further than drawn, so the hips sink to stay on it.
  setRotationAbout(out.pelvis, tilt, rest.hips.x, rest.hips.y)
  out.pelvis.tx += shiftX
  let sink = 0
  for (const side of ['L', 'R'] as const) {
    const leg = rest.legs[side]
    const hip = apply(out.pelvis, leg.hip.x, leg.hip.y, scratch.hip)
    const reach = drawnReach(leg)
    const dx = hip.x - leg.ankle.x
    const below = leg.ankle.y - hip.y
    sink = Math.max(sink, below - Math.sqrt(Math.max(0, reach * reach - dx * dx)))
  }
  out.pelvis.ty += sink
  // The upper body leans on the way of the sway: the abdomen and chest share
  // the lean, less the pelvis' own tilt, so the shoulders take the whole of it.
  const lean = body * FIGURE_LEAN
  setRotationAbout(scratch.rotation, lean * ABDOMEN_SHARE - tilt, rest.hips.x, rest.hips.y)
  compose(out.abdomen, out.pelvis, scratch.rotation)
  setRotationAbout(scratch.rotation, lean * (1 - ABDOMEN_SHARE), rest.waist.x, rest.waist.y)
  compose(out.chest, out.abdomen, scratch.rotation)
  out.chestAngle = Math.atan2(out.chest.b, out.chest.a)
  for (const side of ['L', 'R'] as const) {
    const leg = rest.legs[side]
    const left = side === 'L'
    const foot = left ? out.footL : out.footR
    // A free leg rises onto its toes. Seen from the front the foot tips
    // towards the viewer and shows taller from its toes up to the lifted
    // ankle, but a drawn shoe stretched that far reads as a longer shoe: it
    // stretches FOOT_STRETCH at most. The knee bends for the whole lift.
    const toeToAnkle = Math.hypot(leg.ankle.x - leg.toe.x, leg.ankle.y - leg.toe.y)
    const lift = Math.min(lifts[side], toeToAnkle * 0.6)
    const footLift = Math.min(lift, toeToAnkle * (FOOT_STRETCH - 1))
    if (footLift > 0 && toeToAnkle > 1) {
      scratch.ankle.x = leg.ankle.x
      scratch.ankle.y = leg.ankle.y - footLift
      setSegment(foot, leg.toe, leg.ankle, leg.toe, scratch.ankle)
    } else {
      Object.assign(foot, IDENTITY)
    }
    const hip = apply(out.pelvis, leg.hip.x, leg.hip.y, scratch.hip)
    const ankle = apply(foot, leg.ankle.x, leg.ankle.y, scratch.ankle)
    scratch.lifted.x = ankle.x
    scratch.lifted.y = ankle.y - (lift - footLift)
    if (solveKnee(leg, hip, scratch.lifted, scratch.knee)) {
      balanceKnee(leg, hip, ankle, scratch.knee)
    }
    setSegment(left ? out.thighL : out.thighR, leg.hip, leg.knee, hip, scratch.knee)
    setSegment(left ? out.shinL : out.shinR, leg.knee, leg.ankle, scratch.knee, ankle)
  }
  return out
}

function smoothstep(value: number): number {
  const t = Math.max(0, Math.min(1, value))
  return t * t * (3 - 2 * t)
}

/** How far below the joint at `at` a point is, blended over `band` either side (y grows downwards). */
function across(y: number, at: number, band: number): number {
  return smoothstep((y - (at - band)) / Math.max(1e-6, 2 * band))
}

/** The bones a rest point rides and their weights, summing to 1 (at most four). */
interface BoneWeights {
  bones: Readonly<BoneTransform>[]
  weights: number[]
  count: number
}

const boneWeights: BoneWeights = { bones: [IDENTITY, IDENTITY, IDENTITY, IDENTITY], weights: [0, 0, 0, 0], count: 0 }

function add(w: BoneWeights, bone: Readonly<BoneTransform>, weight: number): void {
  if (!(weight > 1e-6)) return
  w.bones[w.count] = bone
  w.weights[w.count] = weight
  w.count += 1
}

function bonesAt(skin: Readonly<FigureSkin>, rest: Readonly<FigureRest>, pose: Readonly<FigurePose>, y: number): BoneWeights {
  const w = boneWeights
  w.count = 0
  if (skin.kind === 'chest') {
    add(w, pose.chest, 1)
    return w
  }
  if (skin.kind === 'pelvis') {
    add(w, pose.pelvis, 1)
    return w
  }
  if (skin.kind === 'torso') {
    // Above the waist the chest, down to the hips the abdomen, below them the pelvis.
    const belowWaist = across(y, rest.waist.y, Math.max(1, (rest.waist.y - rest.neckBase.y) * JOINT_BLEND))
    const belowHips = across(y, rest.hips.y, Math.max(1, (rest.hips.y - rest.waist.y) * JOINT_BLEND))
    add(w, pose.chest, 1 - belowWaist)
    add(w, pose.abdomen, belowWaist * (1 - belowHips))
    add(w, pose.pelvis, belowWaist * belowHips)
    return w
  }
  const leg = rest.legs[skin.side]
  const left = skin.side === 'L'
  const thighLength = Math.max(1, leg.knee.y - leg.hip.y)
  const shinLength = Math.max(1, leg.ankle.y - leg.knee.y)
  // The top of the leg's drawing, above its hip joint, is still on the pelvis.
  const onLeg = across(y, leg.hip.y, Math.max(1, thighLength * JOINT_BLEND * 0.5))
  // A knee bends at a point: its blend is narrow, or the leg bows.
  const belowKnee = across(y, leg.knee.y, Math.min(thighLength, shinLength) * KNEE_BLEND)
  const belowAnkle = across(y, leg.ankle.y, shinLength * JOINT_BLEND * 0.5)
  add(w, pose.pelvis, 1 - onLeg)
  add(w, left ? pose.thighL : pose.thighR, onLeg * (1 - belowKnee))
  add(w, left ? pose.shinL : pose.shinR, onLeg * belowKnee * (1 - belowAnkle))
  add(w, left ? pose.footL : pose.footR, onLeg * belowKnee * belowAnkle)
  return w
}

const skinPoint = { x: 0, y: 0 }
/**
 * Moves a point done with its own deformation (at `point`, drawn at
 * height `restY`) with the bones it rides, blended by where it was drawn.
 */
export function skinFigurePoint(
  point: Point,
  restY: number,
  skin: Readonly<FigureSkin>,
  rest: Readonly<FigureRest>,
  pose: Readonly<FigurePose>,
): void {
  const w = bonesAt(skin, rest, pose, restY)
  const x = point.x
  const y = point.y
  let outX = 0
  let outY = 0
  for (let i = 0; i < w.count; i++) {
    apply(w.bones[i], x, y, skinPoint)
    outX += skinPoint.x * w.weights[i]
    outY += skinPoint.y * w.weights[i]
  }
  point.x = outX
  point.y = outY
}

/**
 * The skin at a rest height as one affine map, column-major 3×3 like a
 * layer transform: what a rigid layer drawn there rides.
 */
export function figureAffineAt(
  restY: number,
  skin: Readonly<FigureSkin>,
  rest: Readonly<FigureRest>,
  pose: Readonly<FigurePose>,
  out: Float32Array,
): Float32Array {
  const w = bonesAt(skin, rest, pose, restY)
  out.fill(0)
  for (let i = 0; i < w.count; i++) {
    const bone = w.bones[i]
    const weight = w.weights[i]
    out[0] += bone.a * weight
    out[1] += bone.b * weight
    out[3] += bone.c * weight
    out[4] += bone.d * weight
    out[6] += bone.tx * weight
    out[7] += bone.ty * weight
  }
  out[8] = 1
  return out
}

/** out = a · b for column-major 3×3 affine matrices; `out` may be `b`. */
export function multiplyAffine(out: Float32Array, a: Readonly<Float32Array>, b: Readonly<Float32Array>): Float32Array {
  return carryAffine(out, { a: a[0], b: a[1], c: a[3], d: a[4], tx: a[6], ty: a[7] }, b)
}

/** out = bone · m, for a column-major 3×3 affine `m`; `out` may be `m`. */
export function carryAffine(out: Float32Array, bone: Readonly<BoneTransform>, m: Readonly<Float32Array>): Float32Array {
  const [m0, m1, , m3, m4, , m6, m7] = m
  out[0] = bone.a * m0 + bone.c * m1
  out[1] = bone.b * m0 + bone.d * m1
  out[3] = bone.a * m3 + bone.c * m4
  out[4] = bone.b * m3 + bone.d * m4
  out[6] = bone.a * m6 + bone.c * m7 + bone.tx
  out[7] = bone.b * m6 + bone.d * m7 + bone.ty
  out[2] = 0
  out[5] = 0
  out[8] = 1
  return out
}

/**
 * Which bones a layer rides: the skirt the pelvis, a leg's clothing its
 * own leg, the clothing of the body the spine, and the head, hair, arms
 * and what hangs at the neck the chest they hang from.
 */
export function bindFigureSkin(
  baseRole: string,
  layer: { x: number; y: number; w: number; h: number; side?: string | null; group?: string | null },
  rest: Readonly<FigureRest> | null,
): FigureSkin | null {
  if (!rest) return null
  if (layer.group === 'head') return { kind: 'chest' }
  if (baseRole === 'bottomwear') return { kind: 'pelvis' }
  if (baseRole === 'legwear' || baseRole === 'footwear') {
    const side = layer.side === 'L' || layer.side === 'R' ? layer.side : layer.x + layer.w / 2 < rest.hips.x ? 'L' : 'R'
    return { kind: 'leg', side }
  }
  if (CHEST_ROLES.has(baseRole)) return { kind: 'chest' }
  // A coat hangs from the shoulders however low a piece of it lies (its lining behind the legs).
  if (baseRole === 'topwear') return { kind: 'torso' }
  // Anything else low on the figure (a buckle, a garter) is on the leg it sits over.
  if ((layer.y + layer.h / 2 - rest.hips.y) / rest.legSpan > LOW_ON_LEG) {
    return { kind: 'leg', side: layer.x + layer.w / 2 < rest.hips.x ? 'L' : 'R' }
  }
  return { kind: 'torso' }
}

/** A layer of a package on its figure: the bones, which of them carry it, and its regions that sway. */
export function bindFigureLayer(
  baseRole: string,
  layer: Parameters<typeof bindFigureSkin>[1] & { name: string },
  playback: { anchors: Readonly<Anime25DPlaybackAnchors>; partProfile?: Anime25DPartProfile; pixelCanvas: { width: number; height: number } },
): { figure: FigureRest | null; figureSkin: FigureSkin | null; partSways: PartSwayBinding[] | null } {
  const figure = figureRestOf(playback.anchors)
  return {
    figure,
    figureSkin: bindFigureSkin(baseRole, layer, figure),
    partSways: figure ? bindPartSways(playback.partProfile, layer.name, playback.pixelCanvas) : null,
  }
}

/** Parts that hang from the shoulders and neck, carried whole by the chest. */
const CHEST_ROLES = new Set(['handwear', 'neckwear', 'neck', 'collar_front', 'collar_back', 'hair', 'front-hair', 'back-hair', 'earwear', 'headwear'])
/** Below this share of the leg span, a loose part is on a leg. */
const LOW_ON_LEG = 0.3

const figureRests = new WeakMap<object, FigureRest | null>()
/** One rest skeleton per package's anchors. */
export function figureRestOf(anchors: Readonly<Anime25DPlaybackAnchors>): FigureRest | null {
  if (!figureRests.has(anchors)) figureRests.set(anchors, figureRest(anchors))
  return figureRests.get(anchors) ?? null
}

/** What a frame carries of a figure: its bones, this frame's pose and its swaying parts' lags. */
export interface FigureFrame {
  rest: Readonly<FigureRest>
  pose: Readonly<FigurePose>
  sways?: { readonly lags: Readonly<Float32Array> }
}

/**
 * A layer's point at the end of its own deformation, carried on: a hem
 * trails the hips and cloth hanging free in it trails its bone, on their
 * own, then the bones carry the point. A layer the shader carries whole
 * rides its bones in its layer transform instead.
 */
export function carryOnFigure(
  point: Point,
  binding: Readonly<{
    standing: StandingBinding | null
    figureSkin?: FigureSkin | null
    partSways?: readonly PartSwayBinding[] | null
    shaderGlobalTransform: boolean
  }>,
  restX: number,
  restY: number,
  frame: Readonly<{ standing?: Readonly<Anime25DStandingFrame> | null; figure?: FigureFrame | null }>,
): void {
  if (binding.standing && frame.standing) applySkirtTrail(point, binding.standing, restX, restY, frame.standing)
  if (binding.partSways && frame.figure?.sways) applyPartSways(point, restX, restY, binding.partSways, frame.figure.sways.lags)
  if (frame.figure && binding.figureSkin && !binding.shaderGlobalTransform) {
    skinFigurePoint(point, restY, binding.figureSkin, frame.figure.rest, frame.figure.pose)
  }
}

/** What a figure is drawn on: its layers' boxes and the canvas they are on. */
export interface FigureCanvas {
  layers: readonly Readonly<{ x: number; y: number; w: number; h: number }>[]
  pixelCanvas: Readonly<{ width: number }>
}

/**
 * Of the room beside a figure on its canvas, a full sway may take this much:
 * the rest is for the arms swinging out past it.
 */
const SWAY_ROOM = 0.85

/**
 * The share of the full sway (FIGURE_HIP_SHIFT, FIGURE_LEAN) a figure has room
 * for: at a full lean, the top of its drawing goes no nearer an edge of its
 * canvas than the room left there allows, or it would be cut off.
 */
export function figureSwayReach(rest: Readonly<FigureRest>, canvas: FigureCanvas): number {
  let left = Infinity
  let right = -Infinity
  let top = Infinity
  for (const layer of canvas.layers) {
    if (!(layer.w > 0 && layer.h > 0)) continue
    left = Math.min(left, layer.x)
    right = Math.max(right, layer.x + layer.w)
    top = Math.min(top, layer.y)
  }
  if (!Number.isFinite(left)) return 1
  const room = Math.max(0, Math.min(left, canvas.pixelCanvas.width - right)) * SWAY_ROOM
  const travel = FIGURE_HIP_SHIFT * rest.legSpan + Math.tan(FIGURE_LEAN) * Math.max(0, rest.hips.y - top)
  return travel > 0 ? Math.min(1, room / travel) : 1
}

/** A standing figure's bones in a running player: posed each frame from its sway and stance. */
export class FigureBody {
  readonly pose = createFigurePose()

  private constructor(
    readonly rest: FigureRest,
    /** Cloth hanging free in layers it shares, trailing its bones (partSway). */
    readonly sways: PartSways,
    /** A floating figure's springs (figureFloat); a standing one has none. */
    private readonly float: FigureFloat | null,
    /** The share of the full sway its canvas has room for (figureSwayReach). */
    readonly reach: number,
  ) {}

  static of(
    anchors: Readonly<Anime25DPlaybackAnchors>,
    parts?: Anime25DPartProfile,
    stance: FigureStance = 'ground',
    canvas?: FigureCanvas,
  ): FigureBody | null {
    const rest = figureRestOf(anchors)
    if (!rest) return null
    const reach = canvas ? figureSwayReach(rest, canvas) : 1
    return new FigureBody(rest, new PartSways(parts), stance === 'float' ? new FigureFloat(rest) : null, reach)
  }

  /** Its hip shift at a full lean, as a share of the leg span. */
  get hipShift(): number {
    return FIGURE_HIP_SHIFT * this.reach
  }

  /** Steps a floating figure's legs and the swaying parts after the bones moved. */
  stepParts(dt: number, body: number, dynamic: boolean): void {
    body *= this.reach
    this.float?.step(dt, body, dynamic)
    this.sways.step(dt, this.pose, dynamic)
  }

  /** Poses the bones for a sway of `body`: floating, or standing on the stance's legs. */
  step(body: number, stance: Readonly<Anime25DStandingFrame> | undefined): void {
    if (this.float) {
      this.float.pose(body * this.reach, this.pose)
      return
    }
    solveFigurePose(this.rest, {
      body: body * this.reach,
      pelvisTilt: stance?.pelvisTilt ?? 0,
      bendL: stance?.bendL ?? 0,
      bendR: stance?.bendR ?? 0,
    }, this.pose)
  }

  /** Hangs a head-carry transform (column-major 3×3) from the chest. */
  carryHead(m: Float32Array): void {
    carryAffine(m, this.pose.chest, m)
  }
}
