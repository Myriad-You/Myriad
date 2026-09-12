export interface RgbColor {
  r: number
  g: number
  b: number
}

export interface HslColor {
  h: number
  s: number
  l: number
}

export interface DeriveReadableColorOptions {
  candidates: Array<string | null | undefined>
  isDark: boolean
  targetLightness?: number
  minContrast?: number
  fallback?: string
  backdrop?: RgbColor
}

const LIGHT_BACKDROP: RgbColor = { r: 245, g: 245, b: 245 }
const DARK_BACKDROP: RgbColor = { r: 10, g: 10, b: 10 }

const DEFAULT_FALLBACK = '#8b5cf6'

export function clampNumber(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}

export function normalizeHexColor(value: string | null | undefined): string | null {
  if (!value) return null
  let hex = String(value).trim()
  if (!hex.startsWith('#')) return null
  hex = hex.slice(1)
  if (hex.length === 3) {
    hex =
      hex.charAt(0) +
      hex.charAt(0) +
      hex.charAt(1) +
      hex.charAt(1) +
      hex.charAt(2) +
      hex.charAt(2)
  }
  if (hex.length === 8) {
    hex = hex.slice(0, 6)
  }
  if (hex.length !== 6 || !/^[0-9a-f]+$/i.test(hex)) return null
  return `#${hex.toLowerCase()}`
}

export function hexToRgb(value: string | null | undefined): RgbColor | null {
  const hex = normalizeHexColor(value)
  if (!hex) return null
  const n = Number.parseInt(hex.slice(1), 16)
  return {
    r: (n >> 16) & 255,
    g: (n >> 8) & 255,
    b: n & 255,
  }
}

/** @property <color> computes rgb(); do not hexToRgb. */
export function parseCssColor(
  value: string | null | undefined,
): RgbColor | null {
  if (!value) return null

  const hex = hexToRgb(value)
  if (hex) return hex

  const match = /^rgba?\(([^)]*)\)$/i.exec(String(value).trim())
  if (!match) return null

  const parts = match[1].split(/[,/\s]+/).filter(Boolean)
  if (parts.length < 3) return null

  const channel = (raw: string): number | null => {
    const parsed = Number.parseFloat(raw)
    if (!Number.isFinite(parsed)) return null
    const scaled = raw.trim().endsWith('%') ? (parsed / 100) * 255 : parsed
    return clampNumber(Math.round(scaled), 0, 255)
  }

  const r = channel(parts[0])
  const g = channel(parts[1])
  const b = channel(parts[2])
  if (r === null || g === null || b === null) return null

  return { r, g, b }
}

export function rgbToHex(rgb: RgbColor): string {
  const part = (value: number) => {
    const hex = clampNumber(Math.round(value), 0, 255).toString(16)
    return hex.length === 1 ? `0${hex}` : hex
  }
  return `#${part(rgb.r)}${part(rgb.g)}${part(rgb.b)}`
}

export function rgbToHsl(rgb: RgbColor): HslColor {
  const r = rgb.r / 255
  const g = rgb.g / 255
  const b = rgb.b / 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  let h = 0
  let s = 0
  const l = (max + min) / 2

  if (max !== min) {
    const d = max - min
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
    switch (max) {
      case r:
        h = (g - b) / d + (g < b ? 6 : 0)
        break
      case g:
        h = (b - r) / d + 2
        break
      default:
        h = (r - g) / d + 4
        break
    }
    h /= 6
  }

  return { h, s, l }
}

export function hslToRgb(hsl: HslColor): RgbColor {
  const { h, s, l } = hsl
  let r: number
  let g: number
  let b: number

  if (s === 0) {
    r = g = b = l
  } else {
    const hue2rgb = (p: number, q: number, t: number) => {
      let tt = t
      if (tt < 0) tt += 1
      if (tt > 1) tt -= 1
      if (tt < 1 / 6) return p + (q - p) * 6 * tt
      if (tt < 1 / 2) return q
      if (tt < 2 / 3) return p + (q - p) * (2 / 3 - tt) * 6
      return p
    }

    const q = l < 0.5 ? l * (1 + s) : l + s - l * s
    const p = 2 * l - q
    r = hue2rgb(p, q, h + 1 / 3)
    g = hue2rgb(p, q, h)
    b = hue2rgb(p, q, h - 1 / 3)
  }

  return {
    r: r * 255,
    g: g * 255,
    b: b * 255,
  }
}

export function relativeLuminance(rgb: RgbColor): number {
  const channel = (value: number) => {
    const c = value / 255
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  }
  return (
    channel(rgb.r) * 0.2126 + channel(rgb.g) * 0.7152 + channel(rgb.b) * 0.0722
  )
}

export function contrastRatio(foreground: RgbColor, background: RgbColor): number {
  const a = relativeLuminance(foreground)
  const b = relativeLuminance(background)
  const lighter = Math.max(a, b)
  const darker = Math.min(a, b)
  return (lighter + 0.05) / (darker + 0.05)
}

export function getThemeBackdropRgb(isDark: boolean): RgbColor {
  return isDark ? DARK_BACKDROP : LIGHT_BACKDROP
}

function toColorCandidates(
  value: Array<string | null | undefined> | string | null | undefined,
): Array<string | null | undefined> {
  return Array.isArray(value) ? value : [value]
}

export function pickReadableCandidate(
  candidates: Array<string | null | undefined> | string | null | undefined,
  isDark: boolean,
  minContrast: number,
  backdrop?: RgbColor,
): string | null {
  const bg = backdrop ?? getThemeBackdropRgb(isDark)
  for (const value of toColorCandidates(candidates)) {
    const hex = normalizeHexColor(value)
    const rgb = hexToRgb(hex)
    if (rgb && contrastRatio(rgb, bg) >= minContrast) {
      return hex
    }
  }
  return null
}

