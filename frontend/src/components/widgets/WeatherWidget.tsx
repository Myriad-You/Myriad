/**
 * 天气小组件 - 重构版
 * 使用glass毛玻璃效果和现代化设计
 */

import type { TranslationKeys } from '../../i18n'
import type { WeatherData } from '../../utils/dynamicContent'

import type { WidgetConfig } from '../WidgetGrid'
import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  useHomeVisibilityInterval,
  useLoopAnimation,
} from '../../hooks/animation'
import {
  isStandardAnimation,
  useAnimationLevel,
} from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import {
  getAirQualityIcon,
  getWeatherInfo,
  normalizeWeatherIconAssets,
  WEATHER_DETAIL_ICON_ASSETS,
  WEATHER_ICON_ASSETS,
} from '../../utils/dynamicContent'
import { WeatherAssetIcon } from '../weather/WeatherAssetIcon'
import { FitText } from './shared/FitText'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeleton } from './shared/WidgetSkeleton'

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
  if (code === 0 || code === 1) return '#f59e0b'
  // 多云/阴天
  if (code === 2 || code === 3) return '#6b7280'
  // 雾
  if (code === 45 || code === 48) return '#9ca3af'
  // 雨
  if ((code >= 51 && code <= 67) || (code >= 80 && code <= 82)) return '#3b82f6'
  // 雪
  if ((code >= 71 && code <= 77) || (code >= 85 && code <= 86)) return '#6366f1'
  // 雷暴
  if (code >= 95 && code <= 99) return '#8b5cf6'
  // 默认
  return '#10b981'
}

export interface WeatherWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
}

