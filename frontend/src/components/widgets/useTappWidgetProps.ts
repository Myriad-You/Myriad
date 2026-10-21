import type { RegisteredWidget } from '../../tapp/types'
import type { WidgetConfig } from '../widgetGridTypes'
import { useCallback, useMemo } from 'react'
import { acceptTappInstanceSettingsPatch } from './tappInstanceSettings'

/**
 * 卡片和弹窗层交给沙箱的同一份 props。主题与主色只在建 iframe 时读一次，
 * 之后的变化由沙箱订阅推送，这里不跟着重算。
 */
export function useTappWidgetProps(
  widget: RegisteredWidget | null,
  config: WidgetConfig,
  locale: string,
  { isEditMode = false, isPreview = false }: { isEditMode?: boolean; isPreview?: boolean } = {},
) {
  const configString = JSON.stringify(config.config || {})
  const declaredDefaults = useMemo(
    () =>
      Object.fromEntries(
        (widget?.config.settings || [])
          .filter((setting) => setting.defaultValue !== undefined)
          .map((setting) => [setting.key, setting.defaultValue]),
      ),
    [widget],
  )
  return useMemo(() => {
    const isDark = document.documentElement.classList.contains('dark')
    const primaryColor =
      getComputedStyle(document.documentElement)
        .getPropertyValue('--color-primary')
        .trim() || '#8b5cf6'
    return {
      size: config.size,
      config: { ...declaredDefaults, ...(config.config || {}) },
      isEditMode,
      isPreview,
      theme: (isDark ? 'dark' : 'light') as 'light' | 'dark',
      primaryColor,
      locale,
    }
    // configString 代替 config.config 的引用比较。
  }, [config.size, configString, declaredDefaults, isEditMode, isPreview, locale])
}

/** 沙箱写实例设置：按声明校验，合进现有配置交给宿主保存。enabled 为 false 时一律拒绝。 */
export function useTappInstanceSettingsPatch(
  widget: RegisteredWidget | null,
  current: unknown,
  onConfigChange: ((next: any) => void) | undefined,
  enabled = true,
): (patch: Record<string, unknown>) => boolean {
  return useCallback(
    (patch: Record<string, unknown>): boolean => {
      if (!enabled || !widget || !onConfigChange) return false
      const accepted = acceptTappInstanceSettingsPatch(widget.config.settings || [], patch)
      if (!accepted) return false
      onConfigChange({ ...((current as Record<string, unknown>) || {}), ...accepted })
      return true
    },
    [current, enabled, onConfigChange, widget],
  )
}
