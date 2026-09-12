import type { TappInstance } from '../../types'

export interface TappNotificationOptions {
  title: string
  message: string
  type?: 'success' | 'info' | 'warning' | 'error'
  duration?: number
}

export type SandboxMode = 'page' | 'widget'

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

export interface SafeInsets {
  top?: number
  right?: number
  bottom?: number
  left?: number
}

export interface AnimationConfigRef {
  level: 'exlight' | 'light' | 'standard'
  loop: boolean
  spring: boolean
  durationScale: number
  widgetGlow?: boolean
  widgetUiRotation?: boolean
}

export type { TappInstance }
