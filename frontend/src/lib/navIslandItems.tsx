import type { SVGProps } from 'react'
import type { ModuleVisibilityKey } from '../utils/moduleVisibility'
import contract from '../../../shared/nav_island.json' with { type: 'json' }

/**
 * 导航岛的主条目（`shared/nav_island.json`）。博客页上那一份由后端照同一个文件画，
 * 顺序、地址、模块、图标在这里改一次两边都跟着变。
 */
export type NavIslandItemId = 'main' | 'library' | 'phantasi' | 'tapp'

/** 条目文案在 `nav.*` 里的键；后端的 `backend/i18n/nav.*` 用同样的键。 */
export type NavIslandLabelKey =
  | 'backToHome'
  | 'home'
  | 'library'
  | 'phantasiReading'
  | 'tapp'
  | 'openTapp'

export interface NavIslandIconSpec {
  strokeWidth: number
  transform?: string
  paths: string[]
}

export interface NavIslandItem {
  id: NavIslandItemId
  path: string
  module: ModuleVisibilityKey | null
  label: NavIslandLabelKey
  tip: NavIslandLabelKey
  icon: NavIslandIconSpec
}

export const NAV_ISLAND_ITEMS = contract.items as NavIslandItem[]

export function navIslandItem(id: NavIslandItemId): NavIslandItem {
  return NAV_ISLAND_ITEMS.find((item) => item.id === id)!
}

/** 线条图标，颜色跟着文字走。 */
export function NavIslandIcon({
  icon,
  ...props
}: { icon: NavIslandIconSpec } & SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" {...props}>
      <g
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={icon.strokeWidth}
        transform={icon.transform}
      >
        {icon.paths.map((d) => (
          <path key={d} d={d} />
        ))}
      </g>
    </svg>
  )
}
