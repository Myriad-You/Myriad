/**
 * Home dashboard layout: standard (centered 16×4) vs free (same cell size, fixed 16×8).
 *
 * Cell size is derived from the standard stage (max-width + stage pad inside page padding).
 * Free uses the same centered 16-col width; row count is a fixed taller canvas,
 * not the standard 4 rows and not viewport-grown.
 * Visual gutters live as `--home-*` on `.home-shell` (Home.css); the rem steps here
 * are the same numbers.
 */

import type { WidgetConfig } from '../components/widgetGridTypes'
import type { StickerCrop } from './homeStickerCrop'
import { widgetSizeSpan } from './widgetSizeScale'

export type HomeLayoutMode = 'standard' | 'free'

export interface HomeDashboardLayouts {
  standard: WidgetConfig[]
  free: WidgetConfig[]
}

export interface HomePagePadXStep {
  minWidth: number
  rem: number
}

/** Page pad-x: 0.75rem / 1rem / 1.5rem at 0 / 375 / 640. */
export const HOME_PAGE_PAD_X_STEPS: readonly HomePagePadXStep[] = [
  { minWidth: 0, rem: 0.75 },
  { minWidth: 375, rem: 1 },
  { minWidth: 640, rem: 1.5 },
]

/** Standard stage max-width (`--home-standard-max-width`). */
export const HOME_STANDARD_MAX_WIDTH_REM = 80
/** Per-side stage padding (`--home-stage-pad`). */
export const HOME_STANDARD_STAGE_PAD_REM = 0.5
/** Per-side free-layout page pad-y (`--home-free-pad-y`). */
export const HOME_FREE_PAGE_PAD_Y_REM = 1.5
export const HOME_STANDARD_COLS = 16
export const HOME_STANDARD_ROWS = 4
/** Free canvas rows: fixed, taller than standard, independent of viewport. */
export const HOME_FREE_ROWS = 8
/** Free layout: max occupied cells across all widgets (16×8 canvas is 128). */
export const HOME_FREE_MAX_CELLS = 96
export const HOME_STICKER_TYPE = 'sticker'
export const HOME_LAYOUT_MODE_KEY = 'myriad.home-layout-mode'

export function homePagePadXRem(viewportWidth: number): number {
  let rem = HOME_PAGE_PAD_X_STEPS[0].rem
  for (const step of HOME_PAGE_PAD_X_STEPS) {
    if (viewportWidth >= step.minWidth) rem = step.rem
  }
  return rem
}

export function homePagePaddingX(
  viewportWidth: number,
  rootFontSize = 16,
): number {
  return homePagePadXRem(viewportWidth) * rootFontSize
}

export function homeStagePadPx(rootFontSize = 16): number {
  return HOME_STANDARD_STAGE_PAD_REM * 2 * rootFontSize
}

export function homeFreePagePadYPx(rootFontSize = 16): number {
  return HOME_FREE_PAGE_PAD_Y_REM * 2 * rootFontSize
}

export function standardHomeGridWidth(
  viewportWidth: number,
  rootFontSize = 16,
): number {
  const pagePad = homePagePaddingX(viewportWidth, rootFontSize)
  const maxStage = HOME_STANDARD_MAX_WIDTH_REM * rootFontSize
  const inner = Math.max(0, viewportWidth - pagePad * 2)
  const stage = Math.min(maxStage, inner)
  return Math.max(0, stage - homeStagePadPx(rootFontSize))
}

export function standardHomeCellSize(
  viewportWidth: number,
  rootFontSize = 16,
): number {
  return standardHomeGridWidth(viewportWidth, rootFontSize) / HOME_STANDARD_COLS
}

/**
 * First-paint guess for the free-layout host before ResizeObserver.
 * Same rem steps as `.home-shell` CSS variables.
 */
export function estimateFreeHomeHostSize(
  viewportWidth: number,
  viewportHeight: number,
  rootFontSize = 16,
): { width: number; height: number } {
  const stage = homeStagePadPx(rootFontSize)
  return {
    width: standardHomeGridWidth(viewportWidth, rootFontSize),
    height: Math.max(
      0,
      viewportHeight - homeFreePagePadYPx(rootFontSize) - stage,
    ),
  }
}

export function resolveFreeHomeGrid(input: {
  availableWidth: number
  availableHeight: number
  cellSize: number
}): { cols: number; rows: number; cell: number } {
  const cell = input.cellSize > 0 ? input.cellSize : 80
  if (input.availableWidth <= 0 || input.availableHeight <= 0) {
    return {
      cols: HOME_STANDARD_COLS,
      rows: HOME_FREE_ROWS,
      cell,
    }
  }
  return {
    cols: HOME_STANDARD_COLS,
    rows: HOME_FREE_ROWS,
    cell,
  }
}

export function effectiveHomeLayoutMode(
  mode: HomeLayoutMode,
  isDesktop: boolean,
): HomeLayoutMode {
  return isDesktop ? mode : 'standard'
}

