import { API_URL } from '../config'
import {
  sanitizeSiteFaviconUrl,
  sanitizeSiteOgImageUrl,
  sanitizeUmamiScriptUrl,
} from './configUrlPolicy'
import {
  SITE_BRAND_ELEMENT_ID,
  SITE_METADATA_CACHE_KEY,
  SITE_METADATA_CACHE_TIME_KEY,
} from './siteMetadataKeys'

export interface SiteMetadata {
  site_title: string
  site_description: string
  site_favicon: string
  site_keywords: string
  site_og_image: string
  google_site_verification: string
  site_noindex: boolean
  ga_measurement_id: string
  umami_website_id: string
  umami_script_url: string
}

const DEFAULT_METADATA: SiteMetadata = {
  site_title: 'Myriad - A myriad of lights, in one place.',
  site_description: 'A myriad of lights, in one place.',
  site_favicon: '/favicon.webp',
  site_keywords: '',
  site_og_image: '',
  google_site_verification: '',
  site_noindex: false,
  ga_measurement_id: '',
  umami_website_id: '',
  umami_script_url: '',
}

const CACHE_DURATION = 5 * 60 * 1000 // 5 min; stale values still paint

function readStoredMetadata(): SiteMetadata | null {
  try {
    const cached = localStorage.getItem(SITE_METADATA_CACHE_KEY)
    if (!cached) return null
    return normalizeMetadata(JSON.parse(cached) as Partial<SiteMetadata>)
  } catch (error) {
    console.warn('[元数据] 读取缓存失败:', error)
    return null
  }
}

function cacheIsFresh(): boolean {
  try {
    const cacheTime = localStorage.getItem(SITE_METADATA_CACHE_TIME_KEY)
    if (!cacheTime) return false
    return Date.now() - Number.parseInt(cacheTime, 10) < CACHE_DURATION
  } catch {
    return false
  }
}

/** Fresh cache by default; `allowStale` keeps last-known title/icon for first paint. */
function getCachedMetadata(allowStale = false): SiteMetadata | null {
  const parsed = readStoredMetadata()
  if (!parsed) return null
  if (allowStale || cacheIsFresh()) return parsed
  return null
}

function cacheMetadata(metadata: SiteMetadata): void {
  try {
    localStorage.setItem(SITE_METADATA_CACHE_KEY, JSON.stringify(metadata))
    localStorage.setItem(SITE_METADATA_CACHE_TIME_KEY, Date.now().toString())
  } catch (error) {
    console.warn('[元数据] 写入缓存失败:', error)
  }
}

function normalizeMetadata(raw: Partial<SiteMetadata> | null | undefined): SiteMetadata {
  const faviconRaw = raw?.site_favicon || DEFAULT_METADATA.site_favicon
  const favicon =
    sanitizeSiteFaviconUrl(faviconRaw) || DEFAULT_METADATA.site_favicon

  const ogRaw = raw?.site_og_image ?? DEFAULT_METADATA.site_og_image
  const ogSanitized = sanitizeSiteOgImageUrl(ogRaw)
  const site_og_image = ogSanitized ?? ''

  const umamiRaw = raw?.umami_script_url ?? DEFAULT_METADATA.umami_script_url
  const umamiSanitized = sanitizeUmamiScriptUrl(umamiRaw)
  const umami_script_url = umamiSanitized ?? ''

  return {
    site_title: raw?.site_title || DEFAULT_METADATA.site_title,
    site_description: raw?.site_description || DEFAULT_METADATA.site_description,
    site_favicon: favicon,
    site_keywords: raw?.site_keywords ?? DEFAULT_METADATA.site_keywords,
    site_og_image,
    google_site_verification:
      typeof raw?.google_site_verification === 'string'
        ? raw.google_site_verification
        : DEFAULT_METADATA.google_site_verification,
    site_noindex: Boolean(raw?.site_noindex),
    ga_measurement_id:
      raw?.ga_measurement_id ?? DEFAULT_METADATA.ga_measurement_id,
    umami_website_id:
      raw?.umami_website_id ?? DEFAULT_METADATA.umami_website_id,
    umami_script_url,
  }
}

