/** Geo 5 min memory; browser geo 6h localStorage; CN flag in-memory. */

import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { apiService } from '../services/api'
import { dedupedFetch, getUIConfigDeduped } from './requestDedup'

export interface GeoLocationData {
  latitude: number
  longitude: number
  city: string
  country?: string
  countryCode?: string
  region?: string
  ip?: string
}

export interface GeoApiResponse {
  status?: string
  lat?: number
  lon?: number
  latitude?: number
  longitude?: number
  city?: string
  country?: string
  countryCode?: string
  country_code?: string
  country_name?: string
  regionName?: string
  region?: string
  ip?: string
  detected_client_ip?: string
  source?: string
  fallback?: string
}

let userInChinaMainland: boolean | null = null

let chinaCheckPromise: Promise<boolean> | null = null

let geoLocationCache: GeoLocationData | null = null

let geoLocationCacheTime: number = 0

const GEO_CACHE_TTL = 5 * 60 * 1000
const GEO_NEGATIVE_TTL = 60 * 1000

const BROWSER_GEO_CACHE_KEY = 'browser_geo_location_v1'
/** Persist denial so the prompt is not repeated. */
const BROWSER_GEO_DENIED_KEY = 'browser_geo_denied_v1'
const BROWSER_GEO_CACHE_TTL = 6 * 60 * 60 * 1000

let browserGeoSessionAttempted = false
let browserGeoInflight: Promise<GeoLocationData | null> | null = null
let ipGeoInflight: Promise<GeoLocationData | null> | null = null
let preciseInflight: Promise<GeoLocationData | null> | null = null
let ipGeoNegativeUntil = 0

function rememberIpGeo(data: GeoLocationData): GeoLocationData {
  geoLocationCache = data
  geoLocationCacheTime = Date.now()
  ipGeoNegativeUntil = 0
  return data
}

function parsePreciseLocationFlag(value: unknown): boolean {
  return value === true || value === 'true'
}

export async function isPreciseLocationEnabled(): Promise<boolean> {
  try {
    const cfg = await getUIConfigDeduped()
    return parsePreciseLocationFlag(cfg?.precise_location_enabled)
  } catch {
    return false
  }
}

export function hasBrowserGeoFix(): boolean {
  const cached = readBrowserGeoCache(BROWSER_GEO_CACHE_TTL)
  return Boolean(
    cached &&
      Number.isFinite(cached.latitude) &&
      Number.isFinite(cached.longitude),
  )
}

export async function getClientGeoLocation(): Promise<GeoLocationData | null> {
  if (geoLocationCache && Date.now() - geoLocationCacheTime < GEO_CACHE_TTL) {
    return geoLocationCache
  }
  if (Date.now() < ipGeoNegativeUntil) {
    return null
  }
  if (ipGeoInflight) {
    return ipGeoInflight
  }

  ipGeoInflight = (async () => {
    try {
      try {
        const data = await getClientGeoFromBackend()
        if (data) return rememberIpGeo(data)
      } catch (error) {
        console.warn('[GeoLocation] 后端代理获取失败，尝试备用服务:', error)
      }

      try {
        const data = await getGeoFromFallbackServices()
        if (data) return rememberIpGeo(data)
      } catch (error) {
        console.warn('[GeoLocation] 所有服务获取失败:', error)
      }

      ipGeoNegativeUntil = Date.now() + GEO_NEGATIVE_TTL
      return null
    } finally {
      ipGeoInflight = null
    }
  })()

  return ipGeoInflight
}

/** null = not probed; do not await on click. */
export function getCachedIsChinaMainland(): boolean | null {
  return userInChinaMainland
}

export async function isUserInChinaMainland(): Promise<boolean> {
  if (userInChinaMainland !== null) {
    return userInChinaMainland
  }

  if (chinaCheckPromise) {
    return chinaCheckPromise
  }

  chinaCheckPromise = (async () => {
    try {
      const geoData = await getClientGeoLocation()

      if (!geoData) {
        console.warn('[GeoLocation] 无法获取地理位置，默认使用代理')
        userInChinaMainland = false
        return false
      }

      const country = geoData.country?.toLowerCase() || ''
      const countryCode = geoData.countryCode?.toLowerCase() || ''

      const isMainlandChina =
        country === 'china' || countryCode === 'cn' || country.includes('中国')

      userInChinaMainland = isMainlandChina

      console.log(
        `[GeoLocation] 地理位置检测: ${country} (${countryCode}), 中国大陆: ${isMainlandChina}`,
      )

      return isMainlandChina
    } catch (error) {
      console.warn('[GeoLocation] 地理位置检测失败，默认使用代理:', error)
      userInChinaMainland = false
      return false
    } finally {
      chinaCheckPromise = null
    }
  })()

  return chinaCheckPromise
}

