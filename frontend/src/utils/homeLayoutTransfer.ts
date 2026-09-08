/**
 * Home layout file transfer: dedicated export envelope plus the stored
 * `dashboard_layout` shapes. Settings backups are rejected, not mined.
 *
 * v2 may carry sticker originals in `assets` (PNG/JPEG/WebP base64). Layout tiles
 * still store paths only; import re-writes those paths after re-store.
 */

import type { WidgetConfig, WidgetSize } from '../components/widgetGridTypes'
import type { HomeDashboardLayouts, HomeLayoutMode } from './homeLayout'
import {
  cloneHomeWidgets,
  HOME_FREE_ROWS,
  HOME_STANDARD_COLS,
  HOME_STANDARD_ROWS,
  HOME_STICKER_TYPE,
  parseHomeLayoutMode,
} from './homeLayout'
import {
  STICKER_EXTRA_SIZE_KEYS,
  WIDGET_SIZE_KEYS,
  widgetSizeSpan,
} from './widgetSizeScale'

export const HOME_LAYOUT_EXPORT_KIND = 'myriad.home-layout'
export const HOME_LAYOUT_EXPORT_VERSION = 2
export const HOME_LAYOUT_IMPORT_MAX_BYTES = 40 * 1024 * 1024
export const HOME_LAYOUT_IMPORT_MAX_TILES = 200
export const HOME_LAYOUT_ASSET_MAX_BYTES = 10 * 1024 * 1024
export const HOME_LAYOUT_ASSETS_MAX_TOTAL_BYTES = 32 * 1024 * 1024

const SETTINGS_BACKUP_FORMAT = 'myriad-settings-backup'
const MAX_ID_LENGTH = 128
const MAX_TYPE_LENGTH = 256
const MAX_ASSET_KEY_LENGTH = 2048
const IMAGE_CACHE_FILE =
  /^\/api\/brew\/image-cache\/([0-9a-f]{2})\/([0-9a-f]{64})\.(jpg|jpeg|png|webp)$/i

export type HomeLayoutImportReason =
  | 'invalid'
  | 'settings-backup'
  | 'too-large'
  | 'too-many'

export type HomeLayoutAssetMime = 'image/png' | 'image/jpeg' | 'image/webp'

export interface HomeLayoutAsset {
  mime: HomeLayoutAssetMime
  data: string
}

export type HomeLayoutAssetMap = Record<string, HomeLayoutAsset>

export type HomeLayoutImportResult =
  | {
      ok: true
      layouts: HomeDashboardLayouts
      mode: HomeLayoutMode | null
      assets: HomeLayoutAssetMap
    }
  | { ok: false; reason: HomeLayoutImportReason }

export interface HomeLayoutExportDocument {
  kind: typeof HOME_LAYOUT_EXPORT_KIND
  v: number
  mode: HomeLayoutMode
  layouts: HomeDashboardLayouts
  assets: HomeLayoutAssetMap
}

export function homeLayoutExportFilename(now = new Date()): string {
  return `myriad-home-layout-${now.toISOString().slice(0, 10)}.json`
}

export function buildHomeLayoutExport(
  layouts: HomeDashboardLayouts,
  mode: HomeLayoutMode,
  assets: HomeLayoutAssetMap = {},
): HomeLayoutExportDocument {
  const sanitized = sanitizeHomeLayouts(layouts)
  return {
    kind: HOME_LAYOUT_EXPORT_KIND,
    v: HOME_LAYOUT_EXPORT_VERSION,
    mode: mode === 'free' ? 'free' : 'standard',
    layouts: sanitized,
    assets: pickAssetsForLayouts(sanitized, assets),
  }
}

