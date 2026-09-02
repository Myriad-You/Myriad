import type { RgbColor } from './dizzyEye'

export interface ExpressionSymbolSizes {
  anger: { width: number; height: number }
  speechless: { width: number; height: number }
}

type Point = readonly [x: number, y: number]

export function expressionSymbolGeneratedSizes(
  faceWidth: number,
): ExpressionSymbolSizes {
  const width = Math.max(1, faceWidth)
  const angerEdge = clampInt(Math.round(width * 0.27), 52, 224)
  return {
    anger: { width: angerEdge, height: angerEdge },
    speechless: {
      width: clampInt(Math.round(width * 0.15), 38, 128),
      height: clampInt(Math.round(width * 0.28), 58, 216),
    },
  }
}

/** Four inward-facing manga anger arcs with a sticker separation rail. */
export function createAngerMarkBitmap(
  requestedSize: { width: number; height: number },
  sourceTint: Readonly<RgbColor>,
): { width: number; height: number; data: Uint8ClampedArray } {
  const contentWidth = clampInt(Math.round(requestedSize.width), 44, 228)
  const contentHeight = clampInt(Math.round(requestedSize.height), 44, 228)
  const scale = Math.min(contentWidth, contentHeight)
  const padding = Math.ceil(scale * 0.2)
  const width = contentWidth + padding * 2
  const height = contentHeight + padding * 2
  const data = new Uint8ClampedArray(width * height * 4)
  const stickerRadius = Math.max(5.2, scale * 0.15)
  const borderRadius = Math.max(3.6, scale * 0.108)
  const coreRadius = Math.max(2.4, scale * 0.072)
  const border = {
    red: clampInt(sourceTint.red * 0.28 + 72, 56, 136),
    green: clampInt(sourceTint.green * 0.18 + 20, 18, 58),
    blue: clampInt(sourceTint.blue * 0.22 + 32, 26, 76),
  }
  const core = { red: 236, green: 76, blue: 100 }
  const sticker = { red: 255, green: 244, blue: 247 }
  const shadow = { red: 73, green: 38, blue: 55 }
  const branchSeparation = 0.055
  const branches: readonly (readonly Point[])[] = [
    translateStroke(
      quadraticStroke([0.39, 0.055], [0.606, 0.421], [0.84, 0.14]),
      0,
      -branchSeparation,
    ),
    translateStroke(
      quadraticStroke([0.895, 0.35], [0.58, 0.579], [0.853, 0.809]),
      branchSeparation,
      0,
    ),
    translateStroke(
      quadraticStroke([0.625, 0.942], [0.397, 0.549], [0.178, 0.833]),
      0,
      branchSeparation,
    ),
    translateStroke(
      quadraticStroke([0.156, 0.155], [0.436, 0.378], [0.064, 0.613]),
      -branchSeparation,
      0,
    ),
  ]
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let distance = Number.POSITIVE_INFINITY
      for (let branch = 0; branch < branches.length; branch += 1) {
        const points = branches[branch]
        for (let segment = 1; segment < points.length; segment += 1) {
          const start = points[segment - 1]
          const end = points[segment]
          const roughX = Math.sin(y * 0.39 + branch * 2.1 + segment) * 0.22
          const roughY = Math.sin(x * 0.31 - branch * 1.7 - segment) * 0.18
          distance = Math.min(
            distance,
            pointToSegmentDistance(
              x + 0.5 + roughX,
              y + 0.5 + roughY,
              {
                x: padding + start[0] * contentWidth,
                y: padding + start[1] * contentHeight,
              },
              {
                x: padding + end[0] * contentWidth,
                y: padding + end[1] * contentHeight,
              },
            ),
          )
        }
      }
      let shadowDistance = Number.POSITIVE_INFINITY
      for (const points of branches) {
        for (let segment = 1; segment < points.length; segment += 1) {
          const start = points[segment - 1]
          const end = points[segment]
          shadowDistance = Math.min(
            shadowDistance,
            pointToSegmentDistance(
              x - scale * 0.018,
              y - scale * 0.026,
              {
                x: padding + start[0] * contentWidth,
                y: padding + start[1] * contentHeight,
              },
              {
                x: padding + end[0] * contentWidth,
                y: padding + end[1] * contentHeight,
              },
            ),
          )
        }
      }
      const shadowCoverage =
        clamp(stickerRadius + 1.2 - shadowDistance, 0, 1) * 0.42
      const stickerCoverage = clamp(stickerRadius + 0.75 - distance, 0, 1)
      if (shadowCoverage <= 0 && stickerCoverage <= 0) continue
      const borderCoverage = clamp(borderRadius + 0.75 - distance, 0, 1)
      const coreCoverage = clamp(coreRadius + 0.75 - distance, 0, 1)
      const offset = (y * width + x) * 4
      if (shadowCoverage > 0) paint(data, offset, shadow, shadowCoverage)
      if (stickerCoverage > 0) paint(data, offset, sticker, stickerCoverage)
      if (borderCoverage > 0) paint(data, offset, border, borderCoverage)
      if (coreCoverage > 0) paint(data, offset, core, coreCoverage)
    }
  }
  return { width, height, data }
}

