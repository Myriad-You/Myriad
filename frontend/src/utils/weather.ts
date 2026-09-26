import { currentCopy, formatCurrent } from '../i18n/localeCopy'
import {
  hasBrowserGeoFix,
  isPreciseLocationEnabled,
  resolvePreciseLocation,
} from './geoLocation'
import { dedupedFetch } from './requestDedup'

const WEATHER_ICON_BASE = '/icons/weather'
const WEATHER_CACHE_TTL = 30 * 60 * 1000
const LAST_WEATHER_KEY = 'weather_data_last'
const LAST_WEATHER_TIME_KEY = 'weather_time_last'
/** Location the last entry was fetched for; one slot, so no trail of past places. */
const LAST_WEATHER_LOCATION_KEY = 'weather_data_last_location'
/** Retired per-location keys (`weather_data_<lat>,<lon>`) that accumulated a location history. */
const LEGACY_LOCATION_KEY = /^weather_(?:data|time)_-?\d/

export const WEATHER_ICON_ASSETS = {
  sunny: `${WEATHER_ICON_BASE}/sunny.webp`,
  partlyCloudy: `${WEATHER_ICON_BASE}/partly-cloudy.webp`,
  cloudy: `${WEATHER_ICON_BASE}/cloudy.webp`,
  fog: `${WEATHER_ICON_BASE}/fog.webp`,
  drizzle: `${WEATHER_ICON_BASE}/drizzle.webp`,
  rain: `${WEATHER_ICON_BASE}/rain.webp`,
  snow: `${WEATHER_ICON_BASE}/snow.webp`,
  thunderstorm: `${WEATHER_ICON_BASE}/thunderstorm.webp`,
} as const

export const WEATHER_DETAIL_ICON_ASSETS = {
  humidity: `${WEATHER_ICON_BASE}/humidity.webp`,
  wind: `${WEATHER_ICON_BASE}/wind.webp`,
  airGood: `${WEATHER_ICON_BASE}/air-good.webp`,
  airModerate: `${WEATHER_ICON_BASE}/air-moderate.webp`,
  airPoor: `${WEATHER_ICON_BASE}/air-poor.webp`,
} as const

export interface ForecastDay {
  date: string
  maxTemp: number
  minTemp: number
  weather: string
  weatherCode: number
  icon: string
}

export interface WeatherData {
  city: string
  weather: string
  temperature: string
  icon: string
  weatherCode: number
  humidity?: number
  windSpeed?: number
  feelsLike?: number
  aqi?: number
  forecast?: ForecastDay[]
}

let weatherInflight: Promise<WeatherData | null> | null = null

/** Persistent storage is optional; failure must not become a weather failure. */
function readWeatherCache(dataKey: string, timeKey: string): { data: WeatherData, timestamp: number } | null {
  try {
    const cached = localStorage.getItem(dataKey)
    const cacheTime = localStorage.getItem(timeKey)
    if (!cached || !cacheTime) return null
    const timestamp = Number.parseInt(cacheTime)
    if (!Number.isFinite(timestamp) || Date.now() - timestamp >= WEATHER_CACHE_TTL) {
      return null
    }
    return { data: normalizeWeatherIconAssets(JSON.parse(cached)), timestamp }
  } catch {
    return null
  }
}

function writeWeatherCache(dataKey: string, timeKey: string, data: WeatherData, timestamp: number): void {
  try {
    localStorage.setItem(dataKey, JSON.stringify(data))
    localStorage.setItem(timeKey, String(timestamp))
  } catch {
    // Quota/privacy restrictions do not invalidate a successful response.
  }
}

/** Without `locationKey`, any fresh last entry; with it, only one fetched for that location. */
function readLastWeather(locationKey?: string): WeatherData | null {
  if (locationKey !== undefined) {
    try {
      if (localStorage.getItem(LAST_WEATHER_LOCATION_KEY) !== locationKey) return null
    } catch {
      return null
    }
  }
  return readWeatherCache(LAST_WEATHER_KEY, LAST_WEATHER_TIME_KEY)?.data ?? null
}

let legacyLocationKeysSwept = false

