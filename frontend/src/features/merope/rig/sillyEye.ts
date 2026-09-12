import type { RgbColor } from './dizzyEye'

export type SillyEyeSide = 'left' | 'right'

export interface SillyEyeSize {
  width: number
  height: number
  iris: number
}

export interface SillyEyePalette {
  outline: RgbColor
  sclera: RgbColor
  scleraShade: RgbColor
  iris: RgbColor
  irisLight: RgbColor
  irisDark: RgbColor
  pupil: RgbColor
}

const FALLBACK_SCLERA = { red: 250, green: 247, blue: 249 }
const FALLBACK_IRIS = { red: 102, green: 78, blue: 188 }
const SCLERA_RADIUS_X = 0.94
const SCLERA_RADIUS_Y = 0.985
const RIM_THICKNESS = 3.2
export const SILLY_IRIS_REST_SHARE = 0.45

export function sillyEyeGeneratedSize(eye: {
  x0: number
  x1: number
  y0: number
  y1: number
}): SillyEyeSize {
  const eyeWidth = Math.max(1, eye.x1 - eye.x0)
  const eyeHeight = Math.max(1, eye.y1 - eye.y0)
  const width = clampInt(Math.round(eyeWidth * 1.12), 28, 240)
  const preferredHeight = Math.max(eyeHeight * 1.72, width * 0.82)
  const height = clampInt(
    Math.round(Math.min(preferredHeight, width * 1.04)),
    28,
    240,
  )
  return {
    width,
    height,
    iris: clampInt(Math.round(Math.min(width, height) * 0.54), 18, 168),
  }
}

export function sillyIrisTravelRoom(size: Readonly<SillyEyeSize>): {
  x: number
  y: number
} {
  const half = size.iris / 2
  return {
    x: Math.max(0, (size.width * SCLERA_RADIUS_X) / 2 - RIM_THICKNESS - half),
    y: Math.max(0, (size.height * SCLERA_RADIUS_Y) / 2 - RIM_THICKNESS - half),
  }
}

export function sampleSillyEyePalette(
  outline: Readonly<RgbColor>,
  irisPixels?: Uint8ClampedArray,
  scleraPixels?: Uint8ClampedArray,
): SillyEyePalette {
  const iris = mixColor(
    sampleChromaWeightedColor(irisPixels) ?? FALLBACK_IRIS,
    FALLBACK_IRIS,
    0.14,
  )
  const sclera = mixColor(
    sampleLightNeutralColor(scleraPixels) ?? FALLBACK_SCLERA,
    FALLBACK_SCLERA,
    0.16,
  )
  return {
    outline: { ...outline },
    sclera,
    scleraShade: mixColor(sclera, iris, 0.1),
    iris,
    irisLight: mixColor(iris, sclera, 0.34),
    irisDark: mixColor(iris, outline, 0.58),
    pupil: mixColor(outline, { red: 6, green: 5, blue: 12 }, 0.46),
  }
}

export function createSillyEyeWhiteBitmap(
  requestedSize: Readonly<SillyEyeSize>,
  palette: Readonly<SillyEyePalette>,
  side: SillyEyeSide,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(Math.round(requestedSize.width), 28, 240)
  const height = clampInt(Math.round(requestedSize.height), 28, 240)
  const data = new Uint8ClampedArray(width * height * 4)
  const minimumSpan = Math.max(1, Math.min(width, height))
  const samples: readonly (readonly [number, number])[] = [
    [0.25, 0.25],
    [0.75, 0.25],
    [0.25, 0.75],
    [0.75, 0.75],
  ]
  const rotation = side === 'left' ? -0.035 : 0.028
  const cosine = Math.cos(rotation)
  const sine = Math.sin(rotation)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let outlineCoverage = 0
      let fillCoverage = 0
      for (const [offsetX, offsetY] of samples) {
        const nx = ((x + offsetX) / width - 0.5) * 2
        const ny = ((y + offsetY) / height - 0.5) * 2
        const rx = nx * cosine - ny * sine
        const ry = nx * sine + ny * cosine
        const organicEdge =
          Math.sin(rx * 5.1 + ry * 3.3 + (side === 'left' ? 0.4 : 1.5)) * 0.009
        const radius = Math.hypot(
          rx / SCLERA_RADIUS_X,
          ry / (SCLERA_RADIUS_Y + organicEdge),
        )
        const insidePixels = (1 - radius) * minimumSpan * 0.5
        outlineCoverage += clamp(insidePixels + 0.65, 0, 1) * 0.25
        const upperStroke = ry < -0.15 ? RIM_THICKNESS : 2.35
        fillCoverage += clamp(insidePixels - upperStroke + 0.65, 0, 1) * 0.25
      }
      if (outlineCoverage <= 0) continue
      const offset = (y * width + x) * 4
      paint(data, offset, palette.outline, outlineCoverage)
      if (fillCoverage > 0) {
        const normalizedY = (y + 0.5) / height
        const lowerShade = smoothstep((normalizedY - 0.58) / 0.34) * 0.62
        paint(
          data,
          offset,
          mixColor(palette.sclera, palette.scleraShade, lowerShade),
          fillCoverage,
        )
      }
    }
  }
  return { width, height, data }
}