export function cloneHomeWidgets(widgets: WidgetConfig[]): WidgetConfig[] {
  return widgets.map((widget) => ({
    ...widget,
    position: { ...widget.position },
  }))
}

export function isHomeStickerItem(item: { kind?: string }): boolean {
  return item.kind === 'sticker'
}

export function isHomeWidgetItem(item: { kind?: string }): boolean {
  return !isHomeStickerItem(item)
}

export { stickerPixelSize } from './homeStickerSize'

export function findEmptyHomeSlot(
  widgets: WidgetConfig[],
  size: string,
  columns: number,
  rows: number,
): { x: number; y: number } | null {
  const dim = widgetSizeSpan(size)
  const w = Math.min(dim.w, Math.max(1, columns))
  const h = Math.min(dim.h, Math.max(1, rows))
  for (let y = 0; y <= rows - h; y += 1) {
    for (let x = 0; x <= columns - w; x += 1) {
      let hit = false
      for (const other of widgets) {
        const od = widgetSizeSpan(other.size)
        if (
          x < other.position.x + od.w &&
          x + w > other.position.x &&
          y < other.position.y + od.h &&
          y + h > other.position.y
        ) {
          hit = true
          break
        }
      }
      if (!hit) return { x, y }
    }
  }
  return null
}

export function createHomeStickerItem(input: {
  size: WidgetConfig['size']
  position: { x: number; y: number }
  imageUrl: string
  prompt: string
  crop?: StickerCrop
}): WidgetConfig {
  return {
    id: `sticker_${Date.now()}`,
    type: HOME_STICKER_TYPE,
    kind: 'sticker',
    size: input.size,
    position: { ...input.position },
    config: {
      imageUrl: input.imageUrl,
      prompt: input.prompt,
      ...(input.crop ? { crop: input.crop } : {}),
    },
  }
}

export function homeWidgetCellCount(size: string): number {
  const span = widgetSizeSpan(size)
  return span.w * span.h
}

export function homeWidgetsOccupiedCells(
  widgets: Array<{ id?: string; size: string; kind?: string }>,
  omitId?: string,
): number {
  let cells = 0
  for (const widget of widgets) {
    if (omitId && widget.id === omitId) continue
    if (!isHomeWidgetItem(widget)) continue
    cells += homeWidgetCellCount(widget.size)
  }
  return cells
}

export function freeLayoutFitsCellBudget(
  occupied: number,
  extra: number,
): boolean {
  return occupied + extra <= HOME_FREE_MAX_CELLS
}

function compareWidgetAreaDesc(a: WidgetConfig, b: WidgetConfig): number {
  const da = widgetSizeSpan(a.size)
  const db = widgetSizeSpan(b.size)
  return db.w * db.h - da.w * da.h || db.h - da.h || db.w - da.w
}

/** Tops within 1 cell share a visual row (staggered free-canvas items). */
const PACK_BAND_Y_SLACK = 1

function clusterWidgetsIntoBands(widgets: WidgetConfig[]): WidgetConfig[][] {
  const sorted = [...widgets].sort((a, b) => {
    if (a.position.y !== b.position.y) return a.position.y - b.position.y
    if (a.position.x !== b.position.x) return a.position.x - b.position.x
    return compareWidgetAreaDesc(a, b)
  })
  const bands: WidgetConfig[][] = []
  let bandMinY = 0
  for (const widget of sorted) {
    const last = bands.at(-1)
    if (last && widget.position.y <= bandMinY + PACK_BAND_Y_SLACK) {
      last.push(widget)
      continue
    }
    bandMinY = widget.position.y
    bands.push([widget])
  }
  return bands
}

function packShelf(
  items: WidgetConfig[],
  columns: number,
): { widgets: WidgetConfig[]; height: number } {
  const occupied = new Set<string>()
  const packed: WidgetConfig[] = []
  let maxY = 0

  const isOccupied = (x: number, y: number, w: number, h: number) => {
    for (let i = 0; i < w; i++) {
      for (let j = 0; j < h; j++) {
        if (occupied.has(`${x + i},${y + j}`)) return true
      }
    }
    return false
  }

  const markOccupied = (x: number, y: number, w: number, h: number) => {
    for (let i = 0; i < w; i++) {
      for (let j = 0; j < h; j++) {
        occupied.add(`${x + i},${y + j}`)
      }
    }
  }

  const ordered = [...items].sort((a, b) => {
    if (a.position.x !== b.position.x) return a.position.x - b.position.x
    return compareWidgetAreaDesc(a, b)
  })

  for (const widget of ordered) {
    const dim = widgetSizeSpan(widget.size)
    const w = Math.min(dim.w, columns)
    const h = dim.h
    let x = 0
    let y = 0
    let placed = false
    while (!placed && y <= 100) {
      if (x + w <= columns && !isOccupied(x, y, w, h)) {
        markOccupied(x, y, w, h)
        packed.push({ ...widget, position: { x, y } })
        maxY = Math.max(maxY, y + h)
        placed = true
      } else {
        x += 1
        if (x >= columns) {
          x = 0
          y += 1
        }
      }
    }
  }

  return { widgets: packed, height: maxY }
}

