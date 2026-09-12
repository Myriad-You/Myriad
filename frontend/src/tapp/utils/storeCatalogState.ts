/** 远程目录未到；不要用本地示例填列表。 */
export function isStoreCatalogPending(
  loading: boolean,
  remoteEmpty: boolean,
  selectedCategory: string | null,
): boolean {
  if (!loading || !remoteEmpty) return false
  return selectedCategory !== '__installed__'
}

export function selectFeaturedStoreApps<T extends { featured?: boolean }>(
  apps: readonly T[],
  isDiscoverView: boolean,
  limit = 2,
): T[] {
  if (!isDiscoverView || limit <= 0) return []
  return apps.filter((app) => app.featured === true).slice(0, limit)
}
