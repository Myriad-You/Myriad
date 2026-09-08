/**
 * 网站元数据管理工具
 * 统一管理网站标题、描述、图标、SEO（keywords / OG / robots）与 GA4
 */

import { API_URL } from '../config'
import {
  sanitizeSiteFaviconUrl,
  sanitizeSiteOgImageUrl,
  sanitizeUmamiScriptUrl,
} from './configUrlPolicy'
import { configureGoogleAnalytics } from './googleAnalytics'
import { configureUmami } from './umamiAnalytics'

export interface SiteMetadata {
  site_title: string
  site_description: string
  site_favicon: string
  /** 逗号分隔关键词；空串表示不写 keywords meta */
  site_keywords: string
  /** 社交分享预览图 URL；空则回退 favicon */
  site_og_image: string
  /** Google Search Console HTML 标签验证码；空则不写 meta */
  google_site_verification: string
  /** true → robots noindex,nofollow */
  site_noindex: boolean
  /** GA4 Measurement ID（G-XXXXXXXX）；空则不加载 gtag */
  ga_measurement_id: string
  /** Umami website id；与 script url 同时有值才加载 */
  umami_website_id: string
  /** Umami tracker 脚本 URL */
  umami_script_url: string
}

// 默认元数据
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

// 缓存键名
const CACHE_KEY = 'site_metadata'
const CACHE_TIME_KEY = 'site_metadata_time'
const CACHE_DURATION = 5 * 60 * 1000 // 5分钟缓存

/**
 * 从缓存中获取元数据
 */
function getCachedMetadata(): SiteMetadata | null {
  try {
    const cached = localStorage.getItem(CACHE_KEY)
    const cacheTime = localStorage.getItem(CACHE_TIME_KEY)

    if (cached && cacheTime) {
      const age = Date.now() - Number.parseInt(cacheTime)
      if (age < CACHE_DURATION) {
        return normalizeMetadata(JSON.parse(cached))
      }
    }
  } catch (error) {
    console.warn('[元数据] 读取缓存失败:', error)
  }
  return null
}

/**
 * 缓存元数据
 */
function cacheMetadata(metadata: SiteMetadata): void {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(metadata))
    localStorage.setItem(CACHE_TIME_KEY, Date.now().toString())
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
  // null = rejected junk → clear; '' = intentionally empty
  const site_og_image = ogSanitized === null ? '' : ogSanitized

  const umamiRaw = raw?.umami_script_url ?? DEFAULT_METADATA.umami_script_url
  const umamiSanitized = sanitizeUmamiScriptUrl(umamiRaw)
  const umami_script_url = umamiSanitized === null ? '' : umamiSanitized

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

/**
 * 从后端获取元数据
 */
async function fetchMetadata(): Promise<SiteMetadata | null> {
  try {
    // 如果 API_URL 为空，使用相对路径（生产环境）
    const apiUrl = API_URL || ''
    const url = apiUrl
      ? `${apiUrl}/api/config/metadata`
      : '/api/config/metadata'

    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), 3000) // 3秒超时

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

/**
 * 更新页面标题（防止闪烁）
 */
function updateTitle(title: string): void {
  if (document.title !== title) {
    document.title = title
  }
}

/**
 * 确保存在指定 name 的 meta 标签并设置 content
 */
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

/**
 * 确保存在指定 property 的 meta 标签并设置 content（Open Graph）
 */
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

/**
 * 更新页面描述
 */
function updateDescription(description: string): void {
  const metaDesc = document.querySelector('meta[name="description"]')
  if (metaDesc && metaDesc.getAttribute('content') !== description) {
    metaDesc.setAttribute('content', description)
  } else if (!metaDesc && description) {
    upsertMetaByName('description', description)
  }
}

/**
 * 绝对化资源 URL（相对路径 → origin；data/http 原样）
 */
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

/**
 * 从路径或 data URL 推断 favicon MIME type
 */
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

/**
 * 更新网站图标（支持站外链接、相对路径、data URL 本地上传）
 */
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

  // data: 与较长 base64 用字符串比较；避免浏览器规范化差异时重复写
  if (favicon.getAttribute('href') === fullUrl || favicon.href === fullUrl) {
    return
  }

  // Tab icons are display-only. `crossorigin=anonymous` forces a CORS fetch
  // that fails on static hosts without ACAO and hides the favicon. Canvas
  // readback (PWA compose) is a separate fetch and skips those URLs.
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

/**
 * 单页 SEO 覆盖（路由级 title/OG/canonical）。
 * 站级配置仍是底稿；有 page override 时叠在上面。
 */
export interface PageSeoInput {
  /** 完整 document.title；缺省用站级 title */
  title?: string
  description?: string
  /** 分享图；相对路径会绝对化；data: 会被忽略 */
  image?: string
  /**
   * 规范路径或绝对 URL（如 `/tapp/run/com.example`）。
   * 用于 og:url / canonical；缺省用当前 location。
   */
  path?: string
  /** true → robots noindex,nofollow（与站级 noindex 取或） */
  noindex?: boolean
}

/** 当前生效的站级元数据（含未写缓存时的默认） */
let baseMetadata: SiteMetadata = { ...DEFAULT_METADATA }
/** 路由级覆盖；离开页面时应 clearPageSeo */
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
    // Soft policy: path/http(s) only (no data:/javascript:). Private hosts OK.
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

