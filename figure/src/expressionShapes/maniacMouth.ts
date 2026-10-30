import type { MouthExpressionKind, MouthExpressionPalette, MouthExpressionSize, Point, Rgb } from './mouthShared'
import { paint, pointInPolygon, pointToPolylineDistance } from './mouthPaths'
import { clamp, clampInt, mixColor } from './mouthShared'

const MANIAC_TONGUE_HIGHLIGHT_RAIL: readonly Point[] = [
  [-0.49, 0.68],
  [-0.43, 0.79],
  [-0.32, 0.87],
  [-0.18, 0.91],
]

export function createManiacMouthShadowBitmap(
  requestedSize: Readonly<MouthExpressionSize>,
  palette: Readonly<MouthExpressionPalette>,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(Math.round(requestedSize.width), 16, 360)
  const height = clampInt(Math.round(requestedSize.height), 12, 300)
  const data = new Uint8ClampedArray(width * height * 4)
  const right: Point[] = [
    [0.728, -0.832],
    [0.976, -0.816],
    [1.078, -0.657],
    [0.976, -0.508],
    [0.874, -0.6],
    [0.965, -0.669],
    [0.758, -0.713],
  ]
  const shadows = [right.map(([x, y]): Point => [-x, y]), right]
  const color = mixColor(palette.cavity, palette.fill, 0.2)
  const lowerLipShadowColor = mixColor(palette.line, palette.fill, 0.38)
  const samples: readonly Point[] = [
    [0.2, 0.2],
    [0.8, 0.2],
    [0.2, 0.8],
    [0.8, 0.8],
  ]
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let coverage = 0
      for (const [offsetX, offsetY] of samples) {
        const px = ((x + offsetX) / width - 0.5) * 2.2
        const py = ((y + offsetY) / height - 0.5) * 2.2
        if (shadows.some((shadow) => pointInPolygon(px, py, shadow))) {
          coverage += 0.25
        }
      }
      const px = ((x + 0.5) / width - 0.5) * 2.2
      const py = ((y + 0.5) / height - 0.5) * 2.2
      const lowerLipShadow = maniacLowerLipShadowAlpha(px, py)
      if (coverage <= 0 && lowerLipShadow <= 0) continue
      const offset = (y * width + x) * 4
      if (lowerLipShadow > 0) {
        paint(data, offset, lowerLipShadowColor, lowerLipShadow)
      }
      if (coverage > 0) paint(data, offset, color, coverage * 0.18)
    }
  }
  return { width, height, data }
}

export function tongueBoundary(kind: MouthExpressionKind, x: number): number {
  if (kind === 'round') return 0.3 - 0.12 * (1 - x * x)
  if (kind === 'wide') return 0.2 - 0.12 * (1 - x * x)
  if (kind === 'maniac') {
    const normalizedX = clamp(Math.abs(x) / 0.9, 0, 1)
    return -0.08 - 0.18 * (1 - normalizedX * normalizedX)
  }
  if (kind === 'silly') return 0.16 - 0.14 * (1 - x * x)
  return 0.24 - 0.16 * (1 - x * x)
}

export function maniacTongueShadeDepth(x: number): number {
  const normalizedX = clamp(Math.abs(x) / 0.9, 0, 1)
  return 0.17 - normalizedX * normalizedX * 0.07
}

function maniacLowerLipShadowAlpha(x: number, y: number): number {
  const normalizedX = (x + 0.025) / 0.53
  const normalizedY = (y - 0.94) / 0.14
  const radius = Math.hypot(normalizedX, normalizedY)
  if (radius >= 1) return 0
  return (1 - smootherstep((radius - 0.45) / 0.55)) * 0.14
}

export function maniacOutlineTone(
  x: number,
  y: number,
  palette: Readonly<MouthExpressionPalette>,
): Rgb {
  const lowerSoftness = smootherstep((y - 0.18) / 0.72) * 0.13
  const highlightDistance = pointToPolylineDistance(
    x,
    y,
    MANIAC_TONGUE_HIGHLIGHT_RAIL,
  )
  const leftRimLight =
    (1 - smootherstep((highlightDistance - 0.015) / 0.07)) * 0.075
  return mixColor(
    palette.line,
    palette.fill,
    clamp(lowerSoftness + leftRimLight, 0, 0.19),
  )
}

export function maniacCavityTone(
  x: number,
  y: number,
  palette: Readonly<MouthExpressionPalette>,
): Rgb {
  const lowerWarmth = smootherstep((y + 0.63) / 0.52) * 0.075
  const centerDepth =
    (1 - smootherstep(Math.abs(x) / 0.82)) *
    (1 - smootherstep((y + 0.7) / 0.62)) *
    0.055
  return shadeColor(
    mixColor(palette.cavity, palette.fill, lowerWarmth),
    centerDepth,
  )
}

export function maniacTongueTone(
  x: number,
  y: number,
  palette: Readonly<MouthExpressionPalette>,
): Rgb {
  const depth = smootherstep((y + 0.1) / 0.95)
  const centerLight =
    (1 - smootherstep(Math.abs(x) / 0.74)) * (0.035 + depth * 0.055)
  const edgeShade = smootherstep((Math.abs(x) - 0.46) / 0.3) * 0.09
  const highlightDistance = pointToPolylineDistance(
    x,
    y,
    MANIAC_TONGUE_HIGHLIGHT_RAIL,
  )
  const rimHighlight =
    (1 - smootherstep((highlightDistance - 0.012) / 0.065)) *
    smootherstep((y - 0.55) / 0.16) *
    0.17
  const lit = mixColor(
    palette.fill,
    { red: 255, green: 236, blue: 241 },
    clamp(centerLight + rimHighlight, 0, 0.24),
  )
  return mixColor(lit, palette.cavity, edgeShade)
}

function shadeColor(color: Readonly<Rgb>, amount: number): Rgb {
  const retained = 1 - clamp(amount, 0, 1)
  return {
    red: Math.round(color.red * retained),
    green: Math.round(color.green * retained),
    blue: Math.round(color.blue * retained),
  }
}

function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded ** 3 * (bounded * (bounded * 6 - 15) + 10)
}
