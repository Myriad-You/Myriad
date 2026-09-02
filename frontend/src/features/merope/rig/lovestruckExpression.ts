import type { RgbColor } from './dizzyEye'

export interface LovestruckBitmapSize {
  width: number
  height: number
}

type Point = readonly [x: number, y: number]

const HOT_PINK: Readonly<RgbColor> = { red: 244, green: 74, blue: 132 }
const ROSE_EDGE: Readonly<RgbColor> = { red: 148, green: 49, blue: 91 }
const BLUSH_PINK: Readonly<RgbColor> = { red: 250, green: 119, blue: 146 }
const SWEAT_EDGE: Readonly<RgbColor> = { red: 95, green: 157, blue: 194 }
const SWEAT_FILL: Readonly<RgbColor> = { red: 213, green: 242, blue: 250 }
const SWEAT_LIGHT: Readonly<RgbColor> = { red: 248, green: 254, blue: 255 }

export function lovestruckHeartGeneratedSize(eye: {
  x0: number
  x1: number
  y0: number
  y1: number
}): LovestruckBitmapSize {
  const eyeWidth = Math.max(1, eye.x1 - eye.x0)
  const eyeHeight = Math.max(1, eye.y1 - eye.y0)
  const width = clampInt(
    Math.round(Math.max(eyeWidth * 0.29, eyeHeight * 0.54)),
    16,
    112,
  )
  return {
    width,
    height: clampInt(Math.round(width * 0.86), 14, 104),
  }
}

export function lovestruckFaceEffectGeneratedSize(face: {
  x0: number
  x1: number
  y0: number
  y1: number
}): LovestruckBitmapSize {
  const faceWidth = Math.max(1, face.x1 - face.x0)
  const faceHeight = Math.max(1, face.y1 - face.y0)
  return {
    width: clampInt(Math.round(faceWidth * 0.94), 96, 768),
    height: clampInt(Math.round(faceHeight * 0.72), 96, 640),
  }
}

export function lovestruckDroolGeneratedSize(
  mouth: { x0: number; x1: number; y0: number; y1: number },
  face: { x0: number; x1: number },
): LovestruckBitmapSize {
  const mouthWidth = Math.max(1, mouth.x1 - mouth.x0)
  const faceWidth = Math.max(1, face.x1 - face.x0)
  const width = clampInt(
    Math.round(Math.max(mouthWidth * 0.22, faceWidth * 0.018)),
    8,
    42,
  )
  return {
    width,
    height: clampInt(Math.round(width * 1.4), 12, 58),
  }
}

/** Small opaque heart that covers the original pupil without replacing the iris. */
export function createLovestruckHeartBitmap(
  requestedSize: Readonly<LovestruckBitmapSize>,
  sourcePink: Readonly<RgbColor>,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(Math.round(requestedSize.width), 16, 112)
  const height = clampInt(Math.round(requestedSize.height), 14, 104)
  const data = new Uint8ClampedArray(width * height * 4)
  const core = mixColor(sourcePink, HOT_PINK, 0.72)
  const edge = mixColor(core, ROSE_EDGE, 0.38)
  const samples: readonly Point[] = [
    [0.2, 0.2],
    [0.8, 0.2],
    [0.2, 0.8],
    [0.8, 0.8],
  ]
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let outerCoverage = 0
      let innerCoverage = 0
      for (const [offsetX, offsetY] of samples) {
        const nx = ((x + offsetX) / width - 0.5) * 2.12
        const ny = ((y + offsetY) / height - 0.5) * 2.04
        if (insideHeart(nx, ny)) outerCoverage += 0.25
        if (insideHeart(nx / 0.82, (ny + 0.035) / 0.82)) {
          innerCoverage += 0.25
        }
      }
      if (outerCoverage <= 0) continue
      const offset = (y * width + x) * 4
      paint(data, offset, edge, outerCoverage)
      if (innerCoverage > 0) paint(data, offset, core, innerCoverage)
    }
  }
  return { width, height, data }
}

/**
 * One face-local layer carries the broad blush, hatch marks, and three small
 * sweat drops. Keeping them in one atlas rectangle avoids three extra draws.
 */
export function createLovestruckFaceEffectBitmap(
  requestedSize: Readonly<LovestruckBitmapSize>,
  sourcePink: Readonly<RgbColor>,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(Math.round(requestedSize.width), 96, 768)
  const height = clampInt(Math.round(requestedSize.height), 96, 640)
  const data = new Uint8ClampedArray(width * height * 4)
  const blush = mixColor(sourcePink, BLUSH_PINK, 0.8)
  const hatch = mixColor(blush, ROSE_EDGE, 0.3)
  const hatchRails = cheekHatchRails()

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const nx = ((x + 0.5) / width - 0.5) * 2
      const ny = (y + 0.5) / height
      const leftCheek = gaussian(nx, ny, -0.48, 0.49, 0.43, 0.18)
      const rightCheek = gaussian(nx, ny, 0.48, 0.49, 0.43, 0.18)
      const noseBridge = gaussian(nx, ny, 0, 0.47, 0.5, 0.15) * 0.62
      const blushAlpha =
        clamp(
          Math.max(leftCheek, rightCheek) * 0.42 + noseBridge * 0.24,
          0,
          0.5,
        ) *
        smoothstep((ny - 0.24) / 0.11) *
        (1 - smoothstep((ny - 0.72) / 0.14))
      const offset = (y * width + x) * 4
      if (blushAlpha > 0.002) paint(data, offset, blush, blushAlpha)

      let hatchDistance = Number.POSITIVE_INFINITY
      for (const [start, end] of hatchRails) {
        hatchDistance = Math.min(
          hatchDistance,
          pointToSegmentDistance(nx, ny, start, end),
        )
      }
      const hatchCoverage =
        clamp((0.0095 - hatchDistance) * width * 0.8, 0, 1) *
        Math.max(leftCheek, rightCheek) *
        0.28
      if (hatchCoverage > 0) paint(data, offset, hatch, hatchCoverage)
    }
  }

  paintSweatDrop(data, width, height, 0.19, 0.12, 0.025, 0.055, -0.08)
  paintSweatDrop(data, width, height, 0.12, 0.72, 0.027, 0.059, 0.09)
  paintSweatDrop(data, width, height, 0.88, 0.61, 0.028, 0.062, -0.07)
  return { width, height, data }
}

