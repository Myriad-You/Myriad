import type { RgbColor } from './dizzyEye'

export type SqueezeEyeSide = 'left' | 'right'

export interface SqueezeEyeSize {
  width: number
  height: number
}

export const SQUEEZE_EYE_WIDTH_FIT = 1.04
export const SQUEEZE_EYE_HEIGHT_FIT = 0.82
const SQUEEZE_EYE_MIN_WIDTH = 18
const SQUEEZE_EYE_MIN_HEIGHT = 14
const SQUEEZE_EYE_MAX_WIDTH = 192
const SQUEEZE_EYE_MAX_HEIGHT = 128
const SQUEEZE_EYE_MAX_DISPLAY_SCALE = 2.2

export function squeezeEyeGeneratedSize(eye: {
  x0: number
  x1: number
  y0: number
  y1: number
}): SqueezeEyeSize {
  const eyeWidth = Math.max(1, eye.x1 - eye.x0)
  const eyeHeight = Math.max(1, eye.y1 - eye.y0)
  const width = clampInt(
    Math.round(eyeWidth * SQUEEZE_EYE_WIDTH_FIT),
    SQUEEZE_EYE_MIN_WIDTH,
    SQUEEZE_EYE_MAX_WIDTH,
  )
  return {
    width,
    height: clampInt(
      Math.round(Math.max(eyeHeight * SQUEEZE_EYE_HEIGHT_FIT, width * 0.5)),
      SQUEEZE_EYE_MIN_HEIGHT,
      SQUEEZE_EYE_MAX_HEIGHT,
    ),
  }
}

/** Enlarge undersized authored marks while preserving deliberate large art. */
export function squeezeEyeDisplayScale(
  layerWidth: number,
  eye: { x0: number; x1: number },
): number {
  const eyeWidth = eye.x1 - eye.x0
  if (layerWidth <= 0 || eyeWidth <= 0) return 1
  return clamp(
    (eyeWidth * SQUEEZE_EYE_WIDTH_FIT) / layerWidth,
    1,
    SQUEEZE_EYE_MAX_DISPLAY_SCALE,
  )
}

/**
 * Generates inward-facing chevrons: the screen-left eye is `>` and the
 * screen-right eye is `<`. Slightly uneven curves and stroke weights keep the
 * generated mark closer to hand-drawn expression art than a geometric glyph.
 */
export function createSqueezeEyeBitmap(
  requestedSize: Readonly<SqueezeEyeSize>,
  tint: Readonly<RgbColor>,
  side: SqueezeEyeSide,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(
    Math.round(requestedSize.width),
    SQUEEZE_EYE_MIN_WIDTH,
    SQUEEZE_EYE_MAX_WIDTH,
  )
  const height = clampInt(
    Math.round(requestedSize.height),
    SQUEEZE_EYE_MIN_HEIGHT,
    SQUEEZE_EYE_MAX_HEIGHT,
  )
  const data = new Uint8ClampedArray(width * height * 4)
  const strokeRadius = Math.max(2, Math.min(width, height) * 0.105)
  const normalizedStrokes = organicStrokeLayout(side)
  const strokes = normalizedStrokes.map((stroke) =>
    stroke.map(([x, y, radius]) => ({
      x: x * (width - 1),
      y: y * (height - 1),
      radius: radius * strokeRadius,
    })),
  )

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let coverage = 0
      for (
        let strokeIndex = 0;
        strokeIndex < strokes.length;
        strokeIndex += 1
      ) {
        const stroke = strokes[strokeIndex]
        for (let index = 1; index < stroke.length; index += 1) {
          coverage = Math.max(
            coverage,
            segmentCoverage(
              x,
              y,
              stroke[index - 1],
              stroke[index],
              strokeIndex * 7 + index,
            ),
          )
        }
      }
      if (coverage <= 0) continue
      const offset = (y * width + x) * 4
      data[offset] = clampInt(Math.round(tint.red), 0, 255)
      data[offset + 1] = clampInt(Math.round(tint.green), 0, 255)
      data[offset + 2] = clampInt(Math.round(tint.blue), 0, 255)
      data[offset + 3] = Math.round(coverage * 255)
    }
  }
  return { width, height, data }
}

type NormalizedStrokeNode = readonly [x: number, y: number, radius: number]

function organicStrokeLayout(
  side: SqueezeEyeSide,
): readonly (readonly NormalizedStrokeNode[])[] {
  if (side === 'left') {
    return [
      [
        [0.08, 0.17, 0.9],
        [0.23, 0.19, 1.11],
        [0.4, 0.28, 0.94],
        [0.55, 0.39, 1.08],
        [0.69, 0.43, 0.96],
        [0.83, 0.51, 1.06],
      ],
      [
        [0.15, 0.87, 1.09],
        [0.3, 0.82, 0.91],
        [0.47, 0.73, 1.13],
        [0.63, 0.61, 0.96],
        [0.83, 0.51, 1.04],
      ],
    ]
  }
  return [
    [
      [0.9, 0.14, 1.06],
      [0.73, 0.18, 0.92],
      [0.58, 0.3, 1.12],
      [0.39, 0.37, 0.94],
      [0.18, 0.52, 1.04],
    ],
    [
      [0.83, 0.85, 0.92],
      [0.68, 0.79, 1.13],
      [0.54, 0.7, 0.95],
      [0.38, 0.65, 1.1],
      [0.18, 0.52, 1.07],
    ],
  ]
}

function segmentCoverage(
  x: number,
  y: number,
  start: { x: number; y: number; radius: number },
  end: { x: number; y: number; radius: number },
  roughnessPhase: number,
): number {
  const dx = end.x - start.x
  const dy = end.y - start.y
  const lengthSquared = dx * dx + dy * dy
  const progress =
    lengthSquared > 0
      ? clamp(((x - start.x) * dx + (y - start.y) * dy) / lengthSquared, 0, 1)
      : 0
  const distance = Math.hypot(
    x - (start.x + dx * progress),
    y - (start.y + dy * progress),
  )
  const radius = start.radius + (end.radius - start.radius) * progress
  const edgeRoughness =
    Math.sin(x * 0.83 + y * 0.29 + roughnessPhase * 1.7) * 0.24 +
    Math.sin(x * 0.21 - y * 0.67 + roughnessPhase * 0.9) * 0.16
  return clamp(radius + 0.8 + edgeRoughness - distance, 0, 1)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function clampInt(value: number, minimum: number, maximum: number): number {
  return Math.round(clamp(value, minimum, maximum))
}