export function pickBrightestReadableCandidate(
  candidates: Array<string | null | undefined> | string | null | undefined,
  minContrast: number,
  backdrop: RgbColor,
): string | null {
  let best: { hex: string; luminance: number } | null = null
  for (const value of toColorCandidates(candidates)) {
    const hex = normalizeHexColor(value)
    const rgb = hexToRgb(hex)
    if (!rgb || contrastRatio(rgb, backdrop) < minContrast) continue
    const luminance = relativeLuminance(rgb)
    if (!best || luminance > best.luminance) {
      best = { hex: hex!, luminance }
    }
  }
  return best?.hex ?? null
}

function firstUsableRgb(
  candidates: Array<string | null | undefined> | string | null | undefined,
  fallbackColor: string,
): RgbColor {
  for (const value of toColorCandidates(candidates)) {
    const rgb = hexToRgb(value)
    if (rgb) return rgb
  }
  return hexToRgb(fallbackColor) ?? hexToRgb(DEFAULT_FALLBACK)!
}

function softDarkenFromPrimary(
  primaryHex: string,
  backdrop: RgbColor,
  minContrast: number,
): string {
  const rgb = hexToRgb(primaryHex) ?? hexToRgb(DEFAULT_FALLBACK)!
  const hsl = rgbToHsl(rgb)
  const s = clampNumber(hsl.s * 1.06, 0.3, 0.84)
  const floor = 0.36
  let l = hsl.l > 0.5 ? Math.max(floor, hsl.l - 0.08) : hsl.l
  let candidate = hslToRgb({ h: hsl.h, s, l })
  let guard = 0

  while (
    guard < 40 &&
    l > floor &&
    contrastRatio(candidate, backdrop) < minContrast
  ) {
    l -= 0.014
    candidate = hslToRgb({ h: hsl.h, s, l })
    guard += 1
  }

  let weightSteps = 0
  while (
    weightSteps < 2 &&
    l - 0.014 >= floor &&
    contrastRatio(
      hslToRgb({ h: hsl.h, s, l: l - 0.014 }),
      backdrop,
    ) >= minContrast
  ) {
    l -= 0.014
    candidate = hslToRgb({ h: hsl.h, s, l })
    weightSteps += 1
  }

  return rgbToHex(candidate)
}

export function deriveReadableColor(
  options: DeriveReadableColorOptions,
): string {
  const {
    candidates,
    isDark,
    targetLightness = isDark ? 0.78 : 0.48,
    minContrast = isDark ? 3.7 : 2.7,
    fallback = DEFAULT_FALLBACK,
    backdrop,
  } = options

  const bg = backdrop ?? getThemeBackdropRgb(isDark)

  if (!isDark) {
    const base =
      normalizeHexColor(fallback) ||
      normalizeHexColor(
        toColorCandidates(candidates).find((c) => normalizeHexColor(c)) ?? null,
      ) ||
      DEFAULT_FALLBACK
    const weighted = softDarkenFromPrimary(base, bg, minContrast)
    const weightedRgb = hexToRgb(weighted)
    if (
      weightedRgb &&
      contrastRatio(weightedRgb, bg) >= minContrast * 0.92
    ) {
      return weighted
    }
    const brightest = pickBrightestReadableCandidate(
      candidates,
      minContrast,
      bg,
    )
    return brightest ?? weighted
  }

  const readable = pickReadableCandidate(candidates, isDark, minContrast, bg)
  if (readable) return readable

  const rgb = firstUsableRgb(candidates, fallback)
  const hsl = rgbToHsl(rgb)
  const pull = 0.72
  let l = hsl.l + (targetLightness - hsl.l) * pull
  const s = clampNumber(hsl.s, 0.34, 0.86)
  const step = 0.02
  const limit = 0.94

  let candidate = hslToRgb({ h: hsl.h, s, l })
  let guard = 0

  do {
    candidate = hslToRgb({ h: hsl.h, s, l })
    if (contrastRatio(candidate, bg) >= minContrast) break
    l += step
    guard += 1
  } while (guard < 24 && l <= limit)

  return rgbToHex(candidate)
}

export function deriveAdaptiveTitleColor(isDark: boolean): string {
  if (typeof document === 'undefined') {
    return isDark ? '#ffffff' : '#111111'
  }

  const styles = getComputedStyle(document.documentElement)
  const read = (name: string) => styles.getPropertyValue(name).trim()

  const primary = read('--color-primary') || DEFAULT_FALLBACK
  const secondary = read('--color-secondary') || primary
  const accent = read('--color-accent') || secondary
  const light = read('--color-light') || primary

  let backdrop = getThemeBackdropRgb(isDark)
  const bgPrimary = normalizeHexColor(read('--bg-primary'))
  const bgRgb = hexToRgb(bgPrimary)
  if (bgRgb) backdrop = bgRgb

  if (isDark) {
    return deriveReadableColor({
      candidates: [primary, light, secondary, accent],
      isDark: true,
      targetLightness: 0.78,
      minContrast: 3.7,
      fallback: primary,
      backdrop,
    })
  }

  // Light theme: do not use --color-dark.
  return deriveReadableColor({
    candidates: [primary, secondary, accent, light],
    isDark: false,
    targetLightness: 0.48,
    minContrast: 2.7,
    fallback: primary,
    backdrop,
  })
}
