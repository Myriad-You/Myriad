import type { Anime25DTorsoShellRotation } from './torsoDeformation'
import type { Anime25DTurnKeyforms, BoundTurnKeyform } from './turnKeyforms'
import type { Anime25DPlaybackLayer } from './types'
import { HEAD_TURN_RADIANS } from './headTurn'
import { HEAD_YAW_SHARE } from './torsoDeformation'
import { bindTurnKey, turnKeyformOffset } from './turnKeyforms'

/**
 * A standing figure's body turn drawn, not computed: each body part has the
 * shape it takes with the whole figure turned BODY_KEY_YAW either way, its
 * feet where they stood, as a lattice like the head's turn keys ('plus'
 * toward +x). A turn in between mixes rest and key linearly by the torso's
 * yaw. The head goes along whole, by the 'head' key at the neck pivot; its
 * own turn stays with the head keys.
 */
export const BODY_KEY_YAW = (20 * Math.PI) / 180
/**
 * The keyed turn shows this share of the torso's yaw: the most a turn is
 * directed (0.45 rad) draws the body about 13° round, where the full key's
 * 20° already reads as a twist against a head kept on the viewer.
 */
export const BODY_TURN_SHARE = 0.5

type KeyedLayer = Pick<Anime25DPlaybackLayer, 'group' | 'role' | 'side'>

/** Which body key a layer turns with; null for the head (carried whole), what rides a host (a choker) and loose objects. */
export function bodyTurnKeyFamily(layer: KeyedLayer): string | null {
  if (layer.group !== 'body') return null
  const { role, side } = layer
  if (role === 'handwear') return side ? `arm:${side}` : 'topwear'
  if ((role === 'legwear' || role === 'footwear') && side) return `${role}:${side}`
  if (role === 'collar-front' || role === 'collar-back') return 'topwear'
  if (['neck', 'topwear', 'bottomwear', 'legwear', 'footwear'].includes(role)) return role
  return null
}

/**
 * A layer's body key per rest point. A leg measured as thigh and shin
 * ('thigh:L', 'shin:L') takes the thigh's above its knee and the shin's below,
 * blended across it; `kneeY` is the knee found on the portrait, else the
 * middle of the leg's drawing.
 */
export function bindBodyTurnKeyform(
  keyforms: Readonly<Anime25DTurnKeyforms> | undefined,
  layer: KeyedLayer,
  rest: Float32Array,
  kneeY?: number,
): BoundTurnKeyform | null {
  const thigh = layer.role === 'legwear' && layer.side ? keyforms?.[`thigh:${layer.side}`] : undefined
  const shin = layer.role === 'legwear' && layer.side ? keyforms?.[`shin:${layer.side}`] : undefined
  if (thigh && shin) return bentAtKnee(bindTurnKey(thigh, rest), bindTurnKey(shin, rest), rest, kneeY)
  const family = bodyTurnKeyFamily(layer)
  // Each leg and shoe turns by its own key where one was measured, else with both.
  const key = family && (keyforms?.[family] ?? keyforms?.[family.split(':')[0]])
  if (!key) return null
  const bound = bindTurnKey(key, rest)
  const torso = family.startsWith('arm:') ? keyforms?.topwear : undefined
  return torso ? sewnToTorso(bound, bindTurnKey(torso, rest), rest, family.slice(4)) : bound
}

/** A thigh's key gives way to the shin's over this share of the leg's height, about the knee. */
const KNEE_BLEND = 0.12

function bentAtKnee(thigh: BoundTurnKeyform, shin: BoundTurnKeyform, rest: Float32Array, kneeY?: number): BoundTurnKeyform {
  let top = Infinity
  let bottom = -Infinity
  for (let i = 1; i < rest.length; i += 2) {
    top = Math.min(top, rest[i])
    bottom = Math.max(bottom, rest[i])
  }
  const knee = kneeY !== undefined && Number.isFinite(kneeY) ? kneeY : (top + bottom) / 2
  const band = Math.max(1, (bottom - top) * KNEE_BLEND)
  for (const turn of ['plus', 'minus'] as const) {
    const own = thigh[turn]
    const below = shin[turn]
    for (let i = 0; i < own.length; i += 2) {
      const w = smooth((rest[i + 1] - knee + band / 2) / band)
      own[i] += (below[i] - own[i]) * w
      own[i + 1] += (below[i + 1] - own[i + 1]) * w
    }
  }
  return thigh
}

/**
 * A sleeve's top and the side it hangs against the torso by go with the
 * torso, this share of its height and width in, so neither the seam nor the
 * torso behind opens.
 */
const SHOULDER_SEAM = 0.2
const INNER_SIDE = 0.35

/**
 * An arm's key and the torso's are measured apart and part where they meet:
 * the shoulder the sleeve is sewn on, and the torso's side it hangs against
 * (whose hidden part was never drawn). There the arm takes the torso's key,
 * its own further out.
 */
