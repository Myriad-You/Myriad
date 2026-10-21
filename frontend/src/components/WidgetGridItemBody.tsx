import type { RefObject } from 'react'
import type { WidgetConfig, WidgetType } from './widgetGridTypes'
import React, { Suspense, useCallback, useEffect, useLayoutEffect, useRef } from 'react'
import { openWidgetPopup, syncWidgetPopupConfig } from '../lib/widgetPopup'
import { loadWidgetPopupHost } from './widgetPopup/loadWidgetPopupHost'

/** Commits only with its Suspense siblings, so it marks content that is actually on screen. */
function Presented({ onCommit }: { onCommit: () => void }) {
  useLayoutEffect(() => { onCommit() }, [onCommit])
  return null
}

const WidgetGridItemContent = React.memo(
  ({
    widget,
    widgetType,
    isEditMode,
    isPreview,
    onConfigChange,
    openPopup,
    onPresentable,
  }: {
    widget: WidgetConfig
    widgetType: WidgetType
    isEditMode: boolean
    isPreview?: boolean
    onConfigChange?: (newConfig: any) => void
    openPopup?: (params?: unknown) => void
    onPresentable: () => void
  }) => {
    const WidgetComponent = widgetType.component
    return (
      <Suspense fallback={null}>
        <WidgetComponent
          config={widget}
          isEditMode={isEditMode}
          isPreview={isPreview}
          onConfigChange={onConfigChange}
          openPopup={openPopup}
        />
        <Presented onCommit={onPresentable} />
      </Suspense>
    )
  },
  (prev, next) =>
    prev.widget.id === next.widget.id &&
    prev.widget.type === next.widget.type &&
    prev.widget.size === next.widget.size &&
    prev.widget.config === next.widget.config &&
    prev.isEditMode === next.isEditMode &&
    prev.isPreview === next.isPreview &&
    prev.widgetType === next.widgetType &&
    prev.onConfigChange === next.onConfigChange &&
    prev.openPopup === next.openPopup,
)

/** 卡片打开自己的弹窗。编辑、预览里不给；宿主不同（网格、正文）共用这一处。 */
/** 宿主和内容代码一起预热；重复调用拿的是已缓存的模块。 */
function warmWidgetPopup(widgetType: WidgetType): void {
  loadWidgetPopupHost().catch(() => {})
  widgetType.popup?.preload?.().catch(() => {})
}

/** 首屏加载完再预热，不和首屏抢连接。 */
const IDLE_WARM_DELAY_MS = 3000

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
        returnFocus: () =>
          anchorRef?.current?.querySelector<HTMLElement>(
            'button, a[href], [tabindex]:not([tabindex="-1"]), [data-focus-return]',
          ) ?? null,
        onConfigChange: current.onConfigChange
          ? (next) => latest.current.onConfigChange?.(next)
          : undefined,
      })
    },
    [anchorRef],
  )
  return canOpen ? open : undefined
}

/** Keep callback ownership current without repainting content on every grid move. */
export function WidgetGridItemBody({
  onPresentable,
  anchorRef,
  ...props
}: Omit<
  React.ComponentProps<typeof WidgetGridItemContent>,
  'onPresentable' | 'openPopup'
> & {
  onPresentable?: () => void
  /** 卡片外壳；弹窗从它的位置展开、收回到它。 */
  anchorRef?: RefObject<HTMLElement | null>
}) {
  const callback = useRef(props.onConfigChange)
  const presented = useRef(onPresentable)
  useLayoutEffect(() => {
    callback.current = props.onConfigChange
    presented.current = onPresentable
  }, [props.onConfigChange, onPresentable])
  const forwardConfigChange = useCallback((value: unknown) => callback.current?.(value), [])
  const forwardPresentable = useCallback(() => presented.current?.(), [])
  const openPopup = useWidgetPopupOpener(
    props.widget,
    props.widgetType,
    anchorRef,
    !props.isEditMode && !props.isPreview,
    props.onConfigChange ? forwardConfigChange : undefined,
  )
  return (
    <WidgetGridItemContent
      {...props}
      onConfigChange={props.onConfigChange ? forwardConfigChange : undefined}
      openPopup={openPopup}
      onPresentable={forwardPresentable}
    />
  )
}
