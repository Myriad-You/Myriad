import type { WidgetConfig } from './widgetGridTypes'
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  isRedoKey,
  isUndoKey,
  pushWidgetHistory,
  redoWidgetHistory,
  undoWidgetHistory,
} from './widgetGridHistory'

export function useWidgetGridHistory(
  isEditMode: boolean,
  onWidgetsChange?: (widgets: WidgetConfig[]) => void,
): {
  saveToHistory: (widgets: WidgetConfig[]) => void
  handleUndo: () => void
  handleRedo: () => void
} {
  const [widgetHistory, setWidgetHistory] = useState<WidgetConfig[][]>([])
  const [historyIndex, setHistoryIndex] = useState(-1)
  const latestRef = useRef({ widgetHistory, historyIndex, onWidgetsChange })
  latestRef.current = { widgetHistory, historyIndex, onWidgetsChange }

  const saveToHistory = useCallback((next: WidgetConfig[]) => {
    const { widgetHistory: history, historyIndex: index } = latestRef.current
    const pushed = pushWidgetHistory(history, index, next)
    setWidgetHistory(pushed.history)
    setHistoryIndex(pushed.index)
  }, [])

  const handleUndo = useCallback(() => {
    const latest = latestRef.current
    const step = undoWidgetHistory(latest.widgetHistory, latest.historyIndex)
    if (!step) return
    setHistoryIndex(step.index)
    latest.onWidgetsChange?.(step.widgets)
  }, [])

  const handleRedo = useCallback(() => {
    const latest = latestRef.current
    const step = redoWidgetHistory(latest.widgetHistory, latest.historyIndex)
    if (!step) return
    setHistoryIndex(step.index)
    latest.onWidgetsChange?.(step.widgets)
  }, [])

  useEffect(() => {
    if (!isEditMode) return
    const handleKeyDown = (event: KeyboardEvent) => {
      if (isUndoKey(event)) {
        event.preventDefault()
        handleUndo()
      }
      if (isRedoKey(event)) {
        event.preventDefault()
        handleRedo()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [handleRedo, handleUndo, isEditMode])

  return { saveToHistory, handleUndo, handleRedo }
}
