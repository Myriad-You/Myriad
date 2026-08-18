/**
 * Progressive Web App lifecycle: Service Worker + web app manifest.
 *
 * Controlled by site setting `pwa_enabled` (default true). Production only —
 * dev always unregisters SW so local API/proxy work is not hijacked by cache.
 *
 * Install icons are composed at runtime from `site_favicon` (site logo):
 * white canvas background (transparent logos), contained logo with controllable
 * scale, 192/512 + maskable PNGs as **data:** URLs (not blob:). Chrome's
 * installability pipeline cannot fetch page-scoped blob: icon URLs, which made
 * the install affordance appear then vanish after branding.
 * Unproxied cross-origin favicons are skipped (no ACAO → canvas taint / CORS
 * error); the static `/icons/pwa/*` set stays installable.
 */

import { API_URL } from '../config'
import { proxyImageUrl } from './proxyImageUrl'

const SW_URL = '/sw.js'
const MANIFEST_HREF = '/manifest.webmanifest'
const APPLE_TOUCH_HREF = '/icons/pwa/icon-192.png'

/** Solid fill behind site logos that have transparency (iOS/Android icons). */
export const PWA_ICON_BACKGROUND = '#ffffff'

/**
 * How large the logo is drawn inside the square icon (0.35–1).
 * 0.8 leaves ~10% padding on each side for “any” purpose icons.
 */
export const DEFAULT_PWA_LOGO_SCALE = 0.8
export const PWA_LOGO_SCALE_MIN = 0.35
export const PWA_LOGO_SCALE_MAX = 1

/** Maskable safe-zone is ~80% of the canvas; scale logo relative to that. */
const MASKABLE_SAFE_ZONE = 0.8

let lastAppliedEnabled: boolean | null = null
let applyInFlight: Promise<void> | null = null
let brandingGeneration = 0
/** Last composed apple-touch href (data: or static); kept across SW re-apply. */
let lastAppleTouchHref: string | null = null
/** Last branding payload so re-enabling PWA can rebuild icons without a full reload. */
let lastBrandingOptions: {
  name?: string
  description?: string
  themeColor?: string
  iconUrl?: string
  logoScale?: number
  iconBackground?: string
} | null = null

function isProdBrowser(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof navigator !== 'undefined' &&
    // Vite / Astro: DEV is true in astro dev; PROD when built.
    Boolean(import.meta.env.PROD)
  )
}

function supportsServiceWorker(): boolean {
  return typeof navigator !== 'undefined' && 'serviceWorker' in navigator
}

function ensureManifestLink(enabled: boolean): void {
  if (typeof document === 'undefined') return

  const existing = document.querySelectorAll<HTMLLinkElement>(
    'link[rel="manifest"]',
  )
  if (!enabled) {
    existing.forEach((el) => el.remove())
    return
  }

  let link = existing[0]
  if (!link) {
    link = document.createElement('link')
    link.rel = 'manifest'
    document.head.appendChild(link)
  }
  // Do not overwrite a branded blob: manifest with the static file.
  const href = link.getAttribute('href') || ''
  if (!href || href === MANIFEST_HREF) {
    link.href = MANIFEST_HREF
  }
}

function setAppleTouchIconHref(href: string): void {
  if (typeof document === 'undefined') return
  lastAppleTouchHref = href
  let link = document.querySelector<HTMLLinkElement>(
    'link[rel="apple-touch-icon"]',
  )
  if (!link) {
    link = document.createElement('link')
    link.rel = 'apple-touch-icon'
    document.head.appendChild(link)
  }
  if (link.getAttribute('href') !== href) {
    link.href = href
  }
}

function ensureAppleTouchIcon(enabled: boolean): void {
  if (typeof document === 'undefined') return

  const existing = document.querySelectorAll<HTMLLinkElement>(
    'link[rel="apple-touch-icon"]',
  )
  if (!enabled) {
    existing.forEach((el) => el.remove())
    lastAppleTouchHref = null
    return
  }

  const preferred = lastAppleTouchHref || APPLE_TOUCH_HREF
  setAppleTouchIconHref(preferred)
}

