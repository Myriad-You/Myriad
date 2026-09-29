import type { CroppedLayerPixels } from './webglRuntime'

/**
 * A head turns as a ball, not a card: its drawn outline stays near where it
 * is while what is on the ball slides across it, toward the far side, and
 * whatever stands off the surface (the nose, bangs over a forehead) travels
 * further than the skin under it. A sliding card moves outline and features
 * together and reads flat however far it goes.
 *
 * But the features are drawn facing the viewer, not modelled; foreshortened
 * like geometry they stop looking like eyes. As a rigger does, each feature
 * rides the ball as one piece, keeping its drawn shape and only narrowing a
 * little on the far side. Only skin and hair, which have no shape of their
 * own to keep, bend with the surface, and gently.
 *
 * Each row of the face is taken as a slice of the ball with the drawn
 * outline as its rim. Above the widest row the skull goes on as an ellipse,
 * not as the forehead the hair happens to leave uncovered.
 *
 * Like a rigger's keyform, the slide has one shape, drawn for the full turn,
 * and a smaller turn is that shape scaled down. Every point then moves in
 * step with the angle: none stops early while the rest keep going, which
 * would drag the face across itself like a sheet.
 */

export interface HeadSilhouette {
  /** Canvas y of the first sampled row; rows are one canvas pixel apart. */
  top: number
  centerX: Float32Array
  halfWidth: Float32Array
  /** Row where the face is widest; above it the skull takes over. */
  widestY: number
  chinY: number
  crownY: number
  radius: number
  centerAtWidest: number
}

const OPAQUE = 128
/** Head turn at full angleX, radians: how far what stands off the face travels. */
export const HEAD_TURN_RADIANS = 0.26
/** Head nod at full angleY, radians. */
export const HEAD_NOD_RADIANS = 0.2
/**
 * Slide of the middle of the face at a full turn or nod, as a share of the
 * slice's half width. At the far rim the skin then narrows to 0.7 of its
 * width and at the near rim widens to 1.3; a ball's would fold.
 */
const TURN_SLIDE = 0.22
const NOD_SLIDE = 0.17
/** A ball's skin slides furthest a little to the near side of its middle. */
const SLIDE_PEAK = -0.1
/** The near rim comes in by this share of the peak slide, opening the side of the head. */
const NEAR_RIM_SLIDE = 0.2
/** A feature narrows on the far side at most to this, and widens on the near side at most to that. */
const FEATURE_SCALE_MIN = 0.86
const FEATURE_SCALE_MAX = 1.08
/**
 * Share of the head's motion that is the ball turning; the rest is the whole
 * head carried round the neck, outline and all.
 */
export const HEAD_TURN_SHARE = 0.75
/** Hair sits on a slightly larger ball than the skin it covers. */
export const HAIR_TURN_SCALE = 1.12
/** Below the chin the turn fades out over this share of the head's height. */
const BELOW_CHIN_FADE = 0.4

