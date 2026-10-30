import type { Anime25DRenderFrame } from './renderer'
import { applyBodyLift } from './bodyLift'
import { bodyLeanShare } from './poseScale'

export interface TouchMesh {
  /** The positions actually submitted to the vertex buffer, not the rest mesh. */
  positions: Float32Array
  atlasUvs: Float32Array
  indices: Uint16Array
  layerTransform: Float32Array
}

export interface TouchMeshHit {
  triangle: number
  u: number
  v: number
}

/** Mesh geometry only, matching the vertex shader's layer-then-body transform. */
export function hitTestTouchMesh(
  x: number,
  y: number,
  mesh: TouchMesh,
  frame: Pick<
    Anime25DRenderFrame,
    'bodyPivotX' | 'bodyPivotY' | 'bodyRotationCosine' | 'bodyRotationSine'
  > & Partial<Pick<Anime25DRenderFrame, 'bodyBendHeight' | 'bodyLift'>>,
): TouchMeshHit | null {
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null
  if (frame.bodyLift && (frame.bodyLift.amount || frame.bodyLift.pitch)) {
    // The GPU interpolates transformed triangle vertices, not the continuous
    // field inside each triangle. Pick those exact triangles (no inverse-warp
    // approximation near a coarse garment edge).
    const positions = new Float32Array(mesh.positions.length)
    const m = mesh.layerTransform; const point = { x: 0, y: 0 }
    const lean = Math.atan2(frame.bodyRotationSine, frame.bodyRotationCosine)
    for (let i = 0; i < positions.length; i += 2) {
      const x = mesh.positions[i]; const y = mesh.positions[i + 1]
      const px = m[0] * x + m[3] * y + m[6]; const py = m[1] * x + m[4] * y + m[7]
      const angle = lean * bodyLeanShare(py, frame.bodyPivotY, frame.bodyBendHeight ?? 0)
      const c = Math.cos(angle); const s = Math.sin(angle)
      point.x = frame.bodyPivotX + (px - frame.bodyPivotX) * c - (py - frame.bodyPivotY) * s
      point.y = frame.bodyPivotY + (px - frame.bodyPivotX) * s + (py - frame.bodyPivotY) * c
      applyBodyLift(point, frame.bodyLift)
      positions[i] = point.x; positions[i + 1] = point.y
    }
    return hitTestTouchMesh(x, y, { ...mesh, positions, layerTransform: new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1]) },
      { bodyPivotX: 0, bodyPivotY: 0, bodyRotationCosine: 1, bodyRotationSine: 0 })
  }
  const {
    bodyRotationCosine: c,
    bodyRotationSine: s,
    bodyPivotX: px,
    bodyPivotY: py,
  } = frame
  const rotationNorm = c * c + s * s
  if (!Number.isFinite(rotationNorm) || rotationNorm < 1e-12) return null
  const dx = x - px
  const dy = y - py
  const lean = Math.atan2(s, c)
  const bendHeight = frame.bodyBendHeight ?? 0
  // The lean each point takes depends on where it came from; a few fixed-point
  // steps undo the bend exactly enough for a finger.
  let bx = x
  let by = y
  for (let step = 0; step < (bendHeight > 0 ? 4 : 1); step += 1) {
    const angle = lean * bodyLeanShare(by, py, bendHeight)
    const ac = Math.cos(angle)
    const as = Math.sin(angle)
    bx = ac * dx + as * dy + px
    by = -as * dx + ac * dy + py
  }
  const m = mesh.layerTransform
  const determinant = m[0] * m[4] - m[1] * m[3]
  if (!Number.isFinite(determinant) || Math.abs(determinant) < 1e-12)
    return null
  const lx = bx - m[6]
  const ly = by - m[7]
  const qx = (m[4] * lx - m[3] * ly) / determinant
  const qy = (-m[1] * lx + m[0] * ly) / determinant
  const p = mesh.positions
  const uv = mesh.atlasUvs
  for (let i = mesh.indices.length - 3; i >= 0; i -= 3) {
    const a = mesh.indices[i] * 2
    const b = mesh.indices[i + 1] * 2
    const d = mesh.indices[i + 2] * 2
    const abx = p[b] - p[a]
    const aby = p[b + 1] - p[a + 1]
    const adx = p[d] - p[a]
    const ady = p[d + 1] - p[a + 1]
    const area = abx * ady - aby * adx
    if (!Number.isFinite(area) || Math.abs(area) < 1e-12) continue
    const qax = qx - p[a]
    const qay = qy - p[a + 1]
    const wb = (qax * ady - qay * adx) / area
    const wd = (abx * qay - aby * qax) / area
    const wa = 1 - wb - wd
    if (
      !Number.isFinite(wa) ||
      !Number.isFinite(wb) ||
      !Number.isFinite(wd) ||
      Math.min(wa, wb, wd) < -1e-7
    ) {
      continue
}
    const u = wa * uv[a] + wb * uv[b] + wd * uv[d]
    const v = wa * uv[a + 1] + wb * uv[b + 1] + wd * uv[d + 1]
    if (Number.isFinite(u) && Number.isFinite(v))
      return { triangle: i / 3, u, v }
  }
  return null
}

export function touchPointInView(
  x: number,
  y: number,
  bounds: { left: number; top: number; width: number; height: number },
  view: { width: number; height: number },
): { x: number; y: number } | null {
  if (
    ![
      x,
      y,
      bounds.left,
      bounds.top,
      bounds.width,
      bounds.height,
      view.width,
      view.height,
    ].every(Number.isFinite) ||
    bounds.width <= 0 ||
    bounds.height <= 0 ||
    view.width <= 0 ||
    view.height <= 0
  ) {
    return null
}
  const nx = (x - bounds.left) / bounds.width
  const ny = (y - bounds.top) / bounds.height
  if (nx < 0 || nx >= 1 || ny < 0 || ny >= 1) return null
  return { x: nx * view.width, y: ny * view.height }
}

export function sampleTouchAlpha(
  alpha: Uint8Array,
  width: number,
  height: number,
  u: number,
  v: number,
): number {
  if (
    !Number.isInteger(width) ||
    !Number.isInteger(height) ||
    width <= 0 ||
    height <= 0 ||
    alpha.length !== width * height ||
    !Number.isFinite(u) ||
    !Number.isFinite(v) ||
    u < 0 ||
    u > 1 ||
    v < 0 ||
    v > 1
  ) {
    return 0
}
  const x = u * width - 0.5
  const y = v * height - 0.5
  const x0 = Math.floor(x)
  const y0 = Math.floor(y)
  const fx = x - x0
  const fy = y - y0
  const at = (ix: number, iy: number) =>
    alpha[
      Math.max(0, Math.min(height - 1, iy)) * width +
        Math.max(0, Math.min(width - 1, ix))
    ] / 255
  return (
    (at(x0, y0) * (1 - fx) + at(x0 + 1, y0) * fx) * (1 - fy) +
    (at(x0, y0 + 1) * (1 - fx) + at(x0 + 1, y0 + 1) * fx) * fy
  )
}