function setAppleWebAppMeta(enabled: boolean): void {
  if (typeof document === 'undefined') return

  const names = [
    'mobile-web-app-capable',
    'apple-mobile-web-app-capable',
  ] as const

  for (const name of names) {
    let el = document.querySelector<HTMLMetaElement>(`meta[name="${name}"]`)
    if (!enabled) {
      el?.remove()
      continue
    }
    if (!el) {
      el = document.createElement('meta')
      el.setAttribute('name', name)
      document.head.appendChild(el)
    }
    el.setAttribute('content', 'yes')
  }

  // Keep install title meta only while PWA is on; content is set by branding.
  if (!enabled) {
    document
      .querySelectorAll(
        'meta[name="apple-mobile-web-app-title"], meta[name="application-name"]',
      )
      .forEach((el) => el.remove())
  }
}

/**
 * Sync home-screen / install title metas with site branding.
 * (Static HTML ships `Myriad` as SSR fallback until metadata loads.)
 */
function setInstallAppTitle(title: string): void {
  if (typeof document === 'undefined') return
  const value = title.trim()
  if (!value) return

  for (const name of ['apple-mobile-web-app-title', 'application-name'] as const) {
    let el = document.querySelector<HTMLMetaElement>(`meta[name="${name}"]`)
    if (!el) {
      el = document.createElement('meta')
      el.setAttribute('name', name)
      document.head.appendChild(el)
    }
    if (el.getAttribute('content') !== value) {
      el.setAttribute('content', value)
    }
  }
}

/**
 * Resolve a manifest URL field against the document origin.
 * Blob-served manifests treat relative paths as invalid (they resolve against
 * `blob:https://host/uuid` rather than the site origin).
 *
 * `data:` and `blob:` icons are already absolute — leave them unchanged.
 * Prefer `data:` for install icons; `blob:` icons break Chrome installability.
 */
export function resolveManifestUrl(
  value: unknown,
  origin: string,
): string | undefined {
  if (typeof value !== 'string' || !value.trim()) return undefined
  const trimmed = value.trim()
  if (trimmed.startsWith('data:') || trimmed.startsWith('blob:')) {
    return trimmed
  }
  try {
    return new URL(trimmed, origin).href
  } catch {
    return undefined
  }
}

interface ManifestIcon {
  src?: string
  sizes?: string
  type?: string
  purpose?: string
  [key: string]: unknown
}

/**
 * Rewrite relative start_url / scope / id / icon src to absolute URLs so a
 * blob: manifest link remains installable.
 */
export function absolutizeManifestUrls(
  manifest: Record<string, unknown>,
  origin: string,
): Record<string, unknown> {
  const next: Record<string, unknown> = { ...manifest }

  const startUrl = resolveManifestUrl(manifest.start_url ?? '/', origin)
  if (startUrl) next.start_url = startUrl

  const scope = resolveManifestUrl(manifest.scope ?? '/', origin)
  if (scope) next.scope = scope

  // `id` is resolved as a URL against the manifest URL in the installability
  // pipeline; keep it absolute for the same blob: reason.
  const id = resolveManifestUrl(manifest.id ?? '/', origin)
  if (id) next.id = id

  if (Array.isArray(manifest.icons)) {
    next.icons = (manifest.icons as ManifestIcon[]).map((icon) => {
      if (!icon || typeof icon !== 'object') return icon
      const src = resolveManifestUrl(icon.src, origin)
      return src ? { ...icon, src } : { ...icon }
    })
  }

  return next
}

/** Clamp logo scale used when compositing site logo into PWA icons. */
export function clampPwaLogoScale(scale: unknown): number {
  const n = typeof scale === 'number' ? scale : Number(scale)
  if (!Number.isFinite(n)) return DEFAULT_PWA_LOGO_SCALE
  return Math.min(
    PWA_LOGO_SCALE_MAX,
    Math.max(PWA_LOGO_SCALE_MIN, n),
  )
}

/**
 * Contain-fit the logo inside a square canvas at `logoScale` of the canvas size.
 * Pure geometry — unit-tested without canvas.
 */
export function computeContainedLogoRect(
  sourceWidth: number,
  sourceHeight: number,
  canvasSize: number,
  logoScale: number,
): { x: number; y: number; width: number; height: number } {
  const scale = clampPwaLogoScale(logoScale)
  const maxSide = canvasSize * scale
  if (
    !Number.isFinite(sourceWidth) ||
    !Number.isFinite(sourceHeight) ||
    sourceWidth <= 0 ||
    sourceHeight <= 0 ||
    canvasSize <= 0
  ) {
    const fallback = Math.max(0, maxSide)
    return {
      x: (canvasSize - fallback) / 2,
      y: (canvasSize - fallback) / 2,
      width: fallback,
      height: fallback,
    }
  }
  const aspect = sourceWidth / sourceHeight
  let width: number
  let height: number
  if (aspect >= 1) {
    width = maxSide
    height = maxSide / aspect
  } else {
    height = maxSide
    width = maxSide * aspect
  }
  return {
    x: (canvasSize - width) / 2,
    y: (canvasSize - height) / 2,
    width,
    height,
  }
}