function sewnToTorso(arm: BoundTurnKeyform, torso: BoundTurnKeyform, rest: Float32Array, side: string): BoundTurnKeyform {
  let top = Infinity
  let bottom = -Infinity
  let left = Infinity
  let right = -Infinity
  for (let i = 0; i < rest.length; i += 2) {
    left = Math.min(left, rest[i])
    right = Math.max(right, rest[i])
    top = Math.min(top, rest[i + 1])
    bottom = Math.max(bottom, rest[i + 1])
  }
  const seam = Math.max(1, (bottom - top) * SHOULDER_SEAM)
  const inner = Math.max(1, (right - left) * INNER_SIDE)
  // Myriad's sides are the picture's: a left arm hangs against the torso by its right.
  const toward = side === 'L' ? 1 : -1
  for (const turn of ['plus', 'minus'] as const) {
    const own = arm[turn]
    const body = torso[turn]
    for (let i = 0; i < own.length; i += 2) {
      const fromInner = toward > 0 ? right - rest[i] : rest[i] - left
      const w = smooth((rest[i + 1] - top) / seam) * smooth(fromInner / inner)
      own[i] = body[i] + (own[i] - body[i]) * w
      own[i + 1] = body[i + 1] + (own[i + 1] - body[i + 1]) * w
    }
  }
  return arm
}

function smooth(value: number): number {
  const t = Math.max(0, Math.min(1, value))
  return t * t * (3 - 2 * t)
}

/**
 * A figure's keyed body turn: how far through it the torso is, and how far
 * that carries the head (its 'head' key at the neck pivot). With weight-shift
 * keys too (weightKeyforms, kept only beside body keys), the head also rides
 * the weight's 'head' key, and the hips' computed shift is left out (weighted).
 */
export class KeyedBodyTurn {
  private readonly carry = { x: 0, y: 0 }
  private readonly weightCarry = { x: 0, y: 0 }

  private constructor(
    private readonly head: BoundTurnKeyform | null,
    private readonly weightHead: BoundTurnKeyform | null,
    readonly weighted: boolean,
  ) {}

  static bind(
    keyforms: Readonly<Anime25DTurnKeyforms> | undefined,
    neckPivot: { x: number; y: number },
    weightKeyforms?: Readonly<Anime25DTurnKeyforms>,
  ): KeyedBodyTurn | null {
    if (!keyforms) return null
    const at = Float32Array.of(neckPivot.x, neckPivot.y)
    return new KeyedBodyTurn(
      keyforms.head ? bindTurnKey(keyforms.head, at) : null,
      weightKeyforms?.head ? bindTurnKey(weightKeyforms.head, at) : null,
      Boolean(weightKeyforms),
    )
  }

  /**
   * Writes how far through the keyed turn the body is (frame.bodyTurn) and
   * returns the head's sideways carry in it and in the weight shift, or null
   * without a head key.
   */
  step(
    frame: { bodyTurn?: number; standing?: Readonly<StandingBends> | null },
    rotation: Readonly<Anime25DTorsoShellRotation>,
    activation: number,
  ): number | null {
    const amount = bodyTurnAmount(rotation, activation)
    frame.bodyTurn = amount
    if (!this.head) return null
    turnKeyformOffset(this.head, 0, amount, this.carry)
    if (!this.weightHead || !frame.standing) return this.carry.x
    turnKeyformOffset(this.weightHead, 0, weightShift(frame.standing), this.weightCarry)
    return this.carry.x + this.weightCarry.x
  }
}

interface StandingBends {
  bendL: number
  bendR: number
}

/**
 * How far through the keyed weight shift a standing figure is (−1…1): +1 with
 * the weight on the leg on the right of the picture, the left one free and
 * bent (the stance's springs, so the keys pass from leg to leg as they do).
 */
export function weightShift(standing: Readonly<StandingBends>): number {
  return Math.max(-1, Math.min(1, standing.bendL - standing.bendR))
}

/** How far through the keyed turn the body is (−1…1), from the torso's yaw. */
export function bodyTurnAmount(rotation: Readonly<Anime25DTorsoShellRotation>, activation: number): number {
  if (!rotation.active) return 0
  const yaw = Math.atan2(rotation.yawSine, rotation.yawCosine)
  return Math.max(-1, Math.min(1, (yaw / BODY_KEY_YAW) * BODY_TURN_SHARE)) * activation
}

const keyed = { x: 0, y: 0 }

/** Moves a vertex by its part's body key at `amount` of the keyed turn. */
export function addBodyTurn(point: { x: number; y: number }, bound: Readonly<BoundTurnKeyform>, vertex: number, amount: number): void {
  if (amount === 0) return
  turnKeyformOffset(bound, vertex, amount, keyed)
  point.x += keyed.x
  point.y += keyed.y
}

/**
 * The head turns with a keyed body: a directed torso turn (torsoTurn, −1…1)
 * adds as much head turn (angleX, whose full turn is HEAD_TURN_RADIANS) as
 * the body shows, so the head keeps its angle to the shoulders instead of
 * staying on the viewer. Only a keyed head goes along: a computed turn slides
 * the head and the neck's top with it, shearing a choker against the collar.
 * Otherwise, nothing: angleX is the head's own angle, as before.
 */
export function headRidesBody(bodyKeyed: boolean, headKeyed: boolean): number {
  return bodyKeyed && headKeyed ? (HEAD_YAW_SHARE * BODY_TURN_SHARE) / HEAD_TURN_RADIANS : 0
}
