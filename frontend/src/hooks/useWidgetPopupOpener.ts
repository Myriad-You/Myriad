import type { RefObject } from 'react'
import type { WidgetConfig, WidgetType } from '../components/widgetGridTypes'
import { useCallback, useEffect, useLayoutEffect, useRef } from 'react'
import { loadWidgetPopupHost } from '../components/widgetPopup/loadWidgetPopupHost'
import { openWidgetPopup, syncWidgetPopupConfig } from '../lib/widgetPopup'

/** 宿主和内容代码一起预热；重复调用拿的是已缓存的模块。 */
function warmWidgetPopup(widgetType: WidgetType): void {
  loadWidgetPopupHost().catch(() => {})
  widgetType.popup?.preload?.().catch(() => {})
}

/** 首屏加载完再预热，不和首屏抢连接。 */
const IDLE_WARM_DELAY_MS = 3000

/** 卡片打开自己的弹窗。编辑、预览里不给；宿主不同（网格、正文）共用这一处。 */
export function useWidgetPopupOpener(
  widget: WidgetConfig,
  widgetType: WidgetType,
  anchorRef: RefObject<HTMLElement | null> | undefined,
  enabled: boolean,
  onConfigChange?: (newConfig: any) => void,
): ((params?: unknown) => void) | undefined {
  const latest = useRef({ widget, widgetType, onConfigChange })
  useLayoutEffect(() => {
    latest.current = { widget, widgetType, onConfigChange }
  }, [widget, widgetType, onConfigChange])
  useEffect(() => {
    if (widgetType.popup) syncWidgetPopupConfig(widget)
  }, [widget, widgetType.popup])
  const canOpen = enabled && Boolean(widgetType.popup)
  // 等点下去再拉代码，第一次打开会空等几百毫秒。指向卡片就开始拉；手机没有悬停，
  // 按下到点击只有几十毫秒，所以页面空闲时也拉一次（两份加起来约 10 KB）。
  useEffect(() => {
    if (!canOpen) return undefined
    const el = anchorRef?.current
    const warm = () => warmWidgetPopup(latest.current.widgetType)
    el?.addEventListener('pointerenter', warm, { once: true })
    el?.addEventListener('focusin', warm, { once: true })
    el?.addEventListener('touchstart', warm, { once: true, passive: true })
    let idle = 0
    const timer = window.setTimeout(() => {
      idle = window.requestIdleCallback?.(warm, { timeout: 4000 }) ?? 0
      if (!window.requestIdleCallback) warm()
    }, IDLE_WARM_DELAY_MS)
    return () => {
      el?.removeEventListener('pointerenter', warm)
      el?.removeEventListener('focusin', warm)
      el?.removeEventListener('touchstart', warm)
      window.clearTimeout(timer)
      if (idle) window.cancelIdleCallback?.(idle)
    }
  }, [anchorRef, canOpen])
  const open = useCallback(
    (params?: unknown) => {
      const current = latest.current
      warmWidgetPopup(current.widgetType)
      openWidgetPopup({
        widget: current.widget,
        widgetType: current.widgetType,
        params,
        anchor: () => anchorRef?.current?.getBoundingClientRect() ?? null,
        onConfigChange: current.onConfigChange
          ? (next) => latest.current.onConfigChange?.(next)
          : undefined,
      })
    },
    [anchorRef],
  )
  return canOpen ? open : undefined
}
