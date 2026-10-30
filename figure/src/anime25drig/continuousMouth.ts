import type { PaintedLips } from '../expressionShapes/lipMouth'
import type { MouthExpressionPalette } from '../expressionShapes/mouthExpression'
import { LIP_CUT_OFFSET, lipCut, paintLipMouth, soften } from '../expressionShapes/lipMouth'
import { mouthExpressionOuterPath } from '../expressionShapes/mouthExpression'

/**
 * One speaking mouth whose shape moves continuously between the open, wide
 * and round drawings, instead of several drawings handed over one to the
 * next. Each drawing's outline is read as a radius at every angle from its
 * middle; a blend of drawings is the blend of their radii, so any mix of
 * vowels is still a closed, smooth mouth, and it changes as smoothly as the
 * weights do. Cavity, tongue and lips blend the same way, and every pixel is
 * painted by the same code, in the same colours, as the drawings are.
 *
 * The drawings are stored upright-then-tilted with the face and trimmed to
 * themselves; the blend is painted the same way into the same place, so the
 * runtime's warp of that place to the mouth's box applies unchanged.
 */

export interface ContinuousMouthShape {
  wide: number
  round: number
  /** A consonant's narrow mouth: the open outline, flatter, with the lips nearly shut. */
  narrow: number
  /** The rest of the weight is the plain open mouth. */
}

type Point = readonly [x: number, y: number]

const ANGLES = 128

interface Profile {
  radius: Float32Array
  innerScaleX: number
  innerScaleY: number
  innerOffsetY: number
  /** Tongue line y = base + curve·(1 − x²), below which the cavity is tongue. */
  tongueBase: number
  tongueCurve: number
}

// The cavity insets and tongue lines of the drawn cel mouths.
const PROFILES = {
  open: profile(mouthExpressionOuterPath('open'), 0.78, 0.75, 0.025, 0.24, -0.16),
  wide: profile(mouthExpressionOuterPath('wide'), 0.78, 0.75, 0.045, 0.2, -0.12),
  round: profile(mouthExpressionOuterPath('round'), 0.78, 0.75, 0.025, 0.3, -0.12),
} as const

const COS = new Float32Array(ANGLES)
const SIN = new Float32Array(ANGLES)
for (let index = 0; index < ANGLES; index++) {
  const angle = -Math.PI + (2 * Math.PI * (index + 0.5)) / ANGLES
  COS[index] = Math.cos(angle)
  SIN[index] = Math.sin(angle)
}

const blended = {
  radius: new Float32Array(ANGLES),
  /** Nearest and furthest the outline comes to its middle, to settle most points without an angle. */
  minRadius: 0,
  maxRadius: 0,
  innerScaleX: 1,
  innerScaleY: 1,
  innerOffsetY: 0,
  tongueBase: 0,
  tongueCurve: 0,
}

/** How bitmap pixels map into the blend's upright shape space. */
const placement = {
  cos: 1,
  sin: 0,
  /** Shape-space centre of the outline's upright bounds. */
  centerX: 0,
  centerY: 0,
  /** Shape units per upright pixel. */
  unitX: 1,
  unitY: 1,
  /** Pixel position of the upright centre. */
  pixelX: 0,
  pixelY: 0,
}

const SAMPLES: readonly Point[] = [[0.25, 0.25], [0.75, 0.25], [0.25, 0.75], [0.75, 0.75]]
const shapePoint = { x: 0, y: 0 }

/**
 * Paints the blended cel mouth into `out` (RGBA, `width`×`height`), clearing
 * it first: the outline tilted by `roll` and filling the bitmap, as a drawn
 * one is stored.
 */
export function paintContinuousMouth(
  shape: Readonly<ContinuousMouthShape>,
  palette: Readonly<MouthExpressionPalette>,
  width: number,
  height: number,
  out: Uint8ClampedArray,
  roll = 0,
): void {
  out.fill(0)
  blendOutline(shape)
  place(width, height, roll, 0)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let outer = 0
      let inner = 0
      let tongue = 0
      for (const [offsetX, offsetY] of SAMPLES) {
        toShape(x + offsetX, y + offsetY, shapePoint)
        const px = shapePoint.x
        const py = shapePoint.y
        if (!inside(px, py)) continue
        outer += 0.25
        if (!inside(px / blended.innerScaleX, (py - blended.innerOffsetY) / blended.innerScaleY)) continue
        inner += 0.25
        if (py > blended.tongueBase + blended.tongueCurve * (1 - px * px)) tongue += 0.25
      }
      if (outer <= 0) continue
      const offset = (y * width + x) * 4
      paint(out, offset, palette.line, outer)
      if (inner > 0) paint(out, offset, palette.cavity, inner)
      if (tongue > 0) paint(out, offset, palette.fill, tongue)
    }
  }
}

