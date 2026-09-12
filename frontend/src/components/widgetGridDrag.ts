import type { WidgetConfig, WidgetSize, WidgetType } from './widgetGridTypes'
import type { WidgetDragSession, WidgetDragUi } from './widgetPlacementPreview'
import {
  freeLayoutFitsCellBudget,
  homeWidgetCellCount,
  homeWidgetsOccupiedCells,
  isHomeStickerItem,
} from '../utils/homeLayout'
import { widgetHostConfig } from './widgetLibraryModel'
import {
  resolveDragGhostWidget,
  widgetPlacementCollides,
} from './widgetPlacementPreview'

export type { WidgetDragUi }

export function idleWidgetDrag(): WidgetDragUi {
  return {
    dragged: null,
    settling: false,
    previewUncovered: false,
    previewExiting: false,
    hoveredCell: null,
  }
}

export function beginExistingWidgetDrag(
  widgetId: string,
  hoveredCell: { x: number; y: number } | null,
): WidgetDragUi {
  return {
    dragged: { type: 'existing', widgetId },
    settling: false,
    previewUncovered: false,
    previewExiting: false,
    hoveredCell,
  }
}

export function beginLibraryWidgetDrag(
  widgetTypeId: string,
  hoveredCell: { x: number; y: number } | null,
): WidgetDragUi {
  return {
    dragged: { type: 'new', widgetTypeId },
    settling: false,
    previewUncovered: false,
    previewExiting: false,
    hoveredCell,
  }
}

export function settleWidgetDrag(
  current: WidgetDragSession,
  pendingId: string,
  pendingCell: { x: number; y: number },
): WidgetDragUi {
  return {
    dragged: { ...current, pendingId, pendingCell },
    settling: true,
    previewUncovered: false,
    previewExiting: false,
    hoveredCell: null,
  }
}

export function libraryDragFitsBudget(
  isFreeLayout: boolean,
  widgets: WidgetConfig[],
  defaultSize: WidgetSize,
): boolean {
  if (!isFreeLayout) return true
  return freeLayoutFitsCellBudget(
    homeWidgetsOccupiedCells(widgets),
    homeWidgetCellCount(defaultSize),
  )
}

export function shouldClearDragOnEditExit(
  wasEditMode: boolean,
  isEditMode: boolean,
): boolean {
  return wasEditMode && !isEditMode
}

export function createLibraryWidget(input: {
  widgetType: WidgetType
  position: { x: number; y: number }
  id: string
}): WidgetConfig {
  const settingsConfig =
    input.widgetType.settings && input.widgetType.settings.length > 0
      ? Object.fromEntries(
          input.widgetType.settings
            .filter((setting) => setting.defaultValue !== undefined)
            .map((setting) => [setting.key, setting.defaultValue]),
        )
      : undefined
  return {
    id: input.id,
    type: input.widgetType.id,
    size: input.widgetType.defaultSize,
    position: input.position,
    config: widgetHostConfig(input.widgetType.id) ?? settingsConfig,
  }
}

export type WidgetDropDecision =
  | { type: 'idle' }
  | { type: 'settle'; widgets: WidgetConfig[]; pendingId: string }

export function resolveExistingWidgetDrop(input: {
  widget: WidgetConfig
  widgets: WidgetConfig[]
  cell: { x: number; y: number }
  gridWidth: number
  gridHeight: number
}): WidgetDropDecision {
  const moved = { ...input.widget, position: input.cell }
  if (
    widgetPlacementCollides(
      moved,
      input.widgets,
      input.gridWidth,
      input.gridHeight,
      input.widget.id,
    )
  ) {
    return { type: 'idle' }
  }
  return {
    type: 'settle',
    widgets: input.widgets.map((item) =>
      item.id === input.widget.id ? moved : item,
    ),
    pendingId: input.widget.id,
  }
}

export function resolveLibraryWidgetDrop(input: {
  widgetType: WidgetType
  widgets: WidgetConfig[]
  cell: { x: number; y: number }
  gridWidth: number
  gridHeight: number
  isFreeLayout: boolean
  id: string
}): WidgetDropDecision {
  if (
    !libraryDragFitsBudget(
      input.isFreeLayout,
      input.widgets,
      input.widgetType.defaultSize,
    )
  ) {
    return { type: 'idle' }
  }
  const created = createLibraryWidget({
    widgetType: input.widgetType,
    position: input.cell,
    id: input.id,
  })
  if (
    widgetPlacementCollides(
      created,
      input.widgets,
      input.gridWidth,
      input.gridHeight,
    )
  ) {
    return { type: 'idle' }
  }
  return {
    type: 'settle',
    widgets: [...input.widgets, created],
    pendingId: created.id,
  }
}

export function buildWidgetDragPreview(input: {
  dragged: WidgetDragSession | null
  hoveredCell: { x: number; y: number } | null
  widgets: WidgetConfig[]
  widgetTypeById: Map<string, WidgetType>
  gridWidth: number
  gridHeight: number
}): {
  position: { x: number; y: number } | null
  settleCell: { x: number; y: number } | null
  size: { w: number; h: number }
  hasCollision: boolean
  fromLibrary: boolean
  padded: boolean
  widgetType?: WidgetType
  widgetConfig?: WidgetConfig
} | null {
  if (!input.dragged) return null
  const ghost = resolveDragGhostWidget({
    dragged: input.dragged,
    widgets: input.widgets,
    widgetTypeById: input.widgetTypeById,
  })
  if (!ghost) return null
  const hasCollision = input.hoveredCell
    ? widgetPlacementCollides(
        {
          id: 'preview',
          type: ghost.widgetConfig?.type || '',
          size: ghost.widgetConfig?.size || '1x1',
          position: input.hoveredCell,
        },
        input.widgets,
        input.gridWidth,
        input.gridHeight,
        input.dragged.type === 'existing' ? input.dragged.widgetId : undefined,
      )
    : false
  return {
    position: input.hoveredCell,
    settleCell: input.dragged.pendingCell ?? input.hoveredCell ?? null,
    size: ghost.size,
    hasCollision,
    fromLibrary: ghost.fromLibrary,
    padded: ghost.widgetConfig ? !isHomeStickerItem(ghost.widgetConfig) : true,
    widgetType: ghost.widgetType,
    widgetConfig: ghost.widgetConfig,
  }
}
