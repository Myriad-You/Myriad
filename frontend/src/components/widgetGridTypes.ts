import type { TappSettingItem } from '@myriad/tapp-runtime'
import type { ComponentType } from 'react'
import type {
  StickerExtraSizeKey,
  WidgetSizeKey,
} from '../utils/widgetSizeScale'

export type WidgetSize = WidgetSizeKey | StickerExtraSizeKey

export type HomeLayoutItemKind = 'widget' | 'sticker'

export interface WidgetConfig {
  id: string
  type: string
  size: WidgetSize
  position: { x: number; y: number }
  config?: any
  kind?: HomeLayoutItemKind
}

export interface WidgetDragStart {
  point: { x: number; y: number }
  grab: { x: number; y: number }
}

export interface WidgetComponentProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
  onConfigChange?: (newConfig: any) => void
  /** 只在声明了 popup、且不是编辑/预览时提供。 */
  openPopup?: (params?: unknown) => void
}

/** card / md / lg 桌面居中、手机底部面板（card 是竖向窄名片）；stage 桌面大幅、手机全屏。 */
export type WidgetPopupSize = 'card' | 'md' | 'lg' | 'stage'

export interface WidgetPopupProps {
  config: WidgetConfig
  /** 最近一次 openPopup 带来的参数。 */
  params: unknown
  /** false 表示已收回到后面：停掉计时、动画和播放。 */
  active: boolean
  close: () => void
  onConfigChange?: (newConfig: any) => void
}

export interface WidgetPopupDef {
  component: ComponentType<WidgetPopupProps>
  size?: WidgetPopupSize
  /** 宿主角落的关闭按钮。内容自带关闭控件时可以不要；TAPP 弹窗始终有。 */
  chrome?: 'close' | 'none'
  /** 预先拉内容代码。卡片被指向或页面空闲时调用，点开时就不用等。 */
  preload?: () => Promise<unknown>
  /**
   * 几张卡共用一份弹窗。弹窗本身不属于某张卡（比如舞台能切到任何平台）时给，
   * 免得每张卡各开一份、重复取数还多占收回的名额。参数照常由打开它的卡带进来。
   */
  shareKey?: string
}

export interface WidgetType {
  id: string
  name: string
  defaultSize: WidgetSize
  component: ComponentType<WidgetComponentProps>
  /** Resolves once the component can show real content; the grid holds its entrance until then. */
  preload?: () => Promise<unknown>
  supportedSizes?: WidgetSize[]
  settings?: TappSettingItem[]
  /** The component owns its long-press settings UI; the grid only arbitrates drag. */
  componentLongPress?: boolean
  isTappWidget?: boolean
  tappId?: string
  category?: string
  description?: string
  popup?: WidgetPopupDef
}

export interface WidgetGridHandle {
  startNewWidgetDrag: (
    widgetTypeId: string,
    point: { x: number; y: number },
    grab?: { x: number; y: number },
  ) => void
}