/**
 * Paints the blended mouth in the portrait's own lip painting: lips, teeth,
 * cavity and tongue, as the drawn lip mouths are, softened the same way.
 */
export function paintContinuousLipMouth(
  shape: Readonly<ContinuousMouthShape>,
  painted: Readonly<PaintedLips>,
  width: number,
  height: number,
  out: Uint8ClampedArray,
  roll = 0,
): void {
  out.fill(0)
  blendOutline(shape)
  // One pixel of room on each side for the softening, as the drawn ones have.
  place(width, height, roll, 1)
  const cut = lipCut(clamp01(shape.narrow), clamp01(shape.round))
  let top = Infinity
  let bottom = -Infinity
  for (let index = 0; index < ANGLES; index++) {
    const y = blended.radius[index] * SIN[index]
    top = Math.min(top, y)
    bottom = Math.max(bottom, y)
  }
  const openingTop = LIP_CUT_OFFSET + top * cut.openingY
  const openingBottom = LIP_CUT_OFFSET + bottom * cut.openingY
  const scaled = (scaleX: number, scaleY: number) => (x: number, y: number) =>
    inside(x / scaleX, (y - LIP_CUT_OFFSET) / scaleY)
  paintLipMouth(out, width, height, painted, {
    toShape,
    openingRows: (openingBottom - openingTop) / placement.unitY,
    openingTop,
    openingBottom,
    teethMinimum: cut.teethMinimum,
    inOuter: inside,
    inLipLine: scaled(cut.lineX, cut.lineY),
    inOpening: scaled(cut.openingX, cut.openingY),
  })
  out.set(soften(out, width, height))
}

/** Blends the drawings' outlines, cavities and tongues by `shape` into `blended`. */
function blendOutline(shape: Readonly<ContinuousMouthShape>): void {
  const wide = clamp01(shape.wide)
  const round = clamp01(shape.round)
  const scale = wide + round > 1 ? 1 / (wide + round) : 1
  const weights = { open: 1 - (wide + round) * scale, wide: wide * scale, round: round * scale }
  blended.radius.fill(0)
  blended.innerScaleX = 0
  blended.innerScaleY = 0
  blended.innerOffsetY = 0
  blended.tongueBase = 0
  blended.tongueCurve = 0
  for (const kind of ['open', 'wide', 'round'] as const) {
    const weight = weights[kind]
    if (weight <= 0) continue
    const source = PROFILES[kind]
    for (let index = 0; index < ANGLES; index++) blended.radius[index] += source.radius[index] * weight
    blended.innerScaleX += source.innerScaleX * weight
    blended.innerScaleY += source.innerScaleY * weight
    blended.innerOffsetY += source.innerOffsetY * weight
    blended.tongueBase += source.tongueBase * weight
    blended.tongueCurve += source.tongueCurve * weight
  }
  blended.minRadius = Infinity
  blended.maxRadius = 0
  for (let index = 0; index < ANGLES; index++) {
    blended.minRadius = Math.min(blended.minRadius, blended.radius[index])
    blended.maxRadius = Math.max(blended.maxRadius, blended.radius[index])
  }
}

/**
 * Sizes the upright outline so that, tilted by `roll`, its bounds fill the
 * bitmap to `margin` pixels of each edge, as a drawn mouth is trimmed.
 */
function place(width: number, height: number, roll: number, margin: number): void {
  let left = Infinity
  let right = -Infinity
  let top = Infinity
  let bottom = -Infinity
  for (let index = 0; index < ANGLES; index++) {
    const r = blended.radius[index]
    left = Math.min(left, r * COS[index])
    right = Math.max(right, r * COS[index])
    top = Math.min(top, r * SIN[index])
    bottom = Math.max(bottom, r * SIN[index])
  }
  const cos = Math.cos(roll)
  const sin = Math.sin(roll)
  const targetWidth = Math.max(1, width - 2 * margin)
  const targetHeight = Math.max(1, height - 2 * margin)
  const centerX = (left + right) / 2
  const centerY = (top + bottom) / 2
  // Upright pixel size of the outline, refined until its tilted bounds fit.
  let uprightWidth = targetWidth
  let uprightHeight = targetHeight
  let offsetX = 0
  let offsetY = 0
  for (let pass = 0; pass < 6; pass++) {
    const unitX = (right - left) / uprightWidth
    const unitY = (bottom - top) / uprightHeight
    let minX = Infinity
    let maxX = -Infinity
    let minY = Infinity
    let maxY = -Infinity
    for (let index = 0; index < ANGLES; index++) {
      const r = blended.radius[index]
      const ux = (r * COS[index] - centerX) / unitX
      const uy = (r * SIN[index] - centerY) / unitY
      const vx = ux * cos - uy * sin
      const vy = ux * sin + uy * cos
      minX = Math.min(minX, vx)
      maxX = Math.max(maxX, vx)
      minY = Math.min(minY, vy)
      maxY = Math.max(maxY, vy)
    }
    offsetX = (minX + maxX) / 2
    offsetY = (minY + maxY) / 2
    uprightWidth *= targetWidth / Math.max(1e-6, maxX - minX)
    uprightHeight *= targetHeight / Math.max(1e-6, maxY - minY)
  }
  placement.cos = cos
  placement.sin = sin
  placement.centerX = centerX
  placement.centerY = centerY
  placement.unitX = (right - left) / uprightWidth
  placement.unitY = (bottom - top) / uprightHeight
  placement.pixelX = width / 2 - offsetX
  placement.pixelY = height / 2 - offsetY
}

