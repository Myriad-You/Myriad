/** Dev always unregisters SW (must not cache API/proxy). */

import { API_URL } from '../config'
import { proxyImageUrl } from './proxyImageUrl'
import { getUIConfigDeduped } from './requestDedup'

const SW_URL = '/sw.js'
const MANIFEST_HREF = '/manifest.webmanifest'
const APPLE_TOUCH_HREF = '/icons/pwa/icon-192.png'
export const SITE_ICON_API_PATH = '/api/config/site-icon'

export const PWA_ICON_BACKGROUND = '#ffffff'

export const DEFAULT_PWA_LOGO_SCALE = 0.8
export const PWA_LOGO_SCALE_MIN = 0.35
export const PWA_LOGO_SCALE_MAX = 1

const MASKABLE_SAFE_ZONE = 0.8

let lastAppliedEnabled: boolean | null = null
let applyInFlight: Promise<void> | null = null
let brandingGeneration = 0
let lastAppleTouchHref: string | null = null
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

  if (!enabled) {
    document
      .querySelectorAll(
        'meta[name="apple-mobile-web-app-title"], meta[name="application-name"]',
      )
      .forEach((el) => el.remove())
  }
}

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

export function absolutizeManifestUrls(
  manifest: Record<string, unknown>,
  origin: string,
): Record<string, unknown> {
  const next: Record<string, unknown> = { ...manifest }

  const startUrl = resolveManifestUrl(manifest.start_url ?? '/', origin)
  if (startUrl) next.start_url = startUrl

  const scope = resolveManifestUrl(manifest.scope ?? '/', origin)
  if (scope) next.scope = scope

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

export function clampPwaLogoScale(scale: unknown): number {
  const n = typeof scale === 'number' ? scale : Number(scale)
  if (!Number.isFinite(n)) return DEFAULT_PWA_LOGO_SCALE
  return Math.min(
    PWA_LOGO_SCALE_MAX,
    Math.max(PWA_LOGO_SCALE_MIN, n),
  )
}

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

function siteIconEndpoint(apiBase = ''): string {
  const base = apiBase.replaceAll(/\/$/g, '')
  return base ? `${base}${SITE_ICON_API_PATH}` : SITE_ICON_API_PATH
}

/** Same-origin / data raw; hotlink CDNs via proxy; other hosts via site-icon. */
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
    const proxied = proxyImageUrl(abs.href)
    if (proxied && proxied.includes('/api/proxy/image')) {
      if (apiBase) {
        const base = apiBase.replaceAll(/\/$/g, '')
        const q = proxied.indexOf('/api/proxy/image')
        if (q >= 0) return `${base}${proxied.slice(q)}`
      }
      return proxied
    }
    return siteIconEndpoint(apiBase)
  } catch {
    return trimmed
  }
}

/** Canvas readback needs CORS. */
export function pwaIconIsCanvasReadable(src: string, origin: string): boolean {
  const trimmed = src.trim()
  if (!trimmed) return false
  if (trimmed.startsWith('data:') || trimmed.startsWith('blob:')) return true
  if (trimmed.includes('/api/proxy/image')) return true
  if (trimmed.includes(SITE_ICON_API_PATH)) return true
  try {
    return new URL(trimmed, origin).origin === new URL(origin).origin
  } catch {
    return false
  }
}

export function inferPwaIconMime(iconUrl: string): string | undefined {
  if (iconUrl.startsWith('data:image/')) {
    const match = /^data:(image\/[a-zA-Z0-9.+-]+)/.exec(iconUrl)
    return match?.[1]
  }
  if (iconUrl.endsWith('.svg') || iconUrl.includes('.svg?')) return 'image/svg+xml'
  if (iconUrl.endsWith('.webp') || iconUrl.includes('.webp?')) return 'image/webp'
  if (iconUrl.endsWith('.png') || iconUrl.includes('.png?')) return 'image/png'
  if (iconUrl.endsWith('.ico') || iconUrl.includes('.ico?')) return 'image/x-icon'
  if (
    iconUrl.endsWith('.jpg') ||
    iconUrl.endsWith('.jpeg') ||
    iconUrl.includes('.jpg?') ||
    iconUrl.includes('.jpeg?')
  ) {
    return 'image/jpeg'
  }
  if (iconUrl.endsWith('.gif') || iconUrl.includes('.gif?')) return 'image/gif'
  return undefined
}