function sweepLegacyLocationKeys(): void {
  if (legacyLocationKeysSwept) return
  legacyLocationKeysSwept = true
  try {
    const stale: string[] = []
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i)
      if (key && LEGACY_LOCATION_KEY.test(key)) stale.push(key)
    }
    for (const key of stale) localStorage.removeItem(key)
  } catch {
    // Storage unavailable: nothing to sweep.
  }
}

function writeLastWeather(data: WeatherData, timestamp: number, locationKey: string): void {
  writeWeatherCache(LAST_WEATHER_KEY, LAST_WEATHER_TIME_KEY, data, timestamp)
  try {
    localStorage.setItem(LAST_WEATHER_LOCATION_KEY, locationKey)
  } catch {
    // Without the tag the entry still serves the location-agnostic fast path.
  }
  sweepLegacyLocationKeys()
}

export async function getWeatherInfo(): Promise<WeatherData | null> {
  const cached = readLastWeather()
  if (cached) {
    if (!(await isPreciseLocationEnabled()) || hasBrowserGeoFix()) {
      return cached
    }
  }
  if (weatherInflight) return weatherInflight

  weatherInflight = (async () => {
    try {
      const location = await resolvePreciseLocation()
      if (!location) {
        return null
      }

      const weatherData = await getWeatherDataWithCache({
        latitude: location.latitude,
        longitude: location.longitude,
        city: location.city,
      })
      if (!weatherData) {
        return null
      }

      return weatherData
    } catch (error) {
      console.warn('[天气] 获取失败:', error)
      return null
    } finally {
      weatherInflight = null
    }
  })()

  return weatherInflight
}

async function getWeatherDataWithCache(location: {
  latitude: number
  longitude: number
  city: string
}): Promise<WeatherData | null> {
  // Same place = lat/lon to 2 decimals (~1 km).
  const locationKey = `${location.latitude.toFixed(2)},${location.longitude.toFixed(2)}`

  const cached = readLastWeather(locationKey)
  if (cached) {
    return cached
  }

  try {
    const weatherUrl = `https://api.open-meteo.com/v1/forecast?latitude=${location.latitude}&longitude=${location.longitude}&current=temperature_2m,weather_code,relative_humidity_2m,apparent_temperature,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto`
    const aqiUrl = `https://air-quality-api.open-meteo.com/v1/air-quality?latitude=${location.latitude}&longitude=${location.longitude}&current=us_aqi`

    const [weatherData, aqiData] = await Promise.all([
      dedupedFetch(
        weatherUrl,
        async () => {
          const response = await fetch(weatherUrl, {
            signal: AbortSignal.timeout(10000),
          })
          if (!response.ok) {
            throw new Error(
              formatCurrent(currentCopy().errors.weatherFailed, {
                status: response.status,
              }),
            )
          }
          return response.json()
        },
        { cacheTTL: WEATHER_CACHE_TTL },
      ),

      dedupedFetch(
        aqiUrl,
        async () => {
          const response = await fetch(aqiUrl, {
            signal: AbortSignal.timeout(10000),
          })
          if (!response.ok) return null
          return response.json()
        },
        { cacheTTL: WEATHER_CACHE_TTL },
      ).catch(() => null),
    ])

    const current = weatherData.current
    const daily = weatherData.daily

    let aqi
    if (aqiData && aqiData.current && aqiData.current.us_aqi) {
      aqi = aqiData.current.us_aqi
    }

    if (!current) {
      return null
    }

    const forecast: ForecastDay[] = []
    if (daily && daily.time && daily.time.length > 0) {
      for (let i = 1; i < Math.min(daily.time.length, 4); i++) {
        forecast.push({
          date: daily.time[i],
          maxTemp: Math.round(daily.temperature_2m_max[i]),
          minTemp: Math.round(daily.temperature_2m_min[i]),
          weather: getWeatherTextFromWMO(daily.weather_code[i]),
          weatherCode: daily.weather_code[i],
          icon: getWeatherIconFromWMO(daily.weather_code[i]),
        })
      }
    }

    const result: WeatherData = {
      city: location.city,
      weather: getWeatherTextFromWMO(current.weather_code),
      weatherCode: current.weather_code,
      temperature: `${Math.round(current.temperature_2m)}°C`,
      icon: getWeatherIconFromWMO(current.weather_code),
      humidity: current.relative_humidity_2m,
      windSpeed: current.wind_speed_10m,
      feelsLike: Math.round(current.apparent_temperature),
      aqi,
      forecast,
    }

    writeLastWeather(result, Date.now(), locationKey)

    return result
  } catch (error) {
    console.warn('[天气数据] 获取失败:', error)
    return null
  }
}

