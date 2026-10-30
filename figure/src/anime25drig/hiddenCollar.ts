import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import { dilate, erode, insideDistance } from './pixelMasks'

type Box = Pick<Anime25DPlaybackLayer, 'x' | 'y' | 'w' | 'h'>

export interface CollarGarment {
  layer: Box
  image: CroppedLayerPixels
  /** In front of the neck (the front collar): its cloth is read, but what of it lies over the neck shows. */
  inFront?: boolean
  /**
   * The back of a collar split from the top: all of what is hidden becomes
   * cloth, drawn or not. The split shares the top's pixels out between it,
   * the front collar and the top, so on its own it is full of holes, which
   * show through once the front collar and the neck move off them.
   */
  backing?: boolean
}

/** Covered: the neck hides it. */
const COVERED = 128
/** The front collar hides what is behind it at rest. */
const OPAQUE_FRONT = 230
/** Drawn: a garment pixel the neck hides, not empty. */
const DRAWN = 64
/** The cloth is read in a ring this share of the neck's width around what the neck hides. */
const RING = 0.08
/** Holes this share of the neck's width across, in what the neck hides of a garment, are closed. */
const HOLE = 0.04
/** Of the ring, what lies within this colour distance of its middle is the cloth there, not an outline or a shadow. */
const CLOTH_DISTANCE = 48
const MIN_SAMPLES = 40
/** The new cloth blends into the old over this many canvas pixels inside what is hidden. */
const FEATHER = 1.5

/**
 * What a garment has behind the neck is the inside of its collar, which the
 * picture never shows. A decomposition paints it as anything: the neck's own
 * skin with stray strokes, a grey band, holes. Turning the head carries the
 * neck off part of it, and that shows beside the neck. The garment's layers
 * behind it (the top and the back of a collar split from it) take the cloth
 * the garment shows right around the neck instead, the front collar's too, as
 * one smooth colour field, blended into what was there at the edge; small
 * holes close. The same goes for what the front collar hides over the neck,
 * which a front collar carried along with the neck uncovers. Nothing that
 * shows at rest changes.
 * Returns the garments' new pixels by index; empty when the neck hides none
 * of them or too little cloth shows near. Through a collar the neck is drawn
 * only within its clip (rows of left and right edges at rest): outside it,
 * only what the front collar covers over the neck is hidden.
 */
