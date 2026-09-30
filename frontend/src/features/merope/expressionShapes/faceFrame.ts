import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { RasterLayer } from '../rig/anime25dImportTypes'

/** Beyond this the eye line is more likely a detection error than a head tilt. */
const MAX_FACE_ROLL = (20 * Math.PI) / 180
/** Below this, resampling would only soften generated line art. */
const MIN_FACE_ROLL = (0.5 * Math.PI) / 180
const FACE_ALPHA_THRESHOLD = 8

/**
 * The drawn face's own axes: `along` follows the iris-to-iris line and `down`
 * is perpendicular to it. Generated expression art is authored upright in this
 * frame, so a tilted portrait gets tilted glyphs instead of screen-level ones.
 */
export interface Anime25DFaceFrame {
  /** Image-space rotation of the eye line (y down), in radians. */
  roll: number
  cos: number
  sin: number
  /** Midpoint between the iris centres. */
  originX: number
  originY: number
  /** Iris-to-iris distance. */
  eyeSpan: number
  /** False when an eye anchor is missing and the frame is only the face box. */
  landmarks: boolean
}

export interface FacePoint {
  x: number
  y: number
}

export interface PlacedFaceBitmap {
  left: number
  top: number
  width: number
  height: number
  data: Uint8ClampedArray
}

export function resolveAnime25DFaceFrame(
  anchors: Pick<Anime25DRiggerAnchors, 'eyeL' | 'eyeR' | 'face'>,
): Anime25DFaceFrame {
  const { eyeL, eyeR, face } = anchors
  if (eyeL && eyeR && eyeR.icx > eyeL.icx) {
    const dx = eyeR.icx - eyeL.icx
    const dy = eyeR.icy - eyeL.icy
    const measured = Math.atan2(dy, dx)
    const roll =
      Math.abs(measured) < MIN_FACE_ROLL
        ? 0
        : clamp(measured, -MAX_FACE_ROLL, MAX_FACE_ROLL)
    return {
      roll,
      cos: Math.cos(roll),
      sin: Math.sin(roll),
      originX: (eyeL.icx + eyeR.icx) / 2,
      originY: (eyeL.icy + eyeR.icy) / 2,
      eyeSpan: Math.hypot(dx, dy),
      landmarks: true,
    }
  }
  return {
    roll: 0,
    cos: 1,
    sin: 0,
    originX: face.cx,
    originY: face.cy,
    eyeSpan: Math.max(1, face.x1 - face.x0) * 0.46,
    landmarks: false,
  }
}

/** Offsets a point along the eye line and down the face's own vertical. */
export function faceFramePoint(
  frame: Readonly<Anime25DFaceFrame>,
  x: number,
  y: number,
  along: number,
  down: number,
): FacePoint {
  return {
    x: x + along * frame.cos - down * frame.sin,
    y: y + along * frame.sin + down * frame.cos,
  }
}

/**
 * Walks the eye line outward from `start` and returns the last pixel still on
 * the face drawing, i.e. the drawn cheek/temple contour at that height.
 */
export function faceContourAlongEyeLine(
  frame: Readonly<Anime25DFaceFrame>,
  face: Readonly<RasterLayer>,
  start: Readonly<FacePoint>,
  direction: -1 | 1,
): FacePoint | null {
  const maxDistance = Math.hypot(face.width, face.height)
  const allowedGap = Math.max(3, Math.round(face.width * 0.02))
  let last: FacePoint | null = null
  let gap = 0
  for (let distance = 0; distance <= maxDistance; distance += 1) {
    const point = faceFramePoint(
      frame,
      start.x,
      start.y,
      direction * distance,
      0,
    )
    const x = Math.floor(point.x - face.left)
    const y = Math.floor(point.y - face.top)
    const inside =
      x >= 0 &&
      y >= 0 &&
      x < face.width &&
      y < face.height &&
      face.data[(y * face.width + x) * 4 + 3] > FACE_ALPHA_THRESHOLD
    if (inside) {
      last = point
      gap = 0
    } else if (last && ++gap > allowedGap) {
      break
    }
  }
  return last
}

/** Rough mouth box for portraits that ship no mouth drawing at all. */
export function estimateAnime25DMouthAnchor(
  frame: Readonly<Anime25DFaceFrame>,
  face: Readonly<Anime25DRiggerAnchors['face']>,
): Anime25DRiggerAnchors['mouth'] {
  const center = faceFramePoint(
    frame,
    frame.originX,
    frame.originY,
    0,
    Math.max(1, face.y1 - frame.originY) * 0.62,
  )
  const halfWidth = frame.eyeSpan * 0.2
  const halfHeight = frame.eyeSpan * 0.065
  return {
    cx: center.x,
    cy: center.y,
    x0: center.x - halfWidth,
    x1: center.x + halfWidth,
    y0: center.y - halfHeight,
    y1: center.y + halfHeight,
  }
}

