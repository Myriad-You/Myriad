import type { RgbColor } from './dizzyEye'

/**
 * Small drawn accents that sit beside her face and say a mood at a glance:
 * music notes, a question mark, a sleepy "Z z z", sparkles, a blush. Each is
 * drawn here, at import, from a few strokes and shapes in a unit box, with
 * the same sticker look as the anger mark: a soft shadow, a white rim, a
 * dark outline and a coloured body. The blush is drawn softly, without a rim.
 */
export type ExpressionAccent = 'notes' | 'question' | 'sleep' | 'sparkle' | 'blush'

export interface AccentBitmap {
  width: number
  height: number
  data: Uint8ClampedArray
}

type Point = readonly [x: number, y: number]

type Mark =
  | { kind: 'stroke'; points: readonly Point[]; width: number }
  | { kind: 'disc'; center: Point; radius: number }
  | { kind: 'ellipse'; center: Point; rx: number; ry: number; angle: number }
  | { kind: 'polygon'; points: readonly Point[] }

interface Glyph {
  marks: readonly Mark[]
  /** Width over height of the drawn box. */
  aspect: number
  body: Readonly<RgbColor>
  outline: Readonly<RgbColor>
}

const WHITE: Readonly<RgbColor> = { red: 255, green: 252, blue: 250 }
const SHADOW: Readonly<RgbColor> = { red: 66, green: 44, blue: 70 }

function arc(
  center: Point,
  radius: number,
  from: number,
  to: number,
  steps = 14,
): Point[] {
  return Array.from({ length: steps + 1 }, (_, index) => {
    const angle = from + ((to - from) * index) / steps
    return [center[0] + Math.cos(angle) * radius, center[1] + Math.sin(angle) * radius] as const
  })
}

function zigzag(x0: number, y0: number, x1: number, y1: number): Point[] {
  return [
    [x0, y0],
    [x1, y0],
    [x0, y1],
    [x1, y1],
  ]
}

function star(center: Point, outer: number): Point[] {
  return Array.from({ length: 8 }, (_, index) => {
    const radius = index % 2 === 0 ? outer : outer * 0.3
    const angle = -Math.PI / 2 + (index * Math.PI) / 4
    return [center[0] + Math.cos(angle) * radius, center[1] + Math.sin(angle) * radius] as const
  })
}

const GLYPHS: Record<Exclude<ExpressionAccent, 'blush'>, Glyph> = {
  notes: {
    aspect: 1,
    body: { red: 74, green: 58, blue: 112 },
    outline: { red: 46, green: 34, blue: 72 },
    marks: [
      { kind: 'ellipse', center: [0.27, 0.78], rx: 0.13, ry: 0.09, angle: -0.42 },
      { kind: 'ellipse', center: [0.71, 0.66], rx: 0.13, ry: 0.09, angle: -0.42 },
      { kind: 'stroke', points: [[0.385, 0.76], [0.385, 0.2]], width: 0.045 },
      { kind: 'stroke', points: [[0.825, 0.64], [0.825, 0.08]], width: 0.045 },
      { kind: 'stroke', points: [[0.385, 0.2], [0.825, 0.08]], width: 0.075 },
    ],
  },
  question: {
    aspect: 0.72,
    body: { red: 236, green: 108, blue: 82 },
    outline: { red: 120, green: 46, blue: 40 },
    marks: [
      {
        kind: 'stroke',
        points: [...arc([0.5, 0.3], 0.22, Math.PI * 1.08, Math.PI * 2.32), [0.5, 0.58], [0.5, 0.64]],
        width: 0.085,
      },
      { kind: 'disc', center: [0.5, 0.86], radius: 0.075 },
    ],
  },
  sleep: {
    aspect: 1,
    body: { red: 104, green: 140, blue: 226 },
    outline: { red: 44, green: 62, blue: 128 },
    marks: [
      { kind: 'stroke', points: zigzag(0.06, 0.56, 0.44, 0.94), width: 0.06 },
      { kind: 'stroke', points: zigzag(0.5, 0.3, 0.74, 0.54), width: 0.045 },
      { kind: 'stroke', points: zigzag(0.78, 0.07, 0.94, 0.23), width: 0.035 },
    ],
  },
  sparkle: {
    aspect: 1,
    body: { red: 255, green: 214, blue: 88 },
    outline: { red: 186, green: 112, blue: 30 },
    marks: [
      { kind: 'polygon', points: star([0.38, 0.6], 0.36) },
      { kind: 'polygon', points: star([0.8, 0.2], 0.17) },
    ],
  },
}

