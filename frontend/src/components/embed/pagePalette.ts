/**
 * 伴读条和创作台细条的颜色从博客页上取：主题千差万别（暖白、冷灰、藏青、只有浅色的），
 * 写死的一套放在哪个主题上都像外来物。规则只有几条：
 *
 * - 底色和文字色取控件背后那块页面的；面板比页面高一层（浅色更白、深色更亮），带着主题自己的色调。
 * - 深浅由那块页面的底色决定，不看主站设置：主题没有深色时控件也是浅的。
 * - 强调色是主站的那个（和导航岛一家），只调明暗到看得清。
 * - 注释的几种类别色相固定，明暗按底色算到看得清。
 *
 * 这里只有纯计算；从页面上取色、跟着页面变见 pagePaletteDom.ts。
 */

export interface Rgb {
  r: number
  g: number
  b: number
  /** 0–1 */
  a: number
}

interface Oklch {
  l: number
  c: number
  h: number
}

/** `/ 50%`、`/ 0.5` 之后那一段；没写就是不透明。 */
function alphaOf(part: string | undefined): number {
  if (part == null) return 1
  const value = part.endsWith('%') ? Number.parseFloat(part) / 100 : Number.parseFloat(part)
  return Math.min(1, Math.max(0, value))
}

/**
 * 主题用 `oklch()`、`oklab()`、`color(srgb …)` 写的颜色，getComputedStyle 原样给出来（Tailwind v4 的主题全是这种）。
 * `display-p3` 按 sRGB 近似：只用来判深浅、取色调，差一点看不出来。
 */
function parseModern(text: string): Rgb | null {
  const fn = /^(oklch|oklab|color)\(([^)]+)\)$/.exec(text)
  if (!fn) return null
  const [main, alphaPart] = fn[2].split('/').map((part) => part.trim())
  const parts = main.split(/[\s,]+/).filter(Boolean)
  const num = (part: string, percentOf = 1) => (part === 'none' ? 0 : part.endsWith('%') ? (Number.parseFloat(part) / 100) * percentOf : Number.parseFloat(part))
  let rgb: Rgb
  if (fn[1] === 'color') {
    if (parts.length < 4 || !['srgb', 'srgb-linear', 'display-p3'].includes(parts[0])) return null
    const [r, g, b] = parts.slice(1, 4).map((part) => num(part))
    const channel = (v: number) => (parts[0] === 'srgb-linear' ? fromLinear(v) : v * 255)
    rgb = { r: channel(r), g: channel(g), b: channel(b), a: 1 }
  } else {
    if (parts.length < 3) return null
    const l = num(parts[0])
    if (fn[1] === 'oklch') {
      rgb = fromOklch({ l, c: num(parts[1], 0.4), h: num(parts[2]) })
    } else {
      const a = num(parts[1], 0.4)
      const b = num(parts[2], 0.4)
      rgb = fromOklch({ l, c: Math.hypot(a, b), h: (Math.atan2(b, a) * 180) / Math.PI })
    }
  }
  const alpha = alphaOf(alphaPart)
  return [rgb.r, rgb.g, rgb.b, alpha].every(Number.isFinite) ? { ...rgb, a: alpha } : null
}

/** 认 getComputedStyle 给的 `rgb()` / `rgba()`（逗号或空格写法）、`#rgb` / `#rrggbb` 和 `oklch()` 这几种新写法。 */
export function parseColor(value: string | null | undefined): Rgb | null {
  if (!value) return null
  const text = value.trim().toLowerCase()
  if (text === 'transparent') return { r: 0, g: 0, b: 0, a: 0 }
  const modern = parseModern(text)
  if (modern) return modern
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(text)
  if (hex) {
    const digits = hex[1].length === 3 ? [...hex[1]].map((d) => d + d).join('') : hex[1]
    return {
      r: Number.parseInt(digits.slice(0, 2), 16),
      g: Number.parseInt(digits.slice(2, 4), 16),
      b: Number.parseInt(digits.slice(4, 6), 16),
      a: 1,
    }
  }
  const fn = /^rgba?\(([^)]+)\)$/.exec(text)
  if (!fn) return null
  const parts = fn[1].split(/[\s,/]+/).filter(Boolean)
  if (parts.length < 3) return null
  const channel = (part: string) => (part.endsWith('%') ? (Number.parseFloat(part) * 255) / 100 : Number.parseFloat(part))
  const alpha = parts[3] == null ? 1 : parts[3].endsWith('%') ? Number.parseFloat(parts[3]) / 100 : Number.parseFloat(parts[3])
  const [r, g, b] = parts.slice(0, 3).map(channel)
  if (![r, g, b, alpha].every(Number.isFinite)) return null
  return { r, g, b, a: Math.min(1, Math.max(0, alpha)) }
}

