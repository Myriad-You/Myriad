import type { ComponentType } from 'react'
import type { TappSettingItem } from '../tapp/types'
import type {
  StickerExtraSizeKey,
  WidgetSizeKey,
} from '../utils/widgetSizeScale'

export type WidgetSize = WidgetSizeKey | StickerExtraSizeKey

/** Layout tile kind. Absent / `widget` counts toward the free-mode cell budget. */
export type HomeLayoutItemKind = 'widget' | 'sticker'

export interface WidgetConfig {
  id: string
  type: string
  size: WidgetSize
  position: { x: number; y: number }
  config?: any
  kind?: HomeLayoutItemKind
}

export interface WidgetComponentProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
  onConfigChange?: (newConfig: any) => void
}

export interface WidgetType {
  id: string
  name: string
  defaultSize: WidgetSize
  component: ComponentType<WidgetComponentProps>
  supportedSizes?: WidgetSize[]
  settings?: TappSettingItem[]
  /** Catalog: third-party Tapp widget (absent/false = built-in). */
  isTappWidget?: boolean
  tappId?: string
  category?: string
  description?: string
}

export interface WidgetGridHandle {
  startNewWidgetDrag: (
    widgetTypeId: string,
    point: { x: number; y: number },
  ) => void
}
