/**
 * 封面双色渐变和谐化。
 *
 * 取色常会抽出互补主次色（棕↔蓝、红↔青）。直接铺 135° 渐变时，
 * sRGB 中间段会混出脏灰或过电的「雷霆」色。这里把次色拉回主色邻近色相，
 * 并压一点饱和，让两端像同一束光。
 */

export interface GradientPalette {
  primary: string
  secondary: string
  accent: string
  light: string
  dark: string
}

export interface ColorSample {
  r: number
  g: number
  b: number
  percentage: number
  saturation: number
  brightness: number
  chroma: number
}

interface Hsl {
  h: number
  s: number
  l: number
}

/** Analogous window for a gradient pair (~58°). Beyond this the mid-mix goes muddy. */
const MAX_GRADIENT_HUE = 0.16
/** Past this, treat the candidate as a clash and synthesize a companion. */
const CLASH_HUE = 0.28
const NEAR_GRAY_SATURATION = 0.12
/** Soft cap so the play button doesn't go neon. */
const MAX_STOP_SATURATION = 0.68
const MIN_LIGHTNESS_GAP = 0.1

function wrapHue(h: number): number {
  return ((h % 1) + 1) % 1
}

/** Circular hue distance in [0, 0.5] (0.5 = 180°). */
function hueDistance(a: number, b: number): number {
  const d = Math.abs(a - b)
  return Math.min(d, 1 - d)
}

/** Signed shortest path from `from` to `to` in (-0.5, 0.5]. */
function signedHueDelta(from: number, to: number): number {
  let d = to - from
  if (d > 0.5) d -= 1
  if (d <= -0.5) d += 1
  return d
}

function parseHex(hex: string): { r: number; g: number; b: number } {
  const n = hex.trim().replace('#', '')
  if (n.length === 3) {
    return {
      r: Number.parseInt(n[0] + n[0], 16),
      g: Number.parseInt(n[1] + n[1], 16),
      b: Number.parseInt(n[2] + n[2], 16),
    }
  }
  return {
    r: Number.parseInt(n.slice(0, 2), 16) || 0,
    g: Number.parseInt(n.slice(2, 4), 16) || 0,
    b: Number.parseInt(n.slice(4, 6), 16) || 0,
  }
}

function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
  const toHex = (v: number) => clamp(v).toString(16).padStart(2, '0')
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

function rgbToHsl(r: number, g: number, b: number): Hsl {
  r /= 255
  g /= 255
  b /= 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const delta = max - min
  const l = (max + min) / 2

  let h = 0
  let s = 0
  if (delta !== 0) {
    s = l > 0.5 ? delta / (2 - max - min) : delta / (max + min)
    if (max === r) h = ((g - b) / delta + (g < b ? 6 : 0)) / 6
    else if (max === g) h = ((b - r) / delta + 2) / 6
    else h = ((r - g) / delta + 4) / 6
  }
  return { h, s, l }
}

function hslToRgb(h: number, s: number, l: number): {
  r: number
  g: number
  b: number
} {
  if (s === 0) {
    const v = Math.round(l * 255)
    return { r: v, g: v, b: v }
  }

  const hue2rgb = (p: number, q: number, t: number) => {
    if (t < 0) t += 1
    if (t > 1) t -= 1
    if (t < 1 / 6) return p + (q - p) * 6 * t
    if (t < 1 / 2) return q
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6
    return p
  }

  const q = l < 0.5 ? l * (1 + s) : l + s - l * s
  const p = 2 * l - q

  return {
    r: Math.round(hue2rgb(p, q, h + 1 / 3) * 255),
    g: Math.round(hue2rgb(p, q, h) * 255),
    b: Math.round(hue2rgb(p, q, h - 1 / 3) * 255),
  }
}

function getPerceptualBrightness(r: number, g: number, b: number): number {
  return 0.299 * r + 0.587 * g + 0.114 * b
}

function getSaturation(r: number, g: number, b: number): number {
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  return max === 0 ? 0 : (max - min) / max
}

function getChroma(r: number, g: number, b: number): number {
  return Math.max(r, g, b) - Math.min(r, g, b)
}

function lightenColor(r: number, g: number, b: number): string {
  const hsl = rgbToHsl(r, g, b)
  hsl.l = Math.min(0.85, hsl.l + 0.2)
  hsl.s = Math.min(1, hsl.s * 1.1)
  const rgb = hslToRgb(hsl.h, hsl.s, hsl.l)
  return rgbToHex(rgb.r, rgb.g, rgb.b)
}

function darkenColor(r: number, g: number, b: number): string {
  const hsl = rgbToHsl(r, g, b)
  hsl.l = Math.max(0.15, hsl.l - 0.25)
  hsl.s = Math.min(1, hsl.s * 1.15)
  const rgb = hslToRgb(hsl.h, hsl.s, hsl.l)
  return rgbToHex(rgb.r, rgb.g, rgb.b)
}

/** Hue delta in degrees [0, 180]. */
export function hexHueDelta(a: string, b: string): number {
  const A = parseHex(a)
  const B = parseHex(b)
  return hueDistance(rgbToHsl(A.r, A.g, A.b).h, rgbToHsl(B.r, B.g, B.b).h) * 360
}

