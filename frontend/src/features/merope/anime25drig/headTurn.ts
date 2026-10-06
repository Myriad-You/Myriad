import type { CroppedLayerPixels } from './webglRuntime'

/**
 * A head turns about the neck, not as a card: every point of it is a point
 * in depth, turned about the neck's axis and seen straight on, so what stands
 * forward of the axis travels with the turn and what stands behind it goes
 * the other way. The face's front stands well forward, the chin less, the
 * sides of the head on the axis, the ears behind it; the bangs ride a little
 * ahead of the forehead. A sliding card moves outline and features together
 * and reads flat however far it goes.
 *
 * Each row of the face is a slice of the head with the drawn outline as its
 * edge. Over the face the slice is round like a ball, so the far eye narrows
 * and the near one widens as a rigger draws them; near the outline it eases
 * off instead of falling sheer, so no drawn line there bends sharply. Down to the jaw's hinge the outline is the side of the head; from
 * there to the chin it comes forward until the chin moves as one with the
 * front of the face, so the near jaw runs from the ear to the chin. Anything
 * the turn would carry round past the far outline stays on it.
 *
 * Features are drawn on the face: across, each of their points turns with
 * the skin under it, standing off it by the feature's own height, so the far
 * eye narrows with the face there and the nose leads. The top of the neck
 * twists with the head (headTurnNeckOffset), so the jaw never leaves it.
 *
 * Measured against an image model's three-quarter views of the same drawing:
 * at about 33° the near eye travels 0.58 of the head's half width, the far
 * eye 0.42, the nose 0.68, the mouth 0.52, the chin 0.35 and the near ear
 * 0.32, and the far eye narrows to about half.
 *
 * The turn has one shape, drawn for the full turn, and a smaller turn is the
 * same head turned less; nothing stops early while the rest keep going.
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
  /** Where the outline starts to narrow toward the chin: the jaw's hinge. */
  hingeY: number
  /** About where the mouth is; below it the face falls back toward the chin. */
  mouthY: number
}

const OPAQUE = 128
/** Head turn at full angleX, radians: about 30°, the range of a Live2D model's AngleX. */
export const HEAD_TURN_RADIANS = 0.52
/** Head nod at full angleY, radians. */
export const HEAD_NOD_RADIANS = 0.2
/**
 * The head is a ball this many of the face's half widths across, turning
 * about its own middle: its outline, hair and all, stays where it is while
 * what is on its front goes round.
 */
const HEAD_SPHERE = 1.25
/** How far the front of the face stands ahead of the ball's middle, in the face's half widths. */
const FACE_DEPTH = 1.15
/** The chin stands this share of the face's depth forward. */
const CHIN_DEPTH = 0.5
/** The mouth's row, as a share of the way from the widest row to the chin. */
const MOUTH_AT = 0.77
/** The jaw's hinge: the first row below the widest narrower than this share of it. */
const HINGE_WIDTH = 0.9
/**
 * Beyond the face's outline (ears, earrings) depth eases over this many half
 * widths to EAR_DEPTH of the ball's own there: the ears sit on the side of the
 * head behind the cheek, so the near one comes in and the far one goes behind it.
 */
const RIM_FADE = 0.35
const EAR_DEPTH = 0.6
/** Hair stands a little off the ball. */
const HAIR_GAP = 0.05
/**
 * The ball's outline stays put: on the near side the hair is pinned to it,
 * easing in from this far across (−1 at the outline), and stretches between.
 */
const NEAR_PIN_FROM = -0.4
/** The back hair's crown, above the widest row, is the top of the fringe's coiffure. */
const CROWN_SPAN = 0.8
/** How far each kind of feature stands off the face, in head radii. */
const FEATURE_LIFT: Readonly<Record<string, number>> = { nose: 0.24, brow: 0.03 }
const NOD_SLIDE = 0.17
/** For the nod, hair sits on a slightly larger ball than the skin it covers. */
const NOD_HAIR_SCALE = 1.12
/** A nod's slide levels off toward the chin and keeps growing a little to it. */
const NOD_SLIDE_PEAK = -0.3
const NOD_NEAR_RIM_SLIDE = 1.1
/** In a nod a feature shortens at most to this, and lengthens at most to that. */
const FEATURE_SCALE_MIN = 0.86
const FEATURE_SCALE_MAX = 1.08
/** In a turn a feature narrows with the face down to this, and widens at most to that. */
const FEATURE_TURN_MIN = 0.45
const FEATURE_TURN_MAX = 1.08
/** Below the chin the turn fades out over this share of the head's height. */
const BELOW_CHIN_FADE = 0.4
/**
 * The viewer stands this many head radii in front of the head's axis: what
 * the turn brings nearer grows a little and what it takes away shrinks, across
 * and up and down alike, as a ball's near side does.
 */