/**
 * Resolve site logo URL for canvas readback.
 * Same-origin / data: stay as-is; hotlink CDNs use image proxy (CORS + Referer);
 * other external hosts keep original URL (dual-path — not on proxy allowlist).
 */
export function resolvePwaIconSourceUrl(
  iconUrl: string,
  origin: string,
  apiBase = '',
): string {
  const trimmed = iconUrl.trim()
  if (!trimmed) return trimmed
  if (trimmed.startsWith('data:')) return trimmed
  if (trimmed.startsWith('blob:')) return trimmed
  try {
    const abs = new URL(trimmed, origin)
    if (abs.origin === origin || abs.origin === new URL(origin).origin) {
      return abs.href
    }
    // Dual-path via shared proxyImageUrl (needs-proxy hosts only).
    // Temporarily override API_URL base if caller passed a custom apiBase.
    const proxied = proxyImageUrl(abs.href)
    if (!proxied) return abs.href
    if (apiBase && proxied.includes('/api/proxy/image')) {
      const base = apiBase.replace(/\/$/, '')
      // Re-base absolute API host if proxyImageUrl used CONFIG API_URL
      const q = proxied.indexOf('/api/proxy/image')
      if (q >= 0) return `${base}${proxied.slice(q)}`
    }
    return proxied
  } catch {
    return trimmed
  }
}

/**
 * Canvas readback needs a CORS-clean bitmap. Same-origin, data/blob, and the
 * image proxy qualify. Unproxied cross-origin URLs usually have no ACAO
 * (static file hosts, personal CDNs) — fetching them with mode:cors only
 * produces a console error and cannot be drawn.
 */
export function pwaIconIsCanvasReadable(src: string, origin: string): boolean {
  const trimmed = src.trim()
  if (!trimmed) return false
  if (trimmed.startsWith('data:') || trimmed.startsWith('blob:')) return true
  if (trimmed.includes('/api/proxy/image')) return true
  try {
    return new URL(trimmed, origin).origin === new URL(origin).origin
  } catch {
    return false
  }
}

type DrawableImage = CanvasImageSource & {
  width: number
  height: number
}

/**
 * Load a logo for canvas draw. Prefer fetch + createImageBitmap (better ICO /
 * odd MIME handling); fall back to HTMLImageElement.
 */
async function loadImageForCanvas(src: string): Promise<DrawableImage> {
  const fail = (reason: string) =>
    new Error(`[PWA] failed to load icon source (${reason}): ${src.slice(0, 120)}`)

  // Same-origin proxy / data: — fetch avoids partial Image() ICO failures.
  if (!src.startsWith('blob:')) {
    try {
      const response = await fetch(src, {
        mode: 'cors',
        credentials: 'omit',
        cache: 'force-cache',
      })
      if (!response.ok) {
        throw fail(`HTTP ${response.status}`)
      }
      const blob = await response.blob()
      if (blob.size < 16) {
        throw fail('empty body')
      }
      if (typeof createImageBitmap === 'function') {
        try {
          const bitmap = await createImageBitmap(blob)
          if (bitmap.width > 0 && bitmap.height > 0) {
            return bitmap
          }
          bitmap.close()
        } catch {
          /* try Image() below with object URL */
        }
      }
      const objectUrl = URL.createObjectURL(blob)
      try {
        return await loadHtmlImage(objectUrl)
      } finally {
        try {
          URL.revokeObjectURL(objectUrl)
        } catch {
          /* ignore */
        }
      }
    } catch (error) {
      // Fall through to direct Image() for data: or when fetch is blocked.
      if (src.startsWith('data:')) {
        return loadHtmlImage(src)
      }
      // Last attempt: Image with crossOrigin (proxy may still work).
      try {
        return await loadHtmlImage(src, true)
      } catch {
        throw error instanceof Error ? error : fail(String(error))
      }
    }
  }

  return loadHtmlImage(src, !src.startsWith('data:'))
}

