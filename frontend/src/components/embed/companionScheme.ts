/**
 * 伴读上的深浅色按钮。博客主题怎么切深浅色，构建时记在博客页的 `<html data-myriad-scheme>` 上
 * （格式见 crates/myriad-blog/src/scheme.rs）；按钮照它看现在是哪一边，切的时候改的是主站的深浅色偏好，
 * 再重新打开这一页，让后端、导航岛和评论区一起换。
 */

import { SITE_CHROME_COOKIE, siteChromeCookieLine, siteChromeSwitched } from '../../utils/siteChromeCookie'

export const SCHEME_MARK_ATTR = 'data-myriad-scheme'

export interface PageSchemeSwitch {
  kind: 'class' | 'attr'
  target: 'html' | 'body'
  /** 属性名；类的时候是 `class`。 */
  name: string
  dark: string
  light: string | null
}

const TOKEN = /^[\w-]{1,40}$/
const DATA_ATTR = /^data-[a-z0-9-]{1,40}$/

/** 只认后端写的那两种；只跟系统、没有深色的主题没有标记，按钮也就不出现。 */
export function parseSchemeMark(mark: string | null): PageSchemeSwitch | null {
  const parts = mark?.split(' ') ?? []
  const light = (value: string) => (value === '-' ? null : value)
  let parsed: PageSchemeSwitch | null = null
  if (parts.length === 4 && parts[0] === 'class') {
    parsed = { kind: 'class', target: parts[1] as 'html', name: 'class', dark: parts[2], light: light(parts[3]) }
  } else if (parts.length === 5 && parts[0] === 'attr') {
    parsed = { kind: 'attr', target: parts[1] as 'html', name: parts[2], dark: parts[3], light: light(parts[4]) }
  }
  if (!parsed || (parsed.target !== 'html' && parsed.target !== 'body')) return null
  if (!TOKEN.test(parsed.dark) || (parsed.light != null && !TOKEN.test(parsed.light))) return null
  if (parsed.kind === 'attr' && !DATA_ATTR.test(parsed.name)) return null
  return parsed
}

/** 这一页现在是不是深色：按主题自己的办法看 `<html>` / `<body>` 上写着什么。 */
export function pageIsDark(doc: Document, value: PageSchemeSwitch): boolean {
  const element = value.target === 'html' ? doc.documentElement : doc.body
  if (!element) return false
  return value.kind === 'class'
    ? element.classList.contains(value.dark)
    : element.getAttribute(value.name) === value.dark
}

export function pageSchemeSwitch(doc: Document): PageSchemeSwitch | null {
  return parseSchemeMark(doc.documentElement.getAttribute(SCHEME_MARK_ATTR))
}

/** 把主站的深浅色固定成 `dark` 这一边，再重新打开这一页。 */
export function switchSiteScheme(win: Window, dark: boolean): void {
  try {
    win.localStorage.setItem('theme', dark ? 'dark' : 'light')
  } catch {
    // 存不下也照样改 cookie：这一页还是会换。
  }
  const current = win.document.cookie
    .split('; ')
    .find((part) => part.startsWith(`${SITE_CHROME_COOKIE}=`))
    ?.slice(SITE_CHROME_COOKIE.length + 1)
  win.document.cookie = siteChromeCookieLine(siteChromeSwitched(current ?? null, dark), win.location.protocol === 'https:')
  win.location.reload()
}
