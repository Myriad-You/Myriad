/**
 * 壁纸管理 Hook
 *
 * 提供壁纸加载、刷新和状态管理功能
 * 确保壁纸URL和颜色提取的一致性
 *
 * @module useWallpaper
 */

import type { WallpaperErrorCopy } from '../utils/wallpaperError'
import { useCallback, useEffect, useState } from 'react'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { fetchJsonWithRetry } from '../utils/apiRetry'
import { cssBackgroundImage } from '../utils/cssUrl'
import { loadImagePooled } from '../utils/objectPool'
import { proxyImageUrl } from '../utils/proxyImageUrl'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { getCacheInfo } from '../utils/wallpaperColorCache'
import {

  wallpaperUnknownMessage,
} from '../utils/wallpaperError'
import {
  areUrlsEquivalent,
  effectiveWallpaperBlur,
  extractBackgroundUrl,
  normalizeWallpaperUrl,
  wallpaperState,
} from '../utils/wallpaperState'
import { sanitizeWallpaperUrl } from '../utils/wallpaperUrlPolicy'

export { areUrlsEquivalent, normalizeWallpaperUrl }

// 类型定义

/** 公开 API 应为 boolean；兼容网关/旧缓存把 true/false 序列化成字符串的情况 */
function asConfigBool(value: unknown, defaultValue: boolean): boolean {
  if (typeof value === 'boolean') return value
  if (typeof value === 'number') return value !== 0
  if (typeof value === 'string') {
    const s = value.trim().toLowerCase()
    if (s === 'true' || s === '1' || s === 'yes') return true
    if (s === 'false' || s === '0' || s === 'no' || s === '') return false
  }
  if (value == null) return defaultValue
  return defaultValue
}

function asConfigNumber(value: unknown, defaultValue: number): number {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value === 'string' && value.trim() !== '') {
    const n = Number(value)
    if (Number.isFinite(n)) return n
  }
  return defaultValue
}

interface WallpaperConfig {
  wallpaper_url: string
  wallpaper_blur: number
  // Evocative 壁纸动效配置
  evocative_parallax: boolean
  evocative_dynamic_blur: boolean
  evocative_ripple: boolean
  evocative_fps: number
  evocative_ripple_quality: number
}

interface LoadWallpaperResult {
  /** 验证后的实际URL */
  actualUrl: string
  /** 模糊度 */
  blur: number
  /** URL是否经过验证 */
  verified: boolean
  /** @deprecated 使用 evocative 替代 */
  parallaxEnabled: boolean
  /** Evocative 壁纸动效配置 */
  evocative: {
    parallax: boolean
    dynamicBlur: boolean
    ripple: boolean
    fps: number
    rippleQuality: number
  }
}

// 常量

/**
 * Bundled fallback when `wallpaper_url` is empty (unset site + first-run setup).
 * Same-origin WebP under `public/wallpapers/`.
 */
export const DEFAULT_FALLBACK_WALLPAPER_URL = '/wallpapers/default.webp'

/** 图片加载超时时间 */
const IMAGE_LOAD_TIMEOUT = 15000

/** 壁纸元素ID */
const WALLPAPER_ELEMENT_ID = 'wallpaper'

/** 随机图片服务列表 */
const RANDOM_IMAGE_SERVICES = [
  'picsum.photos',
  'loremflickr.com',
  'source.unsplash.com',
  'unsplash.com/random',
  'api.unsplash.com',
  'bing.com/hpimagearchive',
] as const

/** 静态CDN标识 */
const STATIC_CDN_INDICATORS = [
  'cdn.',
  'static.',
  '/static/',
  '/images/',
  '/assets/',
  '/uploads/',
] as const

/** 图片扩展名 */
const IMAGE_EXTENSIONS = [
  '.jpg',
  '.jpeg',
  '.png',
  '.gif',
  '.webp',
  '.bmp',
  '.svg',
] as const

/** 动态脚本扩展名 */
const DYNAMIC_EXTENSIONS = ['.php', '.jsp', '.asp', '.aspx', '.py'] as const

// 工具函数

/**
 * 从颜色缓存中获取一个不同于当前URL的已缓存壁纸
 * 利用 wallpaperColorCache 的缓存信息，避免重复维护缓存
 * @param currentUrl 当前壁纸URL
 * @returns 缓存中的其他壁纸URL，如果没有则返回null
 */
