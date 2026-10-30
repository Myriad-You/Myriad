import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

type Box = Pick<Anime25DPlaybackLayer, 'x' | 'y' | 'w' | 'h'>

const OPAQUE = 128
/** Rows of the neck read for its sides: the top of what shows below the chin. */
const SIDE_ROWS = 0.3
/** A row shows the neck whole when it is at least this share of the neck's typical width. */
const WHOLE_SHARE = 0.7
/** The sides carried up may spread at most this much wider than where they were read. */
const MAX_SPREAD = 1.3
/** Canvas pixels over which the kept neck fades out at its sides. */
const FEATHER = 2

/**
 * The neck goes on up behind the face as a neck. A decomposition paints what
 * the face hides as a broad flat field of skin out to the jaw, which a
 * turning face uncovers as a block. Kept is the neck itself: its two sides,
 * read where it shows below the chin, carried straight on up behind the
 * face; the rest of the hidden field is cleared, so what is behind it shows.
 * Null when there is nothing hidden to trim or too little neck to read.
 */
export function trimHiddenNeck(
  neck: Box,
  neckImage: CroppedLayerPixels,
  face: Box,
  faceImage: CroppedLayerPixels,
): CroppedLayerPixels | null {
  const { width, height, pixels } = neckImage
  if (width < 2 || height < 2 || pixels.length !== width * height * 4) return null
  if (faceImage.pixels.length !== faceImage.width * faceImage.height * 4) return null
  const canvasX = (column: number) => neck.x + ((column + 0.5) * neck.w) / width
  const canvasY = (line: number) => neck.y + ((line + 0.5) * neck.h) / height
  const faceCovers = (x: number, y: number) => {
    const fx = Math.floor(((x - face.x) / face.w) * faceImage.width)
    const fy = Math.floor(((y - face.y) / face.h) * faceImage.height)
    if (fx < 0 || fy < 0 || fx >= faceImage.width || fy >= faceImage.height) return false
    return faceImage.pixels[(fy * faceImage.width + fx) * 4 + 3] >= OPAQUE
  }
  // The neck's span on each row where it shows.
  const spans: Array<{ y: number; left: number; right: number }> = []
  let hidden = false
  for (let line = 0; line < height; line++) {
    const y = canvasY(line)
    let left = Infinity
    let right = -Infinity
    for (let column = 0; column < width; column++) {
      if (pixels[(line * width + column) * 4 + 3] < OPAQUE) continue
      const x = canvasX(column)
      if (faceCovers(x, y)) {
        hidden = true
        continue
      }
      left = Math.min(left, x)
      right = Math.max(right, x)
    }
    if (right - left > 2) spans.push({ y, left, right })
  }
  if (!hidden || spans.length < 3) return null
  // Read the sides where the neck shows whole. Beside the jaw a sliver of it
  // can peek out rows before the chin ends; those few pixels are not its sides.
  const widths = spans.map((span) => span.right - span.left).sort((a, b) => a - b)
  const typical = widths[Math.floor(widths.length / 2)]
  const whole = spans.filter((span) => span.right - span.left >= WHOLE_SHARE * typical)
  if (whole.length < 3) return null
  const read = whole.slice(0, Math.max(3, Math.round(whole.length * SIDE_ROWS)))
  const leftSide = fitLine(read.map((span) => [span.y, span.left]))
  const rightSide = fitLine(read.map((span) => [span.y, span.right]))
  const topY = read[0].y
  const readWidth = read[0].right - read[0].left
  const out = new Uint8ClampedArray(pixels)
  for (let line = 0; line < height; line++) {
    const y = canvasY(line)
    // Carried up from the top of what shows; never spreading past MAX_SPREAD.
    const rise = Math.min(y, topY)
    let left = leftSide(rise)
    let right = rightSide(rise)
    const middle = (left + right) / 2
    const half = Math.min((right - left) / 2, (readWidth * MAX_SPREAD) / 2)
    left = middle - half
    right = middle + half
    for (let column = 0; column < width; column++) {
      const at = (line * width + column) * 4 + 3
      if (out[at] === 0) continue
      const x = canvasX(column)
      if (!faceCovers(x, y)) continue
      const outside = Math.max(left - x, x - right)
      if (outside > 0) out[at] = Math.round(out[at] * Math.max(0, 1 - outside / FEATHER))
    }
  }
  return { pixels: out, width, height }
}

/** Least-squares line through (t, value) pairs, as a function of t. */
function fitLine(points: Array<[number, number]>): (t: number) => number {
  const n = points.length
  let st = 0
  let sv = 0
  let stt = 0
  let stv = 0
  for (const [t, v] of points) {
    st += t
    sv += v
    stt += t * t
    stv += t * v
  }
  const denominator = n * stt - st * st
  const slope = Math.abs(denominator) > 1e-9 ? (n * stv - st * sv) / denominator : 0
  const intercept = (sv - slope * st) / n
  return (t) => intercept + slope * t
}