/**
 * 把站级 + 路由级 SEO 写到 head（title / description / robots / OG / twitter / canonical）
 */
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
  const ogImage = pickShareImage(page?.image, base)

  updateTitle(title)
  updateDescription(description)

  const keywords = base.site_keywords.trim()
  upsertMetaByName('keywords', keywords || null)
  const gsc = base.google_site_verification.trim()
  const gscSafe = /^[\w-]{1,128}$/.test(gsc) ? gsc : ''
  upsertMetaByName('google-site-verification', gscSafe || null)
  upsertMetaByName('robots', noindex ? 'noindex, nofollow' : 'index, follow')

  upsertMetaByProperty('og:type', 'website')
  upsertMetaByProperty('og:title', title || null)
  upsertMetaByProperty('og:description', description || null)
  // 分享卡片应对准「当前页」而不是站根
  upsertMetaByProperty('og:url', pageUrl || null)
  upsertMetaByProperty('og:image', ogImage || null)

  upsertMetaByName('twitter:card', ogImage ? 'summary_large_image' : 'summary')
  upsertMetaByName('twitter:title', title || null)
  upsertMetaByName('twitter:description', description || null)
  upsertMetaByName('twitter:image', ogImage || null)

  upsertCanonicalLink(pageUrl || null)
}

/**
 * 设置当前路由的 SEO（进入页面时调用；离开时 clearPageSeo）
 */
export function setPageSeo(input: PageSeoInput): void {
  pageSeo = { ...input }
  applyEffectiveSeo()
}

/**
 * 清除路由 SEO，恢复站级默认（并保留当前 path 作为 og:url）
 */
export function clearPageSeo(): void {
  pageSeo = null
  applyEffectiveSeo()
}

/**
 * 应用站级元数据到页面（favicon / analytics + 有效 SEO）
 */
function applyMetadata(metadata: SiteMetadata): void {
  baseMetadata = normalizeMetadata(metadata)
  updateFavicon(baseMetadata.site_favicon)
  applyEffectiveSeo()
  configureGoogleAnalytics(baseMetadata.ga_measurement_id)
  configureUmami(baseMetadata.umami_website_id, baseMetadata.umami_script_url)
  // Keep installable PWA name + icons in sync with site branding when PWA is on.
  // Icons are composed from site_favicon (white fill for transparency, scaled logo).
  void import('./pwa')
    .then((m) => {
      m.updateManifestBranding({
        name: baseMetadata.site_title,
        description: baseMetadata.site_description,
        iconUrl: baseMetadata.site_favicon,
      })
    })
    .catch(() => {
      /* optional */
    })
}

// 标记是否已经初始化（防止重复初始化）
let isInitialized = false
let initPromise: Promise<void> | null = null

/**
 * 初始化网站元数据（在页面加载时立即调用）
 * 策略：
 * 1. 优先使用缓存（避免闪烁）
 * 2. 异步获取后端数据库数据
 * 3. 如果都失败，使用默认值
 *
 * 注意：数据库数据优先级高于环境变量
 */
export async function initSiteMetadata(): Promise<void> {
  // 如果已经在初始化中，返回现有的 Promise
  if (initPromise) {
    return initPromise
  }

  // 如果已经初始化过，直接返回
  if (isInitialized) {
    return
  }

  initPromise = (async () => {
    // 1. 先尝试使用缓存（立即应用，避免闪烁）
    const cached = getCachedMetadata()
    if (cached) {
      applyMetadata(cached)
    }

    // 2. 异步获取后端数据库的最新数据（数据库优先）
    const fetched = await fetchMetadata()
    if (fetched) {
      cacheMetadata(fetched)
      // 只有当数据真的变化时才更新（减少 DOM 操作）
      if (!cached || JSON.stringify(cached) !== JSON.stringify(fetched)) {
        applyMetadata(fetched)
      }
    } else if (!cached) {
      // 3. 如果缓存和数据库都失败，使用默认值
      applyMetadata(DEFAULT_METADATA)
    }

    isInitialized = true
    initPromise = null
  })()

  return initPromise
}

/**
 * 强制刷新元数据（清除缓存并重新从数据库获取）
 */
export async function refreshSiteMetadata(): Promise<void> {
  localStorage.removeItem(CACHE_KEY)
  localStorage.removeItem(CACHE_TIME_KEY)
  isInitialized = false
  initPromise = null
  await initSiteMetadata()
}

/**
 * 获取当前元数据
 */
export function getCurrentMetadata(): SiteMetadata {
  return getCachedMetadata() || baseMetadata || DEFAULT_METADATA
}

/**
 * 用站级 title 拼页面标题：`页面 · 站点名`
 */
export function formatPageTitle(pageTitle: string): string {
  const site = getCurrentMetadata().site_title.trim()
  const page = pageTitle.trim()
  if (!page) return site
  if (!site || page === site) return page
  // 避免 "Foo · Foo - bar" 重复
  if (site.startsWith(page)) return site
  return `${page} · ${site}`
}

/**
 * 从 Tapp icon 字段挑可用于 og:image 的 URL（排除 emoji / data: / 内联 SVG）
 */
export function tappIconAsOgImage(icon?: string | null): string | undefined {
  if (!icon || typeof icon !== 'string') return undefined
  const trimmed = icon.trim()
  if (!trimmed || trimmed.startsWith('data:')) return undefined
  if (trimmed.startsWith('<svg') || trimmed.startsWith('<?xml')) return undefined
  // 单字符/emoji 不是 URL
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