/** How big each accent is drawn, from her face's width in pixels. */
export function expressionAccentSize(
  accent: ExpressionAccent,
  faceWidth: number,
  eyeSpan = faceWidth * 0.62,
): { width: number; height: number } {
  const face = Math.max(1, faceWidth)
  if (accent === 'blush') {
    return {
      width: clampInt(eyeSpan * 1.25, 48, 420),
      height: clampInt(face * 0.11, 14, 90),
    }
  }
  const glyph = GLYPHS[accent]
  const height = clampInt(face * (accent === 'sparkle' ? 0.26 : 0.24), 40, 220)
  return { width: clampInt(height * glyph.aspect, 28, 220), height }
}

export function createExpressionAccentBitmap(
  accent: ExpressionAccent,
  requested: { width: number; height: number },
): AccentBitmap {
  return accent === 'blush'
    ? drawBlush(requested)
    : drawGlyph(GLYPHS[accent], requested)
}

function drawGlyph(glyph: Glyph, requested: { width: number; height: number }): AccentBitmap {
  const contentWidth = clampInt(requested.width, 20, 260)
  const contentHeight = clampInt(requested.height, 20, 260)
  const scale = Math.min(contentWidth, contentHeight)
  const rim = Math.max(2.2, scale * 0.07)
  const outline = Math.max(1.2, scale * 0.028)
  const padding = Math.ceil(rim + outline + scale * 0.04)
  const width = contentWidth + padding * 2
  const height = contentHeight + padding * 2
  const data = new Uint8ClampedArray(width * height * 4)
  const shadowShift = scale * 0.025
  const toUnit = (x: number, y: number): Point => [
    (x - padding) / contentWidth,
    (y - padding) / contentHeight,
  ]
  // Distances are measured in pixels: a unit box is not square.
  const inkDistance = (x: number, y: number): number => {
    const [ux, uy] = toUnit(x, y)
    let distance = Number.POSITIVE_INFINITY
    for (const mark of glyph.marks) {
      distance = Math.min(distance, markDistance(mark, ux, uy, contentWidth, contentHeight))
    }
    return distance
  }
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const ink = inkDistance(x + 0.5, y + 0.5)
      const shadow = inkDistance(x + 0.5 - shadowShift, y + 0.5 - shadowShift)
      const shadowCoverage = clamp(rim + outline + 0.75 - shadow, 0, 1) * 0.38
      const rimCoverage = clamp(rim + outline + 0.75 - ink, 0, 1)
      if (shadowCoverage <= 0 && rimCoverage <= 0) continue
      const outlineCoverage = clamp(outline + 0.75 - ink, 0, 1)
      const bodyCoverage = clamp(0.75 - ink, 0, 1)
      const offset = (y * width + x) * 4
      if (shadowCoverage > 0) paint(data, offset, SHADOW, shadowCoverage)
      if (rimCoverage > 0) paint(data, offset, WHITE, rimCoverage)
      if (outlineCoverage > 0) paint(data, offset, glyph.outline, outlineCoverage)
      if (bodyCoverage > 0) paint(data, offset, glyph.body, bodyCoverage)
    }
  }
  return { width, height, data }
}

/** Two soft pink patches with a few short strokes across each, as drawn blush is. */
function drawBlush(requested: { width: number; height: number }): AccentBitmap {
  const width = clampInt(requested.width, 32, 440)
  const height = clampInt(requested.height, 12, 120)
  const data = new Uint8ClampedArray(width * height * 4)
  const patch: Readonly<RgbColor> = { red: 247, green: 132, blue: 156 }
  const stroke: Readonly<RgbColor> = { red: 226, green: 88, blue: 122 }
  const cheeks: readonly number[] = [0.16, 0.84]
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const ux = (x + 0.5) / width
      const uy = (y + 0.5) / height
      let patchCoverage = 0
      let strokeCoverage = 0
      for (const cx of cheeks) {
        const nx = (ux - cx) / 0.16
        const ny = (uy - 0.5) / 0.46
        const radial = Math.hypot(nx, ny)
        patchCoverage = Math.max(patchCoverage, clamp(1 - radial, 0, 1) ** 0.8 * 0.5)
        for (let index = -1; index <= 1; index += 1) {
          const sx = cx + index * 0.055
          const distance = segmentDistance(
            ux * width,
            uy * height,
            [(sx + 0.02) * width, 0.26 * height],
            [(sx - 0.02) * width, 0.74 * height],
          )
          strokeCoverage = Math.max(strokeCoverage, clamp(height * 0.06 + 0.6 - distance, 0, 1) * 0.75)
        }
      }
      if (patchCoverage <= 0 && strokeCoverage <= 0) continue
      const offset = (y * width + x) * 4
      if (patchCoverage > 0) paint(data, offset, patch, patchCoverage)
      if (strokeCoverage > 0) paint(data, offset, stroke, strokeCoverage)
    }
  }
  return { width, height, data }
}