/** Large, asymmetric manga sweat drop; deliberately distinct from eye tears. */
export function createSpeechlessSweatBitmap(requestedSize: {
  width: number
  height: number
}): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(Math.round(requestedSize.width), 34, 132)
  const height = clampInt(Math.round(requestedSize.height), 52, 220)
  const data = new Uint8ClampedArray(width * height * 4)
  const outer: readonly Point[] = [
    [0.54, 0.025],
    [0.68, 0.19],
    [0.82, 0.39],
    [0.91, 0.61],
    [0.87, 0.78],
    [0.73, 0.92],
    [0.52, 0.98],
    [0.31, 0.92],
    [0.15, 0.76],
    [0.11, 0.57],
    [0.2, 0.37],
    [0.36, 0.17],
  ]
  const border = insetPolygon(outer, 0.7, 0.8, 0.022)
  const inner = insetPolygon(outer, 0.48, 0.62, 0.05)
  const samples: readonly Point[] = [
    [0.25, 0.25],
    [0.75, 0.25],
    [0.25, 0.75],
    [0.75, 0.75],
  ]
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let shadowCoverage = 0
      let outerCoverage = 0
      let borderCoverage = 0
      let innerCoverage = 0
      let highlightCoverage = 0
      for (const [offsetX, offsetY] of samples) {
        const px = (x + offsetX) / width
        const py = (y + offsetY) / height
        if (pointInPolygon(px - 0.025, py - 0.018, outer)) {
          shadowCoverage += 0.25
        }
        if (pointInPolygon(px, py, outer)) outerCoverage += 0.25
        if (pointInPolygon(px, py, border)) borderCoverage += 0.25
        if (pointInPolygon(px, py, inner)) innerCoverage += 0.25
        const hx = (px - 0.38) / 0.13
        const hy = (py - 0.56) / 0.19
        if (hx * hx + hy * hy < 1) highlightCoverage += 0.25
      }
      if (shadowCoverage <= 0 && outerCoverage <= 0) continue
      const offset = (y * width + x) * 4
      if (shadowCoverage > 0) {
        paint(
          data,
          offset,
          { red: 34, green: 83, blue: 128 },
          shadowCoverage * 0.38,
        )
      }
      if (outerCoverage > 0) {
        paint(data, offset, { red: 250, green: 253, blue: 255 }, outerCoverage)
      }
      if (borderCoverage > 0) {
        paint(data, offset, { red: 35, green: 125, blue: 199 }, borderCoverage)
      }
      if (innerCoverage > 0) {
        paint(data, offset, { red: 84, green: 211, blue: 248 }, innerCoverage)
      }
      if (highlightCoverage > 0) {
        paint(
          data,
          offset,
          { red: 226, green: 250, blue: 255 },
          highlightCoverage * 0.9,
        )
      }
    }
  }
  return { width, height, data }
}

function insetPolygon(
  points: readonly Point[],
  scaleX: number,
  scaleY: number,
  offsetY: number,
): Point[] {
  return points.map(([x, y]): Point => [
    0.5 + (x - 0.5) * scaleX,
    0.5 + (y - 0.5) * scaleY + offsetY,
  ])
}

function quadraticStroke(
  start: Point,
  control: Point,
  end: Point,
  segments = 14,
): Point[] {
  const output: Point[] = []
  for (let segment = 0; segment <= segments; segment += 1) {
    const progress = segment / segments
    const inverse = 1 - progress
    output.push([
      inverse * inverse * start[0] +
        2 * inverse * progress * control[0] +
        progress * progress * end[0],
      inverse * inverse * start[1] +
        2 * inverse * progress * control[1] +
        progress * progress * end[1],
    ])
  }
  return output
}

function translateStroke(
  points: readonly Point[],
  offsetX: number,
  offsetY: number,
): Point[] {
  return points.map(([x, y]): Point => [x + offsetX, y + offsetY])
}

function pointInPolygon(
  x: number,
  y: number,
  points: readonly Point[],
): boolean {
  let inside = false
  for (
    let index = 0, previous = points.length - 1;
    index < points.length;
    previous = index, index += 1
  ) {
    const [xi, yi] = points[index]
    const [xj, yj] = points[previous]
    const denominator = yj - yi
    if (
      yi > y !== yj > y &&
      x <
        ((xj - xi) * (y - yi)) /
          (Math.abs(denominator) > 1e-8 ? denominator : 1e-8) +
          xi
    ) {
      inside = !inside
    }
  }
  return inside
}

function pointToSegmentDistance(
  x: number,
  y: number,
  start: { x: number; y: number },
  end: { x: number; y: number },
): number {
  const dx = end.x - start.x
  const dy = end.y - start.y
  const lengthSquared = dx * dx + dy * dy
  const progress =
    lengthSquared > 0
      ? clamp(((x - start.x) * dx + (y - start.y) * dy) / lengthSquared, 0, 1)
      : 0
  return Math.hypot(
    x - (start.x + dx * progress),
    y - (start.y + dy * progress),
  )
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
  data[offset] = Math.round(
    color.red * sourceWeight + data[offset] * destinationWeight,
  )
  data[offset + 1] = Math.round(
    color.green * sourceWeight + data[offset + 1] * destinationWeight,
  )
  data[offset + 2] = Math.round(
    color.blue * sourceWeight + data[offset + 2] * destinationWeight,
  )
  data[offset + 3] = Math.round(outputAlpha * 255)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function clampInt(value: number, minimum: number, maximum: number): number {
  return Math.round(clamp(value, minimum, maximum))
}