export function downloadJsonFile(filename: string, value: unknown): void {
  const json = JSON.stringify(value, null, 2)
  const blob = new Blob([json], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = filename
  document.body.appendChild(anchor)
  anchor.click()
  document.body.removeChild(anchor)
  URL.revokeObjectURL(url)
}

export function parseHomeLayoutImportText(
  text: string,
  byteLength = text.length,
): HomeLayoutImportResult {
  if (byteLength > HOME_LAYOUT_IMPORT_MAX_BYTES) {
    return { ok: false, reason: 'too-large' }
  }
  let raw: unknown
  try {
    raw = JSON.parse(text.replace(/^\uFEFF/, ''))
  } catch {
    return { ok: false, reason: 'invalid' }
  }
  return parseHomeLayoutImport(raw)
}

export function parseHomeLayoutImport(raw: unknown): HomeLayoutImportResult {
  if (Array.isArray(raw)) {
    return finalizeImportedLayouts(
      { standard: raw, free: cloneHomeWidgets(raw) },
      null,
      {},
    )
  }
  if (!raw || typeof raw !== 'object') {
    return { ok: false, reason: 'invalid' }
  }
  const record = raw as Record<string, unknown>
  if (looksLikeSettingsDump(record)) {
    return { ok: false, reason: 'settings-backup' }
  }
  if (
    typeof record.kind === 'string' &&
    record.kind !== HOME_LAYOUT_EXPORT_KIND
  ) {
    return { ok: false, reason: 'invalid' }
  }

  if (
    typeof record.dashboard_layout === 'string' ||
    (record.dashboard_layout && typeof record.dashboard_layout === 'object')
  ) {
    let nestedRaw: unknown = record.dashboard_layout
    if (typeof record.dashboard_layout === 'string') {
      try {
        nestedRaw = JSON.parse(record.dashboard_layout)
      } catch {
        return { ok: false, reason: 'invalid' }
      }
    }
    const nested = parseHomeLayoutImport(nestedRaw)
    if (!nested.ok) return nested
    return {
      ...nested,
      mode:
        record.dashboard_layout_mode != null
          ? parseHomeLayoutMode(record.dashboard_layout_mode)
          : nested.mode,
    }
  }

  const source =
    record.layouts && typeof record.layouts === 'object'
      ? (record.layouts as Record<string, unknown>)
      : record
  if (!Array.isArray(source.standard) || !Array.isArray(source.free)) {
    return { ok: false, reason: 'invalid' }
  }

  const mode = record.mode != null ? parseHomeLayoutMode(record.mode) : null
  return finalizeImportedLayouts(
    { standard: source.standard, free: source.free },
    mode,
    record.assets,
  )
}

export function canonicalStickerImageUrl(url: string): string | null {
  if (typeof url !== 'string' || url.length === 0 || url.length > 2048) {
    return null
  }
  if (url.startsWith('inline:')) {
    const id = url.slice('inline:'.length)
    if (!id || id.length > MAX_ID_LENGTH) return null
    return url
  }
  const withoutQuery = url.split('#')[0]?.split('?')[0] ?? url
  const cacheAt = withoutQuery.indexOf('/api/brew/image-cache/')
  const path = cacheAt >= 0 ? withoutQuery.slice(cacheAt) : withoutQuery
  const match = IMAGE_CACHE_FILE.exec(path)
  if (!match) return null
  const subdir = match[1].toLowerCase()
  const stem = match[2].toLowerCase()
  const ext = match[3].toLowerCase()
  if (!stem.startsWith(subdir)) return null
  return `/api/brew/image-cache/${subdir}/${stem}.${ext}`
}

export function isHomeLayoutStickerTile(item: {
  kind?: string
  type?: string
}): boolean {
  return item.kind === 'sticker' || item.type === HOME_STICKER_TYPE
}

export function stickerAssetFitsBudget(occupied: number, extra: number): boolean {
  if (extra <= 0 || extra > HOME_LAYOUT_ASSET_MAX_BYTES) return false
  return occupied + extra <= HOME_LAYOUT_ASSETS_MAX_TOTAL_BYTES
}

export function listStickerImageUrls(
  layouts: HomeDashboardLayouts,
): string[] {
  const urls = new Set<string>()
  for (const tile of layouts.free) {
    if (!isHomeLayoutStickerTile(tile)) continue
    const raw =
      typeof tile.config?.imageUrl === 'string' ? tile.config.imageUrl.trim() : ''
    if (!raw || raw.startsWith('data:')) continue
    const key = canonicalStickerImageUrl(raw)
    if (key && !key.startsWith('inline:')) urls.add(key)
  }
  return [...urls]
}

export function rewriteStickerImageUrls(
  layouts: HomeDashboardLayouts,
  rewrite: (url: string) => string | null,
): HomeDashboardLayouts {
  const mapSide = (tiles: WidgetConfig[]) =>
    tiles.map((tile) => {
      if (!isHomeLayoutStickerTile(tile)) return tile
      const raw =
        typeof tile.config?.imageUrl === 'string'
          ? tile.config.imageUrl.trim()
          : ''
      if (!raw) return tile
      const next = rewrite(raw)
      if (next === raw) return tile
      const config = { ...(tile.config as Record<string, unknown>) }
      if (!next) delete config.imageUrl
      else config.imageUrl = next
      return { ...tile, config }
    })
  return { standard: mapSide(layouts.standard), free: mapSide(layouts.free) }
}

export function stickerAssetDataUrl(asset: HomeLayoutAsset): string {
  return `data:${asset.mime};base64,${asset.data}`
}

function looksLikeSettingsDump(record: Record<string, unknown>): boolean {
  if (record.format === SETTINGS_BACKUP_FORMAT) return true
  if (Array.isArray(record.configurations) && record.effective_config) {
    return true
  }
  return Boolean(record.platforms && record.ai_config && record.ui_config)
}

function finalizeImportedLayouts(
  raw: { standard: unknown[]; free: unknown[] },
  mode: HomeLayoutMode | null,
  rawAssets: unknown,
): HomeLayoutImportResult {
  if (
    raw.standard.length > HOME_LAYOUT_IMPORT_MAX_TILES ||
    raw.free.length > HOME_LAYOUT_IMPORT_MAX_TILES
  ) {
    return { ok: false, reason: 'too-many' }
  }
  const hoisted: HomeLayoutAssetMap = {}
  const free = hoistInlineStickerImages(raw.free, hoisted)
  const assets = {
    ...parseHomeLayoutAssets(rawAssets),
    ...hoisted,
  }
  const layouts = sanitizeHomeLayouts({
    standard: raw.standard as WidgetConfig[],
    free: free as WidgetConfig[],
  })
  if (
    raw.standard.length + raw.free.length > 0 &&
    layouts.standard.length === 0 &&
    layouts.free.length === 0
  ) {
    return { ok: false, reason: 'invalid' }
  }
  return { ok: true, layouts, mode, assets }
}

function hoistInlineStickerImages(
  tiles: unknown[],
  assets: HomeLayoutAssetMap,
): unknown[] {
  return tiles.map((tile, index) => {
    if (!tile || typeof tile !== 'object') return tile
    const record = tile as Record<string, unknown>
    const sticker =
      record.kind === 'sticker' || record.type === HOME_STICKER_TYPE
    if (!sticker || !record.config || typeof record.config !== 'object') {
      return tile
    }
    const config = record.config as Record<string, unknown>
    if (typeof config.imageUrl !== 'string' || !config.imageUrl.startsWith('data:')) {
      return tile
    }
    const parsed = parseStickerDataUrl(config.imageUrl)
    if (!parsed) {
      const next = { ...config }
      delete next.imageUrl
      return { ...record, config: next }
    }
    const key = `inline:${index}`
    assets[key] = parsed
    return { ...record, config: { ...config, imageUrl: key } }
  })
}

export function parseHomeLayoutAssets(raw: unknown): HomeLayoutAssetMap {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return {}
  const out: HomeLayoutAssetMap = {}
  let total = 0
  for (const [rawKey, value] of Object.entries(raw as Record<string, unknown>)) {
    if (Object.keys(out).length >= HOME_LAYOUT_IMPORT_MAX_TILES) break
    const key = canonicalStickerImageUrl(rawKey)
    if (!key || key.length > MAX_ASSET_KEY_LENGTH) continue
    const asset = parseStickerAsset(value)
    if (!asset) continue
    const bytes = estimateBase64Bytes(asset.data)
    if (bytes <= 0 || bytes > HOME_LAYOUT_ASSET_MAX_BYTES) continue
    if (total + bytes > HOME_LAYOUT_ASSETS_MAX_TOTAL_BYTES) continue
    total += bytes
    out[key] = asset
  }
  return out
}

function parseStickerAsset(raw: unknown): HomeLayoutAsset | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const record = raw as Record<string, unknown>
  if (typeof record.data !== 'string' || !record.data) return null
  const data = record.data.replace(/\s/g, '')
  if (!data || data.length > HOME_LAYOUT_ASSET_MAX_BYTES * 2) return null
  const bytes = decodeBase64(data)
  if (!bytes) return null
  const mime = sniffStickerImage(bytes)
  if (!mime) return null
  if (
    record.mime != null &&
    record.mime !== 'image/png' &&
    record.mime !== 'image/jpeg' &&
    record.mime !== 'image/webp'
  ) {
    return null
  }
  return { mime, data }
}