/** The face drawing's outline row by row, or null when it has none to read. */
export function headSilhouetteFromFace(
  layer: { x: number; y: number; w: number; h: number },
  image: CroppedLayerPixels | null,
): HeadSilhouette | null {
  if (!image || image.width < 4 || image.height < 8) return null
  const { width, height, pixels } = image
  if (pixels.length !== width * height * 4) return null
  const scaleX = layer.w / width
  const scaleY = layer.h / height
  const top = Math.ceil(layer.y)
  const rows = Math.max(1, Math.floor(layer.y + layer.h) - top)
  const left = new Float32Array(rows).fill(Number.NaN)
  const right = new Float32Array(rows).fill(Number.NaN)
  for (let row = 0; row < rows; row++) {
    const y = Math.min(height - 1, Math.max(0, Math.floor((top + row + 0.5 - layer.y) / scaleY)))
    let first = -1
    let last = -1
    for (let x = 0; x < width; x++) {
      if (pixels[(y * width + x) * 4 + 3] < OPAQUE) continue
      if (first < 0) first = x
      last = x
    }
    if (first < 0) continue
    left[row] = layer.x + first * scaleX
    right[row] = layer.x + (last + 1) * scaleX
  }
  let firstRow = 0
  while (firstRow < rows && Number.isNaN(left[firstRow])) firstRow++
  let lastRow = rows - 1
  while (lastRow > firstRow && Number.isNaN(left[lastRow])) lastRow--
  const count = lastRow - firstRow + 1
  if (count < 8) return null
  const centerX = new Float32Array(count)
  const halfWidth = new Float32Array(count)
  for (let row = 0; row < count; row++) {
    let l = left[firstRow + row]
    let r = right[firstRow + row]
    if (Number.isNaN(l)) {
      l = left[firstRow + row - 1]
      r = right[firstRow + row - 1]
      left[firstRow + row] = l
      right[firstRow + row] = r
    }
    centerX[row] = (l + r) / 2
    halfWidth[row] = (r - l) / 2
  }
  // A stray lock of hair or a painted highlight must not dent the rim.
  const window = Math.max(1, Math.round(count * 0.02))
  const smoothCenter = smooth(centerX, window)
  const smoothHalf = smooth(halfWidth, window)
  let widest = 0
  for (let row = 1; row < Math.round(count * 0.75); row++) {
    if (smoothHalf[row] > smoothHalf[widest]) widest = row
  }
  const radius = smoothHalf[widest]
  if (!(radius > 2)) return null
  const topY = top + firstRow
  const widestY = topY + widest
  return {
    top: topY,
    centerX: smoothCenter,
    halfWidth: smoothHalf,
    widestY,
    chinY: topY + count - 1,
    crownY: Math.min(topY, widestY - radius * 1.25),
    radius,
    centerAtWidest: smoothCenter[widest],
  }
}

export interface HeadSilhouetteRow {
  centerX: number
  halfWidth: number
  /** How much of the turn this row takes: all of it on the head, none on the chest. */
  weight: number
}

export function headSilhouetteRow(
  silhouette: Readonly<HeadSilhouette>,
  y: number,
  out: HeadSilhouetteRow,
): HeadSilhouetteRow {
  const { widestY, crownY, radius } = silhouette
  out.weight = 1
  if (y <= widestY) {
    const t = (widestY - y) / Math.max(1, widestY - crownY)
    out.centerX = silhouette.centerAtWidest
    out.halfWidth = radius * Math.sqrt(Math.max(0.04, 1 - t * t))
    return out
  }
  const last = silhouette.halfWidth.length - 1
  const row = Math.min(last, Math.max(0, y - silhouette.top))
  const index = Math.floor(row)
  const next = Math.min(last, index + 1)
  const t = row - index
  out.centerX = silhouette.centerX[index] + (silhouette.centerX[next] - silhouette.centerX[index]) * t
  out.halfWidth = Math.max(
    radius * 0.15,
    silhouette.halfWidth[index] + (silhouette.halfWidth[next] - silhouette.halfWidth[index]) * t,
  )
  if (y > silhouette.chinY) {
    const headHeight = Math.max(1, silhouette.chinY - crownY)
    out.weight = 1 - smoothstep((y - silhouette.chinY) / (headHeight * BELOW_CHIN_FADE))
  }
  return out
}

/**
 * The full turn's slide across a slice, by position (−1 near rim … 1 far
 * rim), as a share of its peak. Past the far rim nothing moves; past the near
 * rim everything comes in with it.
 */
export function headTurnSlideShape(u: number): number {
  if (u >= 1) return 0
  if (u <= -1) return NEAR_RIM_SLIDE
  // Level at the peak, falling away faster toward each rim as a ball's does.
  if (u >= SLIDE_PEAK) return 1 - ((u - SLIDE_PEAK) / (1 - SLIDE_PEAK)) ** 1.5
  return 1 - (1 - NEAR_RIM_SLIDE) * ((SLIDE_PEAK - u) / (1 + SLIDE_PEAK)) ** 1.5
}

/** Slide at position `u` for a signed slide amount; a turn the other way is the mirror image. */
function slideAt(amount: number, u: number): number {
  if (amount === 0) return 0
  return amount * headTurnSlideShape(amount > 0 ? u : -u)
}

