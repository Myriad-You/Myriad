import type { RgbColor } from './dizzyEye'
import type { SqueezeEyeSide } from './squeezeEye'
import { createSqueezeEyeBitmap } from './squeezeEye'

export type CryEyeSide = SqueezeEyeSide

export interface CryEyeSize {
  width: number
  height: number
}

const CRY_EYE_WIDTH_FIT = 1.1
const CRY_EYE_HEIGHT_FIT = 1.64
const CRY_EYE_MIN_WIDTH = 24
const CRY_EYE_MIN_HEIGHT = 32
const CRY_EYE_MAX_WIDTH = 224
const CRY_EYE_MAX_HEIGHT = 288
const CRY_EYE_MAX_DISPLAY_SCALE = 2.2

const TEAR_OUTLINE = { red: 76, green: 161, blue: 224 }
const TEAR_FILL = { red: 157, green: 220, blue: 250 }
const TEAR_HIGHLIGHT = { red: 239, green: 251, blue: 255 }

type TearNode = readonly [x: number, y: number, radius: number]

export function cryEyeGeneratedSize(eye: {
  x0: number
  x1: number
  y0: number
  y1: number
}): CryEyeSize {
  const eyeWidth = Math.max(1, eye.x1 - eye.x0)
  const eyeHeight = Math.max(1, eye.y1 - eye.y0)
  const width = clampInt(
    Math.round(eyeWidth * CRY_EYE_WIDTH_FIT),
    CRY_EYE_MIN_WIDTH,
    CRY_EYE_MAX_WIDTH,
  )
  return {
    width,
    height: clampInt(
      Math.round(Math.max(eyeWidth * CRY_EYE_HEIGHT_FIT, eyeHeight * 1.55)),
      CRY_EYE_MIN_HEIGHT,
      CRY_EYE_MAX_HEIGHT,
    ),
  }
}

/** Enlarge undersized authored crying eyes without shrinking deliberate art. */
export function cryEyeDisplayScale(
  layerWidth: number,
  eye: { x0: number; x1: number },
): number {
  const eyeWidth = eye.x1 - eye.x0
  if (layerWidth <= 0 || eyeWidth <= 0) return 1
  return clamp(
    (eyeWidth * CRY_EYE_WIDTH_FIT) / layerWidth,
    1,
    CRY_EYE_MAX_DISPLAY_SCALE,
  )
}

export function createCryEyeBitmap(
  requestedSize: Readonly<CryEyeSize>,
  eyeTint: Readonly<RgbColor>,
  side: CryEyeSide,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(
    Math.round(requestedSize.width),
    CRY_EYE_MIN_WIDTH,
    CRY_EYE_MAX_WIDTH,
  )
  const height = clampInt(
    Math.round(requestedSize.height),
    CRY_EYE_MIN_HEIGHT,
    CRY_EYE_MAX_HEIGHT,
  )
  const data = new Uint8ClampedArray(width * height * 4)
  paintTearStream(data, width, height, side)

  const squeezeWidth = Math.round(width * 0.94)
  const squeeze = createSqueezeEyeBitmap(
    { width: squeezeWidth, height: Math.round(squeezeWidth * 0.5) },
    eyeTint,
    side,
  )
  compositeBitmap(
    data,
    width,
    height,
    squeeze,
    Math.round((width - squeeze.width) / 2),
    Math.round(height * 0.025),
  )
  return { width, height, data }
}

function paintTearStream(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  side: CryEyeSide,
): void {
  const body = tearNodes(side)
  const highlight = highlightNodes(side)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const px = x / Math.max(1, width - 1)
      const py = y / Math.max(1, height - 1)
      const roughness =
        Math.sin(x * 0.37 + y * 0.19 + (side === 'left' ? 0.4 : 1.7)) * 0.004
      const outline = pathCoverage(px, py, body, roughness + 0.022)
      const fill = pathCoverage(px, py, body, roughness)
      const shine = pathCoverage(px, py, highlight, 0)
      const offset = (y * width + x) * 4
      if (outline > 0) {
        blendPixel(data, offset, TEAR_OUTLINE, outline * 0.72)
      }
      if (fill > 0) blendPixel(data, offset, TEAR_FILL, fill * 0.66)
      if (shine > 0) {
        blendPixel(data, offset, TEAR_HIGHLIGHT, shine * 0.82)
      }
    }
  }
}

