/**
 * Pure widget content scale math (no React / DOM).
 * Aligned with viewportBands phone / tablet / desktop.
 */

import type { ViewportBand } from './viewportBands'

export type WidgetSizeKey =
  | '1x1'
  | '2x1'
  | '1x2'
  | '2x2'
  | '2x3'
  | '3x2'
  | '3x3'
  | '2x4'
  | '4x1'
  | '4x2'
  | '4x4'

/** Extra home-sticker tiles (not catalog widget sizes). */
export type StickerExtraSizeKey =
  | '4x3'
  | '8x6'
  | '3x4'
  | '6x8'
  | '7x4'
  | '9x5'
  | '14x8'
  | '4x7'

/** Desktop 16-col design cell (library previews). */
export const STANDARD_CELL_SIZE = 80

/**
 * WidgetGridItem wraps placed tiles in Tailwind `p-1` (4px × 2).
 * Home dock previews omit that wrapper, so they look a bit large unless
 * this inset is applied on top of the live cell / design-cell ratio.
 */
export const GRID_WIDGET_PAD_PX = 4
/** 2×2 is the common catalog tile; 8 / 160. */
export const LIBRARY_DOCK_PREVIEW_INSET_SCALE =
  1 - (GRID_WIDGET_PAD_PX * 2) / (2 * STANDARD_CELL_SIZE)

export function libraryDockPreviewDisplayScale(cellSize: number): number {
  const cell = cellSize > 0 ? cellSize : STANDARD_CELL_SIZE
  return (cell / STANDARD_CELL_SIZE) * LIBRARY_DOCK_PREVIEW_INSET_SCALE
}

/**
 * Resting cell size (px) per viewport band so scale≈1 on a typical layout.
 * Must stay in sync with home grid column bands (viewportBands).
 */
export const STANDARD_CELL_BY_BAND: Record<ViewportBand, number> = {
  desktop: 80,
  tablet: 112,
  phone: 96,
}

const SIZE_SPANS: Record<WidgetSizeKey, { cols: number; rows: number }> = {
  '1x1': { cols: 1, rows: 1 },
  '2x1': { cols: 2, rows: 1 },
  '4x1': { cols: 4, rows: 1 },
  '1x2': { cols: 1, rows: 2 },
  '2x2': { cols: 2, rows: 2 },
  '2x3': { cols: 2, rows: 3 },
  '3x2': { cols: 3, rows: 2 },
  '3x3': { cols: 3, rows: 3 },
  '2x4': { cols: 2, rows: 4 },
  '4x2': { cols: 4, rows: 2 },
  '4x4': { cols: 4, rows: 4 },
}

export const WIDGET_SIZE_KEYS = Object.keys(SIZE_SPANS) as WidgetSizeKey[]

const STICKER_EXTRA_SPANS: Record<StickerExtraSizeKey, { cols: number; rows: number }> =
  {
    '4x3': { cols: 4, rows: 3 },
    '8x6': { cols: 8, rows: 6 },
    '3x4': { cols: 3, rows: 4 },
    '6x8': { cols: 6, rows: 8 },
    '7x4': { cols: 7, rows: 4 },
    '9x5': { cols: 9, rows: 5 },
    '14x8': { cols: 14, rows: 8 },
    '4x7': { cols: 4, rows: 7 },
  }

export const STICKER_EXTRA_SIZE_KEYS = Object.keys(
  STICKER_EXTRA_SPANS,
) as StickerExtraSizeKey[]

export function widgetSizeSpan(size: string): { w: number; h: number } {
  const span =
    SIZE_SPANS[size as WidgetSizeKey] ??
    STICKER_EXTRA_SPANS[size as StickerExtraSizeKey]
  if (span) return { w: span.cols, h: span.rows }
  const match = /^([1-9]\d*)x([1-9]\d*)$/.exec(size)
  if (match) {
    return { w: Number(match[1]), h: Number(match[2]) }
  }
  return { w: 2, h: 2 }
}

export const WIDGET_SCALE_MIN = 0.78
export const WIDGET_SCALE_MAX = 1.06
const SCALE_DAMPEN = 0.62

export const WIDGET_COMPACT_SCALE = 0.9
export const WIDGET_MINI_SCALE = 0.82

export function standardCellSizeForBand(band: ViewportBand): number {
  return STANDARD_CELL_BY_BAND[band]
}

export function getStandardWidgetDimensionsForBand(
  widgetSize: WidgetSizeKey,
  band: ViewportBand,
): { width: number; height: number } {
  const span = SIZE_SPANS[widgetSize] ?? { cols: 1, rows: 1 }
  const cell = standardCellSizeForBand(band)
  return {
    width: span.cols * cell,
    height: span.rows * cell,
  }
}

export function getStandardWidgetDimensions(widgetSize: WidgetSizeKey): {
  width: number
  height: number
} {
  return getStandardWidgetDimensionsForBand(widgetSize, 'desktop')
}

export function resolveWidgetContentScale(input: {
  measuredWidth: number
  measuredHeight: number
  widgetSize: WidgetSizeKey
  band: ViewportBand
  forceScale?: number
}): number {
  if (input.forceScale !== undefined) return input.forceScale
  if (input.measuredWidth <= 0) return 1

  const standard = getStandardWidgetDimensionsForBand(
    input.widgetSize,
    input.band,
  )
  if (!standard.width || !standard.height) return 1

  const rawW = input.measuredWidth / standard.width
  const rawH =
    input.measuredHeight > 0
      ? input.measuredHeight / standard.height
      : rawW
  const raw = Math.sqrt(Math.max(0.01, rawW * rawH))
  const damped = 1 + (raw - 1) * SCALE_DAMPEN
  return Math.max(WIDGET_SCALE_MIN, Math.min(WIDGET_SCALE_MAX, damped))
}
