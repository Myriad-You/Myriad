export interface RgbaLayerImage {
  width: number
  height: number
  data: Uint8ClampedArray
}

export interface ClosedEyeCompensationPart {
  name: string
  x: number
  y: number
  w: number
  h: number
  side: 'L' | 'R' | null
  synthetic?: boolean
  img: RgbaLayerImage
}

const ANGLE_TOLERANCE_DEGREES = 2
const MAX_CORRECTION_DEGREES = 10
const CLOSED_EYE_VERTICAL_ALIGNMENT = 0.55

/**
 * Correct only generated close-eye diffs against their own open eyelashes.
 * Authored close-eye layers and the opposite eye are never used as references.
 */
export function compensateSyntheticClosedEyeAngles(
  parts: ClosedEyeCompensationPart[],
): void {
  for (const side of ['L', 'R'] as const) {
    const closed = parts.find(
      (part) =>
        part.synthetic === true &&
        part.side === side &&
        part.name.startsWith('eye_close'),
    )
    const open = parts.find(
      (part) => part.side === side && part.name.startsWith('eyelash'),
    )
    if (!closed || !open) continue
    const closedAngle = rgbaPrincipalAngleDegrees(closed.img)
    const openAngle = rgbaPrincipalAngleDegrees(open.img)
    if (closedAngle === null || openAngle === null) continue
    const error = principalAngleDelta(openAngle, closedAngle)
    if (Math.abs(error) <= ANGLE_TOLERANCE_DEGREES) continue
    const correction = clamp(
      error,
      -MAX_CORRECTION_DEGREES,
      MAX_CORRECTION_DEGREES,
    )
    rotatePartAroundPlacementAnchor(closed, correction)
  }
}

export function rgbaPrincipalAngleDegrees(
  source: RgbaLayerImage,
): number | null {
  let alphaWeight = 0
  let weightedX = 0
  let weightedY = 0
  for (let y = 0; y < source.height; y += 1) {
    for (let x = 0; x < source.width; x += 1) {
      const alpha = source.data[(y * source.width + x) * 4 + 3]
      alphaWeight += alpha
      weightedX += x * alpha
      weightedY += y * alpha
    }
  }
  if (alphaWeight === 0) return null
  const centerX = weightedX / alphaWeight
  const centerY = weightedY / alphaWeight
  let xx = 0
  let yy = 0
  let xy = 0
  for (let y = 0; y < source.height; y += 1) {
    for (let x = 0; x < source.width; x += 1) {
      const alpha = source.data[(y * source.width + x) * 4 + 3]
      const offsetX = x - centerX
      const offsetY = y - centerY
      xx += alpha * offsetX * offsetX
      yy += alpha * offsetY * offsetY
      xy += alpha * offsetX * offsetY
    }
  }
  return (Math.atan2(2 * xy, xx - yy) * 90) / Math.PI
}

function rotatePartAroundPlacementAnchor(
  part: ClosedEyeCompensationPart,
  degrees: number,
): void {
  const centerX = part.x + part.w / 2
  const closeY = part.y + part.h * CLOSED_EYE_VERTICAL_ALIGNMENT
  const image = rotateRgba(part.img, degrees)
  part.img = image
  part.w = image.width
  part.h = image.height
  part.x = Math.round(centerX - image.width / 2)
  part.y = Math.round(closeY - image.height * CLOSED_EYE_VERTICAL_ALIGNMENT)
}

/** Positive degrees rotate clockwise in canvas coordinates. */
function rotateRgba(source: RgbaLayerImage, degrees: number): RgbaLayerImage {
  const radians = (degrees * Math.PI) / 180
  const cosine = Math.cos(radians)
  const sine = Math.sin(radians)
  const width = Math.ceil(
    Math.abs(source.width * cosine) + Math.abs(source.height * sine),
  )
  const height = Math.ceil(
    Math.abs(source.width * sine) + Math.abs(source.height * cosine),
  )
  const data = new Uint8ClampedArray(width * height * 4)
  const sourceCenterX = (source.width - 1) / 2
  const sourceCenterY = (source.height - 1) / 2
  const targetCenterX = (width - 1) / 2
  const targetCenterY = (height - 1) / 2
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const targetX = x - targetCenterX
      const targetY = y - targetCenterY
      const sourceX = targetX * cosine + targetY * sine + sourceCenterX
      const sourceY = -targetX * sine + targetY * cosine + sourceCenterY
      sampleBilinear(source, sourceX, sourceY, data, (y * width + x) * 4)
    }
  }
  return trimTransparentRgba({ width, height, data })
}

function sampleBilinear(
  source: RgbaLayerImage,
  x: number,
  y: number,
  target: Uint8ClampedArray,
  targetOffset: number,
): void {
  if (x < 0 || y < 0 || x > source.width - 1 || y > source.height - 1) return
  const x0 = Math.floor(x)
  const y0 = Math.floor(y)
  const x1 = Math.min(source.width - 1, x0 + 1)
  const y1 = Math.min(source.height - 1, y0 + 1)
  const xWeight = x - x0
  const yWeight = y - y0
  const topLeft = (y0 * source.width + x0) * 4
  const topRight = (y0 * source.width + x1) * 4
  const bottomLeft = (y1 * source.width + x0) * 4
  const bottomRight = (y1 * source.width + x1) * 4
  for (let channel = 0; channel < 4; channel += 1) {
    target[targetOffset + channel] =
      source.data[topLeft + channel] * (1 - xWeight) * (1 - yWeight) +
      source.data[topRight + channel] * xWeight * (1 - yWeight) +
      source.data[bottomLeft + channel] * (1 - xWeight) * yWeight +
      source.data[bottomRight + channel] * xWeight * yWeight
  }
}

function trimTransparentRgba(source: RgbaLayerImage): RgbaLayerImage {
  let left = source.width
  let top = source.height
  let right = -1
  let bottom = -1
  for (let y = 0; y < source.height; y += 1) {
    for (let x = 0; x < source.width; x += 1) {
      if (source.data[(y * source.width + x) * 4 + 3] <= 1) continue
      left = Math.min(left, x)
      top = Math.min(top, y)
      right = Math.max(right, x)
      bottom = Math.max(bottom, y)
    }
  }
  if (right < left || bottom < top) return source
  const width = right - left + 1
  const height = bottom - top + 1
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const sourceStart = ((top + y) * source.width + left) * 4
    data.set(
      source.data.subarray(sourceStart, sourceStart + width * 4),
      y * width * 4,
    )
  }
  return { width, height, data }
}

function principalAngleDelta(target: number, current: number): number {
  let delta = target - current
  while (delta > 90) delta -= 180
  while (delta < -90) delta += 180
  return delta
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