/** Signed distance in pixels from a point (in the unit box) to a mark's edge. */
function markDistance(mark: Mark, ux: number, uy: number, width: number, height: number): number {
  const px = ux * width
  const py = uy * height
  const at = (point: Point): Point => [point[0] * width, point[1] * height]
  const size = Math.min(width, height)
  if (mark.kind === 'stroke') {
    let distance = Number.POSITIVE_INFINITY
    for (let index = 1; index < mark.points.length; index += 1) {
      distance = Math.min(
        distance,
        segmentDistance(px, py, at(mark.points[index - 1]), at(mark.points[index])),
      )
    }
    return distance - (mark.width * size) / 2
  }
  if (mark.kind === 'disc') {
    const [cx, cy] = at(mark.center)
    return Math.hypot(px - cx, py - cy) - mark.radius * size
  }
  if (mark.kind === 'ellipse') {
    const [cx, cy] = at(mark.center)
    const cos = Math.cos(-mark.angle)
    const sin = Math.sin(-mark.angle)
    const dx = px - cx
    const dy = py - cy
    const rx = mark.rx * size
    const ry = mark.ry * size
    const lx = (dx * cos - dy * sin) / rx
    const ly = (dx * sin + dy * cos) / ry
    return (Math.hypot(lx, ly) - 1) * Math.min(rx, ry)
  }
  const points = mark.points.map(at)
  let distance = Number.POSITIVE_INFINITY
  for (let index = 0; index < points.length; index += 1) {
    distance = Math.min(
      distance,
      segmentDistance(px, py, points[index], points[(index + 1) % points.length]),
    )
  }
  return insidePolygon(px, py, points) ? -distance : distance
}

function segmentDistance(x: number, y: number, start: Point, end: Point): number {
  const dx = end[0] - start[0]
  const dy = end[1] - start[1]
  const lengthSquared = dx * dx + dy * dy
  const progress =
    lengthSquared > 0
      ? clamp(((x - start[0]) * dx + (y - start[1]) * dy) / lengthSquared, 0, 1)
      : 0
  return Math.hypot(x - (start[0] + dx * progress), y - (start[1] + dy * progress))
}

function insidePolygon(x: number, y: number, points: readonly Point[]): boolean {
  let inside = false
  for (let index = 0, previous = points.length - 1; index < points.length; previous = index++) {
    const [xi, yi] = points[index]
    const [xj, yj] = points[previous]
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside
  }
  return inside
}

function paint(
  data: Uint8ClampedArray,
  offset: number,
  color: Readonly<RgbColor>,
  alpha: number,
): void {
  const sourceAlpha = clamp(alpha, 0, 1)
  const destinationAlpha = data[offset + 3] / 255
  const outputAlpha = sourceAlpha + destinationAlpha * (1 - sourceAlpha)
  if (outputAlpha <= 0) return
  const sourceWeight = sourceAlpha / outputAlpha
  const destinationWeight = (destinationAlpha * (1 - sourceAlpha)) / outputAlpha
  data[offset] = Math.round(color.red * sourceWeight + data[offset] * destinationWeight)
  data[offset + 1] = Math.round(color.green * sourceWeight + data[offset + 1] * destinationWeight)
  data[offset + 2] = Math.round(color.blue * sourceWeight + data[offset + 2] * destinationWeight)
  data[offset + 3] = Math.round(outputAlpha * 255)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function clampInt(value: number, minimum: number, maximum: number): number {
  return Math.round(clamp(value, minimum, maximum))
}
