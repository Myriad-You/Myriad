export type TappStorageOperation = 'set' | 'remove' | 'clear'

export interface TappStorageChange {
  tappId: string
  key?: string
  operation: TappStorageOperation
  source: object
}

/** Host settings editor persist — not a sandbox bridge, so every live iframe hears it. */
export const HOST_SETTINGS_WRITE_SOURCE = { kind: 'host-settings-write' } as const

/** Writer sandboxes do not echo their own persist. */
export function isForeignTappKvChange<T extends object>(
  change: TappStorageChange,
  tappId: string,
  source: T | null | undefined,
): source is T {
  return (
    source != null && change.tappId === tappId && change.source !== source
  )
}

const storageTarget = new EventTarget()
const STORAGE_EVENT = 'tapp-storage-changed'

export function emitTappStorageChange(change: TappStorageChange): void {
  storageTarget.dispatchEvent(
    new CustomEvent<TappStorageChange>(STORAGE_EVENT, { detail: change }),
  )
}

export function onTappStorageChange(
  listener: (change: TappStorageChange) => void,
): () => void {
  const handler = (event: Event) =>
    listener((event as CustomEvent<TappStorageChange>).detail)
  storageTarget.addEventListener(STORAGE_EVENT, handler)
  return () => storageTarget.removeEventListener(STORAGE_EVENT, handler)
}

const sharedTarget = new EventTarget()
const SHARED_EVENT = 'tapp-shared-changed'

export function emitTappSharedChange(change: TappStorageChange): void {
  sharedTarget.dispatchEvent(
    new CustomEvent<TappStorageChange>(SHARED_EVENT, { detail: change }),
  )
}

export function onTappSharedChange(
  listener: (change: TappStorageChange) => void,
): () => void {
  const handler = (event: Event) =>
    listener((event as CustomEvent<TappStorageChange>).detail)
  sharedTarget.addEventListener(SHARED_EVENT, handler)
  return () => sharedTarget.removeEventListener(SHARED_EVENT, handler)
}

const settingsTarget = new EventTarget()
const SETTINGS_EVENT = 'tapp-settings-changed'

export function emitTappSettingsChange(change: TappStorageChange): void {
  settingsTarget.dispatchEvent(
    new CustomEvent<TappStorageChange>(SETTINGS_EVENT, { detail: change }),
  )
}

export function emitHostSettingsChange(tappId: string, key: string): void {
  emitTappSettingsChange({
    tappId,
    key,
    operation: 'set',
    source: HOST_SETTINGS_WRITE_SOURCE,
  })
}

export function onTappSettingsChange(
  listener: (change: TappStorageChange) => void,
): () => void {
  const handler = (event: Event) =>
    listener((event as CustomEvent<TappStorageChange>).detail)
  settingsTarget.addEventListener(SETTINGS_EVENT, handler)
  return () => settingsTarget.removeEventListener(SETTINGS_EVENT, handler)
}

export interface TappWidgetInvalidate {
  tappId: string
  widgetId: string
  reason: string
  source: object
}

const invalidateTarget = new EventTarget()
const INVALIDATE_EVENT = 'tapp-widget-invalidate'

export function emitTappWidgetInvalidate(change: TappWidgetInvalidate): void {
  invalidateTarget.dispatchEvent(
    new CustomEvent<TappWidgetInvalidate>(INVALIDATE_EVENT, { detail: change }),
  )
}

export function onTappWidgetInvalidate(
  listener: (change: TappWidgetInvalidate) => void,
): () => void {
  const handler = (event: Event) =>
    listener((event as CustomEvent<TappWidgetInvalidate>).detail)
  invalidateTarget.addEventListener(INVALIDATE_EVENT, handler)
  return () => invalidateTarget.removeEventListener(INVALIDATE_EVENT, handler)
}