function getCachedAlternativeWallpaper(
  currentUrl: string | null,
): string | null {
  try {
    const cacheInfo = getCacheInfo()
    if (!cacheInfo.exists || !cacheInfo.items || cacheInfo.items.length < 2) {
      return null
    }

    // 从颜色缓存中提取完整URL（getCacheInfo返回的是截断的URL用于调试）
    // 需要直接读取localStorage获取完整URL
    const cached = localStorage.getItem('myriad_wallpaper_color_cache_v5')
    if (!cached) return null

    const store = JSON.parse(cached)
    if (!store.items || store.items.length < 2) return null

    // 过滤掉当前URL和过期项
    const now = Date.now()
    const CACHE_DURATION_MS = 6 * 60 * 60 * 1000 // 6小时
    const alternatives = store.items.filter(
      (item: { url: string; timestamp: number }) => {
        // 过滤过期项
        if (now - item.timestamp > CACHE_DURATION_MS) return false
        // 过滤当前URL
        if (areUrlsEquivalent(item.url, currentUrl)) return false
        return true
      },
    )

    if (alternatives.length === 0) return null

    // 随机选择一个（须再过策略：缓存可能含历史脏 URL）
    const safeAlts = alternatives
      .map((item: { url: string }) => sanitizeWallpaperUrl(item.url))
      .filter((u: string | null): u is string => !!u)
    if (safeAlts.length === 0) return null
    const randomIndex = Math.floor(Math.random() * safeAlts.length)
    return safeAlts[randomIndex]
  } catch {
    return null
  }
}

/**
 * 判断URL是否为单一静态图片链接
 * 返回true表示是固定的静态图片，不应显示刷新按钮
 */
function isStaticImageUrl(url: string): boolean {
  if (!url) return true

  const lowerUrl = url.toLowerCase()

  // 随机图片服务 → 可刷新
  if (RANDOM_IMAGE_SERVICES.some((service) => lowerUrl.includes(service))) {
    return false
  }

  // 包含 /random 或 /daily 路径 → 可刷新
  if (lowerUrl.includes('/random') || lowerUrl.includes('/daily')) {
    return false
  }

  // 动态脚本 → 可刷新
  if (DYNAMIC_EXTENSIONS.some((ext) => lowerUrl.endsWith(ext))) {
    return false
  }

  // 不以图片扩展名结尾 → 可能是API → 可刷新
  const endsWithImage = IMAGE_EXTENSIONS.some((ext) => lowerUrl.endsWith(ext))
  if (!endsWithImage) {
    return false
  }

  // 静态CDN图片 → 不可刷新
  if (STATIC_CDN_INDICATORS.some((indicator) => lowerUrl.includes(indicator))) {
    return true
  }

  // 默认可刷新（保守策略）
  return false
}

/**
 * 从常见图床 / 随机图 API 的 JSON 中抽出图片 URL。
 * 支持：url / image / img / src / pic / data.url / images[0].url 等。
 */
function extractImageUrlFromJson(data: unknown): string | null {
  if (!data || typeof data !== 'object') return null
  const obj = data as Record<string, unknown>

  const tryString = (v: unknown): string | null => {
    if (typeof v !== 'string') return null
    // Policy rejects data:/private hosts/non-http schemes (incl. data:image/svg+xml)
    return sanitizeWallpaperUrl(v)
  }

  for (const key of [
    'url',
    'image',
    'img',
    'src',
    'pic',
    'photo',
    'image_url',
    'imgurl',
    'img_url',
  ]) {
    const hit = tryString(obj[key])
    if (hit) return hit
  }

  // nested: data.url / data.image / result.url
  for (const nestKey of ['data', 'result', 'payload', 'images']) {
    const nested = obj[nestKey]
    if (Array.isArray(nested) && nested.length > 0) {
      const first = nested[0]
      if (typeof first === 'string') {
        const hit = tryString(first)
        if (hit) return hit
      }
      if (first && typeof first === 'object') {
        const fromFirst = extractImageUrlFromJson(first)
        if (fromFirst) return fromFirst
      }
    }
    if (nested && typeof nested === 'object') {
      const fromNest = extractImageUrlFromJson(nested)
      if (fromNest) return fromNest
    }
  }

  return null
}

/**
 * Accept only policy-safe final URLs after redirects / JSON extraction.
 * proxyImageUrl may rewrite to absolute `http://localhost…/api/proxy/image?...`
 * in dev — normalize to path form so host policy does not reject same-app proxy.
 */