export function createSillyIrisFromArtwork(
  source: Readonly<{
    data: Uint8ClampedArray
    width: number
    height: number
  }>,
  requestedSize: number,
): { width: number; height: number; data: Uint8ClampedArray } | null {
  const bounds = alphaBounds(source)
  if (!bounds) return null
  const edge = clampInt(Math.round(requestedSize), 18, 168)
  const data = new Uint8ClampedArray(edge * edge * 4)
  const artWidth = bounds.x1 - bounds.x0 + 1
  const artHeight = bounds.y1 - bounds.y0 + 1
  const span = Math.max(artWidth, artHeight)
  const originX = bounds.x0 + (artWidth - span) / 2
  const originY = bounds.y0 + (artHeight - span) / 2
  for (let y = 0; y < edge; y += 1) {
    for (let x = 0; x < edge; x += 1) {
      sampleBilinear(
        source,
        originX + ((x + 0.5) / edge) * span - 0.5,
        originY + ((y + 0.5) / edge) * span - 0.5,
        data,
        (y * edge + x) * 4,
      )
    }
  }
  return { width: edge, height: edge, data }
}

export function createSillyIrisBitmap(
  requestedSize: number,
  palette: Readonly<SillyEyePalette>,
  side: SillyEyeSide,
): { width: number; height: number; data: Uint8ClampedArray } {
  const edge = clampInt(Math.round(requestedSize), 18, 168)
  const data = new Uint8ClampedArray(edge * edge * 4)
  const center = (edge - 1) / 2
  const radius = edge * 0.48
  for (let y = 0; y < edge; y += 1) {
    for (let x = 0; x < edge; x += 1) {
      const nx = (x + 0.5 - center) / radius
      const ny = (y + 0.5 - center) / radius
      const distance = Math.hypot(nx, ny)
      const coverage = clamp((1 - distance) * radius + 0.7, 0, 1)
      if (coverage <= 0) continue
      const offset = (y * edge + x) * 4
      const edgeMix = smoothstep((distance - 0.75) / 0.22)
      const verticalLight = 1 - smoothstep((ny + 0.45) / 1.18)
      const irisTone = mixColor(
        mixColor(palette.irisDark, palette.irisLight, verticalLight * 0.82),
        palette.outline,
        edgeMix * 0.76,
      )
      paint(data, offset, irisTone, coverage)

      const pupilX = nx + (side === 'left' ? 0.025 : -0.02)
      const pupilY = ny - 0.06
      const pupilDistance = Math.hypot(pupilX / 0.52, pupilY / 0.58)
      const pupilCoverage = clamp((1 - pupilDistance) * radius + 0.7, 0, 1)
      if (pupilCoverage > 0) {
        paint(data, offset, palette.pupil, pupilCoverage * coverage)
      }

      const lowerRim =
        smoothstep((distance - 0.66) / 0.18) *
        (1 - smoothstep((distance - 0.92) / 0.08)) *
        smoothstep((ny - 0.12) / 0.48)
      if (lowerRim > 0) {
        paint(data, offset, palette.irisLight, lowerRim * coverage * 0.74)
      }

      const highlightLarge = ellipseCoverage(
        nx,
        ny,
        side === 'left' ? -0.27 : -0.22,
        -0.34,
        0.16,
        0.2,
        radius,
      )
      const highlightSmall = ellipseCoverage(
        nx,
        ny,
        0.11,
        -0.48,
        0.075,
        0.095,
        radius,
      )
      const highlight = Math.max(highlightLarge, highlightSmall)
      if (highlight > 0) {
        paint(data, offset, palette.sclera, highlight * coverage * 0.9)
      }
    }
  }
  return { width: edge, height: edge, data }
}

function alphaBounds(
  source: Readonly<{
    data: Uint8ClampedArray
    width: number
    height: number
  }>,
): { x0: number; y0: number; x1: number; y1: number } | null {
  let x0 = source.width
  let y0 = source.height
  let x1 = -1
  let y1 = -1
  for (let y = 0; y < source.height; y += 1) {
    for (let x = 0; x < source.width; x += 1) {
      if (source.data[(y * source.width + x) * 4 + 3] < 12) continue
      if (x < x0) x0 = x
      if (x > x1) x1 = x
      if (y < y0) y0 = y
      if (y > y1) y1 = y
    }
  }
  return x1 < x0 || y1 < y0 ? null : { x0, y0, x1, y1 }
}

