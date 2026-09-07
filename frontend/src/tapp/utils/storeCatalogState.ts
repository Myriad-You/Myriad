/**
 * Store catalog loading / merchandising predicates.
 * First paint must not invent a catalog from local examples or featured padding.
 */

/** Remote catalog has not arrived; do not fill the list with local examples. */
export function isStoreCatalogPending(
  loading: boolean,
  remoteEmpty: boolean,
  selectedCategory: string | null,
): boolean {
  if (!loading || !remoteEmpty) return false
  // Installed tab is runtime-backed, not the remote catalog.
  return selectedCategory !== '__installed__'
}

/**
 * Editorial featured only. No “first N apps” padding when the source
 * has not marked anything featured.
 */
export function selectFeaturedStoreApps<T extends { featured?: boolean }>(
  apps: readonly T[],
  isDiscoverView: boolean,
  limit = 2,
): T[] {
  if (!isDiscoverView || limit <= 0) return []
  return apps.filter((app) => app.featured === true).slice(0, limit)
}
