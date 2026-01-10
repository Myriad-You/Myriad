/**
 * 地理位置工具 - 统一的IP检测和地理位置服务
 *
 * 提供以下功能：
 * 1. 获取客户端地理位置信息（经纬度、城市、国家等）
 * 2. 检测用户是否在中国大陆（用于判断是否需要代理）
 * 3. 带缓存的地理位置查询
 *
 * 缓存策略：
 * - 地理位置数据缓存5分钟（通过 requestDedup）
 * - IP→位置映射缓存24小时（localStorage）
 * - 中国大陆检测结果缓存在内存中（页面生命周期内）
 */

import { API_URL } from '../config'
import { dedupedFetch } from './requestDedup'

// ===== 类型定义 =====

export interface GeoLocationData {
  /** 纬度 */
  latitude: number
  /** 经度 */
  longitude: number
  /** 城市名称 */
  city: string
  /** 国家名称 */
  country?: string
  /** 国家代码（如 CN, US） */
  countryCode?: string
  /** 地区/省份名称 */
  region?: string
  /** IP 地址 */
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
}

// ===== 内存缓存 =====

/** 用户是否在中国大陆的缓存 */
let userInChinaMainland: boolean | null = null

/** 正在进行的中国大陆检测 Promise */
let chinaCheckPromise: Promise<boolean> | null = null

/** 地理位置数据缓存 */
let geoLocationCache: GeoLocationData | null = null

/** 地理位置数据缓存时间 */
let geoLocationCacheTime: number = 0

/** 地理位置缓存有效期（5分钟） */
const GEO_CACHE_TTL = 5 * 60 * 1000

// ===== 核心 API =====

/**
 * 获取客户端地理位置（去重 + 缓存）
 * 这是获取地理位置的主要入口，使用多级缓存策略
 *
 * @returns 地理位置数据，失败返回 null
 */
export async function getClientGeoLocation(): Promise<GeoLocationData | null> {
  // 1. 检查内存缓存
  if (geoLocationCache && Date.now() - geoLocationCacheTime < GEO_CACHE_TTL) {
    return geoLocationCache
  }

  try {
    // 2. 通过后端代理获取（最准确，能获取真实客户端IP）
    const data = await getClientGeoFromBackend()

    if (data) {
      geoLocationCache = data
      geoLocationCacheTime = Date.now()
      return data
    }
  }
  catch (error) {
    console.warn('[GeoLocation] 后端代理获取失败，尝试备用服务:', error)
  }

  // 3. 尝试备用服务
  try {
    const data = await getGeoFromFallbackServices()

    if (data) {
      geoLocationCache = data
      geoLocationCacheTime = Date.now()
      return data
    }
  }
  catch (error) {
    console.warn('[GeoLocation] 所有服务获取失败:', error)
  }

  return null
}

/**
 * 检测用户是否在中国大陆
 * 用于判断是否需要使用代理访问某些服务（如网易云音乐）
 *
 * 判断逻辑：
 * - country === 'china' 或 countryCode === 'cn' 视为中国大陆
 * - 香港(HK)、澳门(MO)、台湾(TW)不在此范围内
 *
 * @returns 是否在中国大陆
 */
export async function isUserInChinaMainland(): Promise<boolean> {
  // 如果已有缓存结果，直接返回
  if (userInChinaMainland !== null) {
    return userInChinaMainland
  }

  // 如果正在检测中，等待结果
  if (chinaCheckPromise) {
    return chinaCheckPromise
  }

  // 发起检测
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

      // 判断是否在中国大陆
      // 注意：香港(HK)、澳门(MO)、台湾(TW)不在此范围内
      const isMainlandChina
        = country === 'china'
          || countryCode === 'cn'
          || country.includes('中国')

      userInChinaMainland = isMainlandChina

      console.log(`[GeoLocation] 地理位置检测: ${country} (${countryCode}), 中国大陆: ${isMainlandChina}`)

      return isMainlandChina
    }
    catch (error) {
      console.warn('[GeoLocation] 地理位置检测失败，默认使用代理:', error)
      userInChinaMainland = false
      return false
    }
    finally {
      chinaCheckPromise = null
    }
  })()

  return chinaCheckPromise
}

/**
 * 获取客户端唯一标识（用于缓存键）
 * 使用经纬度组合作为标识，同一位置的用户共享缓存
 *
 * @returns 客户端标识字符串
 */
export async function getClientIdentifier(): Promise<string> {
  try {
    const geoData = await getClientGeoLocation()

    // 使用 lat+lon 作为唯一标识（精确到小数点后2位）
    if (geoData?.latitude && geoData?.longitude) {
      return `${geoData.latitude.toFixed(2)},${geoData.longitude.toFixed(2)}`
    }

    if (geoData?.ip) {
      return geoData.ip
    }
  }
  catch (error) {
    console.warn('[GeoLocation] 获取客户端标识失败:', error)
  }

  // 所有方案失败，使用固定标识符
  return 'browser-default'
}

/**
 * 重置所有地理位置缓存
 * 用于测试或用户切换网络时
 */
export function resetGeoCache(): void {
  userInChinaMainland = null
  chinaCheckPromise = null
  geoLocationCache = null
  geoLocationCacheTime = 0

  // 清除 localStorage 中的缓存
  try {
    const keys = Object.keys(localStorage)
    keys.forEach((key) => {
      if (key.startsWith('geo_location_')) {
        localStorage.removeItem(key)
      }
    })
  }
  catch (error) {
    // localStorage 操作失败，静默处理
  }

  console.log('[GeoLocation] 缓存已重置')
}

// ===== 内部实现 =====

/**
 * 通过后端代理获取地理位置
 */
