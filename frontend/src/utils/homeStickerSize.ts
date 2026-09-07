import type { WidgetSize } from '../components/widgetGridTypes'
import {
  STICKER_EXTRA_SIZE_KEYS,
  WIDGET_SIZE_KEYS,
  widgetSizeSpan,
} from './widgetSizeScale'

/**
 * Named sticker tiles plus extras. Generation maps to 1:1 / 3:2 / 2:3 pixels;
 * the placed tile is the cells the user drew.
 */
export const HOME_STICKER_SIZES: readonly WidgetSize[] = [
  ...WIDGET_SIZE_KEYS,
  ...STICKER_EXTRA_SIZE_KEYS,
]

/** Free canvas bounds (same as HOME_STANDARD_COLS × HOME_FREE_ROWS). */
const STICKER_MAX_COLS = 16
const STICKER_MAX_ROWS = 8

const STICKER_ASPECT_FAMILIES: readonly {
  key: string
  aspect: number
  sizes: readonly WidgetSize[]
}[] = [
  { key: '1:1', aspect: 1, sizes: ['1x1', '2x2', '3x3', '4x4'] },
  { key: '2:1', aspect: 2, sizes: ['2x1', '4x2'] },
  { key: '1:2', aspect: 0.5, sizes: ['1x2', '2x4'] },
  { key: '3:2', aspect: 1.5, sizes: ['3x2'] },
  { key: '2:3', aspect: 2 / 3, sizes: ['2x3'] },
  { key: '4:1', aspect: 4, sizes: ['4x1'] },
  { key: '4:3', aspect: 4 / 3, sizes: ['4x3', '8x6'] },
  { key: '3:4', aspect: 0.75, sizes: ['3x4', '6x8'] },
  { key: '16:9', aspect: 16 / 9, sizes: ['7x4', '9x5', '14x8'] },
  { key: '9:16', aspect: 9 / 16, sizes: ['4x7'] },
]

const GENERATE_PIXEL_PRESETS = [
  { aspect: 1, width: 1024, height: 1024 },
  { aspect: 3 / 2, width: 1536, height: 1024 },
  { aspect: 2 / 3, width: 1024, height: 1536 },
] as const

function familyForSize(size: string) {
  return STICKER_ASPECT_FAMILIES.find((family) =>
    (family.sizes as readonly string[]).includes(size),
  )
}

function gcd(a: number, b: number): number {
  let x = Math.abs(a)
  let y = Math.abs(b)
  while (y) {
    const next = x % y
    x = y
    y = next
  }
  return x || 1
}

function closestFamily(aspect: number) {
  let best = STICKER_ASPECT_FAMILIES[0]!
  let bestErr = Infinity
  for (const family of STICKER_ASPECT_FAMILIES) {
    const err = Math.abs(Math.log(family.aspect / aspect))
    if (err < bestErr) {
      best = family
      bestErr = err
    }
  }
  return best
}

export function stickerAspectKey(size: string): string {
  const named = familyForSize(size)
  if (named) return named.key
  const span = widgetSizeSpan(size)
  return closestFamily(span.w / Math.max(1, span.h)).key
}

/** Exact same ratio only: integer scales of the reduced cell pair that fit 16×8. */
export function stickerSizesSharingAspect(size: string): WidgetSize[] {
  const span = widgetSizeSpan(size)
  const d = gcd(span.w, span.h)
  const rw = Math.max(1, span.w / d)
  const rh = Math.max(1, span.h / d)
  const out: WidgetSize[] = []
  for (let k = 1; k * rw <= STICKER_MAX_COLS && k * rh <= STICKER_MAX_ROWS; k++) {
    out.push(`${k * rw}x${k * rh}` as WidgetSize)
  }
  return out.length > 0 ? out : [size as WidgetSize]
}

/** Closest named tile that fits; used only as a generate-ratio hint, not placement. */
export function snapHomeStickerSize(cols: number, rows: number): WidgetSize {
  const w = Math.max(1, cols)
  const h = Math.max(1, rows)
  const exact = `${w}x${h}` as WidgetSize
  if ((HOME_STICKER_SIZES as readonly string[]).includes(exact)) return exact
  const boxAspect = w / h
  const family = closestFamily(boxAspect)
  let best: WidgetSize = family.sizes[0] ?? '1x1'
  let bestArea = 0
  for (const size of family.sizes) {
    const dim = widgetSizeSpan(size)
    if (dim.w > w || dim.h > h) continue
    const area = dim.w * dim.h
    if (area >= bestArea) {
      best = size
      bestArea = area
    }
  }
  if (bestArea > 0) return best
  return closestFamily(boxAspect).sizes[0] ?? '1x1'
}

/** The drawn cells are the sticker. No shrinking to a preset. */
export function placeHomeStickerSelection(
  x: number,
  y: number,
  cols: number,
  rows: number,
): { size: WidgetSize; x: number; y: number } {
  const w = Math.max(1, cols)
  const h = Math.max(1, rows)
  return {
    size: `${w}x${h}` as WidgetSize,
    x,
    y,
  }
}

/** Model pixels: nearest 1:1 / 3:2 / 2:3. Display crop covers the real slot. */
export function stickerPixelSize(size: string): {
  width: number
  height: number
} {
  const span = widgetSizeSpan(size)
  const boxAspect = span.w / Math.max(1, span.h)
  let best = GENERATE_PIXEL_PRESETS[0]
  let bestErr = Infinity
  for (const preset of GENERATE_PIXEL_PRESETS) {
    const err = Math.abs(Math.log(preset.aspect / boxAspect))
    if (err < bestErr) {
      best = preset
      bestErr = err
    }
  }
  return { width: best.width, height: best.height }
}
