import type { Anime25DTorsoShellRotation } from './torsoDeformation'
import type { Anime25DTurnKeyforms, BoundTurnKeyform } from './turnKeyforms'
import type { Anime25DPlaybackLayer } from './types'
import { HEAD_TURN_RADIANS } from './headTurn'
import { isStandingFigure } from './standing'
import { HEAD_YAW_SHARE } from './torsoDeformation'
import { bindTurnKey, turnKeyformOffset } from './turnKeyforms'

/**
 * A standing figure's body turn drawn, not computed: each body part has the
 * shape it takes with the whole figure turned BODY_KEY_YAW either way, its
 * feet where they stood, as a lattice like the head's turn keys ('plus'
 * toward +x). A turn in between mixes rest and key linearly by the torso's
 * yaw. Only the legs and shoes keep their keys (playbackBodyKeys); the rest
 * turns on the torso's cylinder, and the head keeps its own turn keys.
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
  if (thigh && shin) return atHip(bentAtKnee(bindTurnKey(thigh, rest), bindTurnKey(shin, rest), rest, kneeY), rest)
  const family = bodyTurnKeyFamily(layer)
  // Each leg and shoe turns by its own key where one was measured, else with both.
  const key = family && (keyforms?.[family] ?? keyforms?.[family.split(':')[0]])
  if (!key) return null
  const bound = bindTurnKey(key, rest)
  const torso = family.startsWith('arm:') ? keyforms?.topwear : undefined
  if (torso) return sewnToTorso(bound, bindTurnKey(torso, rest), rest, family.slice(4))
  return layer.role === 'legwear' ? atHip(bound, rest) : bound
}

/** A leg's key gives way to the torso's cylinder over this share of its height down from its top. */
export const HIP_BLEND = 0.25

/**
 * A leg's top lies inside the skirt (or the trousers' seat), which turns on
 * the torso's cylinder while the leg turns by its key, drawn where the turned
 * pictures hide it: apart, the thigh's top slid out past the skirt's side.
 * The leg's key, with the share per vertex of the cylinder's turn the leg
 * takes in place of it (`cylinder`): all at its top, none from HIP_BLEND of
 * its height down, as a sleeve takes the torso's key at its shoulder
 * (sewnToTorso).
 */
function atHip(bound: BoundTurnKeyform, rest: Float32Array): BoundTurnKeyform {
  let top = Infinity
  let bottom = -Infinity
  for (let i = 1; i < rest.length; i += 2) {
    top = Math.min(top, rest[i])
    bottom = Math.max(bottom, rest[i])
  }
  const span = Math.max(1, (bottom - top) * HIP_BLEND)
  const weights = new Float32Array(rest.length / 2)
  for (let v = 0; v < weights.length; v++) weights[v] = 1 - smooth((rest[v * 2 + 1] - top) / span)
  bound.cylinder = weights
  return bound
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

/** How far through the keyed turn the body is (−1…1), from the torso's yaw. */
export function bodyTurnAmount(rotation: Readonly<Anime25DTorsoShellRotation>, activation: number, share = BODY_TURN_SHARE): number {
  if (!rotation.active) return 0
  const yaw = Math.atan2(rotation.yawSine, rotation.yawCosine)
  return Math.max(-1, Math.min(1, (yaw / BODY_KEY_YAW) * share)) * activation
}

/**
 * The body keys a standing figure turns by: its legs' and shoes' only, which
 * cross as it turns. Its trunk, sleeves, neck and head turn on the torso's
 * cylinder, one field over all of them by where they are: fitted apart, they
 * part at every seam, and a fit follows a garment's outline, which a turn
 * barely moves, so the trunk slid sideways instead of turning.
 */
export function playbackBodyKeys(
  playback: { bodyTurnKeyforms?: Anime25DTurnKeyforms; anchors: { groundY?: number } },
): Anime25DTurnKeyforms | undefined {
  const keys = playback.bodyTurnKeyforms
  if (!keys || !isStandingFigure(playback.anchors)) return keys
  return Object.fromEntries(Object.entries(keys).filter(([family]) => /^(thigh|shin|legwear|footwear)\b/.test(family)))
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
 * `share` is how much of the torso's turn the body shows (bodyKeyShare).
 */
export function headRidesBody(share: number | null, headKeyed: boolean): number {
  return share !== null && headKeyed ? (HEAD_YAW_SHARE * share) / HEAD_TURN_RADIANS : 0
}

/**
 * How much of the torso's turn a rig's body keys show: all of it on a
 * standing figure, which turns on its whole cylinder (playbackBodyKeys),
 * BODY_TURN_SHARE otherwise; null without body keys.
 */
export function bodyKeyShare(
  playback: { bodyTurnKeyforms?: Anime25DTurnKeyforms; anchors: { groundY?: number } },
): number | null {
  if (!playbackBodyKeys(playback)) return null
  return isStandingFigure(playback.anchors) ? 1 : BODY_TURN_SHARE
}
