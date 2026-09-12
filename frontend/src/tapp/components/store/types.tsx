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
  onInstalled?: () => void
  className?: string
  embeddedChrome?: boolean
  compact?: boolean
  fullscreen?: boolean
}

export type AppSourceType = 'local' | 'remote'

export interface UnifiedAppItem {
  id: string
  name: string
  version: string
  description: string
  longDescription?: string
  preview?: StorePreviewDescriptor
  author: { name: string; email?: string; url?: string }
  icon?: string
  iconSvg?: string
  iconShell?: boolean
  themeColor?: string
  category: TappCategory
  tags: string[]
  permissions: string[]
  license?: string
  homepage?: string
  repository?: string
  size?: number
  downloads?: number
  featured?: boolean
  verified?: boolean
  fromOfficialSource?: boolean
  updatedAt?: string
  source: AppSourceType
  localTapp?: ExampleTapp
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

export const DISCOVER_ALL_PREVIEW_LIMIT = 10

export const DISCOVER_LATEST_LIMIT = 2

export const CATALOG_SKELETON_LIST_ROWS = 6

export const CATALOG_SKELETON_DISCOVER_ALL_ROWS = 6

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

export const PERMISSION_LEVEL_ORDER = [
  'privileged',
  'elevated',
  'basic',
] as const
export type StorePermissionLevel = (typeof PERMISSION_LEVEL_ORDER)[number]

export const LEVEL_LABEL_KEYS = {
  basic: 'basicPermission',
  elevated: 'elevatedPermission',
  privileged: 'privilegedPermission',
} as const