export interface HeadTurn {
  active: boolean
  /** Signed slide of the face's middle across a slice, in half widths. */
  slide: number
  sine: number
  nodSlide: number
  nodSine: number
  silhouette: HeadSilhouette | null
}

export function createHeadTurn(silhouette: HeadSilhouette | null): HeadTurn {
  return { active: false, slide: 0, sine: 0, nodSlide: 0, nodSine: 0, silhouette }
}

/** `angleX` turns toward +x; `angleY` raises the face. */
export function updateHeadTurn(turn: HeadTurn, angleX: number, angleY: number): void {
  const x = turn.silhouette && Number.isFinite(angleX) ? Math.max(-1, Math.min(1, angleX)) : 0
  const y = turn.silhouette && Number.isFinite(angleY) ? Math.max(-1, Math.min(1, angleY)) : 0
  turn.active = x !== 0 || y !== 0
  turn.slide = x * TURN_SLIDE
  turn.sine = Math.sin(x * HEAD_TURN_RADIANS)
  turn.nodSlide = y * NOD_SLIDE
  turn.nodSine = Math.sin(y * HEAD_NOD_RADIANS)
}

export type HeadTurnSurface = 'skin' | 'front-hair' | 'back-hair'

export interface HeadTurnOffset {
  x: number
  y: number
}

const row: HeadSilhouetteRow = { centerX: 0, halfWidth: 1, weight: 1 }

/**
 * Offset of a point on the head for the current turn and nod. `lift` is how
 * far it stands off the surface toward the viewer, in pixels; `crown` is how
 * much of a back-hair drawing is the visible top of the coiffure.
 */
export function headTurnOffset(
  turn: Readonly<HeadTurn>,
  x: number,
  y: number,
  surface: HeadTurnSurface,
  lift: number,
  crown: number,
  out: HeadTurnOffset,
): HeadTurnOffset {
  out.x = 0
  out.y = 0
  const silhouette = turn.silhouette
  if (!turn.active || !silhouette) return out
  headSilhouetteRow(silhouette, y, row)
  if (row.weight <= 0) return out
  const scale = surface === 'skin' ? 1 : HAIR_TURN_SCALE
  const radius = row.halfWidth * scale
  const u = (x - row.centerX) / radius
  const facing = Math.sqrt(Math.max(0, 1 - u * u))
  let dx = radius * slideAt(turn.slide, u) + lift * turn.sine * facing
  // The nod turns each column of the ball, narrower toward its sides.
  const halfHeight = ((silhouette.chinY - silhouette.crownY) / 2) * scale
  const middleY = (silhouette.chinY + silhouette.crownY) / 2
  const across = (x - silhouette.centerAtWidest) / (silhouette.radius * scale)
  const column = halfHeight * Math.sqrt(Math.max(0.04, 1 - across * across))
  // Raising the face slides it toward the crown, so the crown is the far rim.
  const v = (middleY - y) / column
  const facingUp = Math.sqrt(Math.max(0, 1 - v * v))
  let dy = -(column * slideAt(turn.nodSlide, v) + lift * turn.nodSine * facingUp)
  if (surface === 'back-hair') {
    // The back of the head is the same ball seen from behind: it slides the other way.
    const rearX = radius * slideAt(-turn.slide, u)
    const rearY = -column * slideAt(-turn.nodSlide, v)
    dx = rearX + (dx - rearX) * crown
    dy = rearY + (dy - rearY) * crown
  }
  out.x = dx * row.weight
  out.y = dy * row.weight
  return out
}

export interface HeadTurnFeature {
  centerX: number
  centerY: number
  halfWidth: number
  halfHeight: number
}

const FEATURE_PARTS: Readonly<Record<string, readonly string[]>> = {
  eye: ['eyewhite', 'irides', 'eyelash', 'eye-close'],
  brow: ['eyebrow'],
  mouth: ['mouth', 'mouth-close'],
  nose: ['nose'],
}

