import type { BodyLift } from './bodyLift'
import type { Anime25DPlaybackAnchors } from './types'

/**
 * What only a full figure does: it stands on a ground. Shifting its weight,
 * the hips move against the lean while the soles stay planted, and a skirt
 * hung on the hips trails them. A bust is cut off above any ground and never
 * builds one of these.
 */

/**
 * The hip shift at `body` = ±1, as a share of the leg length. The hips move
 * against the lean, so the weight stays over the feet.
 */
export const STANCE_SHIFT = 0.06

/** A skirt hem on the hips: a loose, slow pendulum that trails and then overshoots. */
const SKIRT_FREQUENCY = 1.3
const SKIRT_DAMPING = 0.35

/** A skirt layer: the waist it hangs from and its hem. */
export interface SkirtBinding {
  topY: number
  bottomY: number
}

export class Anime25DStanding {
  /** Where the skirt hem has got to, chasing the hips. */
  private skirtX = 0
  private skirtVelocity = 0

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

  /** Steps the skirt hem after the hips; returns how far it trails them. */
  stepSkirt(dt: number, body: number, phys: boolean): number {
    const hips = this.shift(body)
    if (!phys) {
      this.skirtX = hips
      this.skirtVelocity = 0
      return 0
    }
    const omega = 2 * Math.PI * SKIRT_FREQUENCY
    this.skirtVelocity += (-omega * omega * (this.skirtX - hips) - 2 * SKIRT_DAMPING * omega * this.skirtVelocity) * dt
    this.skirtX += this.skirtVelocity * dt
    // The hem's lag behind the hips the stance field already carries it with.
    return this.skirtX - hips
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

/** A skirt hangs on a standing figure's hips; a bust's lower clothing is cut off with it. */
export function bindSkirt(
  baseRole: string,
  layer: { y: number; h: number },
  anchors: Readonly<Anime25DPlaybackAnchors>,
): SkirtBinding | null {
  return baseRole === 'bottomwear' && anchors.groundY !== undefined
    ? { topY: layer.y, bottomY: layer.y + layer.h }
    : null
}

/** Held at the waist, freest at the hem. */
export function applySkirtSwing(
  point: { x: number },
  skirt: Readonly<SkirtBinding>,
  restY: number,
  swing: number,
): void {
  const along = Math.max(0, Math.min(1, (restY - skirt.topY) / Math.max(1, skirt.bottomY - skirt.topY)))
  point.x += swing * along * along
}
