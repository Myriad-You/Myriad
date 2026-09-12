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

/** 公开 API 应为 boolean；网关/旧缓存可能把 true/false 序列化成字符串。 */
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

  evocative_parallax: boolean
  evocative_dynamic_blur: boolean
  evocative_ripple: boolean
  evocative_fps: number
  evocative_ripple_quality: number
}

interface LoadWallpaperResult {

  actualUrl: string
  blur: number

  verified: boolean
  /** @deprecated 使用 evocative 替代 */
  parallaxEnabled: boolean

  evocative: {
    parallax: boolean
    dynamicBlur: boolean
    ripple: boolean
    fps: number
    rippleQuality: number
  }
}

/** wallpaper_url 为空时的同源打包底图（含首次 setup）。 */
export const DEFAULT_FALLBACK_WALLPAPER_URL = '/wallpapers/default.webp'

const IMAGE_LOAD_TIMEOUT = 15000

const WALLPAPER_ELEMENT_ID = 'wallpaper'

const RANDOM_IMAGE_SERVICES = [
  'picsum.photos',
  'loremflickr.com',
  'source.unsplash.com',
  'unsplash.com/random',
  'api.unsplash.com',
  'bing.com/hpimagearchive',
] as const

const STATIC_CDN_INDICATORS = [
  'cdn.',
  'static.',
  '/static/',
  '/images/',
  '/assets/',
  '/uploads/',
] as const

const IMAGE_EXTENSIONS = [
  '.jpg',
  '.jpeg',
  '.png',
  '.gif',
  '.webp',
  '.bmp',
  '.svg',
] as const

const DYNAMIC_EXTENSIONS = ['.php', '.jsp', '.asp', '.aspx', '.py'] as const

