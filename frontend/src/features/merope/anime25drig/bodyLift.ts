import { applyBodyStance } from './standing'

/**
 * Shared final-space upper-body posture. The chest/head translate as a unit;
 * the lower transition absorbs extension without treating the crop as a joint.
 * This is a 2D shape correction, not a claim of reconstructed 3D anatomy.
 */
export interface BodyLift {
  centerX: number
  upperY: number
  lowerY: number
  amount: number
  /** Sagittal torso projection; the head above shoulderY remains a rigid plane. */
  pitch?: number
  depth?: number
  shoulderY?: number
  /**
   * A standing figure's soles. Shifting its weight, everything from the hips
   * at `lowerY` up moves by `stanceShift` while the soles stay on the ground
   * and the legs lean between them. A bust has no ground.
   */
  groundY?: number
  stanceShift?: number
}

export function applyBodyLift(point: { x: number; y: number }, field?: Readonly<BodyLift>): void {
  if (!field) return
  applyBodyPitch(point, field)
  if (field.amount) {
    const span = Math.max(1, field.lowerY - field.upperY)
    const t = Math.max(0, Math.min(1, (field.lowerY - point.y) / span))
    const amount = Math.max(-span * 0.08, Math.min(span * 0.08, field.amount))
    const gradient = amount * 6 * t * (1 - t) / span
    // Partial width compensation: no whole-torso ballooning. Zero derivative at
    // both ends leaves the chest, shoulders, face and canvas cut undistorted.
    point.x = field.centerX + (point.x - field.centerX) / Math.sqrt(1 + gradient)
    point.y -= amount * t * t * (3 - 2 * t)
  }
  applyBodyStance(point, field)
}

/**
 * Rest-relative projection of a sagittal shell. The cut is a boundary, not a
 * physical hinge: depth and displacement release smoothly near that boundary.
 * No head flattening and no independent deformation of collars/accessories.
 */
export function applyBodyPitch(point: { x: number; y: number }, field: Readonly<BodyLift>): void {
  if (!resolveBodyPitch(field, pitchScratch)) return
  const [top, stretch, offset] = pitchScratch
  const span = Math.max(1, field.lowerY - top)
  const t = Math.max(0, Math.min(1, (field.lowerY - point.y) / span))
  const share = t * t * (3 - 2 * t)
  point.x = field.centerX + (point.x - field.centerX) * (1 + stretch * share)
  point.y += offset * share + (point.y < top
    ? (point.y - top) * stretch
    : stretch * span * t * t * (1 - t))
}

const pitchScratch = new Float64Array(3)

/**
 * The part of the pitch projection that depends on the field alone: shoulder
 * line, upper-body scale minus one, and upper-body offset. It is the only part
 * that needs trigonometry, so the CPU resolves it once per frame and the shader
 * receives numbers. GLSL ES leaves highp sin/cos accuracy to the implementation;
 * headless Chromium's is off by about 1e-4, which moved the head 0.07 px away
 * from the CPU projection that picking and support sampling use.
 */
export function bodyPitchUniform(field: Readonly<BodyLift> | undefined, out: Float32Array): Float32Array {
  if (field) resolveBodyPitch(field, pitchScratch)
  else pitchScratch.fill(0)
  out.set(pitchScratch)
  return out
}

function resolveBodyPitch(field: Readonly<BodyLift>, out: Float64Array): boolean {
  const top = field.shoulderY ?? field.upperY
  out[0] = top; out[1] = 0; out[2] = 0
  const pitch = Math.max(-0.18, Math.min(0.18, field.pitch ?? 0))
  if (!pitch || !(field.depth && field.depth > 0)) return false
  const span = Math.max(1, field.lowerY - top)
  const depth = Math.min(field.depth, span * 0.45)
  // Project an upper-body frame around an interior reference, not the cut.
  // The lower constraint is resolved by a Hermite transition with matched
  // endpoint tangents, so neither shoulders nor the crop acquire a kink.
  const localY = -span * 0.55
  const c = Math.cos(pitch); const s = Math.sin(pitch)
  const rotatedY = localY * c - depth * s
  const rotatedZ = localY * s + depth * c
  const focal = Math.max(span * 6, depth * 12)
  const upperScale = (focal - depth) / (focal - rotatedZ)
  out[1] = upperScale - 1
  out[2] = rotatedY * upperScale - localY
  return true
}

/** Keep the shader equation alongside the CPU equation used by picking/physics. */
export const BODY_LIFT_GLSL = `
// pose = bodyPitchUniform(): shoulder line, upper-body scale - 1, upper-body offset.
vec2 bodyPitch(vec2 p, vec4 f, vec3 pose) {
  if (pose.y == 0.0 && pose.z == 0.0) return p;
  float span = max(1.0, f.z - pose.x);
  float t = clamp((f.z - p.y) / span, 0.0, 1.0);
  float share = t * t * (3.0 - 2.0 * t);
  float correction = p.y < pose.x ? (p.y - pose.x) * pose.y
    : pose.y * span * t * t * (1.0 - t);
  return vec2(f.x + (p.x - f.x) * (1.0 + pose.y * share), p.y + pose.z * share + correction);
}
vec2 bodyLift(vec2 p, vec4 f) {
  float span = max(1.0, f.z - f.y);
  float t = clamp((f.z - p.y) / span, 0.0, 1.0);
  float amount = clamp(f.w, -span * 0.08, span * 0.08);
  float gradient = amount * 6.0 * t * (1.0 - t) / span;
  return vec2(f.x + (p.x - f.x) / sqrt(1.0 + gradient), p.y - amount * t * t * (3.0 - 2.0 * t));
}
`

/** Exact critically damped response: retargets preserve position and velocity. */
export class BodyLiftResponse {
  value = 0
  private velocity = 0
  step(target: number, dt: number): number {
    if (!(dt > 0) || !Number.isFinite(dt) || !Number.isFinite(target)) return this.value
    const d = this.value - target; const k = this.velocity + 11 * d; const decay = Math.exp(-11 * dt)
    this.value = target + (d + k * dt) * decay
    this.velocity = (this.velocity - 11 * k * dt) * decay
    return this.value
  }
}
