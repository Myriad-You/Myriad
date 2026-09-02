import type { RgbColor } from './dizzyEye'

export interface ManiacEyeShadowSize {
  width: number
  height: number
}

/** A lower-lid shadow sized from the detected eye rather than the face frame. */
export function maniacEyeShadowGeneratedSize(eye: {
  x0: number
  x1: number
  y0: number
  y1: number
}): ManiacEyeShadowSize {
  const eyeWidth = Math.max(1, eye.x1 - eye.x0)
  const eyeHeight = Math.max(1, eye.y1 - eye.y0)
  return {
    width: clampInt(Math.round(eyeWidth * 0.54), 18, 128),
    height: clampInt(
      Math.round(Math.max(eyeHeight * 0.5, eyeWidth * 0.19)),
      10,
      64,
    ),
  }
}

/**
 * Paints a short, nose-biased lower-eye shadow. Its upper edge stays defined
 * while the lower edge diffuses into the cheek, avoiding a second-eye shape.
 */
export function createManiacEyeShadowBitmap(
  requestedSize: Readonly<ManiacEyeShadowSize>,
  eyelashTint: Readonly<RgbColor>,
  side: 'left' | 'right',
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(Math.round(requestedSize.width), 18, 160)
  const height = clampInt(Math.round(requestedSize.height), 10, 64)
  const data = new Uint8ClampedArray(width * height * 4)
  const color = shadowColor(eyelashTint)
  const direction = side === 'left' ? 1 : -1

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const nx = ((x + 0.5) / width) * 2 - 1
      const ny = (y + 0.5) / height
      const tipWeight = Math.max(0, 1 - Math.abs(nx) ** 1.65)
      if (tipWeight <= 0) continue

      const inward = nx * direction
      // The outer corner sits a little lower than the nose-side corner on
      // both eyes; mirror that lower-lid angle instead of drawing a flat bar.
      const centerY = 0.33 + nx * nx * 0.07 - inward * 0.06
      const upperThickness = 0.082 * tipWeight ** 0.84
      const lowerThickness = (0.28 + inward * 0.026) * tipWeight ** 0.68
      const normalizedDistance =
        ny < centerY
          ? (centerY - ny) / Math.max(0.001, upperThickness)
          : (ny - centerY) / Math.max(0.001, lowerThickness)
      const edgeSoftness = 1.35 / height
      const core = 1 - smoothstep((normalizedDistance - 0.62) / 0.38)
      const halo =
        1 - smoothstep((normalizedDistance - 0.82) / (0.42 + edgeSoftness))
      const grain =
        0.94 +
        Math.sin(x * 0.39 + direction * 0.8) * 0.022 +
        Math.sin(x * 0.16 + 1.5) * 0.015
      const inwardWeight = 0.92 + Math.max(-0.08, inward * 0.08)
      const coverage =
        (core * 0.68 + Math.max(0, halo - core) * 0.18) *
        tipWeight ** 0.38 *
        grain *
        inwardWeight
      if (coverage <= 0) continue

      const offset = (y * width + x) * 4
      data[offset] = color.red
      data[offset + 1] = color.green
      data[offset + 2] = color.blue
      data[offset + 3] = Math.round(clamp(coverage, 0, 1) * 255)
    }
  }

  return { width, height, data }
}

function shadowColor(tint: Readonly<RgbColor>): RgbColor {
  return {
    red: clampInt(tint.red * 0.35 + 80, 68, 136),
    green: clampInt(tint.green * 0.25 + 36, 34, 76),
    blue: clampInt(tint.blue * 0.45 + 82, 78, 148),
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
