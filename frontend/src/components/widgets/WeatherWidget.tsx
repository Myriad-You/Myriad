/**
 * 天气小组件 - 重构版
 * 使用glass毛玻璃效果和现代化设计
 */

import type { TranslationKeys } from '../../i18n'
import type { WeatherData } from '../../utils/dynamicContent'
import type { WidgetConfig } from '../WidgetGrid'
import { memo, useCallback, useEffect, useId, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useLoopAnimation } from '../../hooks/animation'
import { useHomeVisibilityInterval } from '../../hooks/animation/pages/home'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { usePerformanceProfile } from '../../hooks/usePerformanceProfile'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { getWeatherInfo } from '../../utils/dynamicContent'
import { GlowBackground } from './shared/GlowBackground'

// 缓存配置
const CACHE_KEY = 'weather_data_cache'
const CACHE_DURATION = 30 * 60 * 1000 // 30分钟

/**
 * 根据 WMO 天气代码返回对应的翻译键
 * @param code WMO 天气代码
 * @returns 翻译键名
 */
function getWeatherKeyFromCode(code: number): keyof TranslationKeys['weather'] {
  const weatherKeyMap: Record<number, keyof TranslationKeys['weather']> = {
    0: 'sunny',
    1: 'sunny',
    2: 'partlyCloudy',
    3: 'cloudy',
    45: 'foggy',
    48: 'foggy',
    51: 'lightRain',
    53: 'lightRain',
    55: 'lightRain',
    56: 'freezingRain',
    57: 'freezingRain',
    61: 'lightRain',
    63: 'moderateRain',
    65: 'heavyRain',
    66: 'freezingRain',
    67: 'freezingRain',
    71: 'lightSnow',
    73: 'moderateSnow',
    75: 'heavySnow',
    77: 'sleet',
    80: 'showers',
    81: 'showers',
    82: 'heavyShowers',
    85: 'snowShowers',
    86: 'heavySnowShowers',
    95: 'thunderstorm',
    96: 'thunderstorm',
    99: 'thunderstorm',
  }

  return weatherKeyMap[code] || 'unknown'
}

/**
 * 根据 WMO 天气代码返回对应的主题颜色
 * @param code WMO 天气代码
 * @returns 主题颜色
 */
function getThemeColorFromCode(code: number): string {
  // 晴天
  if (code === 0 || code === 1)
    return '#f59e0b'
  // 多云/阴天
  if (code === 2 || code === 3)
    return '#6b7280'
  // 雾
  if (code === 45 || code === 48)
    return '#9ca3af'
  // 雨
  if ((code >= 51 && code <= 67) || (code >= 80 && code <= 82))
    return '#3b82f6'
  // 雪
  if ((code >= 71 && code <= 77) || (code >= 85 && code <= 86))
    return '#6366f1'
  // 雷暴
  if (code >= 95 && code <= 99)
    return '#8b5cf6'
  // 默认
  return '#10b981'
}

export interface WeatherWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
}