async function getClientGeoFromBackend(): Promise<GeoLocationData | null> {
  const data = await dedupedFetch<GeoApiResponse>(
    `${API_URL}/api/proxy/client-geo`,
    async () => {
      const response = await fetch(`${API_URL}/api/proxy/client-geo`, {
        signal: AbortSignal.timeout(10000),
      })
      if (!response.ok)
        throw new Error('Failed to fetch client geo')
      return response.json()
    },
    { cacheTTL: GEO_CACHE_TTL },
  )

  if (data.status === 'success' || (data.lat && data.lon)) {
    return {
      latitude: data.lat!,
      longitude: data.lon!,
      city: data.city || data.regionName || data.country || '未知',
      country: data.country,
      countryCode: data.countryCode,
      region: data.regionName,
      ip: data.ip,
    }
  }

  return null
}

/**
 * 通过备用服务获取地理位置
 */
async function getGeoFromFallbackServices(): Promise<GeoLocationData | null> {
  // 方案1: ipapi.co（免费，稳定）
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
          city: data.city || data.region || data.country_name || '未知',
          country: data.country_name,
          countryCode: data.country_code,
          region: data.region,
          ip: data.ip,
        }
      }
    }
  }
  catch (error) {
    // 静默失败，尝试下一个服务
  }

  // 方案2: ip-api.com（备用）
  try {
    const response = await fetch('http://ip-api.com/json/?fields=status,lat,lon,city,regionName,country,countryCode', {
      signal: AbortSignal.timeout(10000),
    })

    if (response.ok) {
      const data: GeoApiResponse = await response.json()

      if (data.status === 'success' && data.lat && data.lon) {
        return {
          latitude: data.lat,
          longitude: data.lon,
          city: data.city || data.regionName || data.country || '未知',
          country: data.country,
          countryCode: data.countryCode,
          region: data.regionName,
        }
      }
    }
  }
  catch (error) {
    // 静默失败，尝试下一个服务
  }

  // 方案3: geojs.io（第三备用）
  try {
    const response = await fetch('https://get.geojs.io/v1/ip/geo.json', {
      signal: AbortSignal.timeout(10000),
    })

    if (response.ok) {
      const data: GeoApiResponse = await response.json()

      if (data.latitude && data.longitude) {
        const lat = typeof data.latitude === 'string' ? Number.parseFloat(data.latitude as unknown as string) : data.latitude
        const lon = typeof data.longitude === 'string' ? Number.parseFloat(data.longitude as unknown as string) : data.longitude

        return {
          latitude: lat,
          longitude: lon,
          city: data.city || data.region || data.country || '未知',
          country: data.country,
          countryCode: data.country_code,
          region: data.region,
          ip: data.ip,
        }
      }
    }
  }
  catch (error) {
    // 静默失败
  }

  return null
}

/**
 * 获取带 localStorage 缓存的地理位置
 * IP→地理位置的映射缓存24小时
 *
 * @param clientIdentifier 客户端标识（如IP或位置坐标）
 * @returns 地理位置数据
 */
export async function getGeoLocationWithLocalCache(clientIdentifier: string): Promise<GeoLocationData | null> {
  const cacheKey = `geo_location_${clientIdentifier}`
  const cacheTimeKey = `geo_location_time_${clientIdentifier}`

  // 检查 localStorage 缓存
  try {
    const cached = localStorage.getItem(cacheKey)
    const cacheTime = localStorage.getItem(cacheTimeKey)

    if (cached && cacheTime) {
      const cacheAge = Date.now() - Number.parseInt(cacheTime)
      // IP→位置缓存24小时（位置很少变）
      if (cacheAge < 24 * 60 * 60 * 1000) {
        return JSON.parse(cached)
      }
    }
  }
  catch (error) {
    // localStorage 读取失败，继续获取新数据
  }

  // 缓存失效或不存在，重新获取
  const location = await getClientGeoLocation()

  if (location) {
    // 缓存结果到 localStorage
    try {
      localStorage.setItem(cacheKey, JSON.stringify(location))
      localStorage.setItem(cacheTimeKey, Date.now().toString())
    }
    catch (error) {
      // localStorage 写入失败，静默处理
    }
  }

  return location
}

/**
 * 使用浏览器地理位置 API 获取位置（需要用户授权）
 * 这是最后的备用方案
 *
 * @returns 地理位置数据
 */
export async function getBrowserGeolocation(): Promise<GeoLocationData | null> {
  if (!('geolocation' in navigator)) {
    return null
  }

  try {
    const position = await new Promise<GeolocationPosition>((resolve, reject) => {
      navigator.geolocation.getCurrentPosition(resolve, reject, {
        timeout: 10000,
        maximumAge: 600000, // 10分钟缓存
      })
    })

    // 使用 Nominatim 反向地理编码获取城市名
    let city = '当前位置'
    try {
      const reverseGeoUrl = `https://nominatim.openstreetmap.org/reverse?format=json&lat=${position.coords.latitude}&lon=${position.coords.longitude}&zoom=10&addressdetails=1`

      const reverseResponse = await fetch(reverseGeoUrl, {
        signal: AbortSignal.timeout(5000),
        headers: {
          'User-Agent': 'Myriad Weather App',
        },
      })

      if (reverseResponse.ok) {
        const reverseData = await reverseResponse.json()
        city = reverseData.address?.city
          || reverseData.address?.town
          || reverseData.address?.village
          || reverseData.address?.county
          || reverseData.address?.state
          || '当前位置'
      }
    }
    catch (e) {
      // 反向地理编码失败，使用默认城市名
    }

    return {
      latitude: position.coords.latitude,
      longitude: position.coords.longitude,
      city,
    }
  }
  catch (error) {
    // 浏览器 API 失败（可能用户拒绝授权）
    return null
  }
}
