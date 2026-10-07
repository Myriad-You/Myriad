import type { PhantasiItem } from '../../../types/phantasi'
import { JOURNAL_ROOT, journalItemPath } from './journalRoutes'

interface ReaderSubject {
  key: string
  generation: number
}

const webIdentities = new WeakMap<PhantasiItem, string>()
let nextWebIdentity = 0

/** Search result IDs belong to a result list, not the database namespace. */
export function phantasiItemIdentity(item: PhantasiItem): string {
  if (!item.fromWebSearch) return `db:${item.id}`
  let identity = webIdentities.get(item)
  if (!identity) {
    identity = `web:${Date.now()}:${++nextWebIdentity}:${Math.random()}`
    webIdentities.set(item, identity)
  }
  return identity
}

export function phantasiOpenedItemIdentity(state: unknown): string | undefined {
  const id = phantasiOpenedItemId(state)
  if (id == null) return
  const identity = (state as { phantasiReaderIdentity?: unknown }).phantasiReaderIdentity
  return typeof identity === 'string' && identity.startsWith('web:')
    ? identity
    : `db:${id}`
}

export function phantasiOpenedItemState(
  id: number,
  pushed = true,
  item?: PhantasiItem,
  subject?: ReaderSubject,
) {
  return {
    phantasiOpenedItem: id,
    phantasiReaderPushed: pushed,
    ...(item?.fromWebSearch ? { phantasiReaderIdentity: phantasiItemIdentity(item) } : {}),
    // Search results have no database item to reload on browser Forward.
    ...(item?.fromWebSearch && subject
      ? {
          phantasiWebSearchItem: item,
          phantasiWebSearchSubject: {
            key: subject.key,
            generation: subject.generation,
          },
        }
      : {}),
  }
}

export function phantasiOpenedItemId(
  locationState: unknown,
): number | undefined {
  if (typeof locationState !== 'object' || locationState === null) return
  const id = (locationState as { phantasiOpenedItem?: unknown })
    .phantasiOpenedItem
  return typeof id === 'number' && Number.isSafeInteger(id) ? id : undefined
}

export function phantasiOpenedWebItem(
  locationState: unknown,
  subject: ReaderSubject,
): PhantasiItem | null | undefined {
  const id = phantasiOpenedItemId(locationState)
  if (id == null) return
  const state = locationState as {
    phantasiWebSearchItem?: PhantasiItem
    phantasiWebSearchSubject?: ReaderSubject
  }
  if (!Object.hasOwn(state, 'phantasiWebSearchItem')) return
  const item = state.phantasiWebSearchItem
  const owner = state.phantasiWebSearchSubject
  const identity = phantasiOpenedItemIdentity(locationState)
  const valid = owner?.key === subject.key &&
    owner.generation === subject.generation &&
    item?.id === id &&
    item.fromWebSearch &&
    item.source_id === 0 &&
    identity?.startsWith('web:')
  if (!valid || !item || !identity) return null
  webIdentities.set(item, identity)
  return item
}

export function shouldPopOpenedItem(
  locationState: unknown,
  itemId: number | undefined,
): boolean {
  return (
    itemId != null &&
    phantasiOpenedItemId(locationState) === itemId &&
    (locationState as { phantasiReaderPushed?: boolean })
      .phantasiReaderPushed !== false
  )
}

/** Push when attaching a public item URL; replace only when stripping it. */
export function phantasiItemNavigateMode(
  currentParam: string | undefined,
  nextId: string | undefined,
): 'none' | 'push' | 'replace' {
  if (currentParam === nextId) return 'none'
  if (!nextId) return 'replace'
  return 'push'
}

export function phantasiItemParamId(param: string | undefined): number | null {
  if (!param) return null
  const id = Number(param)
  return Number.isSafeInteger(id) && id > 0 ? id : null
}

/** Failed deep link leaves the item URL only if we are still on that param. */
export function shouldLeaveFailedItemRoute(
  requestedParam: string | undefined,
  currentParam: string | undefined,
  openedId: number | undefined,
): boolean {
  const requested = phantasiItemParamId(requestedParam)
  if (requested == null) return requestedParam != null
  if (currentParam !== requestedParam) return false
  return openedId !== requested
}

export function restoreAfterFailedOpen(
  requestedParam: string | undefined,
  currentParam: string | undefined,
  openedId: number | undefined,
  kept: { id: number; own: boolean } | null,
  listPath = JOURNAL_ROOT,
): { path: string; param: string | undefined } | null {
  if (!shouldLeaveFailedItemRoute(requestedParam, currentParam, openedId)) {
    return null
  }
  if (kept?.own) {
    return { path: journalItemPath(kept.id), param: String(kept.id) }
  }
  return { path: listPath, param: undefined }
}