async function fetchMetadata(): Promise<SiteMetadata | null> {
  try {
    const apiUrl = API_URL || ''
    const url = apiUrl
      ? `${apiUrl}/api/config/metadata`
      : '/api/config/metadata'

    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), 3000)

    const response = await fetch(url, {
      signal: controller.signal,
    })

    clearTimeout(timeoutId)

    if (response.ok) {
      const data = await response.json()
      return normalizeMetadata(data)
    }
  } catch (error) {
    if ((error as Error).name !== 'AbortError') {
      console.warn('[元数据] 获取失败:', error)
    }
  }

  return null
}

function updateTitle(title: string): void {
  if (document.title !== title) {
    document.title = title
  }
}

function upsertMetaByName(name: string, content: string | null): void {
  let el = document.querySelector<HTMLMetaElement>(`meta[name="${name}"]`)
  if (content === null || content === '') {
    el?.remove()
    return
  }
  if (!el) {
    el = document.createElement('meta')
    el.setAttribute('name', name)
    document.head.appendChild(el)
  }
  if (el.getAttribute('content') !== content) {
    el.setAttribute('content', content)
  }
}

function upsertMetaByProperty(property: string, content: string | null): void {
  let el = document.querySelector<HTMLMetaElement>(
    `meta[property="${property}"]`,
  )
  if (content === null || content === '') {
    el?.remove()
    return
  }
  if (!el) {
    el = document.createElement('meta')
    el.setAttribute('property', property)
    document.head.appendChild(el)
  }
  if (el.getAttribute('content') !== content) {
    el.setAttribute('content', content)
  }
}

function updateDescription(description: string): void {
  const metaDesc = document.querySelector('meta[name="description"]')
  if (metaDesc && metaDesc.getAttribute('content') !== description) {
    metaDesc.setAttribute('content', description)
  } else if (!metaDesc && description) {
    upsertMetaByName('description', description)
  }
}

function toAbsoluteUrl(url: string): string {
  if (!url) return ''
  if (
    url.startsWith('data:') ||
    url.startsWith('http://') ||
    url.startsWith('https://')
  ) {
    return url
  }
  try {
    return new URL(url, window.location.origin).href
  } catch {
    return url
  }
}

function inferFaviconType(faviconUrl: string): string | undefined {
  if (faviconUrl.startsWith('data:image/')) {
    const match = /^data:(image\/[a-zA-Z0-9.+-]+)/.exec(faviconUrl)
    return match?.[1]
  }
  if (faviconUrl.endsWith('.svg') || faviconUrl.includes('.svg?')) {
    return 'image/svg+xml'
  }
  if (faviconUrl.endsWith('.webp') || faviconUrl.includes('.webp?')) {
    return 'image/webp'
  }
  if (faviconUrl.endsWith('.png') || faviconUrl.includes('.png?')) {
    return 'image/png'
  }
  if (faviconUrl.endsWith('.ico') || faviconUrl.includes('.ico?')) {
    return 'image/x-icon'
  }
  if (
    faviconUrl.endsWith('.jpg') ||
    faviconUrl.endsWith('.jpeg') ||
    faviconUrl.includes('.jpg?') ||
    faviconUrl.includes('.jpeg?')
  ) {
    return 'image/jpeg'
  }
  if (faviconUrl.endsWith('.gif') || faviconUrl.includes('.gif?')) {
    return 'image/gif'
  }
  return undefined
}

function updateFavicon(faviconUrl: string): void {
  if (!faviconUrl) return

  const safe = sanitizeSiteFaviconUrl(faviconUrl)
  if (!safe) {
    console.warn('[元数据] 拒绝不安全的 favicon URL')
    return
  }
  faviconUrl = safe

  let favicon = document.querySelector<HTMLLinkElement>('link[rel="icon"]')

  if (!favicon) {
    favicon = document.createElement('link')
    favicon.rel = 'icon'
    document.head.appendChild(favicon)
  }

  const isDataUrl = faviconUrl.startsWith('data:')
  const isExternalUrl =
    faviconUrl.startsWith('http://') || faviconUrl.startsWith('https://')

  const fullUrl = isDataUrl || isExternalUrl
    ? faviconUrl
    : new URL(faviconUrl, window.location.origin).href

  if (favicon.getAttribute('href') === fullUrl || favicon.href === fullUrl) {
    return
  }

  // crossorigin=anonymous CORS-fails on hosts without ACAO and hides the favicon.
  favicon.removeAttribute('crossorigin')

  const mime = inferFaviconType(faviconUrl)
  if (mime) {
    favicon.type = mime
  } else if (isExternalUrl) {
    favicon.type = 'image/webp'
  } else {
    favicon.removeAttribute('type')
  }

  favicon.href = fullUrl
}

