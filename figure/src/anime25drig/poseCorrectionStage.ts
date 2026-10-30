import type { CorrectionDriver, PoseCorrection, PoseCorrectionAxis } from './poseCorrections'
import type { Anime25DRenderFrame } from './renderer'
import type { Anime25DSecondaryDeformationBinding } from './secondaryDeformation'
import type { Anime25DShellEllipsoid } from './types'
import { applyBodyLift } from './bodyLift'
import { poseCorrectionInfluence, poseCorrectionWeight } from './poseCorrections'
import { bodyLeanShare } from './poseScale'
import { sampleTouchAlpha, touchPointInView } from './touchHitTest'

export type PoseCorrectionSurface = PoseCorrection['surface']

/** What the stage reads of a layer to pick and show correction points. */
export interface PoseCorrectionStageLayer {
  rest: Float32Array
  deformed: Float32Array
  atlasUvs: Float32Array
  indices: Uint16Array
  layerTransform: Float32Array
  frameOpacity: number
  /** The shell this layer is corrected on; null takes no corrections. */
  surface: PoseCorrectionSurface | null
}

export interface PoseCorrectionAtlas {
  alpha: Uint8Array
  width: number
  height: number
}

type StageFrame = Pick<
  Anime25DRenderFrame,
  'bodyPivotX' | 'bodyPivotY' | 'bodyRotationCosine' | 'bodyRotationSine'
> & Partial<Pick<Anime25DRenderFrame, 'bodyBendHeight' | 'bodyLift'>>

/** Where the vertex shader draws a layer-local point: layer, lean, then lift. */
export function layerPointInView(
  x: number,
  y: number,
  m: Float32Array,
  frame: StageFrame,
): { x: number; y: number } {
  const px = m[0] * x + m[3] * y + m[6]
  const py = m[1] * x + m[4] * y + m[7]
  const lean = Math.atan2(frame.bodyRotationSine, frame.bodyRotationCosine)
  const angle = lean * bodyLeanShare(py, frame.bodyPivotY, frame.bodyBendHeight ?? 0)
  const c = Math.cos(angle)
  const s = Math.sin(angle)
  const point = {
    x: frame.bodyPivotX + (px - frame.bodyPivotX) * c - (py - frame.bodyPivotY) * s,
    y: frame.bodyPivotY + (px - frame.bodyPivotX) * s + (py - frame.bodyPivotY) * c,
  }
  applyBodyLift(point, frame.bodyLift)
  return point
}

function barycentric(
  qx: number,
  qy: number,
  p: Float32Array,
  a: number,
  b: number,
  d: number,
): [number, number, number] | null {
  const abx = p[b] - p[a]
  const aby = p[b + 1] - p[a + 1]
  const adx = p[d] - p[a]
  const ady = p[d + 1] - p[a + 1]
  const area = abx * ady - aby * adx
  if (!Number.isFinite(area) || Math.abs(area) < 1e-12) return null
  const qax = qx - p[a]
  const qay = qy - p[a + 1]
  const wb = (qax * ady - qay * adx) / area
  const wd = (abx * qay - aby * qax) / area
  const wa = 1 - wb - wd
  return Math.min(wa, wb, wd) < -1e-7 ? null : [wa, wb, wd]
}

function viewPositions(layer: PoseCorrectionStageLayer, frame: StageFrame): Float32Array {
  const out = new Float32Array(layer.deformed.length)
  for (let i = 0; i < out.length; i += 2) {
    const point = layerPointInView(layer.deformed[i], layer.deformed[i + 1], layer.layerTransform, frame)
    out[i] = point.x
    out[i + 1] = point.y
  }
  return out
}

/**
 * The correctable surface under a view point, topmost visible layer first,
 * and the rest point drawn there, in head radii like a patch's centre.
 */
export function pickPoseCorrectionPoint(
  x: number,
  y: number,
  layers: readonly PoseCorrectionStageLayer[],
  frame: StageFrame,
  head: Readonly<Anime25DShellEllipsoid>,
  atlas: PoseCorrectionAtlas | null,
): { surface: PoseCorrectionSurface; x: number; y: number } | null {
  for (let index = layers.length - 1; index >= 0; index--) {
    const layer = layers[index]
    if (!layer.surface || !(layer.frameOpacity >= 0.004)) continue
    const view = viewPositions(layer, frame)
    for (let i = layer.indices.length - 3; i >= 0; i -= 3) {
      const a = layer.indices[i] * 2
      const b = layer.indices[i + 1] * 2
      const d = layer.indices[i + 2] * 2
      const w = barycentric(x, y, view, a, b, d)
      if (!w) continue
      const uv = layer.atlasUvs
      const u = w[0] * uv[a] + w[1] * uv[b] + w[2] * uv[d]
      const v = w[0] * uv[a + 1] + w[1] * uv[b + 1] + w[2] * uv[d + 1]
      // A transparent corner of a mesh is not the surface under the pointer.
      if (atlas && sampleTouchAlpha(atlas.alpha, atlas.width, atlas.height, u, v) < 0.25) break
      const r = layer.rest
      const restX = w[0] * r[a] + w[1] * r[b] + w[2] * r[d]
      const restY = w[0] * r[a + 1] + w[1] * r[b + 1] + w[2] * r[d + 1]
      return {
        surface: layer.surface,
        x: (restX - head.centerX) / head.radiusX,
        y: (restY - head.centerY) / head.radiusY,
      }
    }
  }
  return null
}

