import type { RefObject } from 'react'
import type { WidgetConfig, WidgetType } from './widgetGridTypes'
import React, { Suspense, useCallback, useLayoutEffect, useRef } from 'react'
import { useWidgetPopupOpener } from '../hooks/useWidgetPopupOpener'

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