export interface PageSeoInput {
  title?: string
  description?: string
  image?: string
  path?: string
  noindex?: boolean
  /** 转载页：noindex 但仍 follow。缺省 noindex 时是 nofollow。 */
  follow?: boolean
}

let baseMetadata: SiteMetadata = { ...DEFAULT_METADATA }
let pageSeo: PageSeoInput | null = null

function resolvePageAbsoluteUrl(pathOrUrl?: string): string {
  if (typeof window === 'undefined') return ''
  if (pathOrUrl && (pathOrUrl.startsWith('http://') || pathOrUrl.startsWith('https://'))) {
    return pathOrUrl
  }
  const path =
    pathOrUrl ||
    `${window.location.pathname}${window.location.search}` ||
    '/'
  try {
    return new URL(path, window.location.origin).href
  } catch {
    return window.location.href
  }
}

function pickShareImage(
  pageImage: string | undefined,
  metadata: SiteMetadata,
): string {
  const candidates = [
    pageImage?.trim(),
    metadata.site_og_image.trim(),
    metadata.site_favicon,
  ].filter(Boolean) as string[]

  for (const raw of candidates) {
    // path/http(s) only; no data:/javascript:.
    const safe = sanitizeSiteOgImageUrl(raw)
    if (!safe) continue
    const abs = toAbsoluteUrl(safe)
    if (abs) return abs
  }
  return ''
}

function upsertCanonicalLink(href: string | null): void {
  let el = document.querySelector<HTMLLinkElement>('link[rel="canonical"]')
  if (!href) {
    el?.remove()
    return
  }
  if (!el) {
    el = document.createElement('link')
    el.rel = 'canonical'
    document.head.appendChild(el)
  }
  if (el.getAttribute('href') !== href) {
    el.href = href
  }
}

function applyEffectiveSeo(): void {
  if (typeof document === 'undefined') return

  const base = baseMetadata
  const page = pageSeo
  const title = (page?.title?.trim() || base.site_title).trim()
  const description = (
    page?.description?.trim() ||
    base.site_description
  ).trim()
  const pageUrl = resolvePageAbsoluteUrl(page?.path)
  const noindex = Boolean(base.site_noindex || page?.noindex)
  const follow = Boolean(page?.follow) && !base.site_noindex
  const ogImage = pickShareImage(page?.image, base)

  updateTitle(title)
  updateDescription(description)

  const keywords = base.site_keywords.trim()
  upsertMetaByName('keywords', keywords || null)
  const gsc = base.google_site_verification.trim()
  const gscSafe = /^[\w-]{1,128}$/.test(gsc) ? gsc : ''
  upsertMetaByName('google-site-verification', gscSafe || null)
  upsertMetaByName(
    'robots',
    noindex
      ? follow
        ? 'noindex, follow'
        : 'noindex, nofollow'
      : 'index, follow',
  )

  upsertMetaByProperty('og:type', 'website')
  upsertMetaByProperty('og:title', title || null)
  upsertMetaByProperty('og:description', description || null)
  upsertMetaByProperty('og:url', pageUrl || null)
  upsertMetaByProperty('og:image', ogImage || null)

  upsertMetaByName('twitter:card', ogImage ? 'summary_large_image' : 'summary')
  upsertMetaByName('twitter:title', title || null)
  upsertMetaByName('twitter:description', description || null)
  upsertMetaByName('twitter:image', ogImage || null)

  upsertCanonicalLink(pageUrl || null)
}

export function setPageSeo(input: PageSeoInput): void {
  pageSeo = { ...input }
  applyEffectiveSeo()
}

export function clearPageSeo(): void {
  pageSeo = null
  applyEffectiveSeo()
}

