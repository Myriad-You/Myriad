import type { StorePreviewDescriptor } from './storePreview'

export const STORE_PREVIEW_WIDTH = 1280
export const STORE_PREVIEW_HEIGHT = 720
export const STORE_PREVIEW_OVERFLOW_TOLERANCE = 0.2

export type PreviewRenderState = 'checking' | 'ready' | 'fallback'

export interface PreviewCanvas {
  width: number
  height: number
  fit: 'cover' | 'contain'
  focus: { x: number; y: number }
  theme: 'auto' | 'light' | 'dark'
}

export function getPreviewCanvas(preview?: StorePreviewDescriptor | null): PreviewCanvas {
  return {
    width: preview?.viewport.width ?? STORE_PREVIEW_WIDTH,
    height: preview?.viewport.height ?? STORE_PREVIEW_HEIGHT,
    fit: preview?.fit ?? 'cover',
    focus: preview?.focus ?? { x: 0.5, y: 0.5 },
    theme: preview?.theme ?? 'auto',
  }
}

export function previewTransform(
  hostWidth: number,
  hostHeight: number,
  canvas: PreviewCanvas,
): { scale: number; x: number; y: number } {
  const scaleX = hostWidth / canvas.width
  const scaleY = hostHeight / canvas.height
  const scale =
    canvas.fit === 'contain'
      ? Math.min(scaleX, scaleY)
      : Math.max(scaleX, scaleY)
  return {
    scale,
    x: (hostWidth - canvas.width * scale) * canvas.focus.x,
    y: (hostHeight - canvas.height * scale) * canvas.focus.y,
  }
}

function isTransparentColor(color: string): boolean {
  const c = color.trim().toLowerCase()
  if (!c || c === 'transparent') return true
  if (c === 'rgba(0, 0, 0, 0)' || c === 'rgba(0,0,0,0)') return true
  const m = c.match(
    /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)(?:\s*,\s*([\d.]+))?\s*\)$/,
  )
  if (m?.[4] != null && Number.parseFloat(m[4]) <= 0.02) return true
  return false
}

export function isRenderedPreviewAdapted(
  frame: HTMLIFrameElement,
  canvas: PreviewCanvas,
): boolean {
  const document = frame.contentDocument
  const view = frame.contentWindow
  if (!document?.body || !view) return false

  const documentWidth = Math.max(
    document.documentElement.scrollWidth,
    document.body.scrollWidth,
  )
  const overflowLimit =
    canvas.width + Math.max(64, canvas.width * STORE_PREVIEW_OVERFLOW_TOLERANCE)
  if (documentWidth > overflowLimit) {
    return false
  }

  const bodyText = (document.body.textContent || '').trim()
  if (bodyText.length > 0) return true

  const bodyStyle = view.getComputedStyle(document.body)
  if (bodyStyle.backgroundImage !== 'none') return true
  if (!isTransparentColor(bodyStyle.backgroundColor || '')) return true

  let laidOut = 0

  for (const element of Iterator.from(
    document.body.querySelectorAll('*'),
  ).take(600)) {
    const rect = element.getBoundingClientRect()
    if (
      rect.width < 2 ||
      rect.height < 2 ||
      rect.right <= 0 ||
      rect.bottom <= 0 ||
      rect.left >= canvas.width ||
      rect.top >= canvas.height
    ) {
      continue
    }

    const style = view.getComputedStyle(element)
    if (
      style.display === 'none' ||
      style.visibility === 'hidden' ||
      Number.parseFloat(style.opacity || '1') <= 0.02
    ) {
      continue
    }

    laidOut++

    const hasDirectText = Iterator.from(element.childNodes).some(
      (node) => node.nodeType === 3 && Boolean(node.textContent?.trim()),
    )
    const tagName = element.tagName.toLowerCase()
    const hasRenderedMedia =
      tagName === 'svg' ||
      tagName === 'canvas' ||
      tagName === 'video' ||
      tagName === 'img' ||
      tagName === 'picture'
    const hasPaint =
      style.backgroundImage !== 'none' ||
      style.boxShadow !== 'none' ||
      !isTransparentColor(style.backgroundColor || '') ||
      (style.borderTopWidth !== '0px' &&
        style.borderTopStyle !== 'none' &&
        !isTransparentColor(style.borderTopColor || ''))

    if (hasDirectText || hasRenderedMedia || hasPaint) return true
  }

  return laidOut >= 2
}

export const PREVIEW_LOAD_TIMEOUT_MS = 2800
