/**
 * 从博客页上取色、跟着页面变：算法在 pagePalette.ts。控件各取自己背后那一块——伴读的胶囊在页边，
 * 创作台的细条在页头上（Ananke 那种黑页头上它就是深的），创作台展开后的窗口在页面中间。
 */

import type { Palette, Rgb } from './pagePalette'
import { SITE_CHROME_COOKIE } from '../../utils/siteChromeCookie'
import { pageSchemeSwitch } from './companionScheme'
import { derivePalette, over, parseColor } from './pagePalette'

/** 我们自己画在页面上的东西：取色时跳过，取的是主题的页面。 */
const OWN = '#myriad-companion, #myriad-studio-host, #myriad-nav, .myriad-annot-tip'

/** 深浅切换的视图过渡要这么久（companionScheme.ts 里是 240ms），过完再取一次。 */
const SETTLE_MS = 400

/** 页面根上什么都没铺时，浏览器画的底。 */
const CANVAS_LIGHT: Rgb = { r: 255, g: 255, b: 255, a: 1 }
const CANVAS_DARK: Rgb = { r: 18, g: 18, b: 18, a: 1 }

export interface Point {
  x: number
  y: number
}

/** 主站写给博客页的 cookie 里的强调色（`{设置}.{深浅色}.{主色}.{强调色}`，见 siteChromeCookie.ts）。 */
export function siteAccent(doc: Document): Rgb | null {
  const raw = doc.cookie.split('; ').find((part) => part.startsWith(`${SITE_CHROME_COOKIE}=`))
  const accent = raw?.slice(SITE_CHROME_COOKIE.length + 1).split('.')[3]
  return accent && /^[0-9a-f]{6}$/.test(accent) ? parseColor(`#${accent}`) : null
}

function hitAt(doc: Document, point: Point): Element | null {
  const view = doc.defaultView
  if (!view) return null
  const x = Math.min(Math.max(point.x, 0), view.innerWidth - 1)
  const y = Math.min(Math.max(point.y, 0), view.innerHeight - 1)
  const hit = doc.elementsFromPoint(x, y).find((element) => !element.closest(OWN))
  // 视图过渡进行中，命中的只有 <html>（整页都是过渡的画面）：从 <body> 往外找。
  return !hit || hit === doc.documentElement ? doc.body : hit
}

/** 从这个元素往外叠底色，直到不透明；中途碰到背景图就记下来，底色那时就不可信了。 */
function backgroundBehind(element: Element | null, doc: Document): { color: Rgb; image: boolean } {
  const view = doc.defaultView!
  let color: Rgb = { r: 0, g: 0, b: 0, a: 0 }
  let image = false
  for (let node: Element | null = element; node; node = node.parentElement) {
    const style = view.getComputedStyle(node)
    if (style.backgroundImage !== 'none' && !image && color.a < 0.5) image = true
    const layer = parseColor(style.backgroundColor)
    if (layer && layer.a > 0) color = over(color, layer)
    if (color.a >= 0.999) return { color, image }
  }
  const scheme = view.getComputedStyle(doc.documentElement).colorScheme
  const canvas = /\bdark\b/.test(scheme) && !/\blight\b/.test(scheme) ? CANVAS_DARK : CANVAS_LIGHT
  return { color: over(color, canvas), image }
}

export function samplePalette(doc: Document, point: Point, textFrom?: Element | null): Palette {
  const hit = hitAt(doc, point)
  const { color, image } = backgroundBehind(hit, doc)
  const textElement = textFrom ?? hit
  const text = textElement ? parseColor(doc.defaultView!.getComputedStyle(textElement).color) : null
  return derivePalette({ background: color, text, image, accent: siteAccent(doc) })
}

export interface PaletteTarget {
  /** 变量写在这个元素上，名字是 `--{prefix}{名字}`。 */
  element: HTMLElement
  prefix: string
  /** 取色的地方；null 就不取（控件还没摆好）。 */
  at: () => Point | null
  /** 文字色从哪个元素取；不给就用取色点上的元素。 */
  textFrom?: () => Element | null
  /** 写深浅的属性（`data-scheme` 之类），宿主节点的 color-scheme 跟着它。 */
  schemeAttr?: string
  /** 页面滚动时背后会换（固定在顶上的细条）。 */
  followScroll?: boolean
  /** 这一份也写给正文里的划线和提示框（伴读的那份）。 */
  pageStyle?: boolean
}