export const WeatherWidget = memo(
  ({ config, isEditMode: _isEditMode, isPreview }: WeatherWidgetProps) => {
    const { containerRef, scale: _scale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const anim = useAnimationLevel()
    const { t, locale } = useI18n()
    // framer-motion 动态模块（仅在需要动画时加载）
    const [FM, setFM] = useState<null | { motion: any }>(null)

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
        import('motion/react')
          .then((mod) => {
            if (!cancelled) setFM({ motion: mod.motion })
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
            setWeatherData(normalizeWeatherIconAssets(data))
            return true
          }
        }
      } catch (err) {
        console.error(`${t.weatherWidget.loadCacheFailed}:`, err)
      }
      return false
    }, [t])

    // 保存到缓存
    const saveToCache = useCallback(
      (data: WeatherData) => {
        try {
          localStorage.setItem(
            CACHE_KEY,
            JSON.stringify({
              data,
              timestamp: Date.now(),
            }),
          )
        } catch (err) {
          console.error(`${t.weatherWidget.saveCacheFailed}:`, err)
        }
      },
      [t],
    )

    const fetchWeather = useCallback(async () => {
      try {
        const weather = await getWeatherInfo()
        if (weather) {
          setWeatherData(weather)
          saveToCache(weather)
        }
      } catch (error) {
        console.error(`${t.weatherWidget.fetchWeatherFailed}:`, error)
      } finally {
        setLoading(false)
      }
    }, [saveToCache, t])

    useEffect(() => {
      if (isPreview) {
        setWeatherData({
          temperature: '24°',
          weather: t.weatherWidget.sunny,
          city: t.weatherWidget.sampleCity,
          icon: WEATHER_ICON_ASSETS.sunny,
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
    }, [
      loadFromCache,
      fetchWeather,
      isPreview,
      t.weatherWidget.sunny,
      t.weatherWidget.sampleCity,
    ])

    // 🔧 使用首页原子化可见性感知定时器，页面隐藏时自动暂停
    useHomeVisibilityInterval(fetchWeather, CACHE_DURATION, !isPreview)

    // 根据天气状况选择主题色 - 使用 useMemo 缓存
    const themeColor = useMemo(() => {
      if (!weatherData) return '#10b981'
      return getThemeColorFromCode(weatherData.weatherCode ?? 0)
    }, [weatherData])

    // 获取翻译后的天气状态文本
    const weatherText = useMemo(() => {
      if (!weatherData) return ''
      const key = getWeatherKeyFromCode(weatherData.weatherCode ?? 0)
      return t.weather[key] || weatherData.weather
    }, [weatherData, t])

    if (loading) {
      return (
        <div className="h-full w-full p-3">
          <WidgetSkeleton
            preset="hero"
            accent="var(--color-primary)"
            label={t.common.loading}
          />
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
        <WidgetShell
          containerRef={containerRef}
          padding={{ x: 16, y: 12 }}
          contentClassName="flex flex-row"
          background={
            <GlowBackground
              color={themeColor}
              animLevel={anim.level}
              shouldAnimate={anim.loop}
              variant="single"
              size="lg"
            />
          }
        >
          {/* 左侧：主要信息 (60%) */}
          <div className="w-[60%] flex flex-col justify-between">
              {/* 顶部：城市（flex-1 拿到确定宽度，避免 fit-content 测量陷阱） */}
              <div className="flex justify-between items-start">
                <FitText
                  as="div"
                  className="font-bold text-gray-700 dark:text-gray-200 flex-1 min-w-0"
                  max={16}
                  min={11}
                >
                  {weatherData.city}
                </FitText>
              </div>

              {/* 中部：温度和图标 */}
              <div className="flex items-center gap-3 my-auto">
                <MDiv
                  className="shrink-0"
                  initial={{ scale: 0.5, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                >
                  <WeatherAssetIcon
                    icon={weatherData.icon}
                    className="h-12 w-12 object-contain drop-shadow-md"
                    fallbackClassName="text-4xl leading-none drop-shadow-md"
                  />
                </MDiv>
                <div className="flex flex-col justify-center min-w-0 flex-1">
                  <div className="flex items-baseline gap-2 overflow-hidden">
                    <FitText
                      className="font-black text-gray-800 dark:text-gray-100 tracking-tight flex-1 min-w-0"
                      max={42}
                      min={26}
                    >
                      {weatherData.temperature}
                    </FitText>
                  </div>
                  <div className="flex items-baseline gap-2 mt-1 overflow-hidden whitespace-nowrap">
                    <FitText
                      className="text-gray-600 dark:text-gray-400 font-medium flex-1"
                      max={14}
                      min={10}
                    >
                      {weatherText}
                    </FitText>
                    {weatherData.feelsLike !== undefined && (
                      <span className="text-xs text-gray-500 dark:text-gray-500 shrink-0">
                        {t.weather.feelsLike} {weatherData.feelsLike}°
                      </span>
                    )}
                  </div>
                </div>
              </div>

              {/* 底部：详细信息 (一行排列) */}
              <div
                className="flex items-center gap-3 text-gray-500 dark:text-gray-400 overflow-hidden whitespace-nowrap"
                style={{ fontSize: '0.6rem' }}
              >
                {weatherData.humidity !== undefined && (
                  <div
                    className="flex items-center gap-1"
                    title={t.weather.humidity}
                  >
                    <WeatherAssetIcon
                      icon={WEATHER_DETAIL_ICON_ASSETS.humidity}
                      className="h-3.5 w-3.5 shrink-0 object-contain"
                    />
                    <span>{weatherData.humidity}%</span>
                  </div>
                )}
                {weatherData.windSpeed !== undefined && (
                  <div
                    className="flex items-center gap-1"
                    title={t.weather.windSpeed}
                  >
                    <WeatherAssetIcon
                      icon={WEATHER_DETAIL_ICON_ASSETS.wind}
                      className="h-3.5 w-3.5 shrink-0 object-contain"
                    />
                    <span>
                      {Math.round(weatherData.windSpeed)}
                      km/h
                    </span>
                  </div>
                )}
                {weatherData.aqi !== undefined && (
                  <div
                    className="flex items-center gap-1"
                    title={t.weather.airQuality}
                  >
                    <WeatherAssetIcon
                      icon={getAirQualityIcon(weatherData.aqi)}
                      className="h-3.5 w-3.5 shrink-0 object-contain"
                    />
                    <span
                      className={
                        weatherData.aqi <= 50
                          ? 'text-green-500'
                          : weatherData.aqi <= 100
                            ? 'text-yellow-500'
                            : weatherData.aqi <= 150
                              ? 'text-orange-500'
                              : 'text-red-500'
                      }
                    >
                      AQI {weatherData.aqi}
                    </span>
                  </div>
                )}
              </div>
            </div>

            {/* 右侧：未来天气预报 (40%) */}
            <div className="w-[40%] pl-2 flex flex-col justify-between gap-1 h-full">
              {weatherData.forecast ? (
                weatherData.forecast.slice(0, 3).map((day, i) => (
                  <MDiv
                    key={day.date}
                    className="flex-1 flex items-center justify-between px-2 rounded-md hover:bg-white/40 dark:hover:bg-white/5 transition-colors"
                    initial={{ opacity: 0, x: 10 }}
                    animate={{ opacity: 1, x: 0 }}
                    transition={{ delay: 0.1 * i }}
                  >
                    <div
                      className="text-gray-500 dark:text-gray-400 w-8"
                      style={{ fontSize: '0.6rem' }}
                    >
                      {new Date(day.date).toLocaleDateString(locale, {
                        weekday: 'short',
                      })}
                    </div>
                    <div className="shrink-0 leading-none mx-1">
                      <WeatherAssetIcon
                        icon={day.icon}
                        className="h-5 w-5 object-contain"
                        fallbackClassName="text-base leading-none"
                      />
                    </div>
                    <div className="flex items-center gap-1 justify-end flex-1">
                      <span
                        className="font-bold text-gray-800 dark:text-gray-100"
                        style={{ fontSize: '0.6rem' }}
                      >
                        {day.maxTemp}°
                      </span>
                      <span
                        className="text-gray-400 dark:text-gray-500"
                        style={{ fontSize: '0.6rem' }}
                      >
                        {day.minTemp}°
                      </span>
                    </div>
                  </MDiv>
                ))
              ) : (
                <div
                  className="w-full h-full flex items-center justify-center text-gray-400"
                  style={{ fontSize: '0.6rem' }}
                >
                  {t.weather.updating}
                </div>
              )}
            </div>
        </WidgetShell>
      )
    }

    // 4x1 紧凑横版布局 (参考 4x2 但只显示1天预报)
    if (config.size === '4x1') {
      // 获取明天预报 (通常是索引1，索引0为今天)
      const tomorrow =
        weatherData.forecast && weatherData.forecast.length > 1
          ? weatherData.forecast[1]
          : null

      return (
        <WidgetShell
          containerRef={containerRef}
          padding={{ x: 16, y: 8 }}
          contentClassName="flex flex-row"
          background={
            <GlowBackground
              color={themeColor}
              animLevel={anim.level}
              shouldAnimate={anim.loop}
              variant="single"
              size="lg"
            />
          }
        >
          {/* 左侧：主要信息 (75%) */}
          <div className="w-[75%] flex items-center pr-3 gap-3">
              {/* 图标 & 温度 */}
              <div className="flex items-center gap-2 shrink-0">
                <WeatherAssetIcon
                  icon={weatherData.icon}
                  className="h-10 w-10 shrink-0 object-contain drop-shadow-sm"
                  fallbackClassName="text-3xl leading-none"
                />
                <div className="flex flex-col justify-center max-w-26">
                  <FitText
                    as="div"
                    className="font-black text-gray-800 dark:text-gray-100"
                    max={26}
                    min={18}
                  >
                    {weatherData.temperature}
                  </FitText>
                  <FitText
                    as="div"
                    className="text-gray-500 dark:text-gray-400 mt-0.5 font-medium"
                    max={10}
                    min={9}
                  >
                    {weatherText}
                  </FitText>
                </div>
              </div>

              {/* 城市 & 详情 */}
              <div className="flex flex-col justify-center gap-1 min-w-0 flex-1">
                <FitText
                  as="div"
                  className="font-bold text-gray-700 dark:text-gray-200"
                  max={12}
                  min={10}
                >
                  {weatherData.city}
                </FitText>
                <div className="flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400">
                  {weatherData.humidity !== undefined && (
                    <span className="flex items-center gap-0.5 whitespace-nowrap">
                      <WeatherAssetIcon
                        icon={WEATHER_DETAIL_ICON_ASSETS.humidity}
                        className="h-3.5 w-3.5 shrink-0 object-contain"
                      />
                      {weatherData.humidity}%
                    </span>
                  )}
                  {weatherData.windSpeed !== undefined && (
                    <span className="flex items-center gap-0.5 whitespace-nowrap">
                      <WeatherAssetIcon
                        icon={WEATHER_DETAIL_ICON_ASSETS.wind}
                        className="h-3.5 w-3.5 shrink-0 object-contain"
                      />
                      {Math.round(weatherData.windSpeed)}
                    </span>
                  )}
                </div>
              </div>
            </div>

            {/* 右侧：明天预报 (25%) - 极简模式 */}
            <div className="w-[25%] pl-1 flex flex-col items-center justify-center h-full">
              {tomorrow ? (
                <>
                  <div className="text-[10px] text-gray-400 dark:text-gray-500 mb-0.5 scale-90 origin-bottom">
                    {t.weather.tomorrow}
                  </div>
                  <div className="flex items-center gap-1.5">
                    <WeatherAssetIcon
                      icon={tomorrow.icon}
                      className="h-5 w-5 shrink-0 object-contain"
                      fallbackClassName="text-base leading-none"
                    />
                    <div className="flex flex-col items-end leading-none gap-0.5">
                      <span className="font-bold text-gray-800 dark:text-gray-100 text-xs">
                        {tomorrow.maxTemp}°
                      </span>
                      <span className="text-gray-400 dark:text-gray-500 text-[10px]">
                        {tomorrow.minTemp}°
                      </span>
                    </div>
                  </div>
                </>
              ) : (
                <div className="text-xs text-gray-400 text-center">
                  {t.weather.noForecast}
                </div>
              )}
            </div>
        </WidgetShell>
      )
    }

    return (
      <WidgetShell
        containerRef={containerRef}
        padding={{ x: 14, y: 12 }}
        contentClassName="flex flex-col"
        background={
          <GlowBackground
            color={themeColor}
            animLevel={anim.level}
            shouldAnimate={anim.loop}
            variant="single"
            size="md"
          />
        }
      >
        {/* 图标：与温度紧邻，整组内容在卡内垂直居中 */}
        <div className="h-9 shrink-0 mb-1">
            <MSpan
              className="inline-flex h-9 w-9 items-center justify-center origin-center"
              style={{ transformOrigin: 'center center' }}
              initial={{ scale: 0.5, opacity: 0, rotate: -15 }}
              animate={
                canAnimate
                  ? { scale: 1, opacity: 1, rotate: [-2, 2, -2] }
                  : { scale: 1, opacity: 1, rotate: 0 }
              }
              transition={
                canAnimate
                  ? {
                      scale: { duration: 0.6, ease: [0.34, 1.56, 0.64, 1] },
                      opacity: { duration: 0.6 },
                      rotate: { duration: 3, repeat: 2, ease: 'easeInOut' },
                    }
                  : { duration: 0.4 }
              }
            >
              <WeatherAssetIcon
                icon={weatherData.icon}
                className="h-9 w-9 object-contain drop-shadow-sm"
                fallbackClassName="text-3xl leading-none"
              />
            </MSpan>
          </div>

          {/* 主视觉：温度 + 状态同一基线横排，吃满整行宽度（右侧不留死区）。
              温度是数字与语言无关，固定大号；状态 FitText 占余下宽度自适应。
              不加 overflow 裁剪，绝不拆切字形 */}
          <MDiv
            className="flex items-baseline gap-2 min-w-0 mb-1.5 shrink-0"
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{
              duration: 0.6,
              delay: 0.2,
              ease: [0.34, 1.56, 0.64, 1],
            }}
          >
            <div
              className="font-black text-gray-800 dark:text-gray-100 leading-none shrink-0"
              style={{ fontSize: '2.125rem' }}
            >
              {weatherData.temperature}
            </div>
            {/* 状态：占满温度右侧余宽，任何语言单行自适应 */}
            <FitText
              className="flex-1 text-gray-600 dark:text-gray-400 font-medium"
              max={13}
              min={10}
            >
              {weatherText}
            </FitText>
          </MDiv>

          {/* 底部信息组：城市 + 详情，次级信息收在底部，弱化颜色拉开层级 */}
          <MDiv
            className="shrink-0 mb-1"
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{
              duration: 0.6,
              delay: 0.3,
              ease: [0.34, 1.56, 0.64, 1],
            }}
          >
            <FitText
              as="div"
              className="text-gray-500 dark:text-gray-400"
              max={10}
              min={9}
            >
              {weatherData.city}
            </FitText>
          </MDiv>

          {/* 次要信息：湿度/风速/空气质量 - 横向紧凑排列，shrink-0 保证不被主区挤出 */}
          {(weatherData.humidity !== undefined ||
            weatherData.windSpeed !== undefined ||
            weatherData.aqi !== undefined) && (
            <MDiv
              className="flex items-center gap-1.5 text-[10px] shrink-0 overflow-hidden whitespace-nowrap mt-auto"
              initial={{ y: 10, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              transition={
                !isStandardAnimation(anim)
                  ? { duration: 0.2 }
                  : { duration: 0.4, delay: 0.3 }
              }
            >
              {weatherData.humidity !== undefined && (
                <div
                  className="flex items-center gap-0.5"
                  title={t.weather.humidity}
                >
                  <WeatherAssetIcon
                    icon={WEATHER_DETAIL_ICON_ASSETS.humidity}
                    className="h-3.5 w-3.5 shrink-0 object-contain"
                  />
                  <span className="text-gray-600 dark:text-gray-400">
                    {weatherData.humidity}%
                  </span>
                </div>
              )}
              {weatherData.windSpeed !== undefined && (
                <div
                  className="flex items-center gap-0.5"
                  title={t.weather.windSpeed}
                >
                  <WeatherAssetIcon
                    icon={WEATHER_DETAIL_ICON_ASSETS.wind}
                    className="h-3.5 w-3.5 shrink-0 object-contain"
                  />
                  {/* 2x2 空间紧，只显数字，单位收进 title 提示 */}
                  <span className="text-gray-600 dark:text-gray-400">
                    {Math.round(weatherData.windSpeed)}
                  </span>
                </div>
              )}
              {weatherData.aqi !== undefined && (
                <div
                  className="flex items-center gap-0.5"
                  title={t.weather.airQuality}
                >
                  <WeatherAssetIcon
                    icon={getAirQualityIcon(weatherData.aqi)}
                    className="h-3.5 w-3.5 shrink-0 object-contain"
                  />
                  <span
                    className={
                      weatherData.aqi <= 50
                        ? 'text-green-500'
                        : weatherData.aqi <= 100
                          ? 'text-yellow-500'
                          : weatherData.aqi <= 150
                            ? 'text-orange-500'
                            : 'text-red-500'
                    }
                  >
                    {weatherData.aqi}
                  </span>
                </div>
              )}
            </MDiv>
          )}
        </WidgetShell>
    )
  },
)

WeatherWidget.displayName = 'WeatherWidget'
