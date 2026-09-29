import type { RasterLayer } from './anime25dImportTypes'
import type { MouthExpressionKind, MouthExpressionSize } from './mouthExpression'
import { mouthExpressionOuterPath } from './mouthExpression'

/**
 * Speaking mouths for a face whose painted mouth has lips: a painterly or
 * made-up mouth, not a cel line. The cel glyph (dark rim, flat fill) reads as
 * a sticker on such a face. These keep the glyph's silhouettes, so speech
 * morphs exactly as before, but are painted like the portrait's own mouth:
 * lips in its lip colour with a lit lower lip, a band of its teeth, a cavity
 * that deepens away from the lips, a tongue, and soft edges.
 */

interface Rgb {
  red: number
  green: number
  blue: number
}

export interface PaintedLips {
  lips: Rgb
  teeth: Rgb
}

type Point = readonly [x: number, y: number]

export const LIP_MOUTH_KINDS: ReadonlySet<MouthExpressionKind> = new Set(['open', 'wide', 'round', 'narrow'])

/** A lip mouth opens to this share of its width; the glyph heights assume a cel line. */
const LIP_HEIGHT_SHARE: Readonly<Partial<Record<MouthExpressionKind, number>>> = {
  open: 0.5,
  wide: 0.3,
  round: 0.78,
  narrow: 0.2,
}

/** Below this share of lip-coloured pixels, or this height for its width, a mouth is a line. */
const MIN_LIP_SHARE = 0.25
const MIN_MOUTH_ASPECT = 0.22

/** The painted mouth's lips and teeth, or null for a cel line mouth. */
export function detectPaintedLips(mouth: Pick<RasterLayer, 'width' | 'height' | 'data'>): PaintedLips | null {
  const { width, height, data } = mouth
  if (width < 8 || height < 4 || data.length !== width * height * 4) return null
  let visible = 0
  let minX = width
  let maxX = -1
  let minY = height
  let maxY = -1
  const lips: Rgb[] = []
  const teeth: Rgb[] = []
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const index = (y * width + x) * 4
      if (data[index + 3] < 128) continue
      visible++
      minX = Math.min(minX, x)
      maxX = Math.max(maxX, x)
      minY = Math.min(minY, y)
      maxY = Math.max(maxY, y)
      const color = { red: data[index], green: data[index + 1], blue: data[index + 2] }
      const light = luminance(color)
      const saturation = colorSaturation(color)
      if (color.red > color.green * 1.3 && color.red > color.blue * 1.05 && light > 45 && light < 215 && saturation > 0.3) {
        lips.push(color)
      } else if (light > 195 && saturation < 0.22) {
        teeth.push(color)
      }
    }
  }
  if (visible < 80 || maxX < minX) return null
  const aspect = (maxY - minY + 1) / (maxX - minX + 1)
  if (lips.length / visible < MIN_LIP_SHARE || aspect < MIN_MOUTH_ASPECT) return null
  return {
    lips: average(lips),
    teeth: teeth.length >= visible * 0.03 ? average(teeth) : { red: 246, green: 240, blue: 238 },
  }
}

/** The glyph's width, with a height that suits lips rather than a cel line. */
export function lipMouthSize(kind: MouthExpressionKind, size: Readonly<MouthExpressionSize>): MouthExpressionSize {
  const share = LIP_HEIGHT_SHARE[kind]
  if (!share) return size
  return { width: size.width, height: Math.max(10, Math.round(size.width * share)) }
}

export function createLipMouthBitmap(
  kind: MouthExpressionKind,
  requestedSize: Readonly<MouthExpressionSize>,
  painted: Readonly<PaintedLips>,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = Math.max(16, Math.min(200, Math.round(requestedSize.width)))
  const height = Math.max(10, Math.min(160, Math.round(requestedSize.height)))
  const outer = mouthExpressionOuterPath(kind)
  const cut = lipCut(kind === 'narrow' ? 1 : 0, kind === 'round' ? 1 : 0)
  const opening = scalePath(outer, cut.openingX, cut.openingY, LIP_CUT_OFFSET)
  const lipLine = scalePath(outer, cut.lineX, cut.lineY, LIP_CUT_OFFSET)
  const data = new Uint8ClampedArray(width * height * 4)
  const openingTop = Math.min(...opening.map(([, y]) => y))
  const openingBottom = Math.max(...opening.map(([, y]) => y))
  paintLipMouth(data, width, height, painted, {
    toShape: (x, y, out) => {
      out.x = (x / width - 0.5) * 2.2
      out.y = (y / height - 0.5) * 2.2
    },
    openingRows: ((openingBottom - openingTop) / 2.2) * height,
    openingTop,
    openingBottom,
    teethMinimum: cut.teethMinimum,
    inOuter: (x, y) => pointInPolygon(x, y, outer),
    inLipLine: (x, y) => pointInPolygon(x, y, lipLine),
    inOpening: (x, y) => pointInPolygon(x, y, opening),
  })
  return { width, height, data: soften(data, width, height) }
}

