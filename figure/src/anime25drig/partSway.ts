import type { BoneTransform, FigurePose } from './figureSkeleton'
import type { Anime25DPart, Anime25DPartProfile } from './types'
import { cutsOut } from './partSplit'

/**
 * Cloth that hangs free inside a layer it shares with other drawing: an open
 * cardigan's front, a peplum, a coat's tail, a ribbon drawn into the hair.
 * A rigger hangs a warp deformer on that patch of the mesh rather than cut
 * it out (nothing is drawn under it). Here the part's region of the layer
 * trails its bone: a spring chases where the bone carries the part's anchor,
 * and the region takes the lag, none at the anchor and all of it at the far
 * end. The spring is tuned by the part's material (the import's inventory).
 * It is done at rest, before the bones carry the layer.
 */

/** One region of a layer that sways, and the spring it takes its lag from. */
export interface PartSwayBinding {
  /** Index of the part's spring in the figure's sway state. */
  spring: number
  x0: number
  y0: number
  x1: number
  y1: number
  /** Where it hangs from: the lag is none here. */
  anchorY: number
  /** How much more than its bone's own travel the far end swings: light, limp cloth more. */
  gain: number
}

interface SwaySpring {
  bone: keyof Omit<FigurePose, 'chestAngle'>
  anchorX: number
  anchorY: number
  frequency: number
  damping: number
  /** Its lag gives out softly past this. */
  reach: number
  x: number
  y: number
  vx: number
  vy: number
  settled: boolean
}

/** The regions of a layer that sway, from the parts drawn in it with other drawing. */
export function bindPartSways(
  profile: Anime25DPartProfile | undefined,
  layer: string,
  canvas: { width: number; height: number },
): PartSwayBinding[] | null {
  const sways: PartSwayBinding[] = []
  profile?.parts.forEach((part, spring) => {
    // A hanging piece is cut out to a layer of its own and swings there (partSplit).
    if (part.layer !== layer || part.separate || part.motion === 'rigid' || cutsOut(part, canvas)) return
    const [x0, y0, x1, y1] = part.box
    if (!(x1 > x0 && y1 > y0)) return
    sways.push({ spring, x0, y0, x1, y1, anchorY: Math.max(y0, Math.min(part.anchor[1], y1)), gain: swayGain(part) })
  })
  return sways.length ? sways : null
}

/**
 * A hem's far end swings further than its bone moves: by this much for cloth
 * that trails, more for a ribbon that flutters, less as it gets heavier.
 */
function swayGain(part: Readonly<Anime25DPart>): number {
  const kind = part.motion === 'flutter' ? 2.6 : part.motion === 'dangle' ? 2.2 : 1.8
  return kind * (1.2 - 0.4 * part.weight)
}

/** The bone a part's anchor rides on a figure. */
function boneOf(part: Readonly<Anime25DPart>): SwaySpring['bone'] {
  switch (part.bone) {
    case 'hips': return 'pelvis'
    case 'waist': return 'abdomen'
    case 'thighL': case 'thighR': case 'shinL': case 'shinR': case 'footL': case 'footR': return part.bone
    // The head, neck and arms hang from the chest.
    default: return 'chest'
  }
}

/** A sway's spring: slower for limp, long cloth, settling sooner when stiff or heavy. */
function swayTuning(part: Readonly<Anime25DPart>): { frequency: number; damping: number } {
  const flutter = part.motion === 'flutter'
  return {
    frequency: (0.9 + 1.1 * part.stiffness) * (flutter ? 0.8 : 1),
    damping: (flutter ? 0.18 : 0.28) + 0.3 * part.stiffness + 0.12 * part.weight,
  }
}

/**
 * A region's lag gives out softly at this share of its length: a sudden lurch
 * otherwise drags a panel off what it is drawn against (a sleeve), and what
 * was never drawn under it shows. Idling stays well within it.
 */
const LAG_REACH = 0.02

/** The lag of each moving part's region behind its bone, for a figure. */
export class PartSways {
  /** x, y of each part's lag, by its index in the profile (zero for parts that do not sway). */
  readonly lags: Float32Array
  private readonly springs: (SwaySpring | null)[]

  constructor(profile: Anime25DPartProfile | undefined) {
    const parts = profile?.parts ?? []
    this.lags = new Float32Array(parts.length * 2)
    this.springs = parts.map((part) => part.motion === 'rigid' || part.separate
      ? null
      : {
          bone: boneOf(part), anchorX: part.anchor[0], anchorY: part.anchor[1], ...swayTuning(part),
          reach: Math.max(1, LAG_REACH * (part.box[3] - part.anchor[1])), x: 0, y: 0, vx: 0, vy: 0, settled: false,
        })
  }

  /** Steps each spring towards where the bones carry its anchor now. */
  step(dt: number, pose: Readonly<FigurePose>, dynamic: boolean): void {
    this.springs.forEach((spring, index) => {
      if (!spring) return
      const bone: Readonly<BoneTransform> = pose[spring.bone]
      const tx = bone.a * spring.anchorX + bone.c * spring.anchorY + bone.tx
      const ty = bone.b * spring.anchorX + bone.d * spring.anchorY + bone.ty
      if (!dynamic || !spring.settled || !(dt > 0)) {
        spring.x = tx
        spring.y = ty
        spring.vx = spring.vy = 0
        spring.settled = true
      } else {
        const omega = 2 * Math.PI * spring.frequency
        spring.vx += (-omega * omega * (spring.x - tx) - 2 * spring.damping * omega * spring.vx) * dt
        spring.vy += (-omega * omega * (spring.y - ty) - 2 * spring.damping * omega * spring.vy) * dt
        spring.x += spring.vx * dt
        spring.y += spring.vy * dt
      }
      const lagX = spring.x - tx
      const lagY = spring.y - ty
      const lag = Math.hypot(lagX, lagY)
      const keep = lag > 0 ? (spring.reach * Math.tanh(lag / spring.reach)) / lag : 1
      this.lags[index * 2] = lagX * keep
      this.lags[index * 2 + 1] = lagY * keep
    })
  }
}

function smoothstep(value: number): number {
  const t = Math.max(0, Math.min(1, value))
  return t * t * (3 - 2 * t)
}

/** How much of a region's lag a rest point takes: none at the anchor, all at the far end, fading out at its sides. */
export function swayShare(sway: Readonly<PartSwayBinding>, x: number, y: number): number {
  if (y <= sway.anchorY) return 0
  const feather = Math.max(1, (sway.x1 - sway.x0) * 0.12)
  const across = Math.min(smoothstep((x - sway.x0 + feather) / (2 * feather)), smoothstep((sway.x1 + feather - x) / (2 * feather)))
  if (!(across > 0)) return 0
  const along = Math.min(1, (y - sway.anchorY) / Math.max(1, sway.y1 - sway.anchorY))
  const below = y > sway.y1 ? smoothstep(1 - (y - sway.y1) / feather) : 1
  return across * along ** 1.5 * below
}

/** Moves a rest point of a layer by the lag of the regions it lies in. */
export function applyPartSways(
  point: { x: number; y: number },
  restX: number,
  restY: number,
  sways: readonly PartSwayBinding[],
  lags: Readonly<Float32Array>,
): void {
  for (const sway of sways) {
    const share = swayShare(sway, restX, restY)
    if (!(share > 0)) continue
    point.x += lags[sway.spring * 2] * share * sway.gain
    point.y += lags[sway.spring * 2 + 1] * share * sway.gain
  }
}