/**
 * Compact a free or standard layout into `columns`.
 * Keeps source visual rows (later rows cannot fill holes in earlier ones),
 * then left-to-right first-fit inside each row so mixed heights pack side by side.
 * Does not mutate the input.
 */
export function packWidgetsIntoColumns(
  widgets: WidgetConfig[],
  columns: number,
): { widgets: WidgetConfig[]; height: number } {
  const cols = Math.max(1, columns)
  const source = widgets.filter(isHomeWidgetItem)
  if (source.length === 0) {
    return { widgets: [], height: HOME_STANDARD_ROWS }
  }

  const packed: WidgetConfig[] = []
  let destY = 0
  for (const band of clusterWidgetsIntoBands(source)) {
    const shelf = packShelf(band, cols)
    for (const widget of shelf.widgets) {
      packed.push({
        ...widget,
        position: { x: widget.position.x, y: widget.position.y + destY },
      })
    }
    destY += shelf.height
  }

  return { widgets: packed, height: Math.max(HOME_STANDARD_ROWS, destY) }
}

function isWidgetArray(value: unknown): value is WidgetConfig[] {
  return Array.isArray(value)
}

/**
 * Legacy `dashboard_layout` is a widget array (standard only).
 * v2 is `{ v: 2, standard, free }`. Missing `free` copies standard.
 * An explicit empty `free` array is preserved.
 */
export function parseDashboardLayout(raw: unknown): HomeDashboardLayouts {
  if (isWidgetArray(raw)) {
    return { standard: raw, free: cloneHomeWidgets(raw) }
  }
  if (raw && typeof raw === 'object') {
    const record = raw as Record<string, unknown>
    if (isWidgetArray(record.standard)) {
      return {
        standard: record.standard,
        free: isWidgetArray(record.free)
          ? record.free
          : cloneHomeWidgets(record.standard),
      }
    }
  }
  return { standard: [], free: [] }
}

export function parseDashboardLayoutJson(text: string): HomeDashboardLayouts {
  try {
    return parseDashboardLayout(JSON.parse(text) as unknown)
  } catch {
    return { standard: [], free: [] }
  }
}

export function homeLayoutsHaveTiles(layouts: HomeDashboardLayouts): boolean {
  return layouts.standard.length > 0 || layouts.free.length > 0
}

/**
 * First paint of a saved dashboard must keep every tile, including Tapp
 * types that are not in the widget catalog yet.
 *
 * Filtering against a cold registry (Home's first render has no Tapp types)
 * drops third-party tiles. `applyLayouts` then awaits chunk preload; if the
 * restore effect already put those tiles back, the late `setLayouts` from
 * that first apply overwrites them. HTTP cache makes the race likely —
 * Disable cache in DevTools slows the preload enough that restore wins.
 */
export function layoutsForFirstPaint(
  source: HomeDashboardLayouts,
): HomeDashboardLayouts {
  return {
    standard: [...source.standard],
    free: [...source.free],
  }
}

/**
 * After the Tapp registry has loaded, drop tiles whose types are still
 * unknown (uninstalled Tapps). Stickers stay on the free canvas.
 */
export function layoutsAfterWidgetRegistry(
  source: HomeDashboardLayouts,
  registeredIds: ReadonlySet<string>,
): HomeDashboardLayouts {
  const keep = (list: WidgetConfig[], allowStickers: boolean) =>
    list.filter(
      (widget) =>
        (allowStickers && isHomeStickerItem(widget)) ||
        registeredIds.has(widget.type),
    )
  return {
    standard: keep(source.standard, false),
    free: keep(source.free, true),
  }
}

/** In-flight first-paint apply must not replace a newer restore. */
export function shouldAcceptHomeLayoutApply(
  applyGeneration: number,
  currentGeneration: number,
): boolean {
  return applyGeneration === currentGeneration
}

export function serializeDashboardLayout(
  layouts: HomeDashboardLayouts,
): string {
  return JSON.stringify({
    v: 2,
    standard: layouts.standard,
    free: layouts.free,
  })
}

export function parseHomeLayoutMode(raw: unknown): HomeLayoutMode {
  return raw === 'free' ? 'free' : 'standard'
}

export function readHomeLayoutMode(
  storage?: Pick<Storage, 'getItem'> | null,
): HomeLayoutMode {
  return peekStoredHomeLayoutMode(storage) ?? 'standard'
}

/** `null` when the site mode has never been cached locally. */
export function peekStoredHomeLayoutMode(
  storage?: Pick<Storage, 'getItem'> | null,
): HomeLayoutMode | null {
  try {
    const raw = storage?.getItem(HOME_LAYOUT_MODE_KEY)
    if (raw == null) return null
    return parseHomeLayoutMode(raw)
  } catch {
    return null
  }
}

export function persistHomeLayoutMode(
  mode: HomeLayoutMode,
  storage?: Pick<Storage, 'setItem'> | null,
): void {
  try {
    storage?.setItem(HOME_LAYOUT_MODE_KEY, mode)
  } catch {
    // private mode / quota
  }
}
