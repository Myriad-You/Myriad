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

/**
 * 小组件绝对定位，DOM 顺序就是 Tab 顺序：按格子的阅读顺序（先上后下、同行从左到右）排。
 * 给了 frozenIds 时沿用这个顺序，不在里面的（新加的）按阅读顺序接在后面——
 * 编辑时拖来拖去不重排，免得移动 DOM 让 tapp 的 iframe 重新加载。
 */
export function widgetsInReadingOrder<
  T extends { id: string; position: { x: number; y: number } },
>(widgets: readonly T[], frozenIds?: readonly string[] | null): T[] {
  const sorted = widgets.toSorted(
    (a, b) => a.position.y - b.position.y || a.position.x - b.position.x,
  )
  if (!frozenIds) return sorted
  const rank = new Map(frozenIds.map((id, i) => [id, i]))
  const kept = sorted
    .filter((w) => rank.has(w.id))
    .toSorted((a, b) => rank.get(a.id)! - rank.get(b.id)!)
  return [...kept, ...sorted.filter((w) => !rank.has(w.id))]
}