function finalizeWallpaperUrl(
  candidate: string | null | undefined,
): string | null {
  if (!candidate) return null
  let proxied = proxyImageUrl(candidate) || candidate
  const proxyMarker = '/api/proxy/image'
  const idx = proxied.indexOf(proxyMarker)
  if (idx >= 0) {
    proxied = proxied.slice(idx)
  }
  return sanitizeWallpaperUrl(proxied)
}

/**
 * Path ends with a common image extension → treat as direct asset URL.
 * No network probe needed; applyWallpaperToDOM confirms via Image() preload.
 */
function isLikelyDirectImageUrl(url: string): boolean {
  try {
    const path = new URL(
      url,
      typeof location !== 'undefined' ? location.href : 'https://local.invalid',
    ).pathname.toLowerCase()
    return IMAGE_EXTENSIONS.some((ext) => path.endsWith(ext))
  } catch {
    return false
  }
}

/**
 * Resolve non-direct wallpaper URLs without HEAD.
 * Many CDNs/image hosts reject HEAD; SW Cache API also cannot put HEAD.
 * GET only: follow redirects, detect image/* vs JSON 图床 API, extract final URL.
 * Display validity is confirmed later via Image() preload in applyWallpaperToDOM.
 */
async function resolveImageUrlViaGet(url: string): Promise<string | null> {
  const getResp = await fetch(url, {
    method: 'GET',
    redirect: 'follow',
    headers: { Accept: 'application/json, image/*, */*' },
  })
  const getType = getResp.headers.get('content-type') || ''
  if (getType.includes('image/')) {
    // Drain body so the connection can be reused; Image() will fetch for display.
    try {
      await getResp.blob()
    } catch {
      /* ignore body read errors */
    }
    return finalizeWallpaperUrl(getResp.url || url)
  }
  if (getType.includes('json') || getType.includes('text/')) {
    const text = await getResp.text()
    try {
      const data = JSON.parse(text)
      const extracted = extractImageUrlFromJson(data)
      const final = finalizeWallpaperUrl(extracted)
      if (final) return final
    } catch {
      /* not JSON */
    }
  }
  return finalizeWallpaperUrl(getResp.url || url)
}

/**
 * 获取实际的图片 URL：
 * 1) 策略校验（scheme / 主机）
 * 2) 直链图片：跳过探测，交给 Image() 预加载验证（从不使用 HEAD）
 * 3) API / 无扩展名：GET 跟随 302 或解析 JSON 图床
 * 4) 失败则回退原始 URL（仍须通过策略）
 */
async function resolveImageUrl(
  apiUrl: string,
  bustCache = false,
): Promise<string> {
  const base = sanitizeWallpaperUrl(apiUrl)
  if (!base) return ''

  const url = bustCache
    ? base.includes('?')
      ? `${base}&t=${Date.now()}`
      : `${base}?t=${Date.now()}`
    : base

  // Direct image assets (path ends with image extension): no network probe.
  // applyWallpaperToDOM → preloadImage (Image()) is the availability check.
  if (isLikelyDirectImageUrl(url)) {
    return finalizeWallpaperUrl(url) || ''
  }

  try {
    const viaGet = await resolveImageUrlViaGet(url)
    if (viaGet) return viaGet
  } catch {
    /* network / CORS — fall back to sanitized URL; Image() will verify */
  }

  return finalizeWallpaperUrl(url) || ''
}

/**
 * 预加载图片（使用对象池）
 * 直接加载图片，不使用代理
 * @returns 加载成功返回true，失败返回false
 */
function preloadImage(
  url: string,
  timeout = IMAGE_LOAD_TIMEOUT,
): Promise<boolean> {
  // 使用池化的图片加载，减少 GC 压力
  return loadImagePooled(url, { timeout })
}

function setWallpaperAwaiting(active: boolean) {
  const bg = document.getElementById('bg-container')
  if (!bg) return
  bg.classList.toggle('wallpaper-awaiting', active)
}

/**
 * 应用壁纸到DOM并更新全局状态
 * 首次加载：预加载完成后淡入，背景层用呼吸占位避免白屏突兀。
 * @param imageUrl 目标图片URL
 * @param blur 模糊度
 * @param forceRefresh 是否强制刷新（即使URL相同）
 * @returns 验证后的URL，失败返回null
 */