/** Bitmap pixel (x, y) to the upright shape's coordinates. */
function toShape(x: number, y: number, out: { x: number; y: number }): void {
  const vx = x - placement.pixelX
  const vy = y - placement.pixelY
  const ux = vx * placement.cos + vy * placement.sin
  const uy = -vx * placement.sin + vy * placement.cos
  out.x = placement.centerX + ux * placement.unitX
  out.y = placement.centerY + uy * placement.unitY
}

function inside(x: number, y: number): boolean {
  const squared = x * x + y * y
  // Between the outline's nearest and furthest reach the answer needs the angle.
  if (squared <= blended.minRadius * blended.minRadius) return true
  if (squared > blended.maxRadius * blended.maxRadius) return false
  return Math.sqrt(squared) <= radiusAt(Math.atan2(y, x))
}

function radiusAt(angle: number): number {
  const position = ((angle + Math.PI) / (2 * Math.PI)) * ANGLES - 0.5
  const index = Math.floor(position)
  const t = position - index
  const radius = blended.radius
  const first = radius[((index % ANGLES) + ANGLES) % ANGLES]
  const second = radius[(((index + 1) % ANGLES) + ANGLES) % ANGLES]
  return first + (second - first) * t
}

function profile(
  path: readonly Point[],
  innerScaleX: number,
  innerScaleY: number,
  innerOffsetY: number,
  tongueBase: number,
  tongueCurve: number,
): Profile {
  const radius = new Float32Array(ANGLES)
  for (let index = 0; index < ANGLES; index++) {
    radius[index] = rayDistance(path, cosineAt(index), sineAt(index))
  }
  return { radius, innerScaleX, innerScaleY, innerOffsetY, tongueBase, tongueCurve }
}

function cosineAt(index: number): number {
  return Math.cos(-Math.PI + (2 * Math.PI * (index + 0.5)) / ANGLES)
}

function sineAt(index: number): number {
  return Math.sin(-Math.PI + (2 * Math.PI * (index + 0.5)) / ANGLES)
}

/** Distance from the middle to the outline along a ray; the drawings are star-shaped about it. */
function rayDistance(path: readonly Point[], dx: number, dy: number): number {
  let nearest = Infinity
  for (let index = 0; index < path.length; index++) {
    const [ax, ay] = path[index]
    const [bx, by] = path[(index + 1) % path.length]
    const ex = bx - ax
    const ey = by - ay
    const denominator = dx * ey - dy * ex
    if (Math.abs(denominator) < 1e-12) continue
    const t = (ax * ey - ay * ex) / denominator
    const u = (ax * dy - ay * dx) / denominator
    if (t > 0 && u >= 0 && u <= 1) nearest = Math.min(nearest, t)
  }
  return Number.isFinite(nearest) ? nearest : 0
}

function paint(data: Uint8ClampedArray, offset: number, color: Readonly<MouthExpressionPalette['line']>, alpha: number): void {
  const source = Math.max(0, Math.min(1, alpha))
  const destination = data[offset + 3] / 255
  const result = source + destination * (1 - source)
  if (result <= 0) return
  const retained = destination * (1 - source)
  data[offset] = Math.round((color.red * source + data[offset] * retained) / result)
  data[offset + 1] = Math.round((color.green * source + data[offset + 1] * retained) / result)
  data[offset + 2] = Math.round((color.blue * source + data[offset + 2] * retained) / result)
  data[offset + 3] = Math.round(result * 255)
}

function clamp01(value: number): number {
  return Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0
}