export function parseStickerDataUrl(value: string): HomeLayoutAsset | null {
  const match = /^data:(image\/(?:png|jpeg|webp));base64,([A-Za-z0-9+/=\s]+)$/.exec(
    value.trim(),
  )
  if (!match) return null
  return parseStickerAsset({ mime: match[1], data: match[2] })
}

function pickAssetsForLayouts(
  layouts: HomeDashboardLayouts,
  assets: HomeLayoutAssetMap,
): HomeLayoutAssetMap {
  const out: HomeLayoutAssetMap = {}
  for (const url of listStickerImageUrls(layouts)) {
    const asset = assets[url]
    if (asset) out[url] = asset
  }
  for (const tile of layouts.free) {
    if (!isHomeLayoutStickerTile(tile)) continue
    const raw =
      typeof tile.config?.imageUrl === 'string' ? tile.config.imageUrl.trim() : ''
    if (raw.startsWith('inline:') && assets[raw]) out[raw] = assets[raw]
  }
  return out
}

function sanitizeHomeLayouts(
  layouts: HomeDashboardLayouts,
): HomeDashboardLayouts {
  return {
    standard: uniquifyIds(
      sanitizeTileList(layouts.standard, HOME_STANDARD_ROWS, false),
    ),
    free: uniquifyIds(sanitizeTileList(layouts.free, HOME_FREE_ROWS, true)),
  }
}