export const WeatherWidget = memo(({ config, isEditMode, isPreview }: WeatherWidgetProps) => {
  const { containerRef, scale, fontScale } = useWidgetSize(config.size, isPreview ? 1 : undefined)
  const perf = usePerformanceProfile()
  const anim = useAnimationLevel()
  const uniqueId = useId()
  const { t, locale } = useI18n()
  // framer-motion 动态模块（仅在需要动画时加载）
  const [FM, setFM] = useState<null | { motion: any }>(null)

  // 非中文语言时减小标题字体（英文等语言单词较长）
  const isNonChinese = !locale.startsWith('zh')
  const titleFontScale = isNonChinese ? fontScale * 0.9 : fontScale // 温度等标题减少约6-8px
  const infoFontScale = isNonChinese ? fontScale * 0.8 : fontScale // 城市等信息减少约4px

  // 🆕 使用触发式动画 - 组件挂载时播放一次天气图标动画
  const { isAnimating } = useLoopAnimation({
    duration: 3000, // 天气图标摇摆约3秒周期
    trigger: 'mount', // 固定值，组件首次渲染时触发一次
    enabled: anim.loop, // 低端设备禁用
  })

  const canAnimate = anim.loop && isAnimating
  // 需要动画时才加载 framer-motion
  useEffect(() => {
    let cancelled = false
    if (canAnimate && !FM) {
      import('framer-motion')
        .then((mod) => {
          if (!cancelled)
            setFM({ motion: mod.motion })
        })
        .catch(() => {
          // 忽略加载失败，保持静态渲染
        })
    }
    return () => {
      cancelled = true
    }
  }, [canAnimate, FM])
  // 在未加载 framer-motion 时使用原生标签占位
  const MDiv: any = FM ? FM.motion.div : 'div'
  const MSpan: any = FM ? FM.motion.span : 'span'

  const [weatherData, setWeatherData] = useState<WeatherData | null>(null)
  const [loading, setLoading] = useState(true)

  // 从缓存加载
  const loadFromCache = useCallback(() => {
    try {
      const cached = localStorage.getItem(CACHE_KEY)
      if (cached) {
        const { data, timestamp } = JSON.parse(cached)
        if (Date.now() - timestamp < CACHE_DURATION) {
          setWeatherData(data)
          return true
        }
      }
    }
    catch (err) {
      console.error(`${t.weatherWidget.loadCacheFailed}:`, err)
    }
    return false
  }, [t])

  // 保存到缓存
  const saveToCache = useCallback((data: WeatherData) => {
    try {
      localStorage.setItem(CACHE_KEY, JSON.stringify({
        data,
        timestamp: Date.now(),
      }))
    }
    catch (err) {
      console.error(`${t.weatherWidget.saveCacheFailed}:`, err)
    }
  }, [t])

  const fetchWeather = useCallback(async () => {
    try {
      const weather = await getWeatherInfo()
      if (weather) {
        setWeatherData(weather)
        saveToCache(weather)
      }
    }
    catch (error) {
      console.error(`${t.weatherWidget.fetchWeatherFailed}:`, error)
    }
    finally {
      setLoading(false)
    }
  }, [saveToCache, t])

  useEffect(() => {
    if (isPreview) {
      setWeatherData({
        temperature: '24°',
        weather: t.weatherWidget.sunny,
        city: t.weatherWidget.sampleCity,
        icon: '☀️',
        humidity: 45,
        windSpeed: 12,
        weatherCode: 0,
      })
      setLoading(false)
      return
    }

    // 先尝试从缓存加载
    const hasCache = loadFromCache()
    if (hasCache) {
      setLoading(false)
    }

    // 然后获取最新天气
    fetchWeather()
  }, [loadFromCache, fetchWeather, isPreview, t.weatherWidget.sunny, t.weatherWidget.sampleCity])

  // 🔧 使用首页原子化可见性感知定时器，页面隐藏时自动暂停
  useHomeVisibilityInterval(fetchWeather, CACHE_DURATION, !isPreview)

  // 根据天气状况选择主题色 - 使用 useMemo 缓存
  const themeColor = useMemo(() => {
    if (!weatherData)
      return '#10b981'
    return getThemeColorFromCode(weatherData.weatherCode ?? 0)
  }, [weatherData])

  // 获取翻译后的天气状态文本
  const weatherText = useMemo(() => {
    if (!weatherData)
      return ''
    const key = getWeatherKeyFromCode(weatherData.weatherCode ?? 0)
    return t.weather[key] || weatherData.weather
  }, [weatherData, t])

  if (loading) {
    return (
      <div className="h-full w-full flex items-center justify-center">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500" />
      </div>
    )
  }

  if (!weatherData) {
    return (
      <div className="h-full w-full flex items-center justify-center text-gray-400">
        <span>{t.weather.unavailable}</span>
      </div>
    )
  }

  // 4x2 宽版布局 - 左右结构重构 (左3/5 右2/5)
  if (config.size === '4x2') {
    return (
      <div ref={containerRef} className="relative h-full w-full rounded-xl overflow-hidden glass">
        {/* 动态背景光效 */}
        <GlowBackground
          color={themeColor}
          animLevel={anim.level}
          shouldAnimate={anim.loop}
          variant="single"
          size="lg"
        />

        <div className="absolute inset-0 flex flex-row px-4 py-3">
          {/* 左侧：主要信息 (60%) */}
          <div className="w-[60%] flex flex-col justify-between border-r border-gray-200/10 dark:border-white/10">
            {/* 顶部：城市 */}
            <div className="flex justify-between items-start">
              <div
                className="font-bold text-gray-700 dark:text-gray-200 truncate"
                style={{ fontSize: isNonChinese ? '0.75rem' : '1rem' }}
              >
                {weatherData.city}
              </div>
            </div>

            {/* 中部：温度和图标 */}
            <div className="flex items-center gap-3 my-auto">
              <MDiv
                className="text-4xl drop-shadow-md flex-shrink-0"
                initial={{ scale: 0.5, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
              >
                {weatherData.icon}
              </MDiv>
              <div className="flex flex-col justify-center min-w-0">
                <div className="flex items-baseline gap-2 overflow-hidden">
                  <span
                    className="font-black text-gray-800 dark:text-gray-100 leading-none tracking-tight truncate"
                    style={{ fontSize: isNonChinese ? '1.75rem' : '2.25rem' }}
                  >
                    {weatherData.temperature}
                  </span>
                </div>
                <div className="flex items-baseline gap-2 mt-1 overflow-hidden whitespace-nowrap">
                  <span
                    className="text-gray-600 dark:text-gray-400 font-medium truncate"
                    style={{ fontSize: isNonChinese ? '0.7rem' : '0.875rem' }}
                  >
                    {weatherText}
                  </span>
                  {weatherData.feelsLike !== undefined && (
                    <span
                      className="text-xs text-gray-500 dark:text-gray-500 flex-shrink-0"
                    >
                      {t.weather.feelsLike}
                      {' '}
                      {weatherData.feelsLike}
                      °
                    </span>
                  )}
                </div>
              </div>
            </div>

            {/* 底部：详细信息 (一行排列) */}
            <div className="flex items-center gap-3 text-gray-500 dark:text-gray-400 overflow-hidden whitespace-nowrap" style={{ fontSize: '0.6rem' }}>
              {weatherData.humidity !== undefined && (
                <div className="flex items-center gap-1" title={t.weather.humidity}>
                  <span>💧</span>
                  <span>
                    {weatherData.humidity}
                    %
                  </span>
                </div>
              )}
              {weatherData.windSpeed !== undefined && (
                <div className="flex items-center gap-1" title={t.weather.windSpeed}>
                  <span>🍃</span>
                  <span>
                    {Math.round(weatherData.windSpeed)}
                    km/h
                  </span>
                </div>
              )}
              {weatherData.aqi !== undefined && (
                <div className="flex items-center gap-1" title={t.weather.airQuality}>
                  <span>
                    {
                      weatherData.aqi <= 50
                        ? '🌿'
                        : weatherData.aqi <= 100
                          ? '🌫️'
                          : '😷'
                    }
                  </span>
                  <span className={
                    weatherData.aqi <= 50
                      ? 'text-green-500'
                      : weatherData.aqi <= 100
                        ? 'text-yellow-500'
                        : weatherData.aqi <= 150
                          ? 'text-orange-500'
                          : 'text-red-500'
                  }
                  >
                    AQI
                    {' '}
                    {weatherData.aqi}
                  </span>
                </div>
              )}
            </div>
          </div>

          {/* 右侧：未来天气预报 (40%) */}
          <div className="w-[40%] pl-2 flex flex-col justify-between gap-1 h-full">
            {weatherData.forecast
              ? (
                  weatherData.forecast.slice(0, 3).map((day, i) => (
                    <MDiv
                      key={day.date}
                      className="flex-1 flex items-center justify-between px-2 rounded-md hover:bg-white/40 dark:hover:bg-white/5 transition-colors"
                      initial={{ opacity: 0, x: 10 }}
                      animate={{ opacity: 1, x: 0 }}
                      transition={{ delay: 0.1 * i }}
                    >
                      <div className="text-gray-500 dark:text-gray-400 w-8" style={{ fontSize: '0.6rem' }}>
                        {new Date(day.date).toLocaleDateString(locale, { weekday: 'short' })}
                      </div>
                      <div className="flex-shrink-0 leading-none mx-1 text-base">{day.icon}</div>
                      <div className="flex items-center gap-1 justify-end flex-1">
                        <span className="font-bold text-gray-800 dark:text-gray-100" style={{ fontSize: '0.6rem' }}>
                          {day.maxTemp}
                          °
                        </span>
                        <span className="text-gray-400 dark:text-gray-500" style={{ fontSize: '0.6rem' }}>
                          {day.minTemp}
                          °
                        </span>
                      </div>
                    </MDiv>
                  ))
                )
              : (
                  <div className="w-full h-full flex items-center justify-center text-gray-400" style={{ fontSize: '0.6rem' }}>
                    {t.weather.updating}
                  </div>
                )}
          </div>
        </div>
      </div>
    )
  }

  // 4x1 紧凑横版布局 (参考 4x2 但只显示1天预报)
  if (config.size === '4x1') {
    // 获取明天预报 (通常是索引1，索引0为今天)
    const tomorrow = weatherData.forecast && weatherData.forecast.length > 1 ? weatherData.forecast[1] : null

    return (
      <div ref={containerRef} className="relative h-full w-full rounded-xl overflow-hidden glass">
        {/* 动态背景光效 */}
        <GlowBackground
          color={themeColor}
          animLevel={anim.level}
          shouldAnimate={anim.loop}
          variant="single"
          size="lg"
        />

        <div className="absolute inset-0 flex flex-row px-4 py-2">
          {/* 左侧：主要信息 (75%) */}
          <div className="w-[75%] flex items-center pr-3 border-r border-gray-200/10 dark:border-white/10 gap-3">
            {/* 图标 & 温度 */}
            <div className="flex items-center gap-2 flex-shrink-0">
              <div className="text-3xl">{weatherData.icon}</div>
              <div className="flex flex-col justify-center">
                <div
                  className="font-black text-gray-800 dark:text-gray-100 leading-none"
                  style={{ fontSize: isNonChinese ? '1.25rem' : '1.5rem' }}
                >
                  {weatherData.temperature}
                </div>
                <div
                  className="text-gray-500 dark:text-gray-400 mt-0.5 font-medium"
                  style={{ fontSize: isNonChinese ? '0.625rem' : '0.75rem' }}
                >
                  {weatherText}
                </div>
              </div>
            </div>

            {/* 城市 & 详情 */}
            <div className="flex flex-col justify-center gap-1 min-w-0 flex-1">
              <div
                className="font-bold text-gray-700 dark:text-gray-200 truncate"
                style={{ fontSize: isNonChinese ? '0.7rem' : '0.875rem' }}
              >
                {weatherData.city}
              </div>
              <div className="flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400">
                {weatherData.humidity !== undefined && (
                  <span className="flex items-center gap-0.5 whitespace-nowrap">
                    <span>💧</span>
                    {weatherData.humidity}
                    %
                  </span>
                )}
                {weatherData.windSpeed !== undefined && (
                  <span className="flex items-center gap-0.5 whitespace-nowrap">
                    <span>🍃</span>
                    {Math.round(weatherData.windSpeed)}
                  </span>
                )}
              </div>
            </div>
          </div>

          {/* 右侧：明天预报 (25%) - 极简模式 */}
          <div className="w-[25%] pl-1 flex flex-col items-center justify-center h-full">
            {tomorrow
              ? (
                  <>
                    <div className="text-[10px] text-gray-400 dark:text-gray-500 mb-0.5 scale-90 origin-bottom">{t.weather.tomorrow}</div>
                    <div className="flex items-center gap-1.5">
                      <span className="leading-none text-base">{tomorrow.icon}</span>
                      <div className="flex flex-col items-end leading-none gap-0.5">
                        <span className="font-bold text-gray-800 dark:text-gray-100 text-xs">
                          {tomorrow.maxTemp}
                          °
                        </span>
                        <span className="text-gray-400 dark:text-gray-500 text-[10px]">
                          {tomorrow.minTemp}
                          °
                        </span>
                      </div>
                    </div>
                  </>
                )
              : (
                  <div className="text-xs text-gray-400 text-center">{t.weather.noForecast}</div>
                )}
          </div>
        </div>
      </div>
    )
  }

  return (
    <div ref={containerRef} className="relative h-full w-full rounded-xl overflow-hidden glass">
      {/* 动态背景光效 - 呼吸效果 */}
      <GlowBackground
        color={themeColor}
        animLevel={anim.level}
        shouldAnimate={anim.loop}
        variant="single"
        size="md"
      />

      {/* 主内容区：2x2紧凑布局 */}
      <div className="absolute inset-0 flex flex-col p-3">
        {/* 顶部：图标 - 轻微摆动 */}
        <div className="h-9 flex-shrink-0">
          <MSpan
            className="text-3xl leading-none inline-block origin-center"
            style={{ transformOrigin: 'center center' }}
            initial={{ scale: 0.5, opacity: 0, rotate: -15 }}
            animate={canAnimate ? { scale: 1, opacity: 1, rotate: [-2, 2, -2] } : { scale: 1, opacity: 1, rotate: 0 }}
            transition={canAnimate
              ? {
                  scale: { duration: 0.6, ease: [0.34, 1.56, 0.64, 1] },
                  opacity: { duration: 0.6 },
                  rotate: { duration: 3, repeat: 2, ease: 'easeInOut' },
                }
              : { duration: 0.4 }}
          >
            {weatherData.icon}
          </MSpan>
        </div>

        {/* 主要信息：温度和天气状态 */}
        <div className="flex-1 flex flex-col justify-center">
          <MDiv
            className="flex items-baseline gap-2 mb-1"
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{
              duration: 0.6,
              delay: 0.2,
              ease: [0.34, 1.56, 0.64, 1],
            }}
          >
            <span
              className="font-black text-gray-800 dark:text-gray-100 leading-none"
              style={{ fontSize: isNonChinese ? '1.75rem' : '2.25rem' }}
            >
              {weatherData.temperature}
            </span>
            <MSpan
              className="text-gray-600 dark:text-gray-400 font-medium"
              style={{ fontSize: isNonChinese ? '0.7rem' : '0.875rem' }}
              initial={{ opacity: 0 }}
              animate={canAnimate ? { opacity: [0.6, 1, 0.6] } : { opacity: 1 }}
              transition={canAnimate ? { duration: 3, repeat: 2, ease: 'easeInOut' } : { duration: 0.3 }}
            >
              {weatherText}
            </MSpan>
          </MDiv>
          <MDiv
            className="text-xs text-gray-600 dark:text-gray-400 mb-2"
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{
              duration: 0.6,
              delay: 0.3,
              ease: [0.34, 1.56, 0.64, 1],
            }}
          >
            {weatherData.city}
          </MDiv>
        </div>

        {/* 次要信息：湿度/风速 - 横向紧凑排列 */}
        {(weatherData.humidity !== undefined || weatherData.windSpeed !== undefined) && (
          <MDiv
            className="flex items-center gap-2 text-[10px]"
            initial={{ y: 10, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            transition={perf.lowEndDevice ? { duration: 0.2 } : { duration: 0.4, delay: 0.3 }}
          >
            {weatherData.humidity !== undefined && (
              <div className="flex items-center gap-0.5" title={t.weather.humidity}>
                <span>💧</span>
                <span className="text-gray-600 dark:text-gray-400">
                  {weatherData.humidity}
                  %
                </span>
              </div>
            )}
            {weatherData.windSpeed !== undefined && (
              <div className="flex items-center gap-0.5" title={t.weather.windSpeed}>
                <span>🍃</span>
                <span className="text-gray-600 dark:text-gray-400">
                  {Math.round(weatherData.windSpeed)}
                  km/h
                </span>
              </div>
            )}
          </MDiv>
        )}
      </div>
    </div>
  )
})

WeatherWidget.displayName = 'WeatherWidget'