async function applyWallpaperToDOM(
  imageUrl: string,
  blur: number,
  forceRefresh = false,
  copy: WallpaperErrorCopy,
): Promise<string | null> {
  const wallpaperEl = document.getElementById(WALLPAPER_ELEMENT_ID)
  if (!wallpaperEl) {
    console.warn('壁纸元素不存在')
    return null
  }

  const safeUrl = sanitizeWallpaperUrl(imageUrl)
  if (!safeUrl) {
    console.warn('壁纸 URL 未通过安全策略，已拒绝应用')
    wallpaperState.setError(copy.unsafeUrl)
    return null
  }
  // Use sanitized URL for the rest of apply
  imageUrl = safeUrl

  // 检查是否需要更新：如果当前壁纸与目标相同且不是强制刷新，跳过
  const currentUrl = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)
  if (!forceRefresh && currentUrl && areUrlsEquivalent(currentUrl, imageUrl)) {
    // 已经是目标壁纸，只需更新模糊度（如果不同）
    const currentFilter = wallpaperEl.style.filter
    const targetFilter = `blur(${effectiveWallpaperBlur(blur)}px)`
    if (currentFilter !== targetFilter) {
      wallpaperEl.style.filter = targetFilter
    }
    // 确保状态同步
    wallpaperState.updateState(imageUrl, blur)
    wallpaperEl.classList.add('wallpaper-visible')
    setWallpaperAwaiting(false)
    return imageUrl
  }

  const hadVisibleWallpaper =
    wallpaperEl.classList.contains('wallpaper-visible') && !!currentUrl

  // 标记加载状态 + 首次加载呼吸占位
  wallpaperState.setLoading(true)
  if (!hadVisibleWallpaper) {
    setWallpaperAwaiting(true)
    wallpaperEl.classList.remove('wallpaper-visible')
  } else {
    // 切换壁纸时先轻微淡出，再换图淡入
    wallpaperEl.classList.add('wallpaper-fading')
    wallpaperEl.classList.remove('wallpaper-visible')
  }

  try {
    // 预加载图片
    const loaded = await preloadImage(imageUrl)
    if (!loaded) {
      wallpaperState.setError(copy.imageLoadFailed)
      wallpaperEl.classList.remove('wallpaper-fading')
      if (hadVisibleWallpaper) {
        wallpaperEl.classList.add('wallpaper-visible')
      }
      return null
    }

    // 再次检查：预加载期间可能已经切换到目标壁纸
    const currentUrlAfterLoad = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)
    if (
      !forceRefresh &&
      currentUrlAfterLoad &&
      areUrlsEquivalent(currentUrlAfterLoad, imageUrl)
    ) {
      wallpaperState.updateState(imageUrl, blur)
      wallpaperEl.classList.remove('wallpaper-fading')
      wallpaperEl.classList.add('wallpaper-visible')
      setWallpaperAwaiting(false)
      return imageUrl
    }

    // 应用到 DOM（先不可见，再渐显）；强制 url("...") 防 CSS 注入/截断
    wallpaperEl.style.backgroundImage = cssBackgroundImage(imageUrl)
    wallpaperEl.style.filter = `blur(${effectiveWallpaperBlur(blur)}px)`
    wallpaperEl.classList.remove('wallpaper-fading')

    // 更新全局状态
    wallpaperState.updateState(imageUrl, blur)

    // 等两帧再淡入，保证 background-image 已提交绘制
    await new Promise((resolve) => requestAnimationFrame(resolve))
    await new Promise((resolve) => requestAnimationFrame(resolve))
    wallpaperEl.classList.add('wallpaper-visible')
    setWallpaperAwaiting(false)

    // 验证DOM是否已更新
    const appliedUrl = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)

    if (appliedUrl && areUrlsEquivalent(appliedUrl, imageUrl)) {
      return imageUrl
    }

    // 二次验证
    await new Promise((resolve) => setTimeout(resolve, 50))
    const retryUrl = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)

    if (retryUrl && areUrlsEquivalent(retryUrl, imageUrl)) {
      return imageUrl
    }

    console.warn('壁纸应用验证失败', { expected: imageUrl, actual: retryUrl })
    return retryUrl || imageUrl
  } catch (error) {
    wallpaperState.setError(wallpaperUnknownMessage(error, copy))
    wallpaperEl.classList.remove('wallpaper-fading')
    if (hadVisibleWallpaper) {
      wallpaperEl.classList.add('wallpaper-visible')
    }
    return null
  } finally {
    wallpaperState.setLoading(false)
    // Keep awaiting if we never got a visible wallpaper (failed first load)
    if (!wallpaperEl.classList.contains('wallpaper-visible')) {
      setWallpaperAwaiting(true)
    }
  }
}