export function resetGeoCache(): void {
  userInChinaMainland = null
  chinaCheckPromise = null
  geoLocationCache = null
  geoLocationCacheTime = 0
  browserGeoInflight = null
  ipGeoInflight = null
  preciseInflight = null
  ipGeoNegativeUntil = 0
  browserGeoSessionAttempted = false

  try {
    const keys = Object.keys(localStorage)
    keys.forEach((key) => {
      if (
        key.startsWith('geo_location_') ||
        key === BROWSER_GEO_CACHE_KEY ||
        key === BROWSER_GEO_DENIED_KEY
      ) {
        localStorage.removeItem(key)
      }
    })
  } catch {
  }

  console.log('[GeoLocation] 缓存已重置')
}

async function getClientGeoFromBackend(): Promise<GeoLocationData | null> {
  const data = await dedupedFetch<GeoApiResponse>(
    `${API_URL}/api/proxy/client-geo`,
    () => apiService.get<GeoApiResponse>('/proxy/client-geo', { timeout: 10_000 }),
    { cacheTTL: GEO_CACHE_TTL },
  )

  if (
    data.source === 'server-egress' ||
    data.fallback === 'server-public-ip'
  ) {
    console.warn(
      '[GeoLocation] Backend used server egress IP (proxy client-IP trust broken).',
      {
        detected: data.detected_client_ip,
        lookupIp: data.ip,
      },
    )
    return null
  }

  if (data.status === 'success' || (data.lat && data.lon)) {
    return {
      latitude: data.lat!,
      longitude: data.lon!,
      city: data.city || data.regionName || data.country || currentCopy().common.unknown,
      country: data.country,
      countryCode: data.countryCode || data.country_code,
      region: data.regionName,
      ip: data.ip,
    }
  }

  return null
}

async function getGeoFromFallbackServices(): Promise<GeoLocationData | null> {
  try {
    const response = await fetch('https://ipapi.co/json/', {
      signal: AbortSignal.timeout(10000),
    })

    if (response.ok) {
      const data: GeoApiResponse = await response.json()

      if (data.latitude && data.longitude) {
        return {
          latitude: data.latitude,
          longitude: data.longitude,
          city: data.city || data.region || data.country_name || currentCopy().common.unknown,
          country: data.country_name,
          countryCode: data.country_code,
          region: data.region,
          ip: data.ip,
        }
      }
    }
  } catch {
  }

  try {
    const response = await fetch('https://get.geojs.io/v1/ip/geo.json', {
      signal: AbortSignal.timeout(10000),
    })

    if (response.ok) {
      const data: GeoApiResponse = await response.json()

      if (data.latitude && data.longitude) {
        const lat =
          typeof data.latitude === 'string'
            ? Number.parseFloat(data.latitude as unknown as string)
            : data.latitude
        const lon =
          typeof data.longitude === 'string'
            ? Number.parseFloat(data.longitude as unknown as string)
            : data.longitude

        return {
          latitude: lat,
          longitude: lon,
          city: data.city || data.region || data.country || currentCopy().common.unknown,
          country: data.country,
          countryCode: data.country_code,
          region: data.region,
          ip: data.ip,
        }
      }
    }
  } catch {
  }

  return null
}

