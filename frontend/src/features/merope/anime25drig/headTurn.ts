import type { CroppedLayerPixels } from './webglRuntime'

/**
 * A head turns as a ball, not a card. Its drawn outline is where the ball
 * meets the sky, so it stays put while what is on the ball slides across it:
 * features run toward the far side and crowd into it, the near side opens
 * up, and whatever stands off the surface (the nose, bangs over a forehead)
 * travels further than the skin under it. A sliding card moves outline and
 * features together and reads flat however far it goes.
 *
 * Each row of the face is taken as a slice of the ball with the drawn
 * outline as its rim. Above the widest row the skull goes on as an ellipse,
 * not as the forehead the hair happens to leave uncovered.
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
/** Head turn at full angleX, radians. */
export const HEAD_TURN_RADIANS = 0.42
/** Head nod at full angleY, radians. */
export const HEAD_NOD_RADIANS = 0.26
/**
 * The far side crowds into the rim but never folds behind it: a drawn line
 * there keeps at least this share of its width, so an outline stays a line.
 */
const MIN_FAR_SLOPE = 0.5
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

const TABLE_SIZE = 129
const LONGITUDE_SAMPLES = 256

/**
 * Slide of a point on a unit slice, by its position across it (−1 near
 * rim … 1 far rim), for one turn angle. Rebuilt only when the angle
 * changes.
 */
export class HeadTurnTable {
  yaw = 0
  readonly slide = new Float32Array(TABLE_SIZE)
  private readonly x = new Float32Array(LONGITUDE_SAMPLES)
  private readonly turned = new Float32Array(LONGITUDE_SAMPLES)

  update(yaw: number): void {
    const magnitude = Math.min(Math.PI / 3, Math.abs(Number.isFinite(yaw) ? yaw : 0))
    if (magnitude === Math.abs(this.yaw) && Math.sign(yaw) === Math.sign(this.yaw)) return
    this.yaw = Math.sign(yaw) * magnitude
    if (magnitude === 0) {
      this.slide.fill(0)
      return
    }
    const { x, turned } = this
    const last = LONGITUDE_SAMPLES - 1
    for (let index = 0; index <= last; index++) {
      const longitude = -Math.PI / 2 + (Math.PI * index) / last
      x[index] = Math.sin(longitude)
      turned[index] = Math.sin(Math.min(Math.PI / 2, longitude + magnitude))
    }
    // From the far rim inward: never closer to the rim than the minimum slope allows.
    turned[last] = 1
    for (let index = last - 1; index >= 0; index--) {
      const limit = turned[index + 1] - MIN_FAR_SLOPE * (x[index + 1] - x[index])
      if (turned[index] > limit) turned[index] = limit
    }
    let sample = 0
    const slide = this.slide
    for (let index = 0; index < TABLE_SIZE; index++) {
      const u = -1 + (2 * index) / (TABLE_SIZE - 1)
      while (sample < last - 1 && x[sample + 1] < u) sample++
      const span = Math.max(1e-9, x[sample + 1] - x[sample])
      const t = Math.min(1, Math.max(0, (u - x[sample]) / span))
      slide[index] = turned[sample] + (turned[sample + 1] - turned[sample]) * t - u
    }
    // The minimum-slope limit meets the sphere at a corner; round it off.
    smoothInterior(slide, 3)
  }

  /** Slide for position `u` across the slice; the far rim stays, the near rim comes in. */
  slideAt(u: number): number {
    if (this.yaw === 0) return 0
    // A turn the other way is the mirror image.
    const mirrored = this.yaw < 0
    const across = mirrored ? -u : u
    let value = 0
    if (across <= -1) {
      value = this.slide[0]
    } else if (across < 1) {
      const position = ((across + 1) / 2) * (TABLE_SIZE - 1)
      const index = Math.min(TABLE_SIZE - 2, Math.floor(position))
      const t = position - index
      value = this.slide[index] + (this.slide[index + 1] - this.slide[index]) * t
    }
    return mirrored ? -value : value
  }
}

export interface HeadTurn {
  active: boolean
  table: HeadTurnTable
  /** The same turn seen from behind: the back of the head slides the other way. */
  rearTable: HeadTurnTable
  sine: number
  /** A nod is the same ball turning about its other axis; the crown is its far rim. */
  nodTable: HeadTurnTable
  rearNodTable: HeadTurnTable
  nodSine: number
  silhouette: HeadSilhouette | null
}

export function createHeadTurn(silhouette: HeadSilhouette | null): HeadTurn {
  return {
    active: false,
    table: new HeadTurnTable(),
    rearTable: new HeadTurnTable(),
    sine: 0,
    nodTable: new HeadTurnTable(),
    rearNodTable: new HeadTurnTable(),
    nodSine: 0,
    silhouette,
  }
}

/** `angleX` turns toward +x; `angleY` raises the face. */
export function updateHeadTurn(turn: HeadTurn, angleX: number, angleY: number): void {
  const yaw = turn.silhouette ? angleX * HEAD_TURN_RADIANS : 0
  const nod = turn.silhouette ? angleY * HEAD_NOD_RADIANS : 0
  turn.active = yaw !== 0 || nod !== 0
  turn.table.update(yaw)
  turn.rearTable.update(-yaw)
  turn.sine = Math.sin(yaw)
  turn.nodTable.update(nod)
  turn.rearNodTable.update(-nod)
  turn.nodSine = Math.sin(nod)
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
  let dx = radius * turn.table.slideAt(u) + lift * turn.sine * facing
  // The nod turns each column of the ball, narrower toward its sides.
  const halfHeight = ((silhouette.chinY - silhouette.crownY) / 2) * scale
  const middleY = (silhouette.chinY + silhouette.crownY) / 2
  const across = (x - silhouette.centerAtWidest) / (silhouette.radius * scale)
  const column = halfHeight * Math.sqrt(Math.max(0.04, 1 - across * across))
  // Raising the face slides it toward the crown, so the crown is the far rim.
  const v = (middleY - y) / column
  const facingUp = Math.sqrt(Math.max(0, 1 - v * v))
  let dy = -(column * turn.nodTable.slideAt(v) + lift * turn.nodSine * facingUp)
  if (surface === 'back-hair') {
    const rearX = radius * turn.rearTable.slideAt(u)
    const rearY = -column * turn.rearNodTable.slideAt(v)
    dx = rearX + (dx - rearX) * crown
    dy = rearY + (dy - rearY) * crown
  }
  out.x = dx * row.weight
  out.y = dy * row.weight
  return out
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

/** Leaves the ends alone: a window cut short there would bend a straight run. */
function smoothInterior(values: Float32Array, window: number): void {
  const smoothed = smooth(values, window)
  for (let index = window; index < values.length - window; index++) values[index] = smoothed[index]
}

function smoothstep(value: number): number {
  const t = Math.max(0, Math.min(1, value))
  return t * t * (3 - 2 * t)
}