function tearNodes(side: CryEyeSide): readonly TearNode[] {
  return side === 'left'
    ? [
        [0.23, 0.29, 0.038],
        [0.27, 0.39, 0.068],
        [0.24, 0.52, 0.061],
        [0.3, 0.7, 0.076],
        [0.27, 0.89, 0.057],
      ]
    : [
        [0.77, 0.3, 0.041],
        [0.73, 0.4, 0.071],
        [0.76, 0.53, 0.061],
        [0.7, 0.71, 0.079],
        [0.73, 0.9, 0.055],
      ]
}

function highlightNodes(side: CryEyeSide): readonly TearNode[] {
  return side === 'left'
    ? [
        [0.25, 0.36, 0.012],
        [0.27, 0.46, 0.016],
        [0.27, 0.61, 0.013],
      ]
    : [
        [0.75, 0.37, 0.013],
        [0.73, 0.47, 0.016],
        [0.73, 0.62, 0.012],
      ]
}

function pathCoverage(
  x: number,
  y: number,
  nodes: readonly TearNode[],
  radiusExtra: number,
): number {
  let coverage = 0
  for (let index = 1; index < nodes.length; index += 1) {
    const start = nodes[index - 1]
    const end = nodes[index]
    const dx = end[0] - start[0]
    const dy = end[1] - start[1]
    const lengthSquared = dx * dx + dy * dy
    const progress =
      lengthSquared > 0
        ? clamp(
            ((x - start[0]) * dx + (y - start[1]) * dy) / lengthSquared,
            0,
            1,
          )
        : 0
    const nearestX = start[0] + dx * progress
    const nearestY = start[1] + dy * progress
    const radius = start[2] + (end[2] - start[2]) * progress + radiusExtra
    const distance = Math.hypot(x - nearestX, y - nearestY)
    coverage = Math.max(coverage, clamp((radius - distance) * 120, 0, 1))
  }
  return coverage
}

function compositeBitmap(
  destination: Uint8ClampedArray,
  destinationWidth: number,
  destinationHeight: number,
  source: { width: number; height: number; data: Uint8ClampedArray },
  offsetX: number,
  offsetY: number,
): void {
  for (let y = 0; y < source.height; y += 1) {
    const destinationY = y + offsetY
    if (destinationY < 0 || destinationY >= destinationHeight) continue
    for (let x = 0; x < source.width; x += 1) {
      const destinationX = x + offsetX
      if (destinationX < 0 || destinationX >= destinationWidth) continue
      const sourceOffset = (y * source.width + x) * 4
      const alpha = source.data[sourceOffset + 3] / 255
      if (alpha <= 0) continue
      blendPixel(
        destination,
        (destinationY * destinationWidth + destinationX) * 4,
        {
          red: source.data[sourceOffset],
          green: source.data[sourceOffset + 1],
          blue: source.data[sourceOffset + 2],
        },
        alpha,
      )
    }
  }
}

function blendPixel(
  data: Uint8ClampedArray,
  offset: number,
  color: Readonly<RgbColor>,
  sourceAlpha: number,
): void {
  const source = clamp(sourceAlpha, 0, 1)
  const destination = data[offset + 3] / 255
  const output = source + destination * (1 - source)
  if (output <= 0) return
  const retained = destination * (1 - source)
  data[offset] = Math.round(
    (color.red * source + data[offset] * retained) / output,
  )
  data[offset + 1] = Math.round(
    (color.green * source + data[offset + 1] * retained) / output,
  )
  data[offset + 2] = Math.round(
    (color.blue * source + data[offset + 2] * retained) / output,
  )
  data[offset + 3] = Math.round(output * 255)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function clampInt(value: number, minimum: number, maximum: number): number {
  return Math.round(clamp(value, minimum, maximum))
}
