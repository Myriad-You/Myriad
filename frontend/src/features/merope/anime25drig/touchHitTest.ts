import type { Anime25DRenderFrame } from './renderer'
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
  > & Partial<Pick<Anime25DRenderFrame, 'bodyBendHeight'>>,
): TouchMeshHit | null {
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null
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