/** The surface's main layer: the one with the most of its mesh. */
function surfaceLayer(
  layers: readonly PoseCorrectionStageLayer[],
  surface: PoseCorrectionSurface,
): PoseCorrectionStageLayer | null {
  let best: PoseCorrectionStageLayer | null = null
  for (const layer of layers) {
    if (layer.surface !== surface) continue
    if (!best || layer.rest.length > best.rest.length) best = layer
  }
  return best
}

/**
 * Where a patch's spot is drawn without that patch (`origin`) and with it
 * (`pushed`), on the live mesh with every other correction on. The pose may
 * hold a correction at part weight (a soft pitch limit, easing) and the shell
 * blend lets only `gain` of it through: `weight` is the share shown, and the
 * push shows at it, as playback will. A spot off the mesh
 * follows its nearest vertex. `unitX`/`unitY` are the view lengths of one
 * head radius, for drawing the patch's reach.
 */
export function projectPoseCorrectionPatch(
  correction: Pick<PoseCorrection, 'surface' | 'at'>,
  patch: Readonly<PoseCorrection['patches'][number]>,
  layers: readonly PoseCorrectionStageLayer[],
  frame: StageFrame,
  head: Readonly<Anime25DShellEllipsoid>,
  driver: Readonly<CorrectionDriver>,
  gain = 1,
): { origin: { x: number; y: number }; pushed: { x: number; y: number }; weight: number; unitX: number; unitY: number } | null {
  const layer = surfaceLayer(layers, correction.surface)
  if (!layer || layer.rest.length < 2) return null
  const axes = Object.entries(correction.at) as Array<[PoseCorrectionAxis, number]>
  const weight = poseCorrectionWeight(axes, driver) * gain
  const rx = head.centerX + patch.x * head.radiusX
  const ry = head.centerY + patch.y * head.radiusY
  const r = layer.rest
  const p = layer.deformed
  // A vertex as drawn, less this patch's own push at this frame's weight.
  const base = (i: number) => {
    const influence = poseCorrectionInfluence(Math.hypot(
      ((r[i] - head.centerX) / head.radiusX - patch.x) / patch.radiusX,
      ((r[i + 1] - head.centerY) / head.radiusY - patch.y) / patch.radiusY,
    ))
    return {
      x: p[i] - patch.dx * head.radiusX * influence * weight,
      y: p[i + 1] - patch.dy * head.radiusY * influence * weight,
    }
  }
  let local: { x: number; y: number } | null = null
  for (let i = 0; i < layer.indices.length && !local; i += 3) {
    const a = layer.indices[i] * 2
    const b = layer.indices[i + 1] * 2
    const d = layer.indices[i + 2] * 2
    const w = barycentric(rx, ry, r, a, b, d)
    if (w) {
      const [pa, pb, pd] = [base(a), base(b), base(d)]
      local = { x: w[0] * pa.x + w[1] * pb.x + w[2] * pd.x, y: w[0] * pa.y + w[1] * pb.y + w[2] * pd.y }
    }
  }
  if (!local) {
    let nearest = 0
    let distance = Infinity
    for (let i = 0; i < r.length; i += 2) {
      const next = (r[i] - rx) ** 2 + (r[i + 1] - ry) ** 2
      if (next < distance) {
        distance = next
        nearest = i
      }
    }
    const vertex = base(nearest)
    local = { x: vertex.x + rx - r[nearest], y: vertex.y + ry - r[nearest + 1] }
  }
  const m = layer.layerTransform
  return {
    origin: layerPointInView(local.x, local.y, m, frame),
    pushed: layerPointInView(local.x + patch.dx * head.radiusX * weight, local.y + patch.dy * head.radiusY * weight, m, frame),
    weight,
    unitX: head.radiusX * Math.hypot(m[0], m[1]),
    unitY: head.radiusY * Math.hypot(m[3], m[4]),
  }
}