/**
 * Rotates upright generated art by `roll` about its own `pivot` and places the
 * pivot on `target`. Without roll the pixels are copied untouched.
 */
export function placeFaceBitmap(
  bitmap: Readonly<{ width: number; height: number; data: Uint8ClampedArray }>,
  pivotX: number,
  pivotY: number,
  target: Readonly<FacePoint>,
  roll: number,
): PlacedFaceBitmap {
  if (roll === 0) {
    return {
      left: Math.round(target.x - pivotX),
      top: Math.round(target.y - pivotY),
      width: bitmap.width,
      height: bitmap.height,
      data: bitmap.data,
    }
  }
  const cosine = Math.cos(roll)
  const sine = Math.sin(roll)
  let minX = Number.POSITIVE_INFINITY
  let minY = Number.POSITIVE_INFINITY
  let maxX = Number.NEGATIVE_INFINITY
  let maxY = Number.NEGATIVE_INFINITY
  for (const [cornerX, cornerY] of [
    [0, 0],
    [bitmap.width, 0],
    [0, bitmap.height],
    [bitmap.width, bitmap.height],
  ] as const) {
    const localX = cornerX - pivotX
    const localY = cornerY - pivotY
    const x = target.x + localX * cosine - localY * sine
    const y = target.y + localX * sine + localY * cosine
    minX = Math.min(minX, x)
    minY = Math.min(minY, y)
    maxX = Math.max(maxX, x)
    maxY = Math.max(maxY, y)
  }
  const left = Math.floor(minX)
  const top = Math.floor(minY)
  const width = Math.max(1, Math.ceil(maxX) - left)
  const height = Math.max(1, Math.ceil(maxY) - top)
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const offsetY = top + y + 0.5 - target.y
    for (let x = 0; x < width; x += 1) {
      const offsetX = left + x + 0.5 - target.x
      const sourceX = offsetX * cosine + offsetY * sine + pivotX - 0.5
      const sourceY = -offsetX * sine + offsetY * cosine + pivotY - 0.5
      samplePremultiplied(bitmap, sourceX, sourceY, data, (y * width + x) * 4)
    }
  }
  return trimTransparent({ left, top, width, height, data })
}

/** Bilinear in premultiplied space so transparent RGB never bleeds into edges. */
function samplePremultiplied(
  source: Readonly<{ width: number; height: number; data: Uint8ClampedArray }>,
  x: number,
  y: number,
  target: Uint8ClampedArray,
  targetOffset: number,
): void {
  const x0 = Math.floor(x)
  const y0 = Math.floor(y)
  const xWeight = x - x0
  const yWeight = y - y0
  let alpha = 0
  let red = 0
  let green = 0
  let blue = 0
  for (let row = 0; row < 2; row += 1) {
    const sampleY = y0 + row
    if (sampleY < 0 || sampleY >= source.height) continue
    const rowWeight = row === 0 ? 1 - yWeight : yWeight
    for (let column = 0; column < 2; column += 1) {
      const sampleX = x0 + column
      if (sampleX < 0 || sampleX >= source.width) continue
      const weight = rowWeight * (column === 0 ? 1 - xWeight : xWeight)
      const offset = (sampleY * source.width + sampleX) * 4
      const weightedAlpha = source.data[offset + 3] * weight
      if (weightedAlpha === 0) continue
      alpha += weightedAlpha
      red += source.data[offset] * weightedAlpha
      green += source.data[offset + 1] * weightedAlpha
      blue += source.data[offset + 2] * weightedAlpha
    }
  }
  if (alpha <= 0) return
  target[targetOffset] = red / alpha
  target[targetOffset + 1] = green / alpha
  target[targetOffset + 2] = blue / alpha
  target[targetOffset + 3] = alpha
}

function trimTransparent(placed: PlacedFaceBitmap): PlacedFaceBitmap {
  let minX = placed.width
  let minY = placed.height
  let maxX = -1
  let maxY = -1
  for (let y = 0; y < placed.height; y += 1) {
    for (let x = 0; x < placed.width; x += 1) {
      if (placed.data[(y * placed.width + x) * 4 + 3] === 0) continue
      minX = Math.min(minX, x)
      minY = Math.min(minY, y)
      maxX = Math.max(maxX, x)
      maxY = Math.max(maxY, y)
    }
  }
  if (maxX < minX) return placed
  const width = maxX - minX + 1
  const height = maxY - minY + 1
  if (width === placed.width && height === placed.height) return placed
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const start = ((minY + y) * placed.width + minX) * 4
    data.set(placed.data.subarray(start, start + width * 4), y * width * 4)
  }
  return {
    left: placed.left + minX,
    top: placed.top + minY,
    width,
    height,
    data,
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