function getWeatherTextFromWMO(code: number): string {
  const w = currentCopy().weather
  const weatherMap: Record<number, string> = {
    0: w.sunny,
    1: w.sunny,
    2: w.partlyCloudy,
    3: w.cloudy,
    45: w.foggy,
    48: w.foggy,
    51: w.lightRain,
    53: w.lightRain,
    55: w.lightRain,
    56: w.freezingRain,
    57: w.freezingRain,
    61: w.lightRain,
    63: w.moderateRain,
    65: w.heavyRain,
    66: w.freezingRain,
    67: w.freezingRain,
    71: w.lightSnow,
    73: w.moderateSnow,
    75: w.heavySnow,
    77: w.sleet,
    80: w.showers,
    81: w.showers,
    82: w.heavyShowers,
    85: w.snowShowers,
    86: w.heavySnowShowers,
    95: w.thunderstorm,
    96: w.thunderstorm,
    99: w.thunderstorm,
  }

  return weatherMap[code] || w.unknown
}

export function getWeatherIconFromWMO(code: number): string {
  const iconMap: Record<number, string> = {
    0: WEATHER_ICON_ASSETS.sunny,
    1: WEATHER_ICON_ASSETS.partlyCloudy,
    2: WEATHER_ICON_ASSETS.partlyCloudy,
    3: WEATHER_ICON_ASSETS.cloudy,
    45: WEATHER_ICON_ASSETS.fog,
    48: WEATHER_ICON_ASSETS.fog,
    51: WEATHER_ICON_ASSETS.drizzle,
    53: WEATHER_ICON_ASSETS.drizzle,
    55: WEATHER_ICON_ASSETS.drizzle,
    56: WEATHER_ICON_ASSETS.drizzle,
    57: WEATHER_ICON_ASSETS.drizzle,
    61: WEATHER_ICON_ASSETS.rain,
    63: WEATHER_ICON_ASSETS.rain,
    65: WEATHER_ICON_ASSETS.rain,
    66: WEATHER_ICON_ASSETS.rain,
    67: WEATHER_ICON_ASSETS.rain,
    71: WEATHER_ICON_ASSETS.snow,
    73: WEATHER_ICON_ASSETS.snow,
    75: WEATHER_ICON_ASSETS.snow,
    77: WEATHER_ICON_ASSETS.snow,
    80: WEATHER_ICON_ASSETS.drizzle,
    81: WEATHER_ICON_ASSETS.rain,
    82: WEATHER_ICON_ASSETS.thunderstorm,
    85: WEATHER_ICON_ASSETS.snow,
    86: WEATHER_ICON_ASSETS.snow,
    95: WEATHER_ICON_ASSETS.thunderstorm,
    96: WEATHER_ICON_ASSETS.thunderstorm,
    99: WEATHER_ICON_ASSETS.thunderstorm,
  }

  return iconMap[code] || WEATHER_ICON_ASSETS.partlyCloudy
}

export function getAirQualityIcon(aqi: number): string {
  if (aqi <= 50) return WEATHER_DETAIL_ICON_ASSETS.airGood
  if (aqi <= 100) return WEATHER_DETAIL_ICON_ASSETS.airModerate
  return WEATHER_DETAIL_ICON_ASSETS.airPoor
}

export function normalizeWeatherIconAssets(data: WeatherData): WeatherData {
  return {
    ...data,
    icon: getWeatherIconFromWMO(data.weatherCode),
    forecast: data.forecast?.map((day) => ({
      ...day,
      icon: getWeatherIconFromWMO(day.weatherCode),
    })),
  }
}