/**
 * 获取壁纸配置（带自动重试）
 */
async function fetchWallpaperConfig(): Promise<WallpaperConfig | null> {
  console.debug('[Wallpaper] Fetching wallpaper config...')
  try {
    // 先走去重缓存（启动时与其他 config/ui 消费方共享同一次请求），
    // 失败再退回带重试的独立请求，保证壁纸这一视觉核心的健壮性
    const data = await getUIConfigDeduped().catch(() =>
      fetchJsonWithRetry<any>(`${API_URL}/api/config/ui`, {
        maxRetries: 3,
        timeout: 10000,
        onRetry: (error, attempt, delay) => {
          console.warn(
            `壁纸配置获取失败 (尝试 ${attempt}): ${error.message}. ${delay}ms后重试...`,
          )
        },
      }),
    )

    const evocative = {
      evocative_parallax: asConfigBool(data.evocative_parallax, true),
      evocative_dynamic_blur: asConfigBool(data.evocative_dynamic_blur, false),
      evocative_ripple: asConfigBool(data.evocative_ripple, false),
      evocative_fps: asConfigNumber(data.evocative_fps, 30),
      evocative_ripple_quality: asConfigNumber(
        data.evocative_ripple_quality,
        0.85,
      ),
    }

    // Apply-time policy: drop legacy DB junk (javascript:/private hosts/data:)
    const rawWallpaper =
      typeof data.wallpaper_url === 'string' ? data.wallpaper_url : ''
    const wallpaper_url = rawWallpaper
      ? sanitizeWallpaperUrl(rawWallpaper) || ''
      : ''

    console.debug('[Wallpaper] Config received:', {
      wallpaper_url,
      blur: data.wallpaper_blur,
      evocative: {
        parallax: evocative.evocative_parallax,
        dynamicBlur: evocative.evocative_dynamic_blur,
        ripple: evocative.evocative_ripple,
        fps: evocative.evocative_fps,
        rippleQuality: evocative.evocative_ripple_quality,
      },
    })

    // 即使没有壁纸 URL，也返回动效开关（避免图挂了/URL 空时整条 evocative 被丢掉）
    return {
      wallpaper_url,
      wallpaper_blur: asConfigNumber(data.wallpaper_blur, 3),
      ...evocative,
    }
  } catch (error) {
    console.error('壁纸配置获取失败:', error)
    return null
  }
}

// Hook 实现

/**
 * loadWallpaper 去重机制
 * 多个组件同时调用 loadWallpaper 时，只执行一次实际加载
 */
let pendingLoadWallpaper: Promise<LoadWallpaperResult | null> | null = null
let lastLoadTimestamp = 0
const LOAD_DEBOUNCE_MS = 1000 // 1秒内的重复调用直接返回上次结果
let lastLoadResult: LoadWallpaperResult | null = null

/** Drop debounce cache so config save → wallpaperConfigChanged always reloads. */
export function invalidateWallpaperLoadCache(): void {
  lastLoadResult = null
  lastLoadTimestamp = 0
  pendingLoadWallpaper = null
}

/**
 * 壁纸管理 Hook
 */