function sanitizeTileList(
  tiles: unknown[],
  rows: number,
  allowStickers: boolean,
): WidgetConfig[] {
  const out: WidgetConfig[] = []
  for (const tile of tiles) {
    const widget = sanitizeTile(tile, rows, allowStickers)
    if (widget) out.push(widget)
  }
  return out
}

function sanitizeTile(
  raw: unknown,
  rows: number,
  allowStickers: boolean,
): WidgetConfig | null {
  if (!raw || typeof raw !== 'object') return null
  const record = raw as Record<string, unknown>
  if (
    typeof record.id !== 'string' ||
    !record.id ||
    record.id.length > MAX_ID_LENGTH
  ) {
    return null
  }
  if (
    typeof record.type !== 'string' ||
    !record.type ||
    record.type.length > MAX_TYPE_LENGTH
  ) {
    return null
  }
  if (typeof record.size !== 'string') return null
  const position = record.position
  if (!position || typeof position !== 'object') return null
  const coords = position as Record<string, unknown>
  if (!isGridCoord(coords.x) || !isGridCoord(coords.y)) return null

  const sticker =
    record.kind === 'sticker' || record.type === HOME_STICKER_TYPE
  if (sticker && !allowStickers) return null
  if (!isAllowedSize(record.size, sticker)) return null

  const span = widgetSizeSpan(record.size)
  const w = Math.min(span.w, HOME_STANDARD_COLS)
  const h = Math.min(span.h, rows)
  const maxX = Math.max(0, HOME_STANDARD_COLS - w)
  const maxY = Math.max(0, rows - h)

  const widget: WidgetConfig = {
    id: record.id,
    type: sticker ? HOME_STICKER_TYPE : record.type,
    size: record.size as WidgetSize,
    position: {
      x: Math.min(coords.x, maxX),
      y: Math.min(coords.y, maxY),
    },
  }
  if (sticker) widget.kind = 'sticker'
  const config = clonePlainObject(record.config)
  if (config) {
    if (sticker && typeof config.imageUrl === 'string') {
      const url = config.imageUrl.trim()
      if (url.startsWith('data:')) {
        delete config.imageUrl
      }
      else {
        const canonical = canonicalStickerImageUrl(url)
        if (canonical) config.imageUrl = canonical
        else if (!url.startsWith('inline:')) config.imageUrl = url
      }
    }
    widget.config = config
  }
  return widget
}