function readBrowserGeoCache(ttl: number): GeoLocationData | null {
  try {
    const raw = localStorage.getItem(BROWSER_GEO_CACHE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as {
      data: GeoLocationData
      timestamp: number
    }
    if (
      parsed?.data?.latitude != null &&
      parsed?.data?.longitude != null &&
      Date.now() - parsed.timestamp < ttl
    ) {
      return parsed.data
    }
  } catch {
    // ignore
  }
  return null
}

function writeBrowserGeoCache(data: GeoLocationData): void {
  try {
    localStorage.setItem(
      BROWSER_GEO_CACHE_KEY,
      JSON.stringify({ data, timestamp: Date.now() }),
    )
    localStorage.removeItem(BROWSER_GEO_DENIED_KEY)
  } catch {
    // ignore
  }
}

function isBrowserGeoDeniedStored(): boolean {
  try {
    return localStorage.getItem(BROWSER_GEO_DENIED_KEY) === '1'
  } catch {
    return false
  }
}

function markBrowserGeoDenied(): void {
  try {
    localStorage.setItem(BROWSER_GEO_DENIED_KEY, '1')
    localStorage.removeItem(BROWSER_GEO_CACHE_KEY)
  } catch {
    // ignore
  }
}

async function queryGeolocationPermission(): Promise<
  PermissionState | 'unsupported'
> {
  try {
    if (!navigator.permissions?.query) return 'unsupported'
    const status = await navigator.permissions.query({
      name: 'geolocation' as PermissionName,
    })
    return status.state
  } catch {
    return 'unsupported'
  }
}

async function reverseGeocodeCity(
  latitude: number,
  longitude: number,
): Promise<string> {
  try {
    const reverseGeoUrl = `https://nominatim.openstreetmap.org/reverse?format=json&lat=${latitude}&lon=${longitude}&zoom=10&addressdetails=1`
    const reverseResponse = await fetch(reverseGeoUrl, {
      signal: AbortSignal.timeout(5000),
      headers: {
        'User-Agent': 'Myriad Weather App',
      },
    })
    if (!reverseResponse.ok) return currentCopy().common.currentLocation
    const reverseData = await reverseResponse.json()
    return (
      reverseData.address?.city ||
      reverseData.address?.town ||
      reverseData.address?.village ||
      reverseData.address?.county ||
      reverseData.address?.state ||
      currentCopy().common.currentLocation
    )
  } catch {
    return currentCopy().common.currentLocation
  }
}

export async function getBrowserGeolocation(options?: {
  force?: boolean
  /** 6h */
  cacheTTL?: number
}): Promise<GeoLocationData | null> {
  if (!(await isPreciseLocationEnabled())) {
    return null
  }
  if (!('geolocation' in navigator) || !navigator.geolocation) {
    return null
  }

  const cacheTTL = options?.cacheTTL ?? BROWSER_GEO_CACHE_TTL
  const force = options?.force === true

  if (!force) {
    const cached = readBrowserGeoCache(cacheTTL)
    if (cached) return cached
  }

  if (!force && isBrowserGeoDeniedStored()) {
    return null
  }

  if (browserGeoInflight) {
    return browserGeoInflight
  }

  browserGeoInflight = (async () => {
    const permission = await queryGeolocationPermission()

    if (permission === 'denied') {
      markBrowserGeoDenied()
      return null
    }

    if (
      permission !== 'granted' &&
      browserGeoSessionAttempted &&
      !force
    ) {
      return null
    }
    if (permission !== 'granted') {
      browserGeoSessionAttempted = true
    }

    try {
      const position = await new Promise<GeolocationPosition>(
        (resolve, reject) => {
          navigator.geolocation.getCurrentPosition(resolve, reject, {
            enableHighAccuracy: false,
            timeout: 12000,
            maximumAge: 10 * 60 * 1000,
          })
        },
      )

      const latitude = position.coords.latitude
      const longitude = position.coords.longitude
      const city = await reverseGeocodeCity(latitude, longitude)

      const data: GeoLocationData = {
        latitude,
        longitude,
        city,
      }
      writeBrowserGeoCache(data)
      console.debug(
        `[GeoLocation] 浏览器定位成功: ${city} (${latitude.toFixed(4)}, ${longitude.toFixed(4)})`,
      )
      return data
    } catch (error) {
      const code =
        error && typeof error === 'object' && Object.hasOwn(error, 'code')
          ? (error as GeolocationPositionError).code
          : undefined
      if (code === 1) {
        markBrowserGeoDenied()
        console.debug('[GeoLocation] 用户拒绝浏览器定位，回退 IP')
      } else {
        console.debug('[GeoLocation] 浏览器定位失败，回退 IP:', error)
      }
      return null
    } finally {
      browserGeoInflight = null
    }
  })()

  return browserGeoInflight
}

export async function resolvePreciseLocation(): Promise<GeoLocationData | null> {
  if (!(await isPreciseLocationEnabled())) {
    return getClientGeoLocation()
  }
  const cachedBrowser = readBrowserGeoCache(BROWSER_GEO_CACHE_TTL)
  if (
    cachedBrowser &&
    Number.isFinite(cachedBrowser.latitude) &&
    Number.isFinite(cachedBrowser.longitude)
  ) {
    return cachedBrowser
  }
  if (preciseInflight) {
    return preciseInflight
  }

  preciseInflight = (async () => {
    try {
      try {
        const browser = await getBrowserGeolocation()
        if (
          browser &&
          Number.isFinite(browser.latitude) &&
          Number.isFinite(browser.longitude)
        ) {
          return browser
        }
      } catch (error) {
        console.warn('[GeoLocation] 浏览器定位异常:', error)
      }

      try {
        return await getClientGeoLocation()
      } catch (error) {
        console.warn('[GeoLocation] IP 定位失败:', error)
        return null
      }
    } finally {
      preciseInflight = null
    }
  })()

  return preciseInflight
}