/** Which drawn feature a head layer belongs to; every drawing of one eye moves together. */
export function headTurnFeatureKey(layer: { group: string; role: string; side?: string | null }): string | null {
  if (layer.group !== 'head') return null
  const { role, side } = layer
  if (role === 'nose') return 'nose'
  if (role === 'eyebrow') return side ? `brow:${side}` : 'brow'
  if (side && /eye|iris|irides|lovestruck-heart/.test(role)) return `eye:${side}`
  if (/mouth|drool/.test(role) && !/shadow/.test(role)) return 'mouth'
  return null
}

/** Each feature's drawn bounds, from its everyday drawings where it has them. */
export function headTurnFeatures<Layer extends { group: string; role: string; side?: string | null; x: number; y: number; w: number; h: number }>(
  layers: readonly Layer[],
): Map<Layer, HeadTurnFeature> {
  const members = new Map<string, Layer[]>()
  for (const layer of layers) {
    const key = headTurnFeatureKey(layer)
    if (!key || !(layer.w > 0) || !(layer.h > 0)) continue
    members.set(key, [...(members.get(key) ?? []), layer])
  }
  const features = new Map<Layer, HeadTurnFeature>()
  for (const [key, group] of members) {
    const parts = FEATURE_PARTS[key.split(':')[0]] ?? []
    const everyday = group.filter((layer) => parts.includes(layer.role))
    const bounds = everyday.length ? everyday : group
    const left = Math.min(...bounds.map((layer) => layer.x))
    const right = Math.max(...bounds.map((layer) => layer.x + layer.w))
    const top = Math.min(...bounds.map((layer) => layer.y))
    const bottom = Math.max(...bounds.map((layer) => layer.y + layer.h))
    const feature = {
      centerX: (left + right) / 2,
      centerY: (top + bottom) / 2,
      halfWidth: Math.max(1, (right - left) / 2),
      halfHeight: Math.max(1, (bottom - top) / 2),
    }
    for (const layer of group) features.set(layer, feature)
  }
  return features
}

const featureCenter: HeadTurnOffset = { x: 0, y: 0 }
const featureSide: HeadTurnOffset = { x: 0, y: 0 }
const featureOtherSide: HeadTurnOffset = { x: 0, y: 0 }

/**
 * Moves a feature's point with the ball as one piece: the feature goes where
 * the skin under its middle goes, plus its own height off the face, and
 * narrows or widens only as much as that skin does across it, within bounds.
 */
export function moveHeadFeature(
  turn: Readonly<HeadTurn>,
  point: HeadTurnOffset,
  feature: Readonly<HeadTurnFeature>,
  lift: number,
): void {
  const { centerX, centerY, halfWidth, halfHeight } = feature
  headTurnOffset(turn, centerX, centerY, 'skin', lift, 0, featureCenter)
  headTurnOffset(turn, centerX + halfWidth, centerY, 'skin', lift, 0, featureSide)
  headTurnOffset(turn, centerX - halfWidth, centerY, 'skin', lift, 0, featureOtherSide)
  const scaleX = clamp(1 + (featureSide.x - featureOtherSide.x) / (2 * halfWidth), FEATURE_SCALE_MIN, FEATURE_SCALE_MAX)
  headTurnOffset(turn, centerX, centerY + halfHeight, 'skin', lift, 0, featureSide)
  headTurnOffset(turn, centerX, centerY - halfHeight, 'skin', lift, 0, featureOtherSide)
  const scaleY = clamp(1 + (featureSide.y - featureOtherSide.y) / (2 * halfHeight), FEATURE_SCALE_MIN, FEATURE_SCALE_MAX)
  point.x += featureCenter.x + (point.x - centerX) * (scaleX - 1)
  point.y += featureCenter.y + (point.y - centerY) * (scaleY - 1)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function smooth(values: Float32Array, window: number): Float32Array {
  const output = new Float32Array(values.length)
  for (let index = 0; index < values.length; index++) {
    let total = 0
    let count = 0
    for (let offset = -window; offset <= window; offset++) {
      const at = index + offset
      if (at < 0 || at >= values.length) continue
      total += values[at]
      count++
    }
    output[index] = total / count
  }
  return output
}

function smoothstep(value: number): number {
  const t = Math.max(0, Math.min(1, value))
  return t * t * (3 - 2 * t)
}
