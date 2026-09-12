/** Subscribe to active, not cursor (avoids 60fps reconcile). */

import { useSyncExternalStore } from 'react'

export interface WidgetDragCursor {
  x: number
  y: number
}

let cursor: WidgetDragCursor | null = null
let active = false
const cursorListeners = new Set<() => void>()
const activeListeners = new Set<() => void>()

function notify(listeners: Set<() => void>): void {
  for (const listen of listeners) listen()
}

export function getWidgetDragCursor(): WidgetDragCursor | null {
  return cursor
}

export function getWidgetDragActive(): boolean {
  return active
}

export function setWidgetDragCursor(next: WidgetDragCursor | null): void {
  if (
    cursor === next ||
    (next && cursor?.x === next.x && cursor.y === next.y)
  ) {
    return
  }
  cursor = next
  notify(cursorListeners)
  const nextActive = next !== null
  if (active === nextActive) return
  active = nextActive
  notify(activeListeners)
}

export function subscribeWidgetDragCursor(
  onStoreChange: () => void,
): () => void {
  cursorListeners.add(onStoreChange)
  return () => {
    cursorListeners.delete(onStoreChange)
  }
}

export function subscribeWidgetDragActive(
  onStoreChange: () => void,
): () => void {
  activeListeners.add(onStoreChange)
  return () => {
    activeListeners.delete(onStoreChange)
  }
}

export function useWidgetDragCursor(): WidgetDragCursor | null {
  return useSyncExternalStore(
    subscribeWidgetDragCursor,
    getWidgetDragCursor,
    () => null,
  )
}

export function useWidgetDragActive(): boolean {
  return useSyncExternalStore(
    subscribeWidgetDragActive,
    getWidgetDragActive,
    () => false,
  )
}
