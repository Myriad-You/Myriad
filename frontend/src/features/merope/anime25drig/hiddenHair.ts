import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import { insideDistance } from './pixelMasks'

type Box = Pick<Anime25DPlaybackLayer, 'x' | 'y' | 'w' | 'h'>

export interface HairCover {
  layer: Box
  image: CroppedLayerPixels
}

const COVERED = 230
/** The inside of the hair behind the head takes the hair's shadow: this band of its tones, darkest first. */
const SHADOW_FROM = 0.05
const SHADOW_TO = 0.15
/** The shade blends into what was there over this many canvas pixels inside the covered part. */
const FEATHER = 8

/**
 * Behind the head the back hair is the inside of the hair, in its own
 * shadow: a rigger paints it as one dark tone of the hair, so a
 * turning face uncovers the back of the coiffure, not a stain. A
 * decomposition paints there whatever it guessed, often a pale wash. What the
 * face, ears and neck cover takes that dark tone, blending into the drawn
 * hair at its edge; what only the fringe covers is the top of the head and
 * keeps its hair. Nothing that shows at rest changes. Null when nothing is
 * covered or too little hair shows to read its colour.
 */
export function shadeHiddenBackHair(
  hair: Box,
  image: CroppedLayerPixels,
  head: readonly HairCover[],
): CroppedLayerPixels | null {
  const { width, height, pixels } = image
  if (width < 4 || height < 4 || pixels.length !== width * height * 4) return null
  const pixelX = hair.w / width
  const pixelY = hair.h / height
  const covered = new Uint8Array(width * height)
  for (const { layer, image: art } of head) {
    if (art.pixels.length !== art.width * art.height * 4) continue
    for (let y = 0; y < height; y++) {
      const ay = Math.floor((((hair.y + (y + 0.5) * pixelY) - layer.y) / layer.h) * art.height)
      if (ay < 0 || ay >= art.height) continue
      for (let x = 0; x < width; x++) {
        const ax = Math.floor((((hair.x + (x + 0.5) * pixelX) - layer.x) / layer.w) * art.width)
        if (ax < 0 || ax >= art.width) continue
        if (art.pixels[(ay * art.width + ax) * 4 + 3] >= COVERED) covered[y * width + x] = 1
      }
    }
  }
  // The hair's own shadow, read from what shows: its darker tones, as drawn.
  const shows: number[] = []
  let hidden = 0
  for (let p = 0; p < width * height; p++) {
    if (pixels[p * 4 + 3] < 250) continue
    if (covered[p]) hidden++
    else shows.push(p)
  }
  if (hidden === 0 || shows.length < 200) return null
  const lightness = (p: number) => pixels[p * 4] * 0.299 + pixels[p * 4 + 1] * 0.587 + pixels[p * 4 + 2] * 0.114
  shows.sort((left, right) => lightness(left) - lightness(right))
  const shadow = shows.slice(Math.floor(shows.length * SHADOW_FROM), Math.ceil(shows.length * SHADOW_TO))
  const inner = [0, 1, 2].map((channel) => shadow.reduce((sum, p) => sum + pixels[p * 4 + channel], 0) / shadow.length)
  const distance = insideDistance(covered, width, height)
  const feather = Math.max(1, FEATHER / pixelX)
  const out = new Uint8ClampedArray(pixels)
  for (let p = 0; p < width * height; p++) {
    if (!covered[p] || pixels[p * 4 + 3] === 0) continue
    const weight = Math.min(1, distance[p] / feather)
    for (let channel = 0; channel < 3; channel++) {
      out[p * 4 + channel] = Math.round(pixels[p * 4 + channel] * (1 - weight) + inner[channel] * weight)
    }
  }
  return { pixels: out, width, height }
}