function getCachedAlternativeWallpaper(
  currentUrl: string | null,
): string | null {
  try {
    const cacheInfo = getCacheInfo()
    if (!cacheInfo.exists || !cacheInfo.items || cacheInfo.items.length < 2) {
      return null
    }

    const cached = localStorage.getItem('myriad_wallpaper_color_cache_v5')
    if (!cached) return null

    const store = JSON.parse(cached)
    if (!store.items || store.items.length < 2) return null

    const now = Date.now()
    const CACHE_DURATION_MS = 6 * 60 * 60 * 1000
    const alternatives = store.items.filter(
      (item: { url: string; timestamp: number }) => {
        if (now - item.timestamp > CACHE_DURATION_MS) return false

        if (areUrlsEquivalent(item.url, currentUrl)) return false
        return true
      },
    )

    if (alternatives.length === 0) return null

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

function isStaticImageUrl(url: string): boolean {
  if (!url) return true

  const lowerUrl = url.toLowerCase()

  if (RANDOM_IMAGE_SERVICES.some((service) => lowerUrl.includes(service))) {
    return false
  }

  if (lowerUrl.includes('/random') || lowerUrl.includes('/daily')) {
    return false
  }

  if (DYNAMIC_EXTENSIONS.some((ext) => lowerUrl.endsWith(ext))) {
    return false
  }

  const endsWithImage = IMAGE_EXTENSIONS.some((ext) => lowerUrl.endsWith(ext))
  if (!endsWithImage) {
    return false
  }

  if (STATIC_CDN_INDICATORS.some((indicator) => lowerUrl.includes(indicator))) {
  // 默认可刷新（保守）。
    return true
  }

  return false
}

function extractImageUrlFromJson(data: unknown): string | null {
  if (!data || typeof data !== 'object') return null
  const obj = data as Record<string, unknown>

  const tryString = (v: unknown): string | null => {
    if (typeof v !== 'string') return null

    // 策略拒绝 data: / 私有主机 / 非 http（含 data:image/svg+xml）。
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

/** 代理在 dev 可能写成绝对 http://localhost…/api/proxy/image，归一成 path 以免宿主策略误杀。 */
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

/** 路径带常见图片扩展名则当直链，不探测网络；可用性交给 Image() 预加载。 */
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

/** 不用 HEAD：许多 CDN 拒 HEAD，SW Cache 也不能 put HEAD。 */
async function resolveImageUrlViaGet(url: string): Promise<string | null> {
  const getResp = await fetch(url, {
    method: 'GET',
    redirect: 'follow',
    headers: { Accept: 'application/json, image/*, */*' },
  })
  const getType = getResp.headers.get('content-type') || ''
  if (getType.includes('image/')) {
    try {
      // 抽干 body 以便复用连接；展示仍由 Image() 再取。
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

  // 直链不探测；applyWallpaperToDOM 的 Image() 才是可用性检查。
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

function preloadImage(
  url: string,
  timeout = IMAGE_LOAD_TIMEOUT,
): Promise<boolean> {
  return loadImagePooled(url, { timeout })
}

function setWallpaperAwaiting(active: boolean) {
  const bg = document.getElementById('bg-container')
  if (!bg) return
  bg.classList.toggle('wallpaper-awaiting', active)
}

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

  imageUrl = safeUrl

  const currentUrl = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)
  if (!forceRefresh && currentUrl && areUrlsEquivalent(currentUrl, imageUrl)) {
    const currentFilter = wallpaperEl.style.filter
    const targetFilter = `blur(${effectiveWallpaperBlur(blur)}px)`
    if (currentFilter !== targetFilter) {
      wallpaperEl.style.filter = targetFilter
    }

    wallpaperState.updateState(imageUrl, blur)
    wallpaperEl.classList.add('wallpaper-visible')
    setWallpaperAwaiting(false)
    return imageUrl
  }

  const hadVisibleWallpaper =
    wallpaperEl.classList.contains('wallpaper-visible') && !!currentUrl

  wallpaperState.setLoading(true)
  if (!hadVisibleWallpaper) {
    setWallpaperAwaiting(true)
    wallpaperEl.classList.remove('wallpaper-visible')
  } else {
    wallpaperEl.classList.add('wallpaper-fading')
    wallpaperEl.classList.remove('wallpaper-visible')
  }

  try {
    const loaded = await preloadImage(imageUrl)
    if (!loaded) {
      wallpaperState.setError(copy.imageLoadFailed)
      wallpaperEl.classList.remove('wallpaper-fading')
      if (hadVisibleWallpaper) {
        wallpaperEl.classList.add('wallpaper-visible')
      }
      return null
    }

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

    // 强制 url("...")，防 CSS 注入/截断。
    wallpaperEl.style.backgroundImage = cssBackgroundImage(imageUrl)
    wallpaperEl.style.filter = `blur(${effectiveWallpaperBlur(blur)}px)`
    wallpaperEl.classList.remove('wallpaper-fading')

    wallpaperState.updateState(imageUrl, blur)

    // 等两帧再淡入，保证 background-image 已提交绘制。
    await new Promise((resolve) => requestAnimationFrame(resolve))
    await new Promise((resolve) => requestAnimationFrame(resolve))
    wallpaperEl.classList.add('wallpaper-visible')
    setWallpaperAwaiting(false)

    const appliedUrl = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)

    if (appliedUrl && areUrlsEquivalent(appliedUrl, imageUrl)) {
      return imageUrl
    }

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

    // 首次失败也保持 awaiting，避免 #wallpaper 空白。
    if (!wallpaperEl.classList.contains('wallpaper-visible')) {
      setWallpaperAwaiting(true)
    }
  }
}

async function fetchWallpaperConfig(): Promise<WallpaperConfig | null> {
  console.debug('[Wallpaper] Fetching wallpaper config...')
  try {
    // 先走去重缓存；失败再带重试的独立请求。
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

    // 落库脏 URL（javascript: / 私有主机 / data:）在应用时丢掉。
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

    // 没有壁纸 URL 也返回动效开关，避免整条 evocative 被丢掉。
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

let pendingLoadWallpaper: Promise<LoadWallpaperResult | null> | null = null
let lastLoadTimestamp = 0
const LOAD_DEBOUNCE_MS = 1000 // 1秒内重复调用直接返回上次结果
let lastLoadResult: LoadWallpaperResult | null = null

/** 清去重缓存，让 config save → wallpaperConfigChanged 一定重载。 */
export function invalidateWallpaperLoadCache(): void {
  lastLoadResult = null
  lastLoadTimestamp = 0
  pendingLoadWallpaper = null
}

export function useWallpaper() {
  const { t } = useI18n()
  const wallpaperCopy = t.wallpaperStatus
  const [wallpaperUrl, setWallpaperUrl] = useState<string>('')
  const [canRefresh, setCanRefresh] = useState<boolean>(false)
  const [blur, setBlur] = useState<number>(3)
  const [isLoading, setIsLoading] = useState<boolean>(false)

  useEffect(() => {
    return wallpaperState.subscribe((snapshot) => {
      setIsLoading(snapshot.isLoading)
    })
  }, [])

  const loadWallpaper =
    useCallback(async (): Promise<LoadWallpaperResult | null> => {
      const now = Date.now()
      console.debug('[Wallpaper] loadWallpaper called')

      if (lastLoadResult && now - lastLoadTimestamp < LOAD_DEBOUNCE_MS) {
        console.debug('[Wallpaper] Returning cached result (debounce)')

        if (lastLoadResult.actualUrl) {
          setWallpaperUrl(lastLoadResult.actualUrl)
          setBlur(lastLoadResult.blur)
        }
        return lastLoadResult
      }

      if (pendingLoadWallpaper) {
        console.debug('[Wallpaper] Waiting for pending load...')
        const result = await pendingLoadWallpaper

        if (result?.actualUrl) {
          setWallpaperUrl(result.actualUrl)
          setBlur(result.blur)
        }
        return result
      }

      console.debug('[Wallpaper] Starting new load...')

      const doLoad = async (): Promise<LoadWallpaperResult | null> => {
        const defaultEvocative = {
          parallax: true,
          dynamicBlur: false,
          ripple: false,
          fps: 30,
          rippleQuality: 0.85,
        }

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

            fromConfig: boolean,

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

          if (!configuredUrl) {
            console.debug(
              '[Wallpaper] No wallpaper_url; applying bundled fallback',
            )

            return applyAndFinish(DEFAULT_FALLBACK_WALLPAPER_URL, false, true)
          }

          const actualUrl = await resolveImageUrl(configuredUrl)

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

          return applyBundledFallback()
        }
      }

      pendingLoadWallpaper = doLoad()

      try {
        const result = await pendingLoadWallpaper
        return result
      } finally {
        setTimeout(() => {
          pendingLoadWallpaper = null
        }, 100)
      }
    }, [wallpaperCopy])

  const refreshWallpaper = useCallback(async (): Promise<string | null> => {
    try {
      const config = await fetchWallpaperConfig()
      if (!config) {
        return null
      }

      const currentUrl =
        wallpaperUrl || extractBackgroundUrl(WALLPAPER_ELEMENT_ID)

      const cachedAlternative = getCachedAlternativeWallpaper(currentUrl)

      let targetUrl: string

      if (cachedAlternative) {
        targetUrl = cachedAlternative
      } else {
        targetUrl = await resolveImageUrl(config.wallpaper_url, true)

        if (!targetUrl || targetUrl.includes('/api/proxy/music/')) {
          return null
        }

        if (areUrlsEquivalent(targetUrl, currentUrl)) {
          await new Promise((resolve) => setTimeout(resolve, 100))
          targetUrl = await resolveImageUrl(config.wallpaper_url, true)

          if (areUrlsEquivalent(targetUrl, currentUrl)) {
            return currentUrl
          }
        }
      }

      const verifiedUrl = await applyWallpaperToDOM(
        targetUrl,
        config.wallpaper_blur,
        true,
        wallpaperCopy,
      )

      if (!verifiedUrl) {
        return null
      }

      setWallpaperUrl(verifiedUrl)
      setBlur(config.wallpaper_blur)

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