/** Separate so the droplet can follow the animated mouth corner, not the face. */
export function createLovestruckDroolBitmap(
  requestedSize: Readonly<LovestruckBitmapSize>,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(Math.round(requestedSize.width), 8, 42)
  const height = clampInt(Math.round(requestedSize.height), 12, 58)
  const data = new Uint8ClampedArray(width * height * 4)
  paintDropIntoRect(data, width, height, 0, 0, width, height, 0.08)
  return { width, height, data }
}

function insideHeart(x: number, imageY: number): boolean {
  const y = -imageY + 0.13
  const scaledX = x * 1.03
  const scaledY = y * 1.08
  const base = scaledX * scaledX + scaledY * scaledY - 1
  return (
    base * base * base - scaledX * scaledX * scaledY * scaledY * scaledY <= 0
  )
}

function cheekHatchRails(): ReadonlyArray<readonly [Point, Point]> {
  const rails: Array<readonly [Point, Point]> = []
  for (const side of [-1, 1] as const) {
    for (let index = 0; index < 6; index += 1) {
      const centerX = side * (0.25 + index * 0.085)
      const centerY = 0.46 + (index % 2) * 0.028
      rails.push([
        [centerX - 0.055, centerY - 0.045],
        [centerX + 0.035, centerY + 0.04],
      ])
    }
  }
  return rails
}

function paintSweatDrop(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  centerX: number,
  centerY: number,
  radiusX: number,
  radiusY: number,
  rotation: number,
): void {
  const pixelWidth = radiusX * width * 2.8
  const pixelHeight = radiusY * height * 2.5
  const left = Math.round(centerX * width - pixelWidth / 2)
  const top = Math.round(centerY * height - pixelHeight / 2)
  paintDropIntoRect(
    data,
    width,
    height,
    left,
    top,
    Math.max(5, Math.round(pixelWidth)),
    Math.max(8, Math.round(pixelHeight)),
    rotation,
  )
}

function paintDropIntoRect(
  data: Uint8ClampedArray,
  canvasWidth: number,
  canvasHeight: number,
  left: number,
  top: number,
  width: number,
  height: number,
  rotation: number,
): void {
  const outer: readonly Point[] = [
    [0.5, 0.02],
    [0.68, 0.29],
    [0.8, 0.55],
    [0.76, 0.78],
    [0.59, 0.96],
    [0.38, 0.94],
    [0.21, 0.75],
    [0.2, 0.53],
    [0.33, 0.27],
  ]
  const cosine = Math.cos(rotation)
  const sine = Math.sin(rotation)
  for (let localY = 0; localY < height; localY += 1) {
    const y = top + localY
    if (y < 0 || y >= canvasHeight) continue
    for (let localX = 0; localX < width; localX += 1) {
      const x = left + localX
      if (x < 0 || x >= canvasWidth) continue
      const px = (localX + 0.5) / width - 0.5
      const py = (localY + 0.5) / height - 0.5
      const rx = px * cosine - py * sine + 0.5
      const ry = px * sine + py * cosine + 0.5
      if (!pointInPolygon(rx, ry, outer)) continue
      let edgeDistance = Number.POSITIVE_INFINITY
      for (let index = 0; index < outer.length; index += 1) {
        edgeDistance = Math.min(
          edgeDistance,
          pointToSegmentDistance(
            rx,
            ry,
            outer[index],
            outer[(index + 1) % outer.length],
          ),
        )
      }
      const offset = (y * canvasWidth + x) * 4
      paint(data, offset, SWEAT_EDGE, 0.5)
      if (edgeDistance > 0.055) paint(data, offset, SWEAT_FILL, 0.72)
      const highlight = Math.hypot((rx - 0.39) / 0.13, (ry - 0.58) / 0.2)
      if (highlight < 1)
        paint(data, offset, SWEAT_LIGHT, (1 - highlight) * 0.72)
    }
  }
}

function gaussian(
  x: number,
  y: number,
  centerX: number,
  centerY: number,
  radiusX: number,
  radiusY: number,
): number {
  const dx = (x - centerX) / radiusX
  const dy = (y - centerY) / radiusY
  return Math.exp(-(dx * dx + dy * dy) * 1.8)
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
  start: Point,
  end: Point,
): number {
  const dx = end[0] - start[0]
  const dy = end[1] - start[1]
  const lengthSquared = dx * dx + dy * dy
  const progress =
    lengthSquared > 0
      ? clamp(((x - start[0]) * dx + (y - start[1]) * dy) / lengthSquared, 0, 1)
      : 0
  return Math.hypot(
    x - (start[0] + dx * progress),
    y - (start[1] + dy * progress),
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

function mixColor(
  first: Readonly<RgbColor>,
  second: Readonly<RgbColor>,
  amount: number,
): RgbColor {
  const mix = clamp(amount, 0, 1)
  return {
    red: first.red + (second.red - first.red) * mix,
    green: first.green + (second.green - first.green) * mix,
    blue: first.blue + (second.blue - first.blue) * mix,
  }
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function clampInt(value: number, minimum: number, maximum: number): number {
  return Math.round(clamp(value, minimum, maximum))
}