/** 正文里的划线和划线的提示框在主题的页面里，变量写进一张样式表，不碰主题的元素。 */
const PAGE_STYLE_ID = 'myriad-palette'

function writePageStyle(doc: Document, palette: Palette): void {
  let style = doc.getElementById(PAGE_STYLE_ID)
  if (!style) {
    style = doc.createElement('style')
    style.id = PAGE_STYLE_ID
    doc.head.append(style)
  }
  const v = palette.vars
  const kinds = Object.keys(v)
    .filter((name) => name.startsWith('kind-') && !name.endsWith('-bg'))
    .map((name) => `mark.myriad-annot[data-type=${name.slice(5)}]{--myriad-annot:${v[name]}}`)
    .join('')
  const text = `:root{--myriad-tip-fg:${v.fg};--myriad-tip-soft:${v.soft};--myriad-tip-bg:${v.solid};--myriad-tip-line:${v.line};--myriad-tip-scheme:${palette.dark ? 'dark' : 'light'}}${kinds}`
  if (style.textContent !== text) style.textContent = text
}

/**
 * 取一次色、写上，之后页面一变就重取：页面的深浅（`<html>` / `<body>` 上的类和属性）、系统深浅、
 * 窗口大小、样式表加载完；跟着滚动的只在滚动时多取那一个。返回停止的函数。
 */
export function watchPalette(doc: Document, targets: PaletteTarget[]): () => void {
  const view = doc.defaultView
  if (!view || targets.length === 0) return () => {}
  const written = new Map<PaletteTarget, string>()
  const paint = (target: PaletteTarget) => {
    const point = target.at()
    if (!point) return
    const palette = samplePalette(doc, point, target.textFrom?.())
    const key = JSON.stringify(palette)
    if (written.get(target) === key) return
    written.set(target, key)
    for (const [name, value] of Object.entries(palette.vars)) {
      target.element.style.setProperty(`--${target.prefix}${name}`, value)
    }
    if (target.schemeAttr) target.element.setAttribute(target.schemeAttr, palette.dark ? 'dark' : 'light')
    if (target.pageStyle) writePageStyle(doc, palette)
  }
  const paintAll = () => targets.forEach(paint)
  let frame = 0
  const schedule = () => {
    view.cancelAnimationFrame(frame)
    frame = view.requestAnimationFrame(paintAll)
  }
  let scrollFrame = 0
  const scrolling = targets.filter((target) => target.followScroll)
  const onScroll = () => {
    view.cancelAnimationFrame(scrollFrame)
    scrollFrame = view.requestAnimationFrame(() => scrolling.forEach(paint))
  }

  paintAll()
  // 深浅切换时要赶在视图过渡拍新画面之前写好：页面上的改动一到就同步重取，不等下一帧。
  // 过渡进行中取不到具体那一点（只命中 <html>），过渡完再取一次。
  // 主题自己切深浅色用的那个属性（构建时认出来、记在页面上的）也算。
  const watched = [...new Set(['class', 'style', 'data-theme', 'data-scheme', 'data-mode', pageSchemeSwitch(doc)?.name ?? 'class'])]
  let settle = 0
  const observer = new MutationObserver(() => {
    paintAll()
    view.clearTimeout(settle)
    settle = view.setTimeout(paintAll, SETTLE_MS)
  })
  observer.observe(doc.documentElement, { attributes: true, attributeFilter: watched })
  if (doc.body) observer.observe(doc.body, { attributes: true, attributeFilter: watched })
  const media = view.matchMedia('(prefers-color-scheme: dark)')
  media.addEventListener('change', schedule)
  view.addEventListener('resize', schedule)
  view.addEventListener('load', schedule)
  if (scrolling.length) view.addEventListener('scroll', onScroll, { passive: true })
  return () => {
    observer.disconnect()
    view.clearTimeout(settle)
    view.cancelAnimationFrame(frame)
    view.cancelAnimationFrame(scrollFrame)
    media.removeEventListener('change', schedule)
    view.removeEventListener('resize', schedule)
    view.removeEventListener('load', schedule)
    view.removeEventListener('scroll', onScroll)
  }
}
