import { useSyncExternalStore } from 'react'
import { brewItemState } from '../../utils/brewItemState'

export function useArticleFlags() {
  useSyncExternalStore(
    brewItemState.subscribe,
    brewItemState.getSnapshot,
    brewItemState.getSnapshot,
  )
  return brewItemState
}
