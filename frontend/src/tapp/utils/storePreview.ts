export const STORE_PREVIEW_MIN_WIDTH = 1280
export const STORE_PREVIEW_MIN_HEIGHT = 720
export const STORE_PREVIEW_MAX_WIDTH = 3840
export const STORE_PREVIEW_MAX_HEIGHT = 2160
export const STORE_PREVIEW_MAX_STYLES = 8

export type StorePreviewFit = 'cover' | 'contain'
export type StorePreviewTheme = 'auto' | 'light' | 'dark'

export interface StorePreviewDescriptor {
  version: 1
  type: 'snapshot'
  html: string
  styles: string[]
  viewport: {
    width: number
    height: number
  }
  fit: StorePreviewFit
  focus: {
    x: number
    y: number
  }
  theme: StorePreviewTheme
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function finiteNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

function boundedInteger(
  value: unknown,
  fallback: number,
  min: number,
  max: number,
): number {
  const parsed = finiteNumber(value)
  if (parsed == null) return fallback
  return Math.min(max, Math.max(min, Math.round(parsed)))
}

function unitInterval(value: unknown, fallback: number): number {
  const parsed = finiteNumber(value)
  if (parsed == null) return fallback
  return Math.min(1, Math.max(0, parsed))
}

/** preview.html / styles 是商店相对路径，不是内联源码。 */
export function isStorePreviewResourcePath(value: string): boolean {
  const path = value.trim()
  if (!path || path.length > 1024) return false
  if (/[<>\r\n]/.test(path) || /\s/.test(path)) return false
  if (/^[a-z][a-z0-9+.-]*:/i.test(path) && !/^https?:\/\//i.test(path)) {
    return false
  }
  return true
}

export function parseStorePreview(
  value: unknown,
): StorePreviewDescriptor | undefined {
  if (!isRecord(value)) return undefined
  if (value.version !== 1 || value.type !== 'snapshot') return undefined

  const html = typeof value.html === 'string' ? value.html.trim() : ''
  if (!html || !isStorePreviewResourcePath(html)) return undefined

  const styles = Array.isArray(value.styles)
    ? value.styles
        .filter((entry): entry is string => typeof entry === 'string')
        .map((entry) => entry.trim())
        .filter((entry) => entry && isStorePreviewResourcePath(entry))
        .slice(0, STORE_PREVIEW_MAX_STYLES)
    : []
  const viewport = isRecord(value.viewport) ? value.viewport : {}
  const focus = isRecord(value.focus) ? value.focus : {}

  return {
    version: 1,
    type: 'snapshot',
    html,
    styles: Iterator.from(new Set(styles)).toArray(),
    viewport: {
      width: boundedInteger(
        viewport.width,
        STORE_PREVIEW_MIN_WIDTH,
        STORE_PREVIEW_MIN_WIDTH,
        STORE_PREVIEW_MAX_WIDTH,
      ),
      height: boundedInteger(
        viewport.height,
        STORE_PREVIEW_MIN_HEIGHT,
        STORE_PREVIEW_MIN_HEIGHT,
        STORE_PREVIEW_MAX_HEIGHT,
      ),
    },
    fit: value.fit === 'contain' ? 'contain' : 'cover',
    focus: {
      x: unitInterval(focus.x, 0.5),
      y: unitInterval(focus.y, 0.5),
    },
    theme:
      value.theme === 'light' || value.theme === 'dark'
        ? value.theme
        : 'auto',
  }
}
