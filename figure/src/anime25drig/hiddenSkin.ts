import type { Anime25DEyeAnchor, Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import { dilate, erode, insideDistance } from './pixelMasks'

type Box = Pick<Anime25DPlaybackLayer, 'x' | 'y' | 'w' | 'h'>

export interface SkinCover {
  layer: Box
  image: CroppedLayerPixels
}

/** Covered: what is drawn over the skin hides it all but completely. */
const COVERED = 230
/** The skin read for the colour lies within this share of the face's width of what is hidden. */
const NEAR = 0.2
/** The darkest tenth of what shows is shadow, not skin. */
const SHADOW_SHARE = 0.1
/** The new skin blends into the old over this many canvas pixels inside the hidden part. */
const FEATHER = 6
/** Skin that shows at rest keeps the temple this many canvas pixels to either side of it. */
const SEEN_REACH = 8

/**
 * The face under the hair is skin and only skin. A decomposition paints it
 * with the shadows the hair cast on it, which a turning head carries out
 * from under the fringe as dark stains; a rigger paints that part plain.
 * The hidden skin takes a smooth colour field fitted on the skin that shows
 * near it, leaving its shadows out, and blends into what was there at the
 * edge where that is lighter; the shadow there, darkest at the hair's edge,
 * goes at once. Nothing that shows at rest changes. Null when nothing is hidden or
 * too little skin shows to read.
 */
export function fillHiddenSkin(
  face: Box,
  image: CroppedLayerPixels,
  covers: readonly SkinCover[],
): CroppedLayerPixels | null {
  const { width, height, pixels } = image
  if (width < 4 || height < 4 || pixels.length !== width * height * 4) return null
  const pixelX = face.w / width
  const covered = coveredBy(face, image, covers)
  const hidden = new Uint8Array(width * height)
  let hiddenCount = 0
  for (let p = 0; p < width * height; p++) {
    if (covered[p] && pixels[p * 4 + 3] > 64) {
      hidden[p] = 1
      hiddenCount++
    }
  }
  if (hiddenCount === 0) return null
  // What shows near what is hidden, clear of the drawing's own edge.
  const reach = Math.max(1, Math.round((NEAR * face.w) / pixelX))
  const nearHidden = dilate(hidden, width, height, reach)
  const edge = Math.max(1, Math.round(4 / pixelX))
  const solid = new Uint8Array(width * height)
  for (let p = 0; p < width * height; p++) solid[p] = pixels[p * 4 + 3] > 250 ? 1 : 0
  const inner = erode(solid, width, height, edge)
  const samples: number[] = []
  const lightness: number[] = []
  for (let p = 0; p < width * height; p++) {
    if (!inner[p] || covered[p] || !nearHidden[p]) continue
    samples.push(p)
    lightness.push(pixels[p * 4] * 0.299 + pixels[p * 4 + 1] * 0.587 + pixels[p * 4 + 2] * 0.114)
  }
  if (samples.length < 200) return null
  const shadow = quantile(lightness, SHADOW_SHARE)
  const skin = samples.filter((_, i) => lightness[i] >= shadow)
  // A plane per channel over the face, clamped to what the skin there spans.
  const field = [0, 1, 2].map((channel) => fitPlane(skin, pixels, channel, width, height))
  const distance = insideDistance(hidden, width, height)
  const feather = Math.max(1, FEATHER / pixelX)
  const out = new Uint8ClampedArray(pixels)
  for (let p = 0; p < width * height; p++) {
    if (!hidden[p]) continue
    const x = p % width
    const y = (p - x) / width
    const value = field.map((plane) => Math.max(plane.low, Math.min(plane.high, plane.a + plane.b * x + plane.c * y)))
    // The edge of what is hidden is where the hair's shadow is darkest: that goes all at once.
    const darker = pixels[p * 4] * 0.299 + pixels[p * 4 + 1] * 0.587 + pixels[p * 4 + 2] * 0.114 < value[0] * 0.299 + value[1] * 0.587 + value[2] * 0.114
    const weight = darker ? 1 : Math.min(1, distance[p] / feather)
    for (let channel = 0; channel < 3; channel++) {
      out[p * 4 + channel] = Math.round(pixels[p * 4 + channel] * (1 - weight) + value[channel] * weight)
    }
  }
  return { pixels: out, width, height }
}

/**
 * Beside the eyes and above them the head is hair, down to the eyes' height.
 * A decomposition carries the face out under the side locks to the skull's
 * outline, and the plain skin painted there shows as a bare strip when a lock
 * swings off the temple. What the hair covers there is cleared, outline and
 * all, so the back hair behind the face shows instead; a column where skin
 * shows at rest keeps its skin from that height down. Nothing that shows at
 * rest changes. Null when nothing is cleared.
 */
export function clearHiddenTemples(
  face: Box,
  image: CroppedLayerPixels,
  covers: readonly SkinCover[],
  eyeL: Pick<Anime25DEyeAnchor, 'x0' | 'y0'>,
  eyeR: Pick<Anime25DEyeAnchor, 'x1' | 'y0'>,
): CroppedLayerPixels | null {
  const { width, height, pixels } = image
  if (width < 4 || height < 4 || pixels.length !== width * height * 4) return null
  const pixelX = face.w / width
  const pixelY = face.h / height
  const covered = coveredBy(face, image, covers)
  // The highest skin that shows in each column.
  const seen = new Int32Array(width).fill(height)
  for (let x = 0; x < width; x++) {
    for (let y = 0; y < height; y++) {
      const p = y * width + x
      if (!covered[p] && pixels[p * 4 + 3] > 192) {
        seen[x] = y
        break
      }
    }
  }
  const reach = Math.round(SEEN_REACH / pixelX)
  const temple = new Uint8Array(width * height)
  let templeCount = 0
  for (let x = 0; x < width; x++) {
    const canvasX = face.x + (x + 0.5) * pixelX
    const eye = canvasX < eyeL.x0 ? eyeL : canvasX > eyeR.x1 ? eyeR : null
    if (!eye) continue
    let limit = Math.min(height, Math.floor((eye.y0 - face.y) / pixelY))
    for (let k = Math.max(0, x - reach); k <= Math.min(width - 1, x + reach); k++) limit = Math.min(limit, seen[k])
    for (let y = 0; y < limit; y++) {
      const p = y * width + x
      if (covered[p] && pixels[p * 4 + 3] > 0) {
        temple[p] = 1
        templeCount++
      }
    }
  }
  if (templeCount === 0) return null
  // It fades in from the skin that stays, and goes all at once out to the drawing's edge.
  const cleared = new Uint8Array(width * height)
  for (let p = 0; p < width * height; p++) cleared[p] = temple[p] || pixels[p * 4 + 3] === 0 ? 1 : 0
  const distance = insideDistance(cleared, width, height)
  const feather = Math.max(1, FEATHER / pixelX)
  const out = new Uint8ClampedArray(pixels)
  for (let p = 0; p < width * height; p++) {
    if (temple[p]) out[p * 4 + 3] = Math.round(pixels[p * 4 + 3] * (1 - Math.min(1, distance[p] / feather)))
  }
  return { pixels: out, width, height }
}

/** The face's pixels that what is drawn over it hides all but completely. */
function coveredBy(face: Box, image: CroppedLayerPixels, covers: readonly SkinCover[]): Uint8Array {
  const { width, height } = image
  const pixelX = face.w / width
  const pixelY = face.h / height
  const covered = new Uint8Array(width * height)
  for (const cover of covers) {
    const { layer, image: art } = cover
    if (art.pixels.length !== art.width * art.height * 4) continue
    for (let y = 0; y < height; y++) {
      const canvasY = face.y + (y + 0.5) * pixelY
      const ay = Math.floor(((canvasY - layer.y) / layer.h) * art.height)
      if (ay < 0 || ay >= art.height) continue
      for (let x = 0; x < width; x++) {
        const canvasX = face.x + (x + 0.5) * pixelX
        const ax = Math.floor(((canvasX - layer.x) / layer.w) * art.width)
        if (ax < 0 || ax >= art.width) continue
        if (art.pixels[(ay * art.width + ax) * 4 + 3] >= COVERED) covered[y * width + x] = 1
      }
    }
  }
  return covered
}

function fitPlane(
  samples: readonly number[],
  pixels: Uint8ClampedArray,
  channel: number,
  width: number,
  height: number,
): { a: number; b: number; c: number; low: number; high: number } {
  // Normal equations for value ≈ a + b·x + c·y, coordinates centred and scaled.
  let n = 0
  let sx = 0
  let sy = 0
  let sxx = 0
  let syy = 0
  let sxy = 0
  let sv = 0
  let sxv = 0
  let syv = 0
  const values: number[] = []
  for (const p of samples) {
    const x = (p % width) / width - 0.5
    const y = Math.floor(p / width) / height - 0.5
    const v = pixels[p * 4 + channel]
    values.push(v)
    n++
    sx += x
    sy += y
    sxx += x * x
    syy += y * y
    sxy += x * y
    sv += v
    sxv += x * v
    syv += y * v
  }
  const [a, b, c] = solve3(
    [[n, sx, sy], [sx, sxx, sxy], [sy, sxy, syy]],
    [sv, sxv, syv],
  )
  return {
    a: a - b * 0.5 - c * 0.5,
    b: b / width,
    c: c / height,
    low: quantile(values, 0.05),
    high: quantile(values, 0.95),
  }
}

function solve3(m: number[][], v: number[]): [number, number, number] {
  const det = (q: number[][]) =>
    q[0][0] * (q[1][1] * q[2][2] - q[1][2] * q[2][1]) -
    q[0][1] * (q[1][0] * q[2][2] - q[1][2] * q[2][0]) +
    q[0][2] * (q[1][0] * q[2][1] - q[1][1] * q[2][0])
  const d = det(m)
  if (Math.abs(d) < 1e-9) return [v[0] / Math.max(1, m[0][0]), 0, 0]
  const replace = (column: number) => m.map((row, r) => row.map((cell, k) => (k === column ? v[r] : cell)))
  return [det(replace(0)) / d, det(replace(1)) / d, det(replace(2)) / d]
}

function quantile(values: readonly number[], share: number): number {
  const sorted = values.toSorted((left, right) => left - right)
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.floor(share * sorted.length)))]
}
