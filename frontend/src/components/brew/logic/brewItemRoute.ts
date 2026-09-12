import { brewOwnItemPath } from '../constants'

export const BREW_OPENED_ITEM = 'brewOpenedItem'

export function brewOpenedItemState(id: number): { brewOpenedItem: number } {
  return { brewOpenedItem: id }
}

export function shouldPopOpenedItem(
  locationState: unknown,
  itemId: number | undefined,
): boolean {
  if (
    !itemId ||
    typeof locationState !== 'object' ||
    locationState === null ||
    !Object.hasOwn(locationState, 'brewOpenedItem')
  ) {
    return false
  }
  return (
    (locationState as { brewOpenedItem: unknown }).brewOpenedItem === itemId
  )
}

/** Push when attaching a public item URL; replace only when stripping it. */
export function brewItemNavigateMode(
  currentParam: string | undefined,
  nextId: string | undefined,
): 'none' | 'push' | 'replace' {
  if (currentParam === nextId) return 'none'
  if (!nextId) return 'replace'
  return 'push'
}

export function brewItemParamId(param: string | undefined): number | null {
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
  const requested = brewItemParamId(requestedParam)
  if (requested == null) return requestedParam != null
  if (currentParam !== requestedParam) return false
  return openedId !== requested
}

export function restoreAfterFailedOpen(
  requestedParam: string | undefined,
  currentParam: string | undefined,
  openedId: number | undefined,
  kept: { id: number; own: boolean } | null,
): { path: string; param: string | undefined } | null {
  if (!shouldLeaveFailedItemRoute(requestedParam, currentParam, openedId)) {
    return null
  }
  if (kept?.own) {
    return { path: brewOwnItemPath(kept.id), param: String(kept.id) }
  }
  return { path: '/brew', param: undefined }
}
