/**
 * 沙箱类型定义
 */

import type { TappInstance } from '../../types'

/** 通知选项 */
export interface TappNotificationOptions {
  title: string
  message: string
  type?: 'success' | 'info' | 'warning' | 'error'
  duration?: number
}

/** 渲染模式 */
export type SandboxMode = 'page' | 'widget'

/** Widget 渲染属性 */
export interface WidgetRenderProps {
  size: string
  theme: 'light' | 'dark'
  primaryColor: string
  locale: string
  config?: Record<string, unknown>
  isEditMode?: boolean
  isPreview?: boolean
  scale?: number
  fontScale?: number
}

/** 安全区域内边距 */
export interface SafeInsets {
  top?: number
  right?: number
  bottom?: number
  left?: number
}

/** 动画配置引用类型（与 useAnimationLevel 对齐） */
export interface AnimationConfigRef {
  level: 'exlight' | 'light' | 'standard'
  loop: boolean
  spring: boolean
  durationScale: number
  widgetGlow?: boolean
  widgetUiRotation?: boolean
}

export type { TappInstance }