function isAllowedSize(size: string, sticker: boolean): boolean {
  if ((WIDGET_SIZE_KEYS as readonly string[]).includes(size)) return true
  return sticker && (STICKER_EXTRA_SIZE_KEYS as readonly string[]).includes(size)
}

function isGridCoord(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0
}

function clonePlainObject(value: unknown): Record<string, unknown> | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  try {
    const cloned = JSON.parse(JSON.stringify(value)) as unknown
    if (!cloned || typeof cloned !== 'object' || Array.isArray(cloned)) {
      return undefined
    }
    return cloned as Record<string, unknown>
  } catch {
    return undefined
  }
}

function uniquifyIds(widgets: WidgetConfig[]): WidgetConfig[] {
  const seen = new Set<string>()
  return widgets.map((widget) => {
    if (!seen.has(widget.id)) {
      seen.add(widget.id)
      return widget
    }
    let n = 2
    let next = `${widget.id}__${n}`
    while (seen.has(next)) {
      n += 1
      next = `${widget.id}__${n}`
    }
    seen.add(next)
    return { ...widget, id: next }
  })
}

export function sniffStickerImage(bytes: Uint8Array): HomeLayoutAssetMime | null {
  if (
    bytes.length >= 8 &&
    bytes[0] === 0x89 &&
    bytes[1] === 0x50 &&
    bytes[2] === 0x4E &&
    bytes[3] === 0x47
  ) {
    return 'image/png'
  }
  if (
    bytes.length >= 3 &&
    bytes[0] === 0xFF &&
    bytes[1] === 0xD8 &&
    bytes[2] === 0xFF
  ) {
    return 'image/jpeg'
  }
  if (
    bytes.length >= 12 &&
    bytes[0] === 0x52 &&
    bytes[1] === 0x49 &&
    bytes[2] === 0x46 &&
    bytes[3] === 0x46 &&
    bytes[8] === 0x57 &&
    bytes[9] === 0x45 &&
    bytes[10] === 0x42 &&
    bytes[11] === 0x50
  ) {
    return 'image/webp'
  }
  return null
}

export function decodeBase64(data: string): Uint8Array | null {
  try {
    const binary = atob(data)
    const out = new Uint8Array(binary.length)
    for (let i = 0; i < binary.length; i += 1) {
      out[i] = binary.charCodeAt(i)
    }
    return out
  } catch {
    return null
  }
}

export function encodeBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let i = 0; i < bytes.length; i += 8192) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 8192))
  }
  return btoa(binary)
}

function estimateBase64Bytes(data: string): number {
  const padding = data.endsWith('==') ? 2 : data.endsWith('=') ? 1 : 0
  return Math.max(0, Math.floor((data.length * 3) / 4) - padding)
}
