import type { HomeLayoutMode } from '../utils/homeLayout'
import type { WidgetConfig } from './widgetGridTypes'
import {
  HOME_FREE_ROWS,
  HOME_STANDARD_COLS,
  HOME_STANDARD_ROWS,
  packWidgetsIntoColumns,
} from '../utils/homeLayout'
import { widgetSizeSpan } from '../utils/widgetSizeScale'

export function widgetContentMaxRow(widgets: WidgetConfig[]): number {
  let maxY = 0
  for (const widget of widgets) {
    const dim = widgetSizeSpan(widget.size)
    maxY = Math.max(maxY, widget.position.y + dim.h)
  }
  return maxY
}

export function resolveHomeGridMetrics(input: {
  widgets: WidgetConfig[]
  layoutMode: HomeLayoutMode
  customGridColumns?: number
  customGridRows?: number
  autoHeight?: boolean
  gridColumns: number
}): {
  isCompact: boolean
  currentWidgets: WidgetConfig[]
  currentGridWidth: number
  currentGridHeight: number
} {
  const isFreeLayout = input.layoutMode === 'free'
  const isCompact =
    !isFreeLayout &&
    !input.customGridColumns &&
    input.gridColumns < HOME_STANDARD_COLS
  const packed =
    isCompact ? packWidgetsIntoColumns(input.widgets, input.gridColumns) : null
  const currentWidgets = packed ? packed.widgets : input.widgets
  const currentGridWidth = isFreeLayout ? HOME_STANDARD_COLS : input.gridColumns
  const currentGridHeight = isFreeLayout
    ? HOME_FREE_ROWS
    : packed
      ? packed.height
      : input.autoHeight
        ? Math.max(input.customGridRows || 0, widgetContentMaxRow(input.widgets))
        : input.customGridRows || HOME_STANDARD_ROWS
  return {
    isCompact,
    currentWidgets,
    currentGridWidth,
    currentGridHeight,
  }
}

export function homeGridPixelHeight(
  containerWidth: number,
  currentGridWidth: number,
  currentGridHeight: number,
  isFreeLayout: boolean,
): number | undefined {
  if (isFreeLayout || containerWidth <= 0) return undefined
  return (containerWidth * currentGridHeight) / currentGridWidth
}
