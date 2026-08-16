/** Shared types and constants for the host Tapp Store UI. */

import type { ReactNode } from 'react'
import type { ExampleTapp } from '../../examples'
import type { RemoteApp } from '../../services/RemoteStoreService'
import type { TappCategory } from '../../types'
import type { StorePreviewDescriptor } from '../../utils/storePreview'
import {
  FaCog,
  FaDatabase,
  FaGamepad,
  FaLink,
  FaMagic,
  FaMusic,
  FaRobot,
  FaWrench,
} from '@lib/icons'

export interface TappStoreProps {
  /** 安装/卸载成功后的回调（列表页可用来刷新） */
  onInstalled?: () => void
  /** 额外 class（填满父容器时常用 h-full） */
  className?: string
  /**
   * 外层已有标题 chrome（页面壳 / 多窗口标题栏）时隐藏内部大标题，
   * 仅保留搜索、筛选与操作，避免移动端双标题占位。
   */
  embeddedChrome?: boolean
  /** 移动端紧凑布局：更小内边距、触控友好控件 */
  compact?: boolean
  /**
   * 商店页全屏模式：侧栏内容整体下移，避让左上角浮动控制条。
   * 仅全屏页壳传入，多窗口宿主不要开。
   */
  fullscreen?: boolean
}

/** 应用来源类型 */
export type AppSourceType = 'local' | 'remote'

/** 统一的应用列表项 */
export interface UnifiedAppItem {
  id: string
  name: string
  version: string
  description: string
  /** 详细描述 */
  longDescription?: string
  /** 当前宿主语言下的商店静态预览（未解析时回退 remoteApp.preview） */
  preview?: StorePreviewDescriptor
  author: { name: string; email?: string; url?: string }
  icon?: string
  /** 内联 SVG 图标代码（优先于 icon） */
  iconSvg?: string
  /** 可选：自定义全彩图标仍套 material 色壳 */
  iconShell?: boolean
  /** 主题色（优先于分类渐变色） */
  themeColor?: string
  category: TappCategory
  tags: string[]
  permissions: string[]
  /** 许可证 */
  license?: string
  /** 主页 URL */
  homepage?: string
  /** 仓库 URL */
  repository?: string
  /** 文件大小（字节） */
  size?: number
  /** 全网安装次数（edge stats overlay；>0 才展示） */
  downloads?: number
  /** 是否推荐 */
  featured?: boolean
  /** 是否验证 */
  verified?: boolean
  /** 来自官方商店源（展示认证圆点） */
  fromOfficialSource?: boolean
  /** 更新时间 */
  updatedAt?: string
  source: AppSourceType
  /** 本地示例 Tapp 数据 */
  localTapp?: ExampleTapp
  /** 远程应用数据 */
  remoteApp?: RemoteApp & {
    sourceUrl: string
    sourceName: string
    sourceBaseUrl: string
    sourceOfficial?: boolean
  }
}

export type StoreSelection = TappCategory | '__installed__' | null
export type InstalledSortOrder = 'category' | 'date'
export type CategorySortOrder = 'name' | 'date' | 'downloads'

/** Discover home “全部” preview rows before “查看全部”. */
export const DISCOVER_ALL_PREVIEW_LIMIT = 10

/** Discover home “最新” section size. */
export const DISCOVER_LATEST_LIMIT = 2

export interface InstalledTappInfo {
  userRole: string
  isTemporary?: boolean
  version: string
  installedAt: string
}

export const CATEGORY_ICONS: Record<TappCategory, ReactNode> = {
  ai: <FaRobot />,
  data: <FaDatabase />,
  developer: <FaWrench />,
  game: <FaGamepad />,
  media: <FaMusic />,
  productivity: <FaMagic />,
  social: <FaLink />,
  utility: <FaCog />,
}

/** 权限级别顺序（高等级优先，与应用详情页一致） */
export const PERMISSION_LEVEL_ORDER = [
  'privileged',
  'elevated',
  'basic',
] as const
export type StorePermissionLevel = (typeof PERMISSION_LEVEL_ORDER)[number]

/** 权限级别标签的 i18n 键 */
export const LEVEL_LABEL_KEYS = {
  basic: 'basicPermission',
  elevated: 'elevatedPermission',
  privileged: 'privilegedPermission',
} as const