export function useWallpaper() {
  const { t } = useI18n()
  const wallpaperCopy = t.wallpaperStatus
  const [wallpaperUrl, setWallpaperUrl] = useState<string>('')
  const [canRefresh, setCanRefresh] = useState<boolean>(false)
  const [blur, setBlur] = useState<number>(3)
  const [isLoading, setIsLoading] = useState<boolean>(false)

  // 订阅全局状态变化
  useEffect(() => {
    return wallpaperState.subscribe((snapshot) => {
      setIsLoading(snapshot.isLoading)
    })
  }, [])

  /**
   * 加载壁纸配置和显示（带去重）
   * 多个组件同时调用时，只执行一次实际加载
   */
  const loadWallpaper =
    useCallback(async (): Promise<LoadWallpaperResult | null> => {
      const now = Date.now()
      console.debug('[Wallpaper] loadWallpaper called')

      // 1秒内的重复调用，直接返回上次结果
      if (lastLoadResult && now - lastLoadTimestamp < LOAD_DEBOUNCE_MS) {
        console.debug('[Wallpaper] Returning cached result (debounce)')
        // 同步本地状态
        if (lastLoadResult.actualUrl) {
          setWallpaperUrl(lastLoadResult.actualUrl)
          setBlur(lastLoadResult.blur)
        }
        return lastLoadResult
      }

      // 如果有正在进行的加载，等待其完成
      if (pendingLoadWallpaper) {
        console.debug('[Wallpaper] Waiting for pending load...')
        const result = await pendingLoadWallpaper
        // 同步本地状态
        if (result?.actualUrl) {
          setWallpaperUrl(result.actualUrl)
          setBlur(result.blur)
        }
        return result
      }

      console.debug('[Wallpaper] Starting new load...')
      // 执行实际加载
      const doLoad = async (): Promise<LoadWallpaperResult | null> => {
        const defaultEvocative = {
          parallax: true,
          dynamicBlur: false,
          ripple: false,
          fps: 30,
          rippleQuality: 0.85,
        }

        /** Last-resort paint so exception / offline never leaves #wallpaper blank. */
        const applyBundledFallback = async (
          blur = 3,
          evocative: LoadWallpaperResult['evocative'] = defaultEvocative,
        ): Promise<LoadWallpaperResult | null> => {
          try {
            console.debug('[Wallpaper] Applying bundled fallback')
            const verifiedUrl = await applyWallpaperToDOM(
              DEFAULT_FALLBACK_WALLPAPER_URL,
              blur,
              true,
              wallpaperCopy,
            )
            const finalUrl = verifiedUrl || DEFAULT_FALLBACK_WALLPAPER_URL
            const result: LoadWallpaperResult = {
              actualUrl: finalUrl,
              blur,
              verified:
                !!verifiedUrl &&
                areUrlsEquivalent(verifiedUrl, DEFAULT_FALLBACK_WALLPAPER_URL),
              parallaxEnabled: evocative.parallax,
              evocative,
            }
            lastLoadResult = result
            lastLoadTimestamp = Date.now()
            if (verifiedUrl) {
              setWallpaperUrl(verifiedUrl)
            }
            setBlur(blur)
            setCanRefresh(false)
            return result
          } catch (fallbackError) {
            console.error('[Wallpaper] Bundled fallback failed:', fallbackError)
            return null
          }
        }

        try {
          const config = await fetchWallpaperConfig()

          // Config missing (offline / pre-setup): still paint bundled fallback.
          const evocative = config
            ? {
                parallax: config.evocative_parallax,
                dynamicBlur: config.evocative_dynamic_blur,
                ripple: config.evocative_ripple,
                fps: config.evocative_fps,
                rippleQuality: config.evocative_ripple_quality,
              }
            : defaultEvocative

          const blur = config?.wallpaper_blur ?? 3
          const configuredUrl = config?.wallpaper_url ?? ''

          // 动效开关与壁纸图解耦：图失败时仍要把 evocative 交给 AppLayout
          const buildResult = (
            actualUrl: string,
            verified: boolean,
          ): LoadWallpaperResult => ({
            actualUrl,
            blur,
            verified,
            parallaxEnabled: evocative.parallax,
            evocative,
          })

          const applyAndFinish = async (
            imageUrl: string,
            /** True when image is the configured URL (not bundled fallback). */
            fromConfig: boolean,
            /** Force DOM re-apply when switching off a previous wallpaper. */
            forceRefresh = false,
          ): Promise<LoadWallpaperResult> => {
            const verifiedUrl = await applyWallpaperToDOM(
              imageUrl,
              blur,
              forceRefresh,
              wallpaperCopy,
            )
            const finalUrl = verifiedUrl || imageUrl
            const result = buildResult(
              finalUrl,
              !!verifiedUrl && areUrlsEquivalent(verifiedUrl, imageUrl),
            )
            lastLoadResult = result
            lastLoadTimestamp = Date.now()
            if (verifiedUrl) {
              setWallpaperUrl(verifiedUrl)
            }
            setBlur(blur)
            setCanRefresh(
              fromConfig && configuredUrl
                ? !isStaticImageUrl(configuredUrl)
                : false,
            )
            return result
          }

          // Unconfigured / empty → bundled WebP (incl. first-run setup).
          if (!configuredUrl) {
            console.debug(
              '[Wallpaper] No wallpaper_url; applying bundled fallback',
            )
            // forceRefresh: replace whatever was previously painted (clear case).
            return applyAndFinish(DEFAULT_FALLBACK_WALLPAPER_URL, false, true)
          }

          const actualUrl = await resolveImageUrl(configuredUrl)

          // 验证URL有效性（已配置但无效：不伪装成默认壁纸，保留空结果便于排查）
          if (!actualUrl || actualUrl.includes('/api/proxy/music/')) {
            const result = buildResult('', false)
            lastLoadResult = result
            lastLoadTimestamp = Date.now()
            setBlur(blur)
            setCanRefresh(false)
            return result
          }

          const verifiedUrl = await applyWallpaperToDOM(
            actualUrl,
            blur,
            false,
            wallpaperCopy,
          )

          if (!verifiedUrl) {
            const result = buildResult(actualUrl, false)
            lastLoadResult = result
            lastLoadTimestamp = Date.now()
            setBlur(blur)
            setCanRefresh(!isStaticImageUrl(configuredUrl))
            return result
          }

          const result = buildResult(
            verifiedUrl,
            areUrlsEquivalent(verifiedUrl, actualUrl),
          )

          lastLoadResult = result
          lastLoadTimestamp = Date.now()

          setWallpaperUrl(verifiedUrl)
          setBlur(blur)
          setCanRefresh(!isStaticImageUrl(configuredUrl))

          return result
        } catch (error) {
          console.error('加载壁纸失败:', error)
          // Exception path: still paint bundled default so first-run / API
          // throws never leave the surface blank.
          return applyBundledFallback()
        }
      }

      // 设置 pending Promise
      pendingLoadWallpaper = doLoad()

      try {
        const result = await pendingLoadWallpaper
        return result
      } finally {
        // 清除 pending（延迟清除，避免并发问题）
        setTimeout(() => {
          pendingLoadWallpaper = null
        }, 100)
      }
    }, [wallpaperCopy])

  /**
   * 刷新壁纸
   * 优先从颜色缓存中选择不同于当前的壁纸，如果缓存不足则请求新图片
   */
  const refreshWallpaper = useCallback(async (): Promise<string | null> => {
    try {
      const config = await fetchWallpaperConfig()
      if (!config) {
        return null
      }

      // 获取当前壁纸URL
      const currentUrl =
        wallpaperUrl || extractBackgroundUrl(WALLPAPER_ELEMENT_ID)

      // 优先尝试从颜色缓存获取不同的壁纸（复用已缓存的颜色信息）
      const cachedAlternative = getCachedAlternativeWallpaper(currentUrl)

      let targetUrl: string

      if (cachedAlternative) {
        // 使用缓存中的壁纸（已有颜色缓存，切换更快）
        targetUrl = cachedAlternative
      } else {
        // 缓存不足，请求新图片（添加时间戳避免缓存）
        targetUrl = await resolveImageUrl(config.wallpaper_url, true)

        // 验证URL有效性
        if (!targetUrl || targetUrl.includes('/api/proxy/music/')) {
          return null
        }

        // 检查新URL是否与当前相同
        if (areUrlsEquivalent(targetUrl, currentUrl)) {
          // 如果API返回了相同的URL，再尝试一次
          await new Promise((resolve) => setTimeout(resolve, 100))
          targetUrl = await resolveImageUrl(config.wallpaper_url, true)

          // 如果还是相同，直接返回
          if (areUrlsEquivalent(targetUrl, currentUrl)) {
            return currentUrl
          }
        }
      }

      // 应用到DOM并验证（强制刷新）
      const verifiedUrl = await applyWallpaperToDOM(
        targetUrl,
        config.wallpaper_blur,
        true,
        wallpaperCopy,
      )

      if (!verifiedUrl) {
        return null
      }

      // 更新本地状态
      setWallpaperUrl(verifiedUrl)
      setBlur(config.wallpaper_blur)

      // 触发事件通知其他组件
      window.dispatchEvent(
        new CustomEvent('wallpaperChanged', {
          detail: {
            url: verifiedUrl,
            timestamp: wallpaperState.getAppliedTimestamp(),
            fromCache: !!cachedAlternative,
          },
        }),
      )

      return verifiedUrl
    } catch (error) {
      console.error('刷新壁纸失败:', error)
      return null
    }
  }, [wallpaperCopy, wallpaperUrl])

  return {
    wallpaperUrl,
    canRefresh,
    blur,
    isLoading,
    loadWallpaper,
    refreshWallpaper,
  }
}
