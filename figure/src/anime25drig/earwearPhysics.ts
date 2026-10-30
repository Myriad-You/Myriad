import type {
  Anime25DAttachmentPixels,
  Anime25DLayerAttachment,
} from './layerAttachment'
import type { PendulumTuning } from './partPhysics'
import type { Anime25DPart, Anime25DPartProfile, Anime25DPlayback, Anime25DPlaybackLayer } from './types'
import { movingPartIn, PENDULUM_DEFAULT, pendulumTuning } from './partPhysics'
import { cutFrom } from './partSplit'

/**
 * What swings from a layer's attachment: a part the inventory says hangs
 * swings by what it is made of; an earring it said nothing of still swings
 * when its drawing hangs.
 */
export function bindHangingPhysics(
  source: Anime25DPlaybackLayer,
  attachment: Pick<Anime25DLayerAttachment, 'x' | 'y'> | null,
  playback: Pick<Anime25DPlayback, 'partProfile' | 'anchors'>,
  readPixels: (source: Anime25DPlaybackLayer) => Anime25DAttachmentPixels | null,
): EarwearPhysics | null {
  const faceHeight = playback.anchors.face.y1 - playback.anchors.face.y0
  // A part drawn alone in its layer, or cut out to one, swings its layer; one
  // drawn with other things only marks where its host sways or is cut.
  const part = movingPartIn(playback.partProfile, source.name)
  const own = part && (part.separate || cutFrom(source) !== undefined)
  if (part && own && attachment) return bindPartPhysics(source, attachment, part, faceHeight, playback.anchors.face.cx)
  return source.role === 'earwear' ? bindEarwearPhysics(source, attachment, readPixels(source), faceHeight) : null
}

/** How far down a cut piece the swing takes over from its host, as a share of its length. */
const BEND_REACH = 0.6
/** A piece this stiff or stiffer bends only over its link, this share of its length. */
const BEND_STIFFNESS = 0.5
const LINK_REACH = 0.25

/**
 * A piece cut out of the layer it shared (partSplit) bends from its cut
 * instead of swinging stiff: its top stays on its host and its swing takes
 * over below, so the cut never opens. Each vertex's share of the swing.
 */
export function bindHangBend(layer: {
  source: Anime25DPlaybackLayer
  rest: Float32Array
  deformed?: Float32Array | null
  attachment: Pick<Anime25DLayerAttachment, 'y'> | null
  earwearPhysics?: EarwearPhysics | null
  hangBend?: Float32Array | null
}, profile: Anime25DPartProfile | undefined): void {
  const { source, rest, attachment } = layer
  layer.hangBend = null
  // Ribbon and cloth bend over much of their length; a gem or a charm of metal
  // only at its link, and swings whole below it.
  const part = movingPartIn(profile, source.name)
  if (!layer.earwearPhysics || !attachment || cutFrom(source) === undefined || !part) return
  const share = part.stiffness >= BEND_STIFFNESS ? LINK_REACH : BEND_REACH
  const reach = share * Math.max(1, source.y + source.h - attachment.y)
  const weights = new Float32Array(rest.length / 2)
  for (let i = 0; i < weights.length; i++) {
    const t = clamp((rest[i * 2 + 1] - attachment.y) / reach, 0, 1)
    weights[i] = t * t * (3 - 2 * t)
  }
  layer.hangBend = weights
  layer.deformed = rest.slice()
}

/**
 * Bends a cut piece's vertices by its swing: `host` carries it as drawn,
 * `swung` where its swing would carry it whole (both column-major 3×3).
 * Returns whether a vertex moved.
 */
export function bendHanging(
  weights: Readonly<Float32Array>,
  host: Readonly<Float32Array>,
  swung: Readonly<Float32Array>,
  rest: Readonly<Float32Array>,
  deformed: Float32Array,
): boolean {
  const det = host[0] * host[4] - host[3] * host[1]
  if (!det) return false
  // The swing in the piece's own space: host⁻¹ · swung.
  const ia = host[4] / det
  const ib = -host[1] / det
  const ic = -host[3] / det
  const id = host[0] / det
  const r0 = ia * swung[0] + ic * swung[1]
  const r1 = ib * swung[0] + id * swung[1]
  const r3 = ia * swung[3] + ic * swung[4]
  const r4 = ib * swung[3] + id * swung[4]
  const r6 = ia * (swung[6] - host[6]) + ic * (swung[7] - host[7])
  const r7 = ib * (swung[6] - host[6]) + id * (swung[7] - host[7])
  let moved = false
  for (let i = 0; i < weights.length; i++) {
    const x = rest[i * 2]
    const y = rest[i * 2 + 1]
    const w = weights[i]
    const nx = x + w * (r0 * x + r3 * y + r6 - x)
    const ny = y + w * (r1 * x + r4 * y + r7 - y)
    moved ||= Math.abs(nx - deformed[i * 2]) > 1e-3 || Math.abs(ny - deformed[i * 2 + 1]) > 1e-3
    deformed[i * 2] = nx
    deformed[i * 2 + 1] = ny
  }
  return moved
}

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
 * A part the import's inventory says hangs and swings (partPhysics), whatever
 * its layer: its pendulum runs from its anchor to the bottom of its drawing,
 * tuned by what it is made of.
 */