/** 上面一层（可能半透明）叠在下面一层上。 */
export function over(top: Rgb, bottom: Rgb): Rgb {
  const a = top.a + bottom.a * (1 - top.a)
  if (a === 0) return { r: 0, g: 0, b: 0, a: 0 }
  const mix = (t: number, b: number) => (t * top.a + b * bottom.a * (1 - top.a)) / a
  return { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a }
}

function toLinear(v: number) {
  const c = v / 255
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
}
function fromLinear(v: number) {
  const c = v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055
  return c * 255
}

/** WCAG 2 的相对亮度和对比度。 */
export function luminance(color: Rgb): number {
  return 0.2126 * toLinear(color.r) + 0.7152 * toLinear(color.g) + 0.0722 * toLinear(color.b)
}

export function contrast(a: Rgb, b: Rgb): number {
  const [x, y] = [luminance(a), luminance(b)].sort((m, n) => n - m)
  return (x + 0.05) / (y + 0.05)
}

function toOklch(color: Rgb): Oklch {
  const [r, g, b] = [toLinear(color.r), toLinear(color.g), toLinear(color.b)]
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  const h = (Math.atan2(B, A) * 180) / Math.PI
  return { l: L, c: Math.hypot(A, B), h: h < 0 ? h + 360 : h }
}