export function plainHiddenCollars(
  garments: readonly CollarGarment[],
  neck: Box,
  neckImage: CroppedLayerPixels,
  clip?: Float32Array,
): Map<number, CroppedLayerPixels> {
  const out = new Map<number, CroppedLayerPixels>()
  if (neckImage.pixels.length !== neckImage.width * neckImage.height * 4) return out
  const alphaOf = (box: Box, image: CroppedLayerPixels, canvasX: number, canvasY: number) => {
    const ix = Math.floor(((canvasX - box.x) / box.w) * image.width)
    const iy = Math.floor(((canvasY - box.y) / box.h) * image.height)
    if (ix < 0 || iy < 0 || ix >= image.width || iy >= image.height) return 0
    return image.pixels[(iy * image.width + ix) * 4 + 3]
  }
  const fronts = garments.filter((garment) => garment.inFront && garment.image.pixels.length === garment.image.width * garment.image.height * 4)
  // Hidden: where the neck is drawn over it, or the front collar over the neck. A
  // turning head carries both off it.
  const frontAlpha = (canvasX: number, canvasY: number) =>
    Math.max(0, ...fronts.map((front) => alphaOf(front.layer, front.image, canvasX, canvasY)))
  const neckAlpha = (canvasX: number, canvasY: number) => {
    const own = alphaOf(neck, neckImage, canvasX, canvasY)
    if (!clip || insideClip(clip, canvasX, canvasY)) return own
    if (own < COVERED) return 0
    return frontAlpha(canvasX, canvasY)
  }
  // A backing is hidden wherever the front collar covers it all but completely.
  const backingCovered = (canvasX: number, canvasY: number) =>
    neckAlpha(canvasX, canvasY) >= COVERED || frontAlpha(canvasX, canvasY) >= OPAQUE_FRONT
  const hiddenOf: Array<Uint8Array | null> = []
  // The cloth right around the neck, from every garment that shows there: x, y (as shares of the neck's box), rgb.
  const samples: number[][] = []
  for (const { layer, image, inFront, backing } of garments) {
    const { width, height, pixels } = image
    if (width < 4 || height < 4 || pixels.length !== width * height * 4) {
      hiddenOf.push(null)
      continue
    }
    const pixelX = layer.w / width
    const pixelY = layer.h / height
    const covered = new Uint8Array(width * height)
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        const canvasX = layer.x + (x + 0.5) * pixelX
        const canvasY = layer.y + (y + 0.5) * pixelY
        if (backing ? backingCovered(canvasX, canvasY) : neckAlpha(canvasX, canvasY) >= COVERED) covered[y * width + x] = 1
      }
    }
    // Holes are closed against the whole garment, not only what is hidden: a hole
    // under the neck's lower edge has the garment that shows below it on one side.
    const drawn = new Uint8Array(width * height)
    for (let p = 0; p < width * height; p++) drawn[p] = (backing ? covered[p] : pixels[p * 4 + 3] > DRAWN) ? 1 : 0
    const hole = Math.max(1, Math.round((HOLE * neck.w) / pixelX))
    const closed = erode(dilate(drawn, width, height, hole), width, height, hole)
    const hidden = new Uint8Array(width * height)
    let any = false
    for (let p = 0; p < width * height; p++) {
      if (covered[p] && closed[p]) {
        hidden[p] = 1
        any = true
      }
    }
    hiddenOf.push(any && !inFront ? hidden : null)
    const ring = dilate(covered, width, height, Math.max(1, Math.round((RING * neck.w) / pixelX)))
    for (let p = 0; p < width * height; p++) {
      if (!ring[p] || covered[p] || pixels[p * 4 + 3] <= 250) continue
      const x = p % width
      const y = (p - x) / width
      samples.push([
        (layer.x + (x + 0.5) * pixelX - neck.x) / neck.w,
        (layer.y + (y + 0.5) * pixelY - neck.y) / neck.h,
        pixels[p * 4],
        pixels[p * 4 + 1],
        pixels[p * 4 + 2],
      ])
    }
  }
  if (!hiddenOf.some(Boolean) || samples.length < MIN_SAMPLES) return out
  const middle = [2, 3, 4].map((k) => median(samples.map((sample) => sample[k])))
  const cloth = samples.filter((sample) => Math.hypot(
    sample[2] - middle[0],
    sample[3] - middle[1],
    sample[4] - middle[2],
  ) <= CLOTH_DISTANCE)
  if (cloth.length < MIN_SAMPLES) return out
  const field = [2, 3, 4].map((k) => fitPlane(cloth, k))
  garments.forEach(({ layer, image, backing }, index) => {
    const hidden = hiddenOf[index]
    if (!hidden) return
    const { width, height, pixels } = image
    const pixelX = layer.w / width
    const pixelY = layer.h / height
    const distance = insideDistance(hidden, width, height)
    const feather = Math.max(1, FEATHER / pixelX)
    const next = new Uint8ClampedArray(pixels)
    // A hole in the garment (not a backing) takes the garment that shows beside it.
    const holes = backing ? null : fillHoles(pixels, width, height, hidden, Math.max(1, Math.round((2 * HOLE * neck.w) / pixelX)))
    for (let p = 0; p < width * height; p++) {
      if (!hidden[p]) continue
      if (holes?.has(p)) {
        next.set(holes.get(p)!, p * 4)
        continue
      }
      const x = p % width
      const y = (p - x) / width
      const u = (layer.x + (x + 0.5) * pixelX - neck.x) / neck.w
      const v = (layer.y + (y + 0.5) * pixelY - neck.y) / neck.h
      const weight = Math.min(1, distance[p] / feather)
      for (let channel = 0; channel < 3; channel++) {
        const plane = field[channel]
        const value = Math.max(plane.low, Math.min(plane.high, plane.a + plane.b * u + plane.c * v))
        next[p * 4 + channel] = Math.round(pixels[p * 4 + channel] * (1 - weight) + value * weight)
      }
      next[p * 4 + 3] = Math.round(pixels[p * 4 + 3] + (255 - pixels[p * 4 + 3]) * weight)
    }
    out.set(index, { pixels: next, width, height })
  })
  return out
}