const VIEW_DISTANCE = 8

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
  const window = Math.max(1, Math.round(count * 0.05))
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
  let hinge = widest
  while (hinge < count - 1 && smoothHalf[hinge] >= radius * HINGE_WIDTH) hinge++
  const chinY = topY + count - 1
  return {
    top: topY,
    centerX: smoothCenter,
    halfWidth: smoothHalf,
    widestY,
    chinY,
    crownY: Math.min(topY, widestY - radius * 1.25),
    radius,
    centerAtWidest: smoothCenter[widest],
    hingeY: topY + hinge,
    mouthY: widestY + (chinY - widestY) * MOUTH_AT,
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
 * A nod's slide across a column, by position (−1 near rim … 1 far rim), as a
 * share of its peak. A face tipping back is a plane swinging up about the
 * crown, not a band sliding round a globe: the lower on the face, the further
 * it travels, so the mouth rises a little more than the eyes and the chin more
 * again, and the face foreshortens instead of the mouth being left behind.
 */
export function headNodSlideShape(v: number): number {
  const peak = NOD_SLIDE_PEAK
  const nearRim = NOD_NEAR_RIM_SLIDE
  if (v >= 1) return 0
  if (v <= -1) return nearRim
  if (v >= peak) return 1 - ((v - peak) / (1 - peak)) ** 1.5
  return 1 - (1 - nearRim) * ((peak - v) / (1 + peak)) ** 1.5
}

function nodAt(amount: number, v: number): number {
  if (amount === 0) return 0
  return amount * headNodSlideShape(amount > 0 ? v : -v)
}

export interface HeadTurn {
  active: boolean
  /** Of the turn's angle; the sine is signed, positive toward +x. */
  cosine: number
  sine: number
  nodSlide: number
  nodSine: number
  silhouette: HeadSilhouette | null
  /** Changes whenever the turn does; what is worked out per feature is kept until then. */
  version: number
  /** Share of the full turn, −1…1. */
  amount: number
  /** Share of the full nod, −1…1, raised positive. */
  nodAmount: number
  /** The same head nodding only: for parts whose turn is keyed (turnKeyforms). */
  nodOnly: HeadTurn | null
  /** The same head held still: for parts whose turn and nod are both keyed. */
  still: HeadTurn | null
}

export function createHeadTurn(silhouette: HeadSilhouette | null): HeadTurn {
  const still = (): HeadTurn => ({ active: false, cosine: 1, sine: 0, nodSlide: 0, nodSine: 0, silhouette, version: 0, amount: 0, nodAmount: 0, nodOnly: null, still: null })
  return { ...still(), nodOnly: still(), still: still() }
}

/** `angleX` turns toward +x; `angleY` raises the face. */
export function updateHeadTurn(turn: HeadTurn, angleX: number, angleY: number): void {
  const x = turn.silhouette && Number.isFinite(angleX) ? Math.max(-1, Math.min(1, angleX)) : 0
  const y = turn.silhouette && Number.isFinite(angleY) ? Math.max(-1, Math.min(1, angleY)) : 0
  turn.active = x !== 0 || y !== 0
  turn.cosine = Math.cos(x * HEAD_TURN_RADIANS)
  turn.sine = Math.sin(x * HEAD_TURN_RADIANS)
  turn.nodSlide = y * NOD_SLIDE
  turn.nodSine = Math.sin(y * HEAD_NOD_RADIANS)
  turn.amount = x
  turn.nodAmount = y
  turn.version++
  if (turn.nodOnly) {
    turn.nodOnly.active = y !== 0
    turn.nodOnly.nodSlide = turn.nodSlide
    turn.nodOnly.nodSine = turn.nodSine
    turn.nodOnly.version++
  }
}

/** `side`: the side of the head, where the ears are, behind the cheek. */
export type HeadTurnSurface = 'skin' | 'side' | 'front-hair' | 'back-hair'

export interface HeadTurnOffset {
  x: number
  y: number
}

const row: HeadSilhouetteRow = { centerX: 0, halfWidth: 1, weight: 1 }

/**
 * How far forward the face's front stands at row `y`, as a share of its
 * depth: above the widest row the head rounds back toward the crown like a
 * ball, below the mouth the face falls back toward the chin.
 */
function frontShare(silhouette: Readonly<HeadSilhouette>, y: number): number {
  if (y < silhouette.widestY) {
    const up = (silhouette.widestY - y) / Math.max(1, silhouette.widestY - silhouette.crownY)
    return Math.sqrt(Math.max(0, 1 - up * up))
  }
  const t = (y - silhouette.mouthY) / Math.max(1, silhouette.chinY - silhouette.mouthY)
  return 1 - (1 - CHIN_DEPTH) * smoothstep(t)
}

/** How far forward the drawn outline stands at row `y`: none down to the hinge, the chin's at the chin. */
function rimShare(silhouette: Readonly<HeadSilhouette>, y: number): number {
  const t = (y - silhouette.hingeY) / Math.max(1, silhouette.chinY - silhouette.hingeY)
  return frontShare(silhouette, y) * smoothstep(t)
}

/** Where a point of depth `z` (pixels ahead of the head's middle) at `x` lands across. */
function turned(turn: Readonly<HeadTurn>, x: number, z: number): number {
  const axisX = turn.silhouette!.centerAtWidest
  return x + (x - axisX) * (turn.cosine - 1) + z * turn.sine
}

/** Depth of the ball's own surface at `x`, in canvas pixels. */
function ballDepth(silhouette: Readonly<HeadSilhouette>, x: number): number {
  const radius = silhouette.radius * HEAD_SPHERE
  const across = (x - silhouette.centerAtWidest) / radius
  return radius * Math.sqrt(Math.max(0, 1 - across * across))
}

/** How much a point of depth `z` at `x` grows as the turn brings it nearer (below 1: further). */
function nearer(turn: Readonly<HeadTurn>, x: number, z: number): number {
  const silhouette = turn.silhouette!
  const forward = -(x - silhouette.centerAtWidest) * turn.sine + z * (turn.cosine - 1)
  const distance = VIEW_DISTANCE * silhouette.radius
  return distance / Math.max(distance * 0.5, distance - forward)
}

/** Scale of the last point landed across, for its height as well. */
let landedScale = 1

/** The skin's slice at row `y`: above the widest row, the skull's full width. */
function skinSlice(silhouette: Readonly<HeadSilhouette>, y: number): HeadSilhouetteRow {
  headSilhouetteRow(silhouette, y, row)
  if (y < silhouette.widestY) {
    row.centerX = silhouette.centerAtWidest
    row.halfWidth = silhouette.radius
  }
  return row
}

/** Where the slice stops being round and eases to the outline. */
const SLICE_ROUND = 0.8
/** The slice's steepest fall at the outline, in depth per half width. */
const SLICE_EDGE_SLOPE = 2

/**
 * The face's slice, front 1, by position across it (0 middle, 1 outline). Round
 * like a ball over the face, so the far eye narrows and the near one widens as
 * a rigger draws them; near the outline it eases off instead of falling
 * sheer, so the near side widens only so far and no drawn line there bends
 * sharply.
 */
function slice(u: number): number {
  const a = Math.min(1, Math.abs(u))
  if (a <= SLICE_ROUND) return Math.sqrt(1 - a * a)
  const t = a - SLICE_ROUND
  return ROUND_END + ROUND_SLOPE * t + EASE * t * t
}

const ROUND_END = Math.sqrt(1 - SLICE_ROUND * SLICE_ROUND)
const ROUND_SLOPE = -SLICE_ROUND / ROUND_END
const EASE = (-SLICE_EDGE_SLOPE - ROUND_SLOPE) / (2 * (1 - SLICE_ROUND))

/** Depth of the skin at `u` across its slice; beyond ±1, off the face, easing to `side`. */
function skinDepth(front: number, rim: number, depth: number, u: number, side: number): number {
  const onFace = depth * (rim + (front - rim) * slice(u))
  if (Math.abs(u) <= 1) return onFace
  return onFace + (side - onFace) * smoothstep((Math.abs(u) - 1) / RIM_FADE)
}

/** Where the skin at `x`, row `y`, lands across; what would turn past the far outline stays on it. */
function skinTurnedX(turn: Readonly<HeadTurn>, x: number, y: number): number {
  const silhouette = turn.silhouette!
  const slice = skinSlice(silhouette, y)
  const centerX = slice.centerX
  const halfWidth = Math.max(1, slice.halfWidth)
  const front = frontShare(silhouette, y)
  const rim = rimShare(silhouette, y)
  const side = turn.sine >= 0 ? 1 : -1
  let u = (x - centerX) / halfWidth
  // Past the point where the slice turns edge-on, everything is behind it.
  if (side * u > 0 && side * u <= 1) {
    const edgeOn = edgeOnAt(turn, y, centerX, halfWidth, front, rim)
    if (side * u > edgeOn) u = side * edgeOn
  }
  return skinLanding(turn, centerX, halfWidth, front, rim, u)
}

/** Where the skin at `u` across a slice lands, seen in perspective. */
function skinLanding(turn: Readonly<HeadTurn>, centerX: number, halfWidth: number, front: number, rim: number, u: number): number {
  const silhouette = turn.silhouette!
  const at = centerX + u * halfWidth
  const z = skinDepth(front, rim, FACE_DEPTH * silhouette.radius, u, EAR_DEPTH * ballDepth(silhouette, at))
  landedScale = nearer(turn, at, z)
  const axisX = silhouette.centerAtWidest
  return axisX + (turned(turn, at, z) - axisX) * landedScale
}

/** Rows' edge-on points for the current turn, by whole canvas row. */
const edgeOnRows = new WeakMap<Readonly<HeadTurn>, { version: number; rows: Map<number, number> }>()

/**
 * How far toward the far side (0…1) the slice at row `y` turns edge-on: where
 * its landing, perspective and all, stops moving further across.
 */
function edgeOnAt(turn: Readonly<HeadTurn>, y: number, centerX: number, halfWidth: number, front: number, rim: number): number {
  let cache = edgeOnRows.get(turn)
  if (!cache || cache.version !== turn.version) edgeOnRows.set(turn, (cache = { version: turn.version, rows: new Map() }))
  const key = Math.round(y)
  const known = cache.rows.get(key)
  if (known !== undefined) return known
  const side = turn.sine >= 0 ? 1 : -1
  const landing = (v: number) => side * skinLanding(turn, centerX, halfWidth, front, rim, side * v)
  // Golden-section search for the furthest landing over 0…1.
  let a = 0
  let b = 1
  const r = (Math.sqrt(5) - 1) / 2
  let c = b - r * (b - a)
  let d = a + r * (b - a)
  for (let i = 0; i < 24; i++) {
    if (landing(c) > landing(d)) b = d
    else a = c
    c = b - r * (b - a)
    d = a + r * (b - a)
  }
  const edgeOn = landing(1) >= landing((a + b) / 2) ? 1 : (a + b) / 2
  cache.rows.set(key, edgeOn)
  return edgeOn
}

/**
 * Where hair at `x`, row `y`, lands across. It lies on the ball, a little off
 * it: what turns past the far outline stays on it, and on the near side the
 * hair is pinned to the outline and widens toward it, so the head's outline
 * stays where it is as a turning ball's does.
 */
function hairTurnedX(turn: Readonly<HeadTurn>, x: number, y: number): number {
  const silhouette = turn.silhouette!
  const axisX = silhouette.centerAtWidest
  const radius = silhouette.radius * HEAD_SPHERE
  const depth = radius * frontShare(silhouette, y)
  const gap = HAIR_GAP * silhouette.radius
  const side = turn.sine >= 0 ? 1 : -1
  // In the turn's own direction: m > 0 toward the far side.
  const at = (m: number) => {
    const u = Math.max(-1, Math.min(1, m))
    return side * (turned(turn, axisX + side * m * radius, depth * Math.sqrt(1 - u * u) + gap) - axisX)
  }
  const m = (side * (x - axisX)) / radius
  // Slope of the landing against the drawing: cosine − a·m/√(1−m²).
  const a = (depth * Math.abs(turn.sine)) / radius
  let landed: number
  const edgeOn = a > 1e-6 ? (turn.cosine / a) / Math.sqrt(1 + (turn.cosine / a) ** 2) : 1
  if (m > edgeOn) {
    landed = at(edgeOn) + Math.max(0, m - 1) * radius
  } else if (m < NEAR_PIN_FROM) {
    if (m >= -1) {
      const pin = smoothstep((NEAR_PIN_FROM - m) / (NEAR_PIN_FROM + 1))
      landed = at(m) + (-radius - at(-1)) * pin
    } else {
      landed = -radius + (m + 1) * radius
    }
  } else {
    landed = at(m)
  }
  const u = Math.max(-1, Math.min(1, m))
  landedScale = nearer(turn, x, depth * Math.sqrt(1 - u * u) + gap)
  return axisX + side * landed * landedScale
}

const turnMove: HeadTurnOffset = { x: 0, y: 0 }

/**
 * Where a point at `x`, row `y`, on `surface` goes in the turn, weighted
 * below the chin: across as it turns, and up or down as it comes nearer or
 * goes further.
 */
function turnAcross(turn: Readonly<HeadTurn>, surface: HeadTurnSurface, x: number, y: number): HeadTurnOffset {
  turnMove.x = 0
  turnMove.y = 0
  const silhouette = turn.silhouette!
  const weight = headSilhouetteRow(silhouette, y, row).weight
  if (weight <= 0) return turnMove
  // The crown of the back hair is the top of the fringe's coiffure; lower
  // down the back hair is the back of the ball, behind the outline: it stays.
  const share = surface === 'back-hair'
    ? smoothstep((silhouette.widestY - y) / (CROWN_SPAN * silhouette.radius))
    : 1
  let landed: number
  if (surface === 'skin') {
    landed = skinTurnedX(turn, x, y)
  } else if (surface === 'side') {
    const z = EAR_DEPTH * ballDepth(silhouette, x)
    landedScale = nearer(turn, x, z)
    landed = silhouette.centerAtWidest + (turned(turn, x, z) - silhouette.centerAtWidest) * landedScale
  } else {
    landed = hairTurnedX(turn, x, y)
  }
  turnMove.x = (landed - x) * weight * share
  const middleY = (silhouette.crownY + silhouette.chinY) / 2
  turnMove.y = (y - middleY) * (landedScale - 1) * weight * share
  return turnMove
}

/**
 * Offset of a point on the head for the current turn and nod. `lift` is how
 * far it stands off the surface toward the viewer, in pixels (the nod's
 * only: across, the depth is the head's own); `crown` is how much of a
 * back-hair drawing is the visible top of the coiffure, for the nod.
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
  const weight = row.weight
  if (weight <= 0) return out
  let dx = 0
  let perspectiveY = 0
  if (turn.sine !== 0) {
    turnAcross(turn, surface, x, y)
    dx = turnMove.x
    perspectiveY = turnMove.y
  }
  // The nod turns each column of the ball, narrower toward its sides.
  const scale = surface === 'skin' ? 1 : NOD_HAIR_SCALE
  const halfHeight = ((silhouette.chinY - silhouette.crownY) / 2) * scale
  const middleY = (silhouette.chinY + silhouette.crownY) / 2
  const across = (x - silhouette.centerAtWidest) / (silhouette.radius * scale)
  const column = halfHeight * Math.sqrt(Math.max(0.04, 1 - across * across))
  // Raising the face slides it toward the crown, so the crown is the far rim.
  const v = (middleY - y) / column
  const facingUp = Math.sqrt(Math.max(0, 1 - v * v))
  let dy = -(column * nodAt(turn.nodSlide, v) + lift * turn.nodSine * facingUp)
  if (surface === 'back-hair') {
    // The back of the head is the same ball seen from behind: it slides the other way.
    const rearY = -column * nodAt(-turn.nodSlide, v)
    dy = rearY + (dy - rearY) * crown
  }
  out.x = dx
  out.y = dy * weight + perspectiveY
  return out
}

const neckOffset: HeadTurnOffset = { x: 0, y: 0 }

/**
 * How far the top of the neck goes with the turn and the nod: as the skin at
 * the chin's row does, so the jaw does not leave the neck behind, and the
 * shadow drawn under the chin rises and falls with it. The caller fades it
 * down the neck. The result is shared; read it before the next call.
 */
export function headTurnNeckMove(turn: Readonly<HeadTurn>, x: number, y: number): Readonly<HeadTurnOffset> {
  const silhouette = turn.silhouette
  if (!turn.active || !silhouette) {
    neckOffset.x = 0
    neckOffset.y = 0
    return neckOffset
  }
  return headTurnOffset(turn, x, Math.min(y, silhouette.chinY), 'skin', 0, 0, neckOffset)
}

/** How far across the top of the neck goes with the turn (headTurnNeckMove's x). */
export function headTurnNeckOffset(turn: Readonly<HeadTurn>, x: number, y: number): number {
  return headTurnNeckMove(turn, x, y).x
}

export interface HeadTurnFeature {
  centerX: number
  centerY: number
  halfWidth: number
  halfHeight: number
  /** eye, brow, mouth or nose. */
  kind: string
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
      kind: key.split(':')[0],
    }
    for (const layer of group) features.set(layer, feature)
  }
  return features
}