function loadHtmlImage(
  src: string,
  crossOrigin = false,
): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    if (crossOrigin && !src.startsWith('data:')) {
      img.crossOrigin = 'anonymous'
    }
    img.onload = () => {
      if ((img.naturalWidth || img.width) <= 0) {
        reject(new Error('[PWA] icon decoded with zero size'))
        return
      }
      resolve(img)
    }
    img.onerror = () =>
      reject(
        new Error(`[PWA] failed to load icon source: ${src.slice(0, 120)}`),
      )
    img.src = src
  })
}

function sourcePixelSize(img: DrawableImage): { width: number; height: number } {
  if (img instanceof HTMLImageElement) {
    return {
      width: img.naturalWidth || img.width,
      height: img.naturalHeight || img.height,
    }
  }
  return { width: img.width, height: img.height }
}

/**
 * Export canvas as a PNG data URL for web-app-manifest icons.
 * data: is self-contained and installable; blob: is not (out-of-process fetch).
 */
export function canvasToPngDataUrl(canvas: HTMLCanvasElement): string {
  let dataUrl: string
  try {
    dataUrl = canvas.toDataURL('image/png')
  } catch (error) {
    throw new Error(
      `[PWA] canvas.toDataURL failed (tainted canvas?): ${
        error instanceof Error ? error.message : String(error)
      }`,
    )
  }
  if (!dataUrl.startsWith('data:image/png')) {
    throw new Error('[PWA] canvas.toDataURL did not return image/png data URL')
  }
  // Reject near-empty exports (encode failure / fully transparent glitch).
  if (dataUrl.length < 64) {
    throw new Error('[PWA] composed PNG data URL is too small')
  }
  return dataUrl
}

/**
 * Draw a loaded logo onto a square canvas and export PNG data URL.
 */
function renderLogoToPngDataUrl(
  img: DrawableImage,
  size: number,
  logoScale: number,
  background: string,
): string {
  const canvasSize = Math.max(16, Math.floor(size))
  const canvas = document.createElement('canvas')
  canvas.width = canvasSize
  canvas.height = canvasSize
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('[PWA] 2d context unavailable')

  ctx.fillStyle = background.trim() || PWA_ICON_BACKGROUND
  ctx.fillRect(0, 0, canvasSize, canvasSize)

  const pixels = sourcePixelSize(img)
  const rect = computeContainedLogoRect(
    pixels.width,
    pixels.height,
    canvasSize,
    logoScale,
  )
  ctx.drawImage(img, rect.x, rect.y, rect.width, rect.height)

  return canvasToPngDataUrl(canvas)
}

/**
 * Compose one square PWA PNG: solid background + contained site logo.
 * Returns a data:image/png URL safe for web app manifest installability.
 */
export async function composePwaIconPng(options: {
  sourceUrl: string
  size: number
  /** 0.35–1; default DEFAULT_PWA_LOGO_SCALE */
  logoScale?: number
  /** CSS color; default white for transparent logos */
  background?: string
  origin?: string
  apiBase?: string
}): Promise<string> {
  const origin =
    options.origin ||
    (typeof window !== 'undefined' ? window.location.origin : 'http://localhost')
  const fetchUrl = resolvePwaIconSourceUrl(
    options.sourceUrl,
    origin,
    options.apiBase ?? API_URL ?? '',
  )
  if (!pwaIconIsCanvasReadable(fetchUrl, origin)) {
    throw new Error(
      '[PWA] icon source is cross-origin without CORS; cannot compose',
    )
  }
  const img = await loadImageForCanvas(fetchUrl)
  try {
    return renderLogoToPngDataUrl(
      img,
      options.size,
      options.logoScale ?? DEFAULT_PWA_LOGO_SCALE,
      options.background?.trim() || PWA_ICON_BACKGROUND,
    )
  } finally {
    if (typeof ImageBitmap !== 'undefined' && img instanceof ImageBitmap) {
      img.close()
    }
  }
}

