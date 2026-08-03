/**
 * Tapp 颜色与图标壳（默认方案）
 *
 * 默认：多色 material 壳 + 现有 iconSvg/emoji/URL 白 glyph。
 * 不做宿主重绘；standalone 全彩图铺满不套壳。
 * 质感与 Dock「应用」入口一致：顶内高光 / 底边压暗 / 可选斜向 glaze。
 */

import type { CSSProperties } from 'react'
import type { TappCategory, TappPermission } from '../types'
import {
  hasStandaloneTappIcon,
  isTappIconFullColorMedia,
} from '../components/TappIcon'
import { normalizeTappCategory } from './tappCategories'

/** 类别渐变色（hex；壳填充用） */
export const CATEGORY_COLORS: Record<
  TappCategory,
  { fromHex: string; toHex: string }
> = {
  utility: { fromHex: '#10b981', toHex: '#14b8a6' },
  productivity: { fromHex: '#f97316', toHex: '#f59e0b' },
  game: { fromHex: '#f43f5e', toHex: '#ec4899' },
  social: { fromHex: '#0ea5e9', toHex: '#06b6d4' },
  developer: { fromHex: '#475569', toHex: '#71717a' },
  media: { fromHex: '#ef4444', toHex: '#f97316' },
  ai: { fromHex: '#8b5cf6', toHex: '#a855f7' },
  data: { fromHex: '#14b8a6', toHex: '#06b6d4' },
}

/** @deprecated Material shell no longer uses Tailwind gradient utility classes. */
export const DEFAULT_TAPP_BG = 'tapp-icon-shell--fill'

/** 图标样式返回类型（默认 material 方案） */
export interface IconStyle {
  /** Shell fill class (`tapp-icon-shell--fill`) or empty when standalone */
  className: string
  /** CSS vars `--tapp-shell-a/b` for the dual-stop fill */
  style?: CSSProperties
  /**
   * Self-contained app icon — no tinted shell behind it.
   * Accent still describes brand color for non-icon surfaces.
   */
  standalone: boolean
  /**
   * Full-color custom art forced onto a material shell (`iconShell: true`).
   * Badge keeps shell but skips monochrome glyph wash (opacity / white plate).
   */
  insetMedia: boolean
  /** Solid accent (hex) for dock dots, glows, etc. */
  accentColor: string
  /** True when non-standalone — badge applies material inset finish */
  material: boolean
}

/** Minimal fields for shell / accent resolution. */
export interface TappIconStyleSource {
  icon?: string
  iconSvg?: string
  /** Optional: keep material shell under custom full-color icon */
  iconShell?: boolean
  themeColor?: string
  category?: string
  id?: string
  permissions?: readonly TappPermission[] | string[]
}

const SHELL_A = '--tapp-shell-a'
const SHELL_B = '--tapp-shell-b'

function materialShellStyle(fromHex: string, toHex: string): CSSProperties {
  return {
    [SHELL_A]: fromHex,
    [SHELL_B]: toHex,
  } as CSSProperties
}

function mixHex(
  hex: string,
  toward: '#ffffff' | '#000000',
  amount: number,
): string {
  const raw = hex.trim().replace('#', '')
  const full =
    raw.length === 3
      ? raw
          .split('')
          .map((c) => c + c)
          .join('')
      : raw
  if (!/^[0-9a-f]{6}$/i.test(full)) return hex
  const n = Number.parseInt(full, 16)
  const r = (n >> 16) & 0xFF
  const g = (n >> 8) & 0xFF
  const b = n & 0xFF
  const tr = toward === '#ffffff' ? 255 : 0
  const tg = toward === '#ffffff' ? 255 : 0
  const tb = toward === '#ffffff' ? 255 : 0
  const t = Math.min(1, Math.max(0, amount))
  const mr = Math.round(r + (tr - r) * t)
  const mg = Math.round(g + (tg - g) * t)
  const mb = Math.round(b + (tb - b) * t)
  return `#${((1 << 24) | (mr << 16) | (mg << 8) | mb).toString(16).slice(1)}`
}

function shellStopsFromTheme(themeColor: string): { from: string; to: string } {
  const base = themeColor.trim()
  // Subtle dual-stop: light white up top, light black down bottom
  return {
    from: mixHex(base, '#ffffff', 0.07),
    to: mixHex(base, '#000000', 0.1),
  }
}

