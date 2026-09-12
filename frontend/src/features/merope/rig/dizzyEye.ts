export type DizzyEyeSide = 'left' | 'right'

export interface RgbColor {
  red: number
  green: number
  blue: number
}

const DEFAULT_DIZZY_TINT: Readonly<RgbColor> = {
  red: 54,
  green: 47,
  blue: 72,
}

export const DIZZY_EYE_FIT = 0.92
export const DIZZY_EYE_MIN_SIZE = 18
export const DIZZY_EYE_MAX_SIZE = 192
const DIZZY_EYE_MAX_DISPLAY_SCALE = 2.2

export function dizzyEyeGeneratedSize(eye: {
  x0: number
  x1: number
  y0: number
  y1: number
}): number {
  const span = Math.max(eye.x1 - eye.x0, eye.y1 - eye.y0)
  return clampInt(
    Math.round(span * DIZZY_EYE_FIT),
    DIZZY_EYE_MIN_SIZE,
    DIZZY_EYE_MAX_SIZE,
  )
}

/** Enlarge already-imported small spirals without shrinking authored artwork. */
export function dizzyEyeDisplayScale(
  layerWidth: number,
  layerHeight: number,
  eye: { x0: number; x1: number; y0: number; y1: number },
): number {
  const layerSpan = Math.max(layerWidth, layerHeight)
  const eyeSpan = Math.max(eye.x1 - eye.x0, eye.y1 - eye.y0)
  if (layerSpan <= 0 || eyeSpan <= 0) return 1
  return clamp(
    (eyeSpan * DIZZY_EYE_FIT) / layerSpan,
    1,
    DIZZY_EYE_MAX_DISPLAY_SCALE,
  )
}

export function sampleDizzyEyeTint(
  rgba: Uint8ClampedArray | undefined,
): RgbColor {
  if (!rgba) return { ...DEFAULT_DIZZY_TINT }
  let red = 0
  let green = 0
  let blue = 0
  let total = 0
  for (let index = 0; index + 3 < rgba.length; index += 4) {
    const alpha = rgba[index + 3]
    if (alpha < 24) continue
    const luminance = (rgba[index] + rgba[index + 1] + rgba[index + 2]) / 3
    const darkness = 1 - luminance / 255
    const weight = alpha * darkness * darkness
    red += rgba[index] * weight
    green += rgba[index + 1] * weight
    blue += rgba[index + 2] * weight
    total += weight
  }
  if (total <= 0.001) return { ...DEFAULT_DIZZY_TINT }
  return {
    red: red / total,
    green: green / total,
    blue: blue / total,
  }
}

/** Pure RGBA generator so import tests do not depend on Canvas implementation. */
export function createDizzyEyeBitmap(
  size: number,
  tint: Readonly<RgbColor>,
  side: DizzyEyeSide,
): { width: number; height: number; data: Uint8ClampedArray } {
  const edge = clampInt(
    Math.round(size),
    DIZZY_EYE_MIN_SIZE,
    DIZZY_EYE_MAX_SIZE,
  )
  const data = new Uint8ClampedArray(edge * edge * 4)
  const center = (edge - 1) / 2
  const radius = edge * 0.46
  const strokeRadius = Math.max(1.25, edge * 0.05)
  const turns = 2.35
  const samples = 128
  const points: Array<{ x: number; y: number }> = []
  for (let sample = 0; sample <= samples; sample += 1) {
    const progress = sample / samples
    const radians = progress * turns * Math.PI * 2
    const spiralRadius = radius * (0.08 + progress * 0.92)
    const direction = side === 'left' ? 1 : -1
    points.push({
      x: center + Math.cos(radians) * spiralRadius,
      y: center + Math.sin(radians) * spiralRadius * direction,
    })
  }
  for (let y = 0; y < edge; y += 1) {
    for (let x = 0; x < edge; x += 1) {
      let distance = Number.POSITIVE_INFINITY
      for (let point = 1; point < points.length; point += 1) {
        distance = Math.min(
          distance,
          pointToSegmentDistance(
            x + 0.5,
            y + 0.5,
            points[point - 1],
            points[point],
          ),
        )
      }
      const coverage = clamp(strokeRadius + 0.75 - distance, 0, 1)
      if (coverage <= 0) continue
      const offset = (y * edge + x) * 4
      data[offset] = clampInt(Math.round(tint.red), 0, 255)
      data[offset + 1] = clampInt(Math.round(tint.green), 0, 255)
      data[offset + 2] = clampInt(Math.round(tint.blue), 0, 255)
      data[offset + 3] = Math.round(coverage * 255)
    }
  }
  return { width: edge, height: edge, data }
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

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function clampInt(value: number, minimum: number, maximum: number): number {
  return Math.round(clamp(value, minimum, maximum))
}