/** Alpha-weighted so resampling never drags an opaque halo into the edge. */
function sampleBilinear(
  source: Readonly<{
    data: Uint8ClampedArray
    width: number
    height: number
  }>,
  x: number,
  y: number,
  output: Uint8ClampedArray,
  offset: number,
): void {
  const baseX = Math.floor(x)
  const baseY = Math.floor(y)
  const fractionX = x - baseX
  const fractionY = y - baseY
  let red = 0
  let green = 0
  let blue = 0
  let alpha = 0
  for (let stepY = 0; stepY <= 1; stepY += 1) {
    for (let stepX = 0; stepX <= 1; stepX += 1) {
      const weight =
        (stepX ? fractionX : 1 - fractionX) *
        (stepY ? fractionY : 1 - fractionY)
      if (weight <= 0) continue
      const sourceX = Math.max(0, Math.min(source.width - 1, baseX + stepX))
      const sourceY = Math.max(0, Math.min(source.height - 1, baseY + stepY))
      const index = (sourceY * source.width + sourceX) * 4
      const sourceAlpha = (source.data[index + 3] / 255) * weight
      red += source.data[index] * sourceAlpha
      green += source.data[index + 1] * sourceAlpha
      blue += source.data[index + 2] * sourceAlpha
      alpha += sourceAlpha
    }
  }
  if (alpha <= 0.0001) return
  output[offset] = Math.round(red / alpha)
  output[offset + 1] = Math.round(green / alpha)
  output[offset + 2] = Math.round(blue / alpha)
  output[offset + 3] = Math.round(alpha * 255)
}

function ellipseCoverage(
  x: number,
  y: number,
  centerX: number,
  centerY: number,
  radiusX: number,
  radiusY: number,
  pixelScale: number,
): number {
  const distance = Math.hypot((x - centerX) / radiusX, (y - centerY) / radiusY)
  return clamp(
    (1 - distance) * pixelScale * Math.min(radiusX, radiusY) + 0.7,
    0,
    1,
  )
}

function sampleChromaWeightedColor(
  pixels: Uint8ClampedArray | undefined,
): RgbColor | null {
  if (!pixels) return null
  let red = 0
  let green = 0
  let blue = 0
  let total = 0
  for (let index = 0; index + 3 < pixels.length; index += 4) {
    const alpha = pixels[index + 3] / 255
    if (alpha < 0.1) continue
    const maximum = Math.max(
      pixels[index],
      pixels[index + 1],
      pixels[index + 2],
    )
    const minimum = Math.min(
      pixels[index],
      pixels[index + 1],
      pixels[index + 2],
    )
    const chroma = maximum - minimum
    if (chroma < 14) continue
    const luminance =
      pixels[index] * 0.299 +
      pixels[index + 1] * 0.587 +
      pixels[index + 2] * 0.114
    const weight =
      alpha * (chroma / 255) ** 1.35 * (0.3 + (1 - luminance / 255) * 0.7)
    red += pixels[index] * weight
    green += pixels[index + 1] * weight
    blue += pixels[index + 2] * weight
    total += weight
  }
  return total > 0.001
    ? { red: red / total, green: green / total, blue: blue / total }
    : null
}

function sampleLightNeutralColor(
  pixels: Uint8ClampedArray | undefined,
): RgbColor | null {
  if (!pixels) return null
  let red = 0
  let green = 0
  let blue = 0
  let total = 0
  for (let index = 0; index + 3 < pixels.length; index += 4) {
    const alpha = pixels[index + 3] / 255
    if (alpha < 0.1) continue
    const maximum = Math.max(
      pixels[index],
      pixels[index + 1],
      pixels[index + 2],
    )
    const minimum = Math.min(
      pixels[index],
      pixels[index + 1],
      pixels[index + 2],
    )
    const luminance =
      pixels[index] * 0.299 +
      pixels[index + 1] * 0.587 +
      pixels[index + 2] * 0.114
    if (luminance < 150 || maximum - minimum > 72) continue
    const weight = alpha * (0.3 + luminance / 255)
    red += pixels[index] * weight
    green += pixels[index + 1] * weight
    blue += pixels[index + 2] * weight
    total += weight
  }
  return total > 0.001
    ? { red: red / total, green: green / total, blue: blue / total }
    : null
}

function paint(
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