/** Resolve shell gradient stops (theme → category → id/permissions → default). */
function resolveShellStops(source: TappIconStyleSource): {
  from: string
  to: string
  accent: string
} {
  if (source.themeColor?.trim()) {
    const base = source.themeColor.trim()
    const stops = shellStopsFromTheme(base)
    return { from: stops.from, to: stops.to, accent: base }
  }

  if (source.category) {
    const normalized = normalizeTappCategory(source.category)
    const colors = CATEGORY_COLORS[normalized]
    if (colors) {
      return {
        from: colors.fromHex,
        to: colors.toHex,
        accent: colors.fromHex,
      }
    }
  }

  const id = source.id || ''
  const idParts = id.split('.')
  const lastPart = idParts[idParts.length - 1]?.toLowerCase() || ''
  const idLower = id.toLowerCase()
  for (const [category, colors] of Object.entries(CATEGORY_COLORS)) {
    if (lastPart.includes(category) || idLower.includes(category)) {
      return {
        from: colors.fromHex,
        to: colors.toHex,
        accent: colors.fromHex,
      }
    }
  }

  const permissions = source.permissions || []
  if (
    permissions.includes('ai:generate') ||
    permissions.includes('ai:chat') ||
    permissions.includes('ai:image')
  ) {
    const c = CATEGORY_COLORS.ai
    return { from: c.fromHex, to: c.toHex, accent: c.fromHex }
  }
  if (
    permissions.includes('media:control') ||
    permissions.includes('media:read')
  ) {
    const c = CATEGORY_COLORS.media
    return { from: c.fromHex, to: c.toHex, accent: c.fromHex }
  }
  if (permissions.includes('platform:register')) {
    const c = CATEGORY_COLORS.data
    return { from: c.fromHex, to: c.toHex, accent: c.fromHex }
  }
  if (permissions.includes('widget:register')) {
    const c = CATEGORY_COLORS.utility
    return { from: c.fromHex, to: c.toHex, accent: c.fromHex }
  }

  return {
    from: DEFAULT_TAPP_ACCENT_HEX,
    to: mixHex(DEFAULT_TAPP_ACCENT_HEX, '#000000', 0.2),
    accent: DEFAULT_TAPP_ACCENT_HEX,
  }
}

/**
 * Default icon presentation: material multi-stop shell + existing glyph art.
 * `standalone` → full-bleed media, no shell.
 * `iconShell: true` → keep shell even for full-color custom icons.
 */
export function getTappIconStyle(source: TappIconStyleSource): IconStyle {
  const standalone = hasStandaloneTappIcon(source)
  const stops = resolveShellStops(source)
  // Full-color media sitting on a forced shell (not monochrome glyph treatment)
  const insetMedia =
    !standalone &&
    source.iconShell === true &&
    isTappIconFullColorMedia(source)

  if (standalone) {
    return {
      className: '',
      standalone: true,
      insetMedia: false,
      accentColor: stops.accent,
      material: false,
    }
  }

  return {
    className: 'tapp-icon-shell--fill',
    style: materialShellStyle(stops.from, stops.to),
    standalone: false,
    insetMedia,
    accentColor: stops.accent,
    material: true,
  }
}

/**
 * @deprecated Use getTappIconStyle().className — material fill class only
 * (pair with style CSS vars from getTappIconStyle).
 */
export function getTappIconGradient(source: TappIconStyleSource): string {
  return getTappIconStyle(source).className || DEFAULT_TAPP_BG
}

/**
 * @deprecated Use getTappIconStyle({ category }).className
 */
export function getCategoryGradient(category: string | undefined): string {
  return getTappIconStyle({ category }).className || DEFAULT_TAPP_BG
}

/** 默认强调色（DOM 可用 CSS 变量） */
export const DEFAULT_TAPP_ACCENT =
  'var(--bg-accent, var(--color-primary, #6366f1))'

/** SVG data-URI / canvas 等无法解析 CSS 变量时的实色回退 */
export const DEFAULT_TAPP_ACCENT_HEX = '#6366f1'

/**
 * 实心强调色：Dock 指示点 / 装饰光晕。
 * 始终返回可绘制颜色（themeColor 或分类 hex）。
 */
export function getTappIconAccentColor(source: TappIconStyleSource): string {
  return resolveShellStops(source).accent
}
