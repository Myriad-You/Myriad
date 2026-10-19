/**
 * 博客页上的导航岛要和主站长一个颜色，但博客页不跑脚本，主站的深浅色设置和壁纸取出的
 * 主色只在这边。所以这里把它们写进一个只发给 `/blog` 的 cookie，后端返回博客页时读。
 * 格式和 backend/src/api/blog.rs 的 `SiteChrome::parse` 对应：
 *   `{设置}.{现在的深浅色}.{主色}.{强调色}`，例如 `auto.dark.6b7280.c7d2fe`
 * 强调色（--cfg-accent）是按现在的深浅色算出来的，只对那一边有效。
 */

export const SITE_CHROME_COOKIE = 'myriad_chrome'

const MAX_AGE_SECONDS = 400 * 24 * 60 * 60

type ThemeSetting = 'light' | 'dark' | 'auto'

/** `#abc`、`#aabbcc`、`rgb(1 2 3)`、`rgb(1, 2, 3)` → `aabbcc`；别的格式不认。 */
export function toHex6(value: string): string | null {
  const color = value.trim().toLowerCase()
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/.exec(color)
  if (short) return short.slice(1).map((digit) => digit + digit).join('')
  const long = /^#([0-9a-f]{6})$/.exec(color)
  if (long) return long[1]
  const rgb = /^rgba?\(\s*(\d{1,3})[\s,]+(\d{1,3})[\s,]+(\d{1,3})\s*(?:[,/]\s*[\d.]+%?\s*)?\)$/.exec(color)
  if (!rgb) return null
  const channels = rgb.slice(1).map(Number)
  if (channels.some((channel) => channel > 255)) return null
  return channels.map((channel) => channel.toString(16).padStart(2, '0')).join('')
}

export function siteChromeValue(input: {
  setting: ThemeSetting
  dark: boolean
  primary: string
  accent: string
}): string | null {
  const primary = toHex6(input.primary)
  const accent = toHex6(input.accent)
  if (!primary || !accent) return null
  return [input.setting, input.dark ? 'dark' : 'light', primary, accent].join('.')
}

function readSetting(): ThemeSetting {
  try {
    const stored = localStorage.getItem('theme')
    return stored === 'light' || stored === 'dark' ? stored : 'auto'
  } catch {
    return 'auto'
  }
}

let lastWritten: string | null = null

function writeCookie() {
  const html = document.documentElement
  const styles = getComputedStyle(html)
  const value = siteChromeValue({
    setting: readSetting(),
    dark: html.classList.contains('dark'),
    primary: styles.getPropertyValue('--color-primary') || '#94a3b8',
    accent: styles.getPropertyValue('--cfg-accent') || styles.getPropertyValue('--color-primary') || '#94a3b8',
  })
  if (!value || value === lastWritten) return
  lastWritten = value
  const secure = location.protocol === 'https:' ? '; Secure' : ''
  document.cookie = `${SITE_CHROME_COOKIE}=${value}; Path=/blog; Max-Age=${MAX_AGE_SECONDS}; SameSite=Lax${secure}`
}

let started = false

/** 主色随壁纸变、深浅色随设置变：都落在 `<html>` 的 class 和 style 上，变了就重写。 */
export function ensureSiteChromeCookieSync(): void {
  if (started || typeof document === 'undefined') return
  started = true
  let pending: ReturnType<typeof setTimeout> | null = null
  const schedule = () => {
    if (pending != null) return
    pending = setTimeout(() => {
      pending = null
      writeCookie()
    }, 300)
  }
  writeCookie()
  new MutationObserver(schedule).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class', 'style'],
  })
  window.addEventListener('storage', (event) => {
    if (event.key === 'theme' || event.key === null) schedule()
  })
}