/**
 * The colours of the hidden pixels the garment left empty, spread in from
 * the garment that shows (drawn, not hidden) up to `reach` pixels away. Holes
 * farther than that are left out (they take the collar's cloth).
 */
function fillHoles(
  pixels: Uint8ClampedArray,
  width: number,
  height: number,
  hidden: Uint8Array,
  reach: number,
): Map<number, [number, number, number, number]> {
  const filled = new Map<number, [number, number, number, number]>()
  const source = (p: number) => !hidden[p] && pixels[p * 4 + 3] > 250
  let front: number[] = []
  for (let p = 0; p < width * height; p++) {
    if (hidden[p] && pixels[p * 4 + 3] <= DRAWN) front.push(p)
  }
  for (let pass = 0; pass < reach && front.length; pass++) {
    const next: number[] = []
    const found: Array<[number, [number, number, number, number]]> = []
    for (const p of front) {
      const x = p % width
      const y = (p - x) / width
      let r = 0
      let g = 0
      let b = 0
      let n = 0
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          const nx = x + dx
          const ny = y + dy
          if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue
          const q = ny * width + nx
          const known = filled.get(q)
          if (known) {
            r += known[0]
            g += known[1]
            b += known[2]
            n++
          } else if (source(q)) {
            r += pixels[q * 4]
            g += pixels[q * 4 + 1]
            b += pixels[q * 4 + 2]
            n++
          }
        }
      }
      if (n) found.push([p, [Math.round(r / n), Math.round(g / n), Math.round(b / n), 255]])
      else next.push(p)
    }
    for (const [p, colour] of found) filled.set(p, colour)
    front = next
  }
  return filled
}

/** Within the clip's rows (left x, y, right x, y) at rest, between consecutive rows. */
function insideClip(rows: Float32Array, x: number, y: number): boolean {
  for (let row = 0; row + 7 < rows.length; row += 4) {
    const y0 = rows[row + 1]
    const y1 = rows[row + 5]
    if (y < Math.min(y0, y1) || y > Math.max(y0, y1)) continue
    const t = y1 === y0 ? 0 : (y - y0) / (y1 - y0)
    const left = rows[row] + (rows[row + 4] - rows[row]) * t
    const right = rows[row + 2] + (rows[row + 6] - rows[row + 2]) * t
    if (x >= left && x <= right) return true
  }
  return false
}

/** value ≈ a + b·u + c·v over the samples' channel k, clamped to what they span. */
function fitPlane(samples: readonly number[][], k: number): { a: number; b: number; c: number; low: number; high: number } {
  let n = 0
  let su = 0
  let sv = 0
  let suu = 0
  let svv = 0
  let suv = 0
  let sz = 0
  let suz = 0
  let svz = 0
  for (const sample of samples) {
    const [u, v] = sample
    const z = sample[k]
    n++
    su += u
    sv += v
    suu += u * u
    svv += v * v
    suv += u * v
    sz += z
    suz += u * z
    svz += v * z
  }
  const [a, b, c] = solve3([[n, su, sv], [su, suu, suv], [sv, suv, svv]], [sz, suz, svz])
  const values = samples.map((sample) => sample[k])
  return { a, b, c, low: quantile(values, 0.05), high: quantile(values, 0.95) }
}

function solve3(m: number[][], v: number[]): [number, number, number] {
  const det = (q: number[][]) =>
    q[0][0] * (q[1][1] * q[2][2] - q[1][2] * q[2][1]) -
    q[0][1] * (q[1][0] * q[2][2] - q[1][2] * q[2][0]) +
    q[0][2] * (q[1][0] * q[2][1] - q[1][1] * q[2][0])
  const d = det(m)
  if (Math.abs(d) < 1e-9) return [v[0] / Math.max(1, m[0][0]), 0, 0]
  const replace = (column: number) => m.map((row, r) => row.map((cell, j) => (j === column ? v[r] : cell)))
  return [det(replace(0)) / d, det(replace(1)) / d, det(replace(2)) / d]
}

function median(values: number[]): number {
  return quantile(values, 0.5)
}

function quantile(values: readonly number[], share: number): number {
  const sorted = values.toSorted((left, right) => left - right)
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.floor(share * sorted.length)))]
}
