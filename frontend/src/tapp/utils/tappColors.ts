/**
 * Tapp 颜色工具
 * 根据 Tapp 类别和配置提供一致的颜色方案
 */

import type { TappManifest } from '../types'

/** 类别颜色配置 */
export const CATEGORY_COLORS: Record<string, { from: string, to: string }> = {
  // 演示
  demo: { from: 'from-blue-500', to: 'to-cyan-500' },
  // 工具
  tool: { from: 'from-emerald-500', to: 'to-teal-500' },
  tools: { from: 'from-emerald-500', to: 'to-teal-500' },
  utility: { from: 'from-emerald-500', to: 'to-teal-500' },
  utilities: { from: 'from-emerald-500', to: 'to-teal-500' },
  // 效率
  productivity: { from: 'from-orange-500', to: 'to-amber-500' },
  // 游戏
  game: { from: 'from-rose-500', to: 'to-pink-500' },
  games: { from: 'from-rose-500', to: 'to-pink-500' },
  // 娱乐
  entertainment: { from: 'from-purple-500', to: 'to-fuchsia-500' },
  // 社交
  social: { from: 'from-sky-500', to: 'to-cyan-500' },
  // 开发
  development: { from: 'from-slate-600', to: 'to-zinc-500' },
  dev: { from: 'from-slate-600', to: 'to-zinc-500' },
  // 媒体
  media: { from: 'from-red-500', to: 'to-orange-500' },
  // AI
  ai: { from: 'from-violet-500', to: 'to-purple-500' },
  // 音乐
  music: { from: 'from-green-500', to: 'to-emerald-500' },
  // 可视化
  visualization: { from: 'from-cyan-500', to: 'to-blue-500' },
  // 数据
  data: { from: 'from-teal-500', to: 'to-cyan-500' },
  // 小组件
  widget: { from: 'from-amber-500', to: 'to-yellow-500' },
  // 平台
  platform: { from: 'from-teal-500', to: 'to-cyan-500' },
  // 测试
  test: { from: 'from-gray-500', to: 'to-slate-500' },
}

/** 默认使用全局壁纸色 */
export const DEFAULT_TAPP_BG = 'bg-[var(--bg-accent,rgb(var(--color-accent,16_185_129)))]'

/** 图标样式返回类型 */
export interface IconStyle {
  className: string
  style?: React.CSSProperties
}

/**
 * 根据 Tapp manifest 获取图标背景样式（支持自定义主题色）
 */
export function getTappIconStyle(manifest: TappManifest): IconStyle {
  // 1. 优先使用 manifest 中的 themeColor
  if (manifest.themeColor) {
    return {
      className: '',
      style: {
        background: `linear-gradient(to bottom right, ${manifest.themeColor}, ${manifest.themeColor}99)`,
      },
    }
  }

  // 2. 使用分类渐变色
  return { className: getTappIconGradient(manifest) }
}

/**
 * 根据 Tapp manifest 获取图标背景渐变色（类名方式，不支持自定义颜色）
 * 注意：自定义 themeColor 需要使用 getTappIconStyle 函数
 */
export function getTappIconGradient(manifest: TappManifest): string {
  // 注意：themeColor 不能通过 Tailwind 动态类名支持，需使用 getTappIconStyle

  // 1. 尝试从 ID 推断类别 (如 com.example.demo-xxx → demo)
  const idParts = manifest.id.split('.')
  const lastPart = idParts[idParts.length - 1]?.toLowerCase() || ''

  // 检查 ID 中是否包含类别关键词
  for (const [category, colors] of Object.entries(CATEGORY_COLORS)) {
    if (lastPart.includes(category) || manifest.id.toLowerCase().includes(category)) {
      return `bg-gradient-to-br ${colors.from} ${colors.to}`
    }
  }

  // 2. 尝试从权限推断类型
  const permissions = manifest.permissions || []
  if (permissions.includes('ai:generate') || permissions.includes('ai:chat') || permissions.includes('ai:image')) {
    return `bg-gradient-to-br ${CATEGORY_COLORS.ai.from} ${CATEGORY_COLORS.ai.to}`
  }
  if (permissions.includes('media:control') || permissions.includes('media:read')) {
    return `bg-gradient-to-br ${CATEGORY_COLORS.media.from} ${CATEGORY_COLORS.media.to}`
  }
  if (permissions.includes('platform:register')) {
    return `bg-gradient-to-br ${CATEGORY_COLORS.platform.from} ${CATEGORY_COLORS.platform.to}`
  }
  if (permissions.includes('widget:register')) {
    return `bg-gradient-to-br ${CATEGORY_COLORS.widget.from} ${CATEGORY_COLORS.widget.to}`
  }

  // 4. 默认使用全局壁纸色
  return DEFAULT_TAPP_BG
}

/**
 * 根据类别名称获取颜色
 */
export function getCategoryGradient(category: string | undefined): string {
  if (!category)
    return DEFAULT_TAPP_BG

  const lowerCategory = category.toLowerCase()
  if (CATEGORY_COLORS[lowerCategory]) {
    const colors = CATEGORY_COLORS[lowerCategory]
    return `bg-gradient-to-br ${colors.from} ${colors.to}`
  }

  return DEFAULT_TAPP_BG
}