async function composeBrandedIconSet(options: {
  iconUrl: string
  logoScale: number
  background: string
  origin: string
}): Promise<{
  icons: ManifestIcon[]
  appleTouch: string
} | null> {
  let img: DrawableImage | null = null
  try {
    const scale = clampPwaLogoScale(options.logoScale)
    const maskableScale = clampPwaLogoScale(scale * MASKABLE_SAFE_ZONE)
    const bg = options.background.trim() || PWA_ICON_BACKGROUND
    const fetchUrl = resolvePwaIconSourceUrl(
      options.iconUrl,
      options.origin,
      API_URL || '',
    )
    if (!pwaIconIsCanvasReadable(fetchUrl, options.origin)) {
      return null
    }
    // Decode once — reuse for 192 / 512 / maskable (ICO + proxy more reliable).
    img = await loadImageForCanvas(fetchUrl)

    const icon192 = renderLogoToPngDataUrl(img, 192, scale, bg)
    const icon512 = renderLogoToPngDataUrl(img, 512, scale, bg)
    const maskable512 = renderLogoToPngDataUrl(img, 512, maskableScale, bg)

    for (const src of [icon192, icon512, maskable512]) {
      if (!src.startsWith('data:image/png')) {
        throw new Error('[PWA] composed icon is not a PNG data URL')
      }
    }

    return {
      appleTouch: icon192,
      icons: [
        {
          src: icon192,
          sizes: '192x192',
          type: 'image/png',
          purpose: 'any',
        },
        {
          src: icon512,
          sizes: '512x512',
          type: 'image/png',
          purpose: 'any',
        },
        {
          src: maskable512,
          sizes: '512x512',
          type: 'image/png',
          purpose: 'maskable',
        },
      ],
    }
  } catch (error) {
    console.warn('[PWA] site logo icon compose failed; using static icons', error)
    return null
  } finally {
    if (
      img &&
      typeof ImageBitmap !== 'undefined' &&
      img instanceof ImageBitmap
    ) {
      img.close()
    }
  }
}

/**
 * Update manifest name/short_name/icons from site branding when possible.
 * Manifest document is served as a blob: URL (no backend endpoint). Icons use
 * **data:** PNG URLs so Chrome installability still holds after branding.
 *
 * Relative start_url / scope / id / static icon paths are absolutized — blob
 * manifests resolve relative URLs against `blob:https://host/uuid`.
 */
export function updateManifestBranding(options: {
  name?: string
  description?: string
  themeColor?: string
  /** Site logo / favicon URL (relative, absolute, or data:) */
  iconUrl?: string
  /**
   * Logo size inside the icon square (0.35–1). Default 0.8.
   * Maskable icons use 80% of this so the mark stays in the safe zone.
   */
  logoScale?: number
  /** Icon canvas fill; default `#ffffff` for transparent logos */
  iconBackground?: string
}): void {
  if (typeof document === 'undefined' || !import.meta.env.PROD) return

  // Always remember latest branding so toggling PWA back on can recompose.
  lastBrandingOptions = { ...options }
  if (lastAppliedEnabled === false) return

  const name = options.name?.trim()
  if (!name) return

  // iOS/Android install title (home screen label) — sync even before blob manifest lands.
  setInstallAppTitle(name)

  const origin = window.location.origin
  const gen = ++brandingGeneration
  const iconUrl = options.iconUrl?.trim() || ''
  const logoScale = clampPwaLogoScale(options.logoScale)
  const iconBackground =
    options.iconBackground?.trim() || PWA_ICON_BACKGROUND

  void (async () => {
    try {
      const response = await fetch(MANIFEST_HREF)
      if (!response.ok) return
      const base = (await response.json()) as Record<string, unknown> | null
      if (!base || gen !== brandingGeneration) return

      const short = name.length > 12 ? name.slice(0, 12) : name
      const branded: Record<string, unknown> = {
        ...base,
        name,
        short_name: short,
        ...(options.description?.trim()
          ? { description: options.description.trim() }
          : {}),
        ...(options.themeColor?.trim()
          ? {
              theme_color: options.themeColor.trim(),
              background_color: options.themeColor.trim(),
            }
          : {}),
      }

      if (iconUrl) {
        const composed = await composeBrandedIconSet({
          iconUrl,
          logoScale,
          background: iconBackground,
          origin,
        })
        if (gen !== brandingGeneration) return
        if (composed) {
          branded.icons = composed.icons
          setAppleTouchIconHref(composed.appleTouch)
        }
      }

      // data: icons pass through; HTTPS static icons are absolutized.
      const next = absolutizeManifestUrls(branded, origin)
      const blob = new Blob([JSON.stringify(next)], {
        type: 'application/manifest+json',
      })
      const url = URL.createObjectURL(blob)
      let link = document.querySelector<HTMLLinkElement>('link[rel="manifest"]')
      if (!link) {
        link = document.createElement('link')
        link.rel = 'manifest'
        document.head.appendChild(link)
      }
      const prev = link.href
      link.href = url
      if (prev.startsWith('blob:')) {
        try {
          URL.revokeObjectURL(prev)
        } catch {
          /* ignore */
        }
      }
    } catch {
      /* keep static manifest */
    }
  })()
}

