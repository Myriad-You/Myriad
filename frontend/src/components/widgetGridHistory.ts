import type { WidgetConfig } from './widgetGridTypes'

export const WIDGET_HISTORY_LIMIT = 20

export function pushWidgetHistory(
  history: WidgetConfig[][],
  index: number,
  next: WidgetConfig[],
  limit = WIDGET_HISTORY_LIMIT,
): { history: WidgetConfig[][]; index: number } {
  const trimmed = history.slice(0, index + 1)
  trimmed.push(next)
  if (trimmed.length > limit) {
    trimmed.shift()
    return { history: trimmed, index }
  }
  return { history: trimmed, index: index + 1 }
}

export function undoWidgetHistory(
  history: WidgetConfig[][],
  index: number,
): { index: number; widgets: WidgetConfig[] } | null {
  if (index <= 0) return null
  return { index: index - 1, widgets: history[index - 1] }
}

export function redoWidgetHistory(
  history: WidgetConfig[][],
  index: number,
): { index: number; widgets: WidgetConfig[] } | null {
  if (index >= history.length - 1) return null
  return { index: index + 1, widgets: history[index + 1] }
}

export function isUndoKey(event: {
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  key: string
}): boolean {
  return (event.ctrlKey || event.metaKey) && event.key === 'z' && !event.shiftKey
}

export function isRedoKey(event: {
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  key: string
}): boolean {
  return (
    (event.ctrlKey || event.metaKey) &&
    ((event.shiftKey && event.key === 'z') || event.key === 'y')
  )
}