/** The opening sits a little high: the lower lip is the fuller one. */
export const LIP_CUT_OFFSET = -0.06

export interface LipCut {
  openingX: number
  openingY: number
  lineX: number
  lineY: number
  /** Shallowest band of teeth, as a share of the opening's depth. */
  teethMinimum: number
}

/**
 * Where the lips part inside a mouth's outline, for a blend of the narrow and
 * round drawings (the rest being open or wide, which part alike). A narrow
 * mouth is mostly lip with a thin slit; a round one shows less of its teeth.
 */
export function lipCut(narrow: number, round: number): LipCut {
  const n = Math.max(0, Math.min(1, narrow))
  const r = Math.max(0, Math.min(1 - n, round))
  return {
    openingX: 0.8 + 0.04 * n,
    openingY: 0.62 - 0.16 * n,
    lineX: 0.845 + 0.035 * n,
    lineY: 0.7 - 0.14 * n,
    teethMinimum: 0.24 + 0.06 * n - 0.08 * r,
  }
}

/** A lip mouth's shape, however it is described, in the drawings' ±1.1 space. */
export interface LipMouthGeometry {
  /** Where bitmap position (x, y), in pixels, lies in shape space. */
  toShape: (x: number, y: number, out: { x: number; y: number }) => void
  /** How many pixels deep the opening is drawn. */
  openingRows: number
  openingTop: number
  openingBottom: number
  teethMinimum: number
  inOuter: (x: number, y: number) => boolean
  inLipLine: (x: number, y: number) => boolean
  inOpening: (x: number, y: number) => boolean
}

/**
 * Paints lips, teeth, cavity and tongue for any lip mouth shape into `data`,
 * unsoftened. The drawn mouths and the continuous speaking mouth share it, so
 * the continuous one is the same painting at every blend.
 */
export function paintLipMouth(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  painted: Readonly<PaintedLips>,
  shape: Readonly<LipMouthGeometry>,
): void {
  const { openingTop, openingBottom, openingRows } = shape
  const point = { x: 0, y: 0 }
  // A band of teeth at least a few pixels deep, or softening greys it into the cavity.
  const teethDepth = Math.min(0.45, Math.max(shape.teethMinimum, 3.5 / Math.max(1, openingRows)))
  const lipShade = shade(painted.lips, 0.14)
  const lipLight = mix(painted.lips, { red: 255, green: 236, blue: 236 }, 0.3)
  const lineColor = shade(painted.lips, 0.55)
  const cavityTop = shade(mix(painted.lips, { red: 70, green: 18, blue: 30 }, 0.55), 0.35)
  const cavityBottom = shade(mix(painted.lips, { red: 90, green: 26, blue: 38 }, 0.45), 0.18)
  const tongue = mix(painted.lips, { red: 245, green: 150, blue: 160 }, 0.45)
  const grid = 4
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let outerCoverage = 0
      let lineCoverage = 0
      let openCoverage = 0
      for (let sy = 0; sy < grid; sy++) {
        for (let sx = 0; sx < grid; sx++) {
          shape.toShape(x + (sx + 0.5) / grid, y + (sy + 0.5) / grid, point)
          const px = point.x
          const py = point.y
          if (!shape.inOuter(px, py)) continue
          outerCoverage++
          if (!shape.inLipLine(px, py)) continue
          lineCoverage++
          if (shape.inOpening(px, py)) openCoverage++
        }
      }
      if (outerCoverage === 0) continue
      const samples = grid * grid
      shape.toShape(x + 0.5, y + 0.5, point)
      const px = point.x
      const py = point.y
      const offset = (y * width + x) * 4
      // Lips: the upper lip in shadow, the lower lip lit across its middle.
      const lower = smoothstep((py - openingTop) / Math.max(0.01, openingBottom - openingTop))
      const highlight = lower * (1 - smoothstep(Math.abs(px) / 0.55)) * smoothstep((py - openingBottom) / 0.2)
      paint(data, offset, mix(mix(lipShade, painted.lips, lower), lipLight, highlight * 0.7), outerCoverage / samples)
      if (lineCoverage > 0) paint(data, offset, lineColor, lineCoverage / samples)
      if (openCoverage > 0) {
        const depth = (py - openingTop) / Math.max(0.01, openingBottom - openingTop)
        const tongueTop = 0.62 + 0.12 * px * px
        let color = mix(cavityTop, cavityBottom, smoothstep(depth / 0.8))
        if (depth > tongueTop) color = mix(color, tongue, smoothstep((depth - tongueTop) / 0.18))
        // Upper teeth, shaded where the lip overhangs them.
        const teeth = depth < teethDepth && Math.abs(px) < 0.5 ? 1 - smoothstep((depth - teethDepth * 0.7) / (teethDepth * 0.3)) : 0
        if (teeth > 0) color = mix(color, mix(shade(painted.teeth, 0.12), painted.teeth, smoothstep(depth / teethDepth)), teeth)
        paint(data, offset, color, openCoverage / samples)
      }
    }
  }
}

