/** Subscribe to active, not cursor (avoids 60fps reconcile). */

import { createStore, useStore } from './store'

export interface WidgetDragCursor {
  x: number
  y: number
}

const cursor = createStore<WidgetDragCursor | null>(
  null,
  (a, b) => a === b || (a !== null && b !== null && a.x === b.x && a.y === b.y),
)
const active = createStore(false)

export const getWidgetDragCursor = cursor.get
export const getWidgetDragActive = active.get
export const subscribeWidgetDragCursor = cursor.subscribe
export const subscribeWidgetDragActive = active.subscribe

export function setWidgetDragCursor(next: WidgetDragCursor | null): void {
  cursor.set(next)
  active.set(next !== null)
}

export function useWidgetDragCursor(): WidgetDragCursor | null {
  return useStore(cursor)
}

export function useWidgetDragActive(): boolean {
  return useStore(active)
}