function rawRgb({ l: L, c, h }: Oklch): [number, number, number] {
  const A = c * Math.cos((h * Math.PI) / 180)
  const B = c * Math.sin((h * Math.PI) / 180)
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3
  return [
    fromLinear(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    fromLinear(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    fromLinear(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
  ]
}

/** 超出屏幕能显示的范围就降饱和度，色相和明暗不动。 */
function fromOklch(color: Oklch): Rgb {
  const inGamut = (rgb: number[]) => rgb.every((v) => v >= -0.5 && v <= 255.5)
  let c = color.c
  let rgb = rawRgb({ ...color, c })
  for (let i = 0; i < 24 && !inGamut(rgb); i++) {
    c *= 0.85
    rgb = rawRgb({ ...color, c })
  }
  const clamp = (v: number) => Math.min(255, Math.max(0, v))
  return { r: clamp(rgb[0]), g: clamp(rgb[1]), b: clamp(rgb[2]), a: 1 }
}

function mix(a: Rgb, b: Rgb, amountOfA: number): Rgb {
  const t = amountOfA
  return { r: a.r * t + b.r * (1 - t), g: a.g * t + b.g * (1 - t), b: a.b * t + b.b * (1 - t), a: 1 }
}

/** 色相、饱和度不动，只挪明暗，直到对 `against` 的对比度够 `min`；挪到头也不够就停在最远处。 */
function untilContrast(color: Rgb, against: Rgb, min: number): Rgb {
  // 留一点余量：最后要取整成 #rrggbb，正好卡在线上的会掉到线下。
  const target = min + 0.1
  if (contrast(color, against) >= target) return color
  const base = toOklch(color)
  const toward = luminance(against) > 0.18 ? -1 : 1
  for (let step = 1; step <= 50; step++) {
    const next = fromOklch({ ...base, l: Math.min(1, Math.max(0, base.l + toward * step * 0.02)) })
    if (contrast(next, against) >= target) return next
  }
  return fromOklch({ ...base, l: toward < 0 ? 0 : 1 })
}

export function hex(color: Rgb): string {
  const part = (v: number) => Math.round(v).toString(16).padStart(2, '0')
  return `#${part(color.r)}${part(color.g)}${part(color.b)}`
}

function rgba(color: Rgb, alpha: number): string {
  return `rgb(${Math.round(color.r)} ${Math.round(color.g)} ${Math.round(color.b)} / ${Math.round(alpha * 1000) / 10}%)`
}

/** 注释的类别，色相照正文里划线的颜色（crates/myriad-blog/src/postprocess.rs）。 */
export const KIND_HUES = {
  context: 152,
  reference: 255,
  implicit: 305,
  abbreviation: 350,
  term: 45,
} as const
export type KindName = keyof typeof KIND_HUES

/** AI 的紫和客串声音的暖棕：只是两种色相。 */
const AI_HUE = 300
const GUEST_HUE = 55

/** 主站没给强调色时用的那道金色。 */
export const FALLBACK_ACCENT = '#b8860b'

export interface PaletteInput {
  /** 控件背后那块页面的底色，已经叠到不透明。 */
  background: Rgb
  /** 那块页面的文字色（可能半透明，叠在底色上算）。 */
  text: Rgb | null
  /** 背后是图片、渐变：底色不可信，深浅看文字色。 */
  image?: boolean
  /** 主站的强调色。 */
  accent?: Rgb | null
}

export interface Palette {
  dark: boolean
  /** 写到宿主节点上的 CSS 变量，名字不带前缀。 */
  vars: Record<string, string>
}

const WHITE: Rgb = { r: 255, g: 255, b: 255, a: 1 }
const BLACK: Rgb = { r: 0, g: 0, b: 0, a: 1 }

export function derivePalette({ background, text, image = false, accent }: PaletteInput): Palette {
  const page = toOklch(background)
  const ink = text ? over(text, background) : null
  const dark = image && ink ? toOklch(ink).l > 0.6 : page.l < 0.5
  // 主题的色调留一点（暖的还暖、藏青还藏青），饱和度压低：控件不该比页面抢眼。
  const tint = image ? 0 : Math.min(page.c, 0.03)
  const baseL = image ? (dark ? 0.24 : 0.985) : page.l
  const solid = fromOklch({
    l: dark ? Math.min(0.42, Math.max(0.2, baseL + 0.05)) : Math.min(1, Math.max(0.965, baseL + 0.02)),
    c: tint,
    h: page.h,
  })
  const fg = untilContrast(ink && contrast(ink, solid) >= 3 ? ink : dark ? fromOklch({ l: 0.9, c: tint, h: page.h }) : fromOklch({ l: 0.27, c: tint, h: page.h }), solid, 7)
  const soft = untilContrast(mix(fg, solid, 0.62), solid, 4.5)
  const line = mix(fg, solid, dark ? 0.2 : 0.13)
  const accentColor = untilContrast(accent ?? parseColor(FALLBACK_ACCENT)!, solid, 3)
  const onAccent = contrast(WHITE, accentColor) >= contrast(BLACK, accentColor) ? WHITE : BLACK

  const tone = (hue: number, chroma: number) => {
    const base = fromOklch({ l: dark ? 0.78 : 0.52, c: chroma, h: hue })
    const chip = mix(base, solid, dark ? 0.22 : 0.13)
    return { fg: untilContrast(base, chip, 4.5), bg: chip }
  }

  const vars: Record<string, string> = {
    fg: hex(fg),
    soft: hex(soft),
    line: hex(line),
    solid: hex(solid),
    chip: rgba(solid, dark ? 0.86 : 0.88),
    hover: rgba(fg, dark ? 0.09 : 0.06),
    accent: hex(accentColor),
    'on-accent': hex(onAccent),
    ai: hex(tone(AI_HUE, 0.2).fg),
    guest: hex(tone(GUEST_HUE, 0.06).fg),
    shadow: dark ? 'rgb(0 0 0 / 34%)' : rgba(mix(fg, BLACK, 0.4), 0.12),
  }
  for (const [kind, hue] of Object.entries(KIND_HUES)) {
    const colors = tone(hue, 0.17)
    vars[`kind-${kind}`] = hex(colors.fg)
    vars[`kind-${kind}-bg`] = hex(colors.bg)
  }
  return { dark, vars }
}