function applyMetadata(metadata: SiteMetadata): void {
  baseMetadata = normalizeMetadata(metadata)
  updateFavicon(baseMetadata.site_favicon)
  applyEffectiveSeo()
  void import('./googleAnalytics').then((m) => {
    m.configureGoogleAnalytics(baseMetadata.ga_measurement_id)
  })
  void import('./umamiAnalytics').then((m) => {
    m.configureUmami(
      baseMetadata.umami_website_id,
      baseMetadata.umami_script_url,
    )
  })
  void import('./pwa')
    .then((m) => {
      m.updateManifestBranding({
        name: baseMetadata.site_title,
        description: baseMetadata.site_description,
        iconUrl: baseMetadata.site_favicon,
      })
    })
    .catch(() => {
    })
}

function readDocumentBrand(): Partial<SiteMetadata> | null {
  if (typeof document === 'undefined') return null
  const raw = document.getElementById(SITE_BRAND_ELEMENT_ID)?.textContent?.trim()
  if (!raw || raw === '{}') return null
  try {
    const parsed = JSON.parse(raw) as Partial<SiteMetadata>
    if (!parsed || typeof parsed !== 'object') return null
    if (typeof parsed.site_title !== 'string' || !parsed.site_title.trim()) {
      return null
    }
    return parsed
  } catch {
    return null
  }
}

function mergeDocumentBrand(brand: Partial<SiteMetadata>): SiteMetadata {
  const previous = readStoredMetadata() ?? DEFAULT_METADATA
  const next = normalizeMetadata({
    ...previous,
    site_title: brand.site_title || previous.site_title,
    site_description: brand.site_description || previous.site_description,
    site_favicon: brand.site_favicon || previous.site_favicon,
    site_og_image: brand.site_og_image || previous.site_og_image,
  })
  cacheMetadata(next)
  return next
}

/** Sync: document first-byte brand, else last-known cache, before usePageSeo. */
export function applyStoredSiteMetadata(): void {
  if (typeof window === 'undefined') return
  const fromDoc = readDocumentBrand()
  const next = fromDoc ? mergeDocumentBrand(fromDoc) : getCachedMetadata(true)
  if (!next) return
  baseMetadata = next
  updateFavicon(baseMetadata.site_favicon)
  applyEffectiveSeo()
}

applyStoredSiteMetadata()

let isInitialized = false
let initPromise: Promise<void> | null = null

export async function initSiteMetadata(): Promise<void> {
  if (initPromise) {
    return initPromise
  }

  if (isInitialized) {
    return
  }

  initPromise = (async () => {
    const cached = getCachedMetadata(true)
    if (cached) {
      applyMetadata(cached)
    }

    const fetched = await fetchMetadata()
    if (fetched) {
      cacheMetadata(fetched)
      if (!cached || JSON.stringify(cached) !== JSON.stringify(fetched)) {
        applyMetadata(fetched)
      }
    }

    isInitialized = true
    initPromise = null
  })()

  return initPromise
}

export async function refreshSiteMetadata(): Promise<void> {
  localStorage.removeItem(SITE_METADATA_CACHE_KEY)
  localStorage.removeItem(SITE_METADATA_CACHE_TIME_KEY)
  isInitialized = false
  initPromise = null
  await initSiteMetadata()
}

export function getCurrentMetadata(): SiteMetadata {
  return getCachedMetadata(true) ?? baseMetadata ?? DEFAULT_METADATA
}

export function formatPageTitle(pageTitle: string): string {
  const site = getCurrentMetadata().site_title.trim()
  const page = pageTitle.trim()
  if (!page) return site
  if (!site || page === site) return page
  if (site.startsWith(page)) return site
  return `${page} · ${site}`
}

export function tappIconAsOgImage(icon?: string | null): string | undefined {
  if (!icon || typeof icon !== 'string') return undefined
  const trimmed = icon.trim()
  if (!trimmed || trimmed.startsWith('data:')) return undefined
  if (trimmed.startsWith('<svg') || trimmed.startsWith('<?xml')) return undefined
  if (!trimmed.includes('/') && !trimmed.includes('.')) return undefined
  if (
    trimmed.startsWith('http://') ||
    trimmed.startsWith('https://') ||
    trimmed.startsWith('/')
  ) {
    return trimmed
  }
  return undefined
}
