import type { PhantasiSource } from '../types/phantasi'
import { getSources } from './phantasiApi'

export type FriendLinkFetchSurface = 'widget' | 'overlay' | 'layout'

export function shouldFetchFriendLinks(
  surface: FriendLinkFetchSurface,
): boolean {
  return surface === 'widget'
}

/** Catalog friend-link sources. Only display surfaces may call this. */
export async function fetchFriendLinkSources(
  surface: FriendLinkFetchSurface,
  signal?: AbortSignal,
): Promise<PhantasiSource[]> {
  if (!shouldFetchFriendLinks(surface)) return []
  return getSources(undefined, {
    view: 'catalog',
    category: 'friends',
    signal,
  })
}