/** A view-space drag as a patch displacement, in head radii. */
export function viewDeltaToPoseCorrection(
  dx: number,
  dy: number,
  layers: readonly PoseCorrectionStageLayer[],
  surface: PoseCorrectionSurface,
  frame: StageFrame,
  head: Readonly<Anime25DShellEllipsoid>,
): { dx: number; dy: number } {
  const m = surfaceLayer(layers, surface)?.layerTransform
  const lean = Math.atan2(frame.bodyRotationSine, frame.bodyRotationCosine)
  // Undo the lean, then the layer's own linear part.
  const c = Math.cos(-lean)
  const s = Math.sin(-lean)
  const ux = dx * c - dy * s
  const uy = dx * s + dy * c
  let lx = ux
  let ly = uy
  if (m) {
    const determinant = m[0] * m[4] - m[1] * m[3]
    if (Number.isFinite(determinant) && Math.abs(determinant) > 1e-12) {
      lx = (m[4] * ux - m[3] * uy) / determinant
      ly = (-m[1] * ux + m[0] * uy) / determinant
    }
  }
  return { dx: lx / head.radiusX, dy: ly / head.radiusY }
}

/** A player layer, as far as corrections go. */
export interface PoseCorrectionPlayerLayer extends Omit<PoseCorrectionStageLayer, 'surface'> {
  attachment: unknown
  neckwearBridge?: unknown
  shaderGlobalTransform: boolean
  secondaryDeformation: Pick<Anime25DSecondaryDeformationBinding, 'shellMode'>
}

/** What the workbench reads of a live player to place correction points. */
export interface PoseCorrectionStageSource {
  canvas: HTMLCanvasElement | OffscreenCanvas
  frame: Anime25DRenderFrame
  layers: readonly PoseCorrectionPlayerLayer[]
  head: Readonly<Anime25DShellEllipsoid>
  atlas: PoseCorrectionAtlas | null
  /** The share of a correction's push the shell blend lets through. */
  gain: number
  /** The pose corrections are weighed against this frame. */
  driver: Readonly<CorrectionDriver>
}

/** The layers corrections bind to, as `previewPoseCorrections` binds them. */
function stageLayers(source: PoseCorrectionStageSource): PoseCorrectionStageLayer[] {
  return source.layers.flatMap(layer =>
    layer.attachment || layer.neckwearBridge || layer.shaderGlobalTransform
      ? []
      : [{ ...layer, surface: layer.secondaryDeformation.shellMode }])
}

/** The stage canvas on screen. */
export function poseCorrectionStageRect(source: PoseCorrectionStageSource): DOMRect | null {
  if (!(source.canvas instanceof HTMLCanvasElement)) return null
  const rect = source.canvas.getBoundingClientRect()
  return rect.width > 0 && rect.height > 0 ? rect : null
}

export function pickPoseCorrectionAtClient(
  source: PoseCorrectionStageSource,
  clientX: number,
  clientY: number,
): { surface: PoseCorrectionSurface; x: number; y: number } | null {
  const rect = poseCorrectionStageRect(source)
  const point = rect && touchPointInView(clientX, clientY, rect, {
    width: source.frame.viewWidth,
    height: source.frame.viewHeight,
  })
  return point
    ? pickPoseCorrectionPoint(point.x, point.y, stageLayers(source), source.frame, source.head, source.atlas)
    : null
}

export function projectPoseCorrectionToClient(
  source: PoseCorrectionStageSource,
  correction: Pick<PoseCorrection, 'surface' | 'at'>,
  patch: Readonly<PoseCorrection['patches'][number]>,
): { originX: number; originY: number; pushedX: number; pushedY: number; weight: number; unitX: number; unitY: number } | null {
  const rect = poseCorrectionStageRect(source)
  const view = rect && projectPoseCorrectionPatch(correction, patch, stageLayers(source), source.frame, source.head, source.driver, source.gain)
  if (!rect || !view) return null
  const sx = rect.width / source.frame.viewWidth
  const sy = rect.height / source.frame.viewHeight
  return {
    originX: rect.left + view.origin.x * sx,
    originY: rect.top + view.origin.y * sy,
    pushedX: rect.left + view.pushed.x * sx,
    pushedY: rect.top + view.pushed.y * sy,
    weight: view.weight,
    unitX: view.unitX * sx,
    unitY: view.unitY * sy,
  }
}

export function clientDeltaToPoseCorrection(
  source: PoseCorrectionStageSource,
  surface: PoseCorrectionSurface,
  clientDx: number,
  clientDy: number,
): { dx: number; dy: number } | null {
  const rect = poseCorrectionStageRect(source)
  if (!rect) return null
  return viewDeltaToPoseCorrection(
    clientDx * source.frame.viewWidth / rect.width,
    clientDy * source.frame.viewHeight / rect.height,
    stageLayers(source),
    surface,
    source.frame,
    source.head,
  )
}
