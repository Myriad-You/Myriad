export type TappStorageOperation = 'set' | 'remove' | 'clear'

export interface TappStorageChange {
  tappId: string
  key?: string
  operation: TappStorageOperation
  source: object
}

export const HOST_SETTINGS_WRITE_SOURCE = { kind: 'host-settings-write' } as const

export function isForeignTappKvChange<T extends object>(
  change: TappStorageChange,
  tappId: string,
  source: T | null | undefined,
): source is T {
  return (
    source != null && change.tappId === tappId && change.source !== source
  )
}

function createKvBus() {
  const target = new EventTarget()
  const EVENT = 'change'
  return {
    emit(change: TappStorageChange) {
      target.dispatchEvent(new CustomEvent(EVENT, { detail: change }))
    },
    on(listener: (change: TappStorageChange) => void): () => void {
      const handler = (event: Event) =>
        listener((event as CustomEvent<TappStorageChange>).detail)
      target.addEventListener(EVENT, handler)
      return () => target.removeEventListener(EVENT, handler)
    },
  }
}

const storageBus = createKvBus()
const sharedBus = createKvBus()
const privateBus = createKvBus()
const settingsBus = createKvBus()

export function emitTappStorageChange(change: TappStorageChange): void {
  storageBus.emit(change)
}

export function onTappStorageChange(
  listener: (change: TappStorageChange) => void,
): () => void {
  return storageBus.on(listener)
}

export function emitTappSharedChange(change: TappStorageChange): void {
  sharedBus.emit(change)
}

export function onTappSharedChange(
  listener: (change: TappStorageChange) => void,
): () => void {
  return sharedBus.on(listener)
}

export function emitTappPrivateChange(change: TappStorageChange): void {
  privateBus.emit(change)
}

export function onTappPrivateChange(
  listener: (change: TappStorageChange) => void,
): () => void {
  return privateBus.on(listener)
}

export function emitTappSettingsChange(change: TappStorageChange): void {
  settingsBus.emit(change)
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
  return settingsBus.on(listener)
}

/** 同 Tapp 的外沙箱 KV 变更：转发 onChanged，可选 remount。 */
export function bindTappKvChange(
  subscribe: (listener: (change: TappStorageChange) => void) => () => void,
  getBridge: () => { emit: (action: string, payload: unknown) => void } | null,
  tappId: string,
  action: string,
  after?: () => void,
): () => void {
  return subscribe((change) => {
    const bridge = getBridge()
    if (!isForeignTappKvChange(change, tappId, bridge)) return
    bridge.emit(action, {
      key: change.key,
      operation: change.operation,
    })
    after?.()
  })
}

interface TappKvBridge {
  subscribe: (listener: (change: TappStorageChange) => void) => () => void
  action: string
  remount?: string
}

/** storage / shared 仍兼容 remount；settings / private 只广播。 */
export const TAPP_KV_BRIDGES: readonly TappKvBridge[] = [
  {
    subscribe: onTappStorageChange,
    action: 'storageChanged',
    remount: 'storage-changed',
  },
  {
    subscribe: onTappSharedChange,
    action: 'sharedChanged',
    remount: 'shared-changed',
  },
  { subscribe: onTappSettingsChange, action: 'settingsChanged' },
  { subscribe: onTappPrivateChange, action: 'privateChanged' },
]

export function bindAllTappKvChanges(
  getBridge: () => { emit: (action: string, payload: unknown) => void } | null,
  tappId: string,
  onRemount?: (reason: string) => void,
): () => void {
  const offs = TAPP_KV_BRIDGES.map((row) =>
    bindTappKvChange(
      row.subscribe,
      getBridge,
      tappId,
      row.action,
      row.remount && onRemount ? () => onRemount(row.remount as string) : undefined,
    ),
  )
  return () => {
    for (const off of offs) off()
  }
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