export function bindPartPhysics(
  source: Anime25DPlaybackLayer,
  attachment: Pick<Anime25DLayerAttachment, 'x' | 'y'> | null,
  part: Readonly<Anime25DPart>,
  faceHeight: number,
  centerX: number,
): EarwearPhysics | null {
  if (!attachment || faceHeight <= 0) return null
  const length = Math.max(source.y + source.h, part.box[3]) - attachment.y
  if (!(length > faceHeight * 0.05)) return null
  return new EarwearPhysics(length, faceHeight, attachment.x < centerX ? -1 : 1, pendulumTuning(part))
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
    private readonly tuning: Readonly<PendulumTuning> = PENDULUM_DEFAULT,
  ) {}

  /**
   * Called after the host's final mesh; matrix must be freshly written each frame.
   * `keyRoll`, for a keyed earring: the rotation of the host's key at the
   * anchor, which its vertices have taken off (the carry is to bring it back).
   * The pendulum swings about the keyed pose: it hangs from the host's carry
   * without that rotation, gives it back, and leaves foreshortening to the key.
   */
  apply(
    a: Pick<Anime25DLayerAttachment, 'x' | 'y'>,
    m: Float32Array,
    time: number,
    yaw: number,
    nod: number,
    enabled: boolean,
    keyRoll: number | null = null,
  ): void {
    const x = m[0] * a.x + m[3] * a.y + m[6]
    const y = m[1] * a.x + m[4] * a.y + m[7]
    // Depth is unavailable in the raster host mesh. This is an explicit bounded
    // pose-derived proxy, not a second application of its screen-space translation.
    const z = this.faceHeight * (this.side * yaw * 0.12 + nod * 0.08)
    const dt = this.time === null ? 0 : time - this.time
    const hostRoll = Math.atan2(m[1], m[0]) - (keyRoll ?? 0)
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
      const omega2 = clamp((this.faceHeight / this.length) * 24, 36, 180) * this.tuning.stiffness
      const damping = this.tuning.damping * Math.sqrt(omega2)
      const rollLimit = 0.65 * this.tuning.reach
      const pitchLimit = 0.55 * this.tuning.reach
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
        if (Math.abs(this.roll) > rollLimit) {
          this.roll = clamp(this.roll, -rollLimit, rollLimit)
          this.rollVelocity = 0
        }
        if (Math.abs(this.pitch) > pitchLimit) {
          this.pitch = clamp(this.pitch, -pitchLimit, pitchLimit)
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
    const twist = keyRoll === null ? clamp(yaw * 0.4, -0.45, 0.45) : 0
    const ct = Math.cos(twist)
    const st = Math.sin(twist)
    // XY projection of Rz(roll) Rx(pitch) Ry(twist): no stretching in 3D,
    // no invented back-face texture and no modification of draw order.
    const m0 = c * ct - s * sp * st
    const m1 = s * ct + c * sp * st
    const m3 = -s * cp
    const m4 = c * cp
    // Then Rz(keyRoll), the key's rotation the vertices took off.
    const kc = Math.cos(keyRoll ?? 0)
    const ks = Math.sin(keyRoll ?? 0)
    m[0] = m0 * kc + m3 * ks
    m[1] = m1 * kc + m4 * ks
    m[3] = m3 * kc - m0 * ks
    m[4] = m4 * kc - m1 * ks
    m[6] = x - m[0] * a.x - m[3] * a.y
    m[7] = y - m[1] * a.x - m[4] * a.y
  }
}

function clamp(v: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, v))
}

/** An earring hung this far out from the face's middle, as a share of its half-width, is on a side of the head. */
const EAR_SIDE_OUT = 0.3

/** Which side of the head an earring hangs on (+1 the right of the picture), or 0 in front of the face. */
export function earSideOf(x: number, face: Readonly<{ cx: number; x0: number; x1: number }>): number {
  const out = (x - face.cx) / Math.max(1, (face.x1 - face.x0) / 2)
  return Math.abs(out) < EAR_SIDE_OUT ? 0 : Math.sign(out)
}
