import type {
  Anime25DAttachmentPixels,
  Anime25DLayerAttachment,
} from './layerAttachment'
import type { Anime25DPlaybackLayer } from './types'

/** A rigid pendant, not a deformable strand. No asset/schema migration needed. */
export function bindEarwearPhysics(
  source: Anime25DPlaybackLayer,
  attachment: Pick<Anime25DLayerAttachment, 'y'> | null,
  art: Anime25DAttachmentPixels | null,
  faceHeight: number,
): EarwearPhysics | null {
  if (
    source.role !== 'earwear' ||
    !attachment ||
    !art ||
    source.phys ||
    source.fade
  ) {
    return null
}
  if (
    art.width <= 0 ||
    art.height <= 0 ||
    art.pixels.length !== art.width * art.height * 4
  ) {
    return null
}
  let left = art.width
  let right = -1
  let top = art.height
  let bottom = -1
  for (let y = 0; y < art.height; y++) {
    for (let x = 0; x < art.width; x++) {
      if (art.pixels[(y * art.width + x) * 4 + 3] < 32) continue
      left = Math.min(left, x)
      right = Math.max(right, x)
      top = Math.min(top, y)
      bottom = Math.max(bottom, y)
    }
  }
  if (right < left || bottom < top || faceHeight <= 0) return null
  const width = ((right - left + 1) * source.w) / art.width
  const height = ((bottom - top + 1) * source.h) / art.height
  // Conservative silhouette evidence: studs and unresolved paired drawings stay rigid.
  if (height < faceHeight * 0.09 || height < width * 1.25) return null
  const length =
    source.y + ((bottom + 1) * source.h) / art.height - attachment.y
  if (length <= 0) return null
  return new EarwearPhysics(length, faceHeight, source.side === 'L' ? -1 : 1)
}

/**
 * Damped angular pendula driven by support acceleration, with a rigid 3D projection.
 * Cubism's input/model/output separation is retained; constants are artistic, not
 * measured jewellery properties. There is no wind/random forcing at rest.
 */
export class EarwearPhysics {
  private time: number | null = null
  private x = 0
  private y = 0
  private z = 0
  private vx = 0
  private vy = 0
  private vz = 0
  private hasVelocity = false
  private roll = 0
  private rollVelocity = 0
  private pitch = 0
  private pitchVelocity = 0

  constructor(
    private readonly length: number,
    private readonly faceHeight: number,
    private readonly side: number,
  ) {}

  /** Called after the host's final mesh; matrix must be freshly written each frame. */
  apply(
    a: Pick<Anime25DLayerAttachment, 'x' | 'y'>,
    m: Float32Array,
    time: number,
    yaw: number,
    nod: number,
    enabled: boolean,
  ): void {
    const x = m[0] * a.x + m[3] * a.y + m[6]
    const y = m[1] * a.x + m[4] * a.y + m[7]
    // Depth is unavailable in the raster host mesh. This is an explicit bounded
    // pose-derived proxy, not a second application of its screen-space translation.
    const z = this.faceHeight * (this.side * yaw * 0.12 + nod * 0.08)
    const dt = this.time === null ? 0 : time - this.time
    const hostRoll = Math.atan2(m[1], m[0])
    if (
      !enabled ||
      this.time === null ||
      dt < 0 ||
      dt > 0.1 ||
      Math.hypot(x - this.x, y - this.y, z - this.z) > this.faceHeight * 0.4
    ) {
      this.roll = hostRoll
      this.pitch = 0
      this.rollVelocity = this.pitchVelocity = 0
      this.hasVelocity = false
    } else if (dt > 0) {
      const vx = (x - this.x) / dt
      const vy = (y - this.y) / dt
      const vz = (z - this.z) / dt
      const limit = this.faceHeight * 30
      const ax = this.hasVelocity
        ? clamp((vx - this.vx) / dt, -limit, limit)
        : 0
      const ay = this.hasVelocity
        ? clamp((vy - this.vy) / dt, -limit, limit)
        : 0
      const az = this.hasVelocity
        ? clamp((vz - this.vz) / dt, -limit, limit)
        : 0
      const omega2 = clamp((this.faceHeight / this.length) * 24, 36, 180)
      const damping = 0.65 * Math.sqrt(omega2)
      const count = Math.ceil(dt * 240)
      const h = dt / count
      for (let i = 0; i < count; i++) {
        this.rollVelocity +=
          (-omega2 * Math.sin(this.roll - hostRoll * 0.15) -
            damping * this.rollVelocity +
            (ax * Math.cos(this.roll) + ay * Math.sin(this.roll)) /
              this.length) *
          h
        this.pitchVelocity +=
          (-omega2 * Math.sin(this.pitch) -
            damping * this.pitchVelocity +
            (-az * Math.cos(this.pitch) + ay * Math.sin(this.pitch)) /
              this.length) *
          h
        this.roll += this.rollVelocity * h
        this.pitch += this.pitchVelocity * h
        if (Math.abs(this.roll) > 0.65) {
          this.roll = clamp(this.roll, -0.65, 0.65)
          this.rollVelocity = 0
        }
        if (Math.abs(this.pitch) > 0.55) {
          this.pitch = clamp(this.pitch, -0.55, 0.55)
          this.pitchVelocity = 0
        }
      }
      this.vx = vx
      this.vy = vy
      this.vz = vz
      this.hasVelocity = true
    }
    if (dt !== 0 || this.time === null) {
      this.x = x
      this.y = y
      this.z = z
      this.time = time
    }
    if (!enabled) return
    const c = Math.cos(this.roll)
    const s = Math.sin(this.roll)
    const cp = Math.cos(this.pitch)
    const sp = Math.sin(this.pitch)
    const twist = clamp(yaw * 0.4, -0.45, 0.45)
    const ct = Math.cos(twist)
    const st = Math.sin(twist)
    // XY projection of Rz(roll) Rx(pitch) Ry(twist): no stretching in 3D,
    // no invented back-face texture and no modification of draw order.
    m[0] = c * ct - s * sp * st
    m[1] = s * ct + c * sp * st
    m[3] = -s * cp
    m[4] = c * cp
    m[6] = x - m[0] * a.x - m[3] * a.y
    m[7] = y - m[1] * a.x - m[4] * a.y
  }
}

function clamp(v: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, v))
}
