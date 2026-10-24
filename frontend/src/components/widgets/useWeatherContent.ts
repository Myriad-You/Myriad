import type { WeatherData } from '../../utils/dynamicContent'
import { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useVisibilityInterval } from '../../hooks/animation'
import { getWeatherInfo, normalizeWeatherIconAssets, WEATHER_ICON_ASSETS } from '../../utils/dynamicContent'
import { formatUserFacingError } from '../../utils/formatUserFacingError'

const CACHE_KEY = 'weather_data_cache'
const CACHE_DURATION = 30 * 60 * 1000

/** The service owns shared transport; each widget owns publication of its result. */
export function useWeatherContent(isPreview = false) {
  const { t } = useI18n()
  const copy = useRef(t.weatherWidget)
  useEffect(() => { copy.current = t.weatherWidget }, [t.weatherWidget])
  const [weatherData, setWeatherData] = useState<WeatherData | null>(null)
  const [loading, setLoading] = useState(true)
  const [fetchError, setFetchError] = useState('')
  const refresh = useRef<(() => void) | null>(null)

  useEffect(() => {
    if (isPreview) return
    let active = true
    let generation = 0
    try {
      const cached = localStorage.getItem(CACHE_KEY)
      if (cached) {
        const { data, timestamp } = JSON.parse(cached)
        if (Date.now() - timestamp < CACHE_DURATION) {
          setWeatherData(normalizeWeatherIconAssets(data))
          setLoading(false)
        }
      }
    } catch (error) {
      console.error(`${copy.current.loadCacheFailed}:`, error)
    }
    const reload = async () => {
      const request = ++generation
      const current = () => active && request === generation
      try {
        const weather = await getWeatherInfo()
        if (!current() || !weather) return
        setWeatherData(weather)
        setFetchError('')
        try {
          localStorage.setItem(CACHE_KEY, JSON.stringify({ data: weather, timestamp: Date.now() }))
        } catch (error) {
          console.error(`${copy.current.saveCacheFailed}:`, error)
        }
      } catch (error) {
        if (!current()) return
        console.error(`${copy.current.fetchWeatherFailed}:`, error)
        // 错误码表只在出错时才拉，不压在每张卡片的首屏上（博客里每个小组件各启动一次）。
        const message = await formatUserFacingError(error, copy.current.fetchWeatherFailed)
        if (current()) setFetchError(message)
      } finally {
        if (current()) setLoading(false)
      }
    }
    refresh.current = reload
    void reload()
    return () => {
      active = false
      refresh.current = null
    }
  }, [isPreview])

  useVisibilityInterval(() => refresh.current?.(), { delay: CACHE_DURATION, enabled: !isPreview })

  return isPreview
    ? {
        weatherData: {
          temperature: '24°', weather: t.weatherWidget.sunny, city: t.weatherWidget.sampleCity,
          icon: WEATHER_ICON_ASSETS.sunny, humidity: 45, windSpeed: 12, weatherCode: 0,
        } satisfies WeatherData,
        loading: false,
        fetchError: '',
      }
    : { weatherData, loading, fetchError }
}