/** When canvas compose fails, still point the manifest at the site icon. */
export function fallbackManifestIcons(
  iconUrl: string,
  origin: string,
): ManifestIcon[] {
  const src = resolveManifestUrl(iconUrl, origin)
  if (!src) return []
  const type = inferPwaIconMime(iconUrl)
  return [
    {
      src,
      sizes: 'any',
      ...(type ? { type } : {}),
      purpose: 'any',
    },
  ]
}

type DrawableImage = CanvasImageSource & {
  width: number
  height: number
}

const inflightIconBlobs = new Map<string, Promise<Blob>>()

function fetchIconBlob(src: string): Promise<Blob> {
  const existing = inflightIconBlobs.get(src)
  if (existing) return existing
  const pending = (async () => {
    const response = await fetch(src, {
      mode: 'cors',
      credentials: 'omit',
      cache: 'force-cache',
    })
    if (!response.ok) {
      throw new Error(`[PWA] failed to load icon source (HTTP ${response.status}): ${src.slice(0, 120)}`)
    }
    const blob = await response.blob()
    if (blob.size < 16) {
      throw new Error(`[PWA] failed to load icon source (empty body): ${src.slice(0, 120)}`)
    }
    return blob
  })().finally(() => {
    inflightIconBlobs.delete(src)
  })
  inflightIconBlobs.set(src, pending)
  return pending
}

async function loadImageForCanvas(src: string): Promise<DrawableImage> {
  const fail = (reason: string) =>
    new Error(`[PWA] failed to load icon source (${reason}): ${src.slice(0, 120)}`)

  if (!src.startsWith('blob:')) {
    try {
      const blob = await fetchIconBlob(src)
      if (typeof createImageBitmap === 'function') {
        try {
          const bitmap = await createImageBitmap(blob)
          if (bitmap.width > 0 && bitmap.height > 0) {
            return bitmap
          }
          bitmap.close()
        } catch {
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
      if (src.startsWith('data:')) {
        return loadHtmlImage(src)
      }
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
  if (dataUrl.length < 64) {
    throw new Error('[PWA] composed PNG data URL is too small')
  }
  return dataUrl
}

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

export function updateManifestBranding(options: {
  name?: string
  description?: string
  themeColor?: string
  iconUrl?: string
  logoScale?: number
  iconBackground?: string
}): void {
  if (typeof document === 'undefined' || !import.meta.env.PROD) return

  lastBrandingOptions = { ...options }
  if (lastAppliedEnabled === false) return

  const name = options.name?.trim()
  if (!name) return

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
        } else {
          const fallback = fallbackManifestIcons(iconUrl, origin)
          if (fallback.length > 0) {
            branded.icons = fallback
            setAppleTouchIconHref(fallback[0]!.src as string)
          }
        }
      }

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

export async function applyPwaEnabled(enabled: boolean): Promise<void> {
  if (applyInFlight) {
    await applyInFlight
  }

  const run = async () => {
    // Dev: strip SW so HMR/API are never cached.
    if (!isProdBrowser()) {
      ensureManifestLink(false)
      ensureAppleTouchIcon(false)
      setAppleWebAppMeta(false)
      const hadController =
        supportsServiceWorker() && Boolean(navigator.serviceWorker.controller)
      await unregisterAllServiceWorkers()
      await clearMyriadCaches()
      lastAppliedEnabled = false
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
  return true
}

export async function syncPwaFromServer(): Promise<boolean> {
  let enabled = true
  try {
    // Shares the shell's in-flight request; the 3s cap only bounds our wait.
    const data = (await Promise.race([
      getUIConfigDeduped(),
      new Promise<never>((_, reject) => setTimeout(reject, 3000)),
    ])) as { pwa_enabled?: unknown }
    enabled = parsePwaEnabled(data?.pwa_enabled)
  } catch {
  }
  await applyPwaEnabled(enabled)
  return enabled
}

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