function colorSampleFromHsl(hsl: Hsl, percentage = 0): ColorSample {
  const rgb = hslToRgb(hsl.h, hsl.s, hsl.l)
  return {
    r: rgb.r,
    g: rgb.g,
    b: rgb.b,
    percentage,
    saturation: getSaturation(rgb.r, rgb.g, rgb.b),
    brightness: getPerceptualBrightness(rgb.r, rgb.g, rgb.b),
    chroma: getChroma(rgb.r, rgb.g, rgb.b),
  }
}

function synthesizeCompanionHsl(anchor: Hsl): Hsl {
  const h = wrapHue(anchor.h + 0.048)
  const s =
    anchor.s < NEAR_GRAY_SATURATION ? anchor.s : Math.min(0.6, anchor.s * 0.82)
  const l =
    anchor.l < 0.48
      ? Math.min(0.64, anchor.l + 0.16)
      : Math.max(0.26, anchor.l - 0.16)
  return { h, s, l }
}

function companionScore(primary: ColorSample, candidate: ColorSample): number {
  const p = rgbToHsl(primary.r, primary.g, primary.b)
  const c = rgbToHsl(candidate.r, candidate.g, candidate.b)
  const hd = hueDistance(p.h, c.h)
  const lightGap = Math.abs(p.l - c.l)

  if (p.s < NEAR_GRAY_SATURATION || c.s < NEAR_GRAY_SATURATION) {
    return candidate.percentage + lightGap * 40
  }

  let score = candidate.percentage
  if (hd <= MAX_GRADIENT_HUE) score += 50 + lightGap * 35
  else if (hd <= CLASH_HUE) score += 8
  else score -= (hd - CLASH_HUE) * 180

  if (p.s > 0.7 && c.s > 0.7 && hd > MAX_GRADIENT_HUE) score -= 30
  return score
}

export function pickGradientCompanion(
  primary: ColorSample,
  candidates: ColorSample[],
): ColorSample {
  let best: ColorSample | null = null
  let bestScore = Number.NEGATIVE_INFINITY
  for (const candidate of candidates) {
    if (
      candidate.r === primary.r &&
      candidate.g === primary.g &&
      candidate.b === primary.b
    ) {
      continue
    }
    const score = companionScore(primary, candidate)
    if (score > bestScore) {
      best = candidate
      bestScore = score
    }
  }
  if (best && bestScore >= 12) return best
  return colorSampleFromHsl(
    synthesizeCompanionHsl(rgbToHsl(primary.r, primary.g, primary.b)),
  )
}

function softenAnchor(hsl: Hsl): Hsl {
  let { s } = hsl
  if (s > 0.82) s = 0.72 + (s - 0.82) * 0.35
  s = Math.min(s, 0.78)
  return { h: hsl.h, s, l: hsl.l }
}

function fitStopToAnchor(anchor: Hsl, stop: Hsl): Hsl {
  if (anchor.s < NEAR_GRAY_SATURATION && stop.s < NEAR_GRAY_SATURATION) {
    let l = stop.l
    if (Math.abs(l - anchor.l) < MIN_LIGHTNESS_GAP) {
      l =
        anchor.l < 0.5
          ? Math.min(0.72, anchor.l + MIN_LIGHTNESS_GAP)
          : Math.max(0.22, anchor.l - MIN_LIGHTNESS_GAP)
    }
    return { h: stop.h, s: stop.s, l }
  }

  let h = stop.h
  let s = stop.s
  let l = stop.l
  const hd = hueDistance(anchor.h, h)
  if (hd > MAX_GRADIENT_HUE) {
    const dir = signedHueDelta(anchor.h, h)
    const sign = dir === 0 ? 1 : Math.sign(dir)
    h = wrapHue(anchor.h + sign * 0.055)
  }
  s = Math.min(s, anchor.s * 0.92, MAX_STOP_SATURATION)
  if (Math.abs(l - anchor.l) < MIN_LIGHTNESS_GAP) {
    l =
      anchor.l < 0.5
        ? Math.min(0.72, anchor.l + MIN_LIGHTNESS_GAP)
        : Math.max(0.22, anchor.l - MIN_LIGHTNESS_GAP)
  }
  return { h, s, l }
}

export function harmonizeGradientPalette(
  palette: GradientPalette,
): GradientPalette {
  const pRgb = parseHex(palette.primary)
  const sRgb = parseHex(palette.secondary)
  const aRgb = parseHex(palette.accent)
  const pHsl = softenAnchor(rgbToHsl(pRgb.r, pRgb.g, pRgb.b))
  const sHsl = fitStopToAnchor(pHsl, rgbToHsl(sRgb.r, sRgb.g, sRgb.b))
  const aHsl = fitStopToAnchor(pHsl, rgbToHsl(aRgb.r, aRgb.g, aRgb.b))
  const primary = hslToRgb(pHsl.h, pHsl.s, pHsl.l)
  const secondary = hslToRgb(sHsl.h, sHsl.s, sHsl.l)
  const accent = hslToRgb(aHsl.h, aHsl.s, aHsl.l)

  return {
    primary: rgbToHex(primary.r, primary.g, primary.b),
    secondary: rgbToHex(secondary.r, secondary.g, secondary.b),
    accent: rgbToHex(accent.r, accent.g, accent.b),
    light: lightenColor(primary.r, primary.g, primary.b),
    dark: darkenColor(primary.r, primary.g, primary.b),
  }
}