async function clearMyriadCaches(): Promise<void> {
  if (!('caches' in globalThis)) return
  try {
    const keys = await caches.keys()
    await Promise.all(
      keys
        .filter((key) => key.startsWith('myriad-'))
        .map((key) => caches.delete(key)),
    )
  } catch {
    /* ignore */
  }
}

async function unregisterAllServiceWorkers(): Promise<void> {
  if (!supportsServiceWorker()) return
  try {
    const registrations = await navigator.serviceWorker.getRegistrations()
    await Promise.all(registrations.map((r) => r.unregister()))
  } catch {
    /* ignore */
  }
}

async function registerServiceWorker(): Promise<void> {
  if (!supportsServiceWorker() || !isProdBrowser()) return
  try {
    await navigator.serviceWorker.register(SW_URL)
  } catch (error) {
    console.warn('[PWA] Service Worker registration failed:', error)
  }
}

/**
 * Apply PWA on/off: SW + manifest + Apple install meta.
 * Safe to call repeatedly; skips work when state is unchanged.
 */
export async function applyPwaEnabled(enabled: boolean): Promise<void> {
  if (applyInFlight) {
    await applyInFlight
  }

  const run = async () => {
    // Dev: always strip SW so HMR / API proxy are never cached by production SW.
    if (!isProdBrowser()) {
      ensureManifestLink(false)
      ensureAppleTouchIcon(false)
      setAppleWebAppMeta(false)
      const hadController =
        supportsServiceWorker() && Boolean(navigator.serviceWorker.controller)
      await unregisterAllServiceWorkers()
      await clearMyriadCaches()
      lastAppliedEnabled = false
      // One-shot reload so an already-controlling SW drops control after unregister.
      if (
        hadController &&
        !sessionStorage.getItem('myriad-dev-sw-reset')
      ) {
        sessionStorage.setItem('myriad-dev-sw-reset', '1')
        window.location.reload()
      }
      return
    }

    if (lastAppliedEnabled === enabled) return
    lastAppliedEnabled = enabled

    ensureManifestLink(enabled)
    ensureAppleTouchIcon(enabled)
    setAppleWebAppMeta(enabled)

    if (enabled) {
      await registerServiceWorker()
      // Rebuild name/icons from last site metadata (static → site logo).
      if (lastBrandingOptions) {
        updateManifestBranding(lastBrandingOptions)
      }
    } else {
      await unregisterAllServiceWorkers()
      await clearMyriadCaches()
      lastAppleTouchHref = null
    }
  }

  applyInFlight = run().finally(() => {
    applyInFlight = null
  })
  await applyInFlight
}

function parsePwaEnabled(raw: unknown): boolean {
  if (typeof raw === 'boolean') return raw
  if (typeof raw === 'string') {
    const s = raw.trim().toLowerCase()
    if (s === 'false' || s === '0' || s === 'off' || s === 'no') return false
    return true
  }
  // Missing key → keep historical default (PWA on)
  return true
}

/**
 * Read public UI config and sync PWA state.
 * Call on boot and after admin saves `pwa_enabled`.
 */
export async function syncPwaFromServer(): Promise<boolean> {
  let enabled = true
  try {
    const base = API_URL || ''
    const url = base ? `${base}/api/config/ui` : '/api/config/ui'
    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), 3000)
    const response = await fetch(url, { signal: controller.signal })
    clearTimeout(timeoutId)
    if (response.ok) {
      const data = (await response.json()) as { pwa_enabled?: unknown }
      enabled = parsePwaEnabled(data?.pwa_enabled)
    }
  } catch {
    // Network failure: leave prior state; first boot still applies default true via apply.
  }
  await applyPwaEnabled(enabled)
  return enabled
}

/**
 * Boot entry for Astro shell scripts: after load, sync from server.
 * DEV unregisters; PROD registers only when `pwa_enabled` is true.
 */
export function initPwaLifecycle(): void {
  if (typeof window === 'undefined') return

  const run = () => {
    void syncPwaFromServer()
  }

  if (document.readyState === 'complete') {
    run()
  } else {
    window.addEventListener('load', run, { once: true })
  }
}
