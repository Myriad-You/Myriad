/**
 * 伴读上的深浅色按钮。博客主题怎么切深浅色，构建时记在博客页的 `<html data-myriad-scheme>` 上
 * （格式见 crates/myriad-blog/src/scheme.rs）；按钮照它看现在是哪一边。切的时候改主站的深浅色偏好
 * （下一页后端照它出），这一页不刷新：照主题的办法原地改，导航岛和同源的小窗（评论、创作台、伴读自己）一起换。
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

function setPageScheme(doc: Document, value: PageSchemeSwitch, dark: boolean): void {
  const element = value.target === 'html' ? doc.documentElement : doc.body
  if (!element) return
  if (value.kind === 'class') {
    element.classList.toggle(value.dark, dark)
    if (value.light) element.classList.toggle(value.light, !dark)
    return
  }
  const next = dark ? value.dark : value.light
  if (next == null) element.removeAttribute(value.name)
  else element.setAttribute(value.name, next)
}

/** 主站应用的页面（伴读、评论、创作台）认 `<html>` 上的 dark / light 类。 */
function setAppScheme(doc: Document | null, dark: boolean): void {
  const html = doc?.documentElement
  if (!html) return
  html.classList.toggle('dark', dark)
  html.classList.toggle('light', !dark)
}

function frameDocument(frame: HTMLIFrameElement): Document | null {
  try {
    return frame.contentDocument
  } catch {
    return null
  }
}

/**
 * 把主站的深浅色固定成 `dark` 这一边，这一页原地换过去。`self` 是伴读自己的页面：
 * 它写的 localStorage 不会通知它自己。
 */
export function switchSiteScheme(win: Window, self: Document, dark: boolean): void {
  const doc = win.document
  try {
    win.localStorage.setItem('theme', dark ? 'dark' : 'light')
  } catch {
    // 存不下也照样改 cookie：下一页还是这一边。
  }
  const current = doc.cookie
    .split('; ')
    .find((part) => part.startsWith(`${SITE_CHROME_COOKIE}=`))
    ?.slice(SITE_CHROME_COOKIE.length + 1)
  doc.cookie = siteChromeCookieLine(siteChromeSwitched(current ?? null, dark), win.location.protocol === 'https:')
  const value = pageSchemeSwitch(doc)
  const apply = () => {
    if (value) setPageScheme(doc, value, dark)
    doc.getElementById('myriad-nav')?.setAttribute('data-scheme', dark ? 'dark' : 'light')
    setAppScheme(self, dark)
    for (const frame of doc.querySelectorAll('iframe')) setAppScheme(frameDocument(frame), dark)
  }
  const quiet = win.matchMedia('(prefers-reduced-motion: reduce)').matches
  if (quiet || typeof doc.startViewTransition !== 'function') apply()
  else doc.startViewTransition(apply)
}
