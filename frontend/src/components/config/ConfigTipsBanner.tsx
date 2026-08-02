/**
 * 配置侧栏横版 banner（收藏夹上方）。
 * 按时段问候 + 上次登录；多 slide 时自动轮播（悬停/聚焦暂停）。
 */

import type { GreetingIconName } from '../../utils/dynamicContent'
import React, { useEffect, useId, useMemo, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import {
  getGreeting,

} from '../../utils/dynamicContent'
import { WEATHER_ICON_ASSETS } from '../../utils/weather'
import './ConfigTipsBanner.css'

export type GreetingPeriod =
  | 'morning'
  | 'forenoon'
  | 'noon'
  | 'afternoon'
  | 'dusk'
  | 'evening'
  | 'night'

interface BannerSlide {
  id: string
  html: string
  period?: GreetingPeriod
}

export interface ConfigTipsBannerProps {
  className?: string
}

const ROTATE_MS = 9000

const GREETING_ICON_SRC: Record<GreetingIconName, string> = {
  sunrise: '/icons/greeting/sunrise.webp',
  sun: WEATHER_ICON_ASSETS.sunny,
  'cloud-sun': WEATHER_ICON_ASSETS.partlyCloudy,
  sunset: '/icons/greeting/sunset.webp',
  moon: '/icons/greeting/night.webp',
}

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function periodFromHour(hour: number): GreetingPeriod {
  if (hour >= 5 && hour < 8) return 'morning'
  if (hour >= 8 && hour < 11) return 'forenoon'
  if (hour >= 11 && hour < 13) return 'noon'
  if (hour >= 13 && hour < 17) return 'afternoon'
  if (hour >= 17 && hour < 19) return 'dusk'
  if (hour >= 19 && hour < 22) return 'evening'
  return 'night'
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

/** Compact local datetime for the narrow sidebar meta line. */
function formatLastLogin(iso: string, locale: string): string | null {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return null
  return date.toLocaleString(locale, {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

/** 主：问候 · 次：上次登录 */
function buildGreetingHtml(opts: {
  period: GreetingPeriod
  text: string
  lastLoginLine: string | null
  iconSrc: string
}): string {
  const { period, text, lastLoginLine, iconSrc } = opts
  const metaHtml = lastLoginLine
    ? `<p class="gb-meta"><span class="gb-sub">${escapeHtml(lastLoginLine)}</span></p>`
    : ''
  return `
<div class="gb" data-period="${period}">
  <div class="gb-main">
    <img class="gb-icon" src="${escapeHtml(iconSrc)}" alt="" draggable="false" decoding="async" />
    <div class="gb-copy">
      <p class="gb-hello">${escapeHtml(text)}</p>
      ${metaHtml}
    </div>
  </div>
</div>`.trim()
}

export const ConfigTipsBanner: React.FC<ConfigTipsBannerProps> = ({
  className = '',
}) => {
  const { t, locale } = useI18n()
  const { user } = useAuth()
  const labelId = useId()
  const [now, setNow] = useState(() => new Date())
  const [index, setIndex] = useState(0)
  const [paused, setPaused] = useState(false)

  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 30_000)
    return () => window.clearInterval(id)
  }, [])

  const greetingSlide = useMemo<BannerSlide>(() => {
    const period = periodFromHour(now.getHours())
    const greeting = getGreeting(
      user?.username,
      {
        morning: t.greeting?.morning ?? '早上好',
        forenoon: t.greeting?.forenoon ?? t.greeting?.morning ?? '上午好',
        noon: t.greeting?.noon ?? '中午好',
        afternoon: t.greeting?.afternoon ?? '下午好',
        dusk: t.greeting?.dusk ?? t.greeting?.evening ?? '傍晚好',
        evening: t.greeting?.evening ?? '晚上好',
        night: t.greeting?.night ?? '夜深了',
      },
      locale,
    )

    const tpl =
      t.config.tipsBanner?.lastLogin ?? '上次登录 {time}'
    const neverLabel =
      t.config.tipsBanner?.lastLoginNever ?? '上次登录 —'
    let lastLoginLine: string | null = null
    if (user?.last_login_at) {
      const formatted = formatLastLogin(user.last_login_at, locale)
      if (formatted) {
        lastLoginLine = tpl.replace('{time}', formatted)
      }
    } else if (user) {
      // Logged in but no timestamp (legacy row / never set)
      lastLoginLine = neverLabel
    }

    return {
      id: `greeting-${period}`,
      period,
      html: buildGreetingHtml({
        period,
        text: greeting.text,
        lastLoginLine,
        iconSrc: GREETING_ICON_SRC[greeting.icon],
      }),
    }
  }, [now, t, locale, user?.username, user?.last_login_at, user])

  // 后续可在此追加 slide；>1 时自动轮播
  const slides = useMemo<BannerSlide[]>(
    () => [greetingSlide],
    [greetingSlide],
  )

  const count = slides.length
  const current = count > 0 ? slides[index % count] : null

  useEffect(() => {
    if (index >= count) setIndex(0)
  }, [count, index])

  useEffect(() => {
    if (count <= 1 || paused || prefersReducedMotion()) return
    const id = window.setInterval(() => {
      setIndex((i) => (i + 1) % count)
    }, ROTATE_MS)
    return () => window.clearInterval(id)
  }, [count, paused])

  if (!current) return null

  const bannerLabel = t.config.tipsBanner?.label ?? 'Banner'
  const period = current.period

  return (
    <div
      className={`config-tips-banner${className ? ` ${className}` : ''}`}
      role="region"
      aria-labelledby={labelId}
      aria-live="polite"
      data-period={period}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) {
          setPaused(false)
        }
      }}
    >
      <div className="config-tips-banner-card" data-period={period}>
        <span id={labelId} className="config-tips-banner-sr">
          {bannerLabel}
        </span>
        <div
          className="config-tips-banner-body"
          // 信任源：本组件拼装 + i18n 静态文案
          dangerouslySetInnerHTML={{ __html: current.html }}
          data-tip-id={current.id}
        />
      </div>
    </div>
  )
}

ConfigTipsBanner.displayName = 'ConfigTipsBanner'

export default ConfigTipsBanner