const featureCenter: HeadTurnOffset = { x: 0, y: 0 }
const featureTop: HeadTurnOffset = { x: 0, y: 0 }
const featureBottom: HeadTurnOffset = { x: 0, y: 0 }

/**
 * Moves a feature's point with the head as one piece, keeping its drawn shape
 * as a rigger does: its middle goes where the face under it goes, standing
 * off it by the feature's own height (the nose more than the eyes), and it
 * narrows as the face under it does, the far eye to half, but on the near
 * side widens only a little. Up and down, in a nod, likewise.
 */
export function moveHeadFeature(
  turn: Readonly<HeadTurn>,
  point: HeadTurnOffset,
  feature: Readonly<HeadTurnFeature>,
  lift: number,
): void {
  const silhouette = turn.silhouette
  if (!turn.active || !silhouette) return
  // Every point of a feature moves alike: work its move out once per turn.
  let move = featureMoves.get(feature)
  if (!move || move.turn !== turn || move.version !== turn.version || move.lift !== lift) {
    move = move ?? { turn, version: -1, lift, x: 0, y: 0, scaleX: 1, scaleY: 1 }
    const { centerX, centerY, halfWidth, halfHeight } = feature
    headTurnOffset(turn, centerX, centerY, 'skin', lift, 0, featureCenter)
    headTurnOffset(turn, centerX - halfWidth, centerY, 'skin', lift, 0, featureTop)
    headTurnOffset(turn, centerX + halfWidth, centerY, 'skin', lift, 0, featureBottom)
    move.scaleX = clamp(1 + (featureBottom.x - featureTop.x) / (2 * halfWidth), FEATURE_TURN_MIN, FEATURE_TURN_MAX)
    headTurnOffset(turn, centerX, centerY - halfHeight, 'skin', lift, 0, featureTop)
    headTurnOffset(turn, centerX, centerY + halfHeight, 'skin', lift, 0, featureBottom)
    move.scaleY = clamp(1 + (featureBottom.y - featureTop.y) / (2 * halfHeight), FEATURE_SCALE_MIN, FEATURE_SCALE_MAX)
    const standOff = (FEATURE_LIFT[feature.kind] ?? 0) * silhouette.radius * turn.sine *
      headSilhouetteRow(silhouette, centerY, row).weight
    move.x = featureCenter.x + standOff
    move.y = featureCenter.y
    move.turn = turn
    move.version = turn.version
    move.lift = lift
    featureMoves.set(feature, move)
  }
  point.x += move.x + (point.x - feature.centerX) * (move.scaleX - 1)
  point.y += move.y + (point.y - feature.centerY) * (move.scaleY - 1)
}

const featureMoves = new WeakMap<Readonly<HeadTurnFeature>, {
  turn: Readonly<HeadTurn>
  version: number
  lift: number
  x: number
  y: number
  scaleX: number
  scaleY: number
}>()

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