/** One pass of a small blur: painted edges are soft, never pixel-crisp. */
export function soften(data: Uint8ClampedArray, width: number, height: number): Uint8ClampedArray {
  const output = new Uint8ClampedArray(data.length)
  const kernel = [1, 2, 1]
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let alpha = 0
      let red = 0
      let green = 0
      let blue = 0
      let total = 0
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          const nx = x + dx
          const ny = y + dy
          if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue
          const weight = kernel[dx + 1] * kernel[dy + 1]
          const index = (ny * width + nx) * 4
          const a = (data[index + 3] / 255) * weight
          red += data[index] * a
          green += data[index + 1] * a
          blue += data[index + 2] * a
          alpha += a
          total += weight
        }
      }
      const index = (y * width + x) * 4
      if (alpha <= 0) continue
      output[index] = Math.round(red / alpha)
      output[index + 1] = Math.round(green / alpha)
      output[index + 2] = Math.round(blue / alpha)
      output[index + 3] = Math.round((alpha / total) * 255)
    }
  }
  return output
}

function scalePath(path: readonly Point[], scaleX: number, scaleY: number, offsetY: number): Point[] {
  return path.map(([x, y]) => [x * scaleX, y * scaleY + offsetY] as const)
}

function pointInPolygon(x: number, y: number, polygon: readonly Point[]): boolean {
  let inside = false
  for (let index = 0, previous = polygon.length - 1; index < polygon.length; previous = index++) {
    const [xi, yi] = polygon[index]
    const [xj, yj] = polygon[previous]
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside
  }
  return inside
}

function paint(data: Uint8ClampedArray, offset: number, color: Readonly<Rgb>, alpha: number): void {
  const source = Math.max(0, Math.min(1, alpha))
  const destination = data[offset + 3] / 255
  const out = source + destination * (1 - source)
  if (out <= 0) return
  const retained = destination * (1 - source)
  data[offset] = Math.round((color.red * source + data[offset] * retained) / out)
  data[offset + 1] = Math.round((color.green * source + data[offset + 1] * retained) / out)
  data[offset + 2] = Math.round((color.blue * source + data[offset + 2] * retained) / out)
  data[offset + 3] = Math.round(out * 255)
}

function average(colors: readonly Rgb[]): Rgb {
  let red = 0
  let green = 0
  let blue = 0
  for (const color of colors) {
    red += color.red
    green += color.green
    blue += color.blue
  }
  const count = Math.max(1, colors.length)
  return { red: Math.round(red / count), green: Math.round(green / count), blue: Math.round(blue / count) }
}

function mix(left: Readonly<Rgb>, right: Readonly<Rgb>, amount: number): Rgb {
  const t = Math.max(0, Math.min(1, amount))
  return {
    red: Math.round(left.red + (right.red - left.red) * t),
    green: Math.round(left.green + (right.green - left.green) * t),
    blue: Math.round(left.blue + (right.blue - left.blue) * t),
  }
}

function shade(color: Readonly<Rgb>, amount: number): Rgb {
  const kept = 1 - Math.max(0, Math.min(1, amount))
  return { red: Math.round(color.red * kept), green: Math.round(color.green * kept), blue: Math.round(color.blue * kept) }
}

function luminance(color: Readonly<Rgb>): number {
  return color.red * 0.299 + color.green * 0.587 + color.blue * 0.114
}

function colorSaturation(color: Readonly<Rgb>): number {
  const max = Math.max(color.red, color.green, color.blue)
  const min = Math.min(color.red, color.green, color.blue)
  return max <= 0 ? 0 : (max - min) / max
}

function smoothstep(value: number): number {
  const t = Math.max(0, Math.min(1, value))
  return t * t * (3 - 2 * t)
}
