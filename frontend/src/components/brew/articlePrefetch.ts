import { getItem } from '../../services/brewApi'

const MAX_PREFETCH = 1

type PrefetchLoader = (id: number, signal?: AbortSignal) => Promise<unknown>

let generation = 0
let controller = new AbortController()
let loadDetail: PrefetchLoader = loadItem

function loadItem(id: number, signal?: AbortSignal): Promise<unknown> {
  return getItem(id, undefined, signal ? { signal } : undefined)
}

function bumpPrefetch(): AbortSignal {
  controller.abort()
  controller = new AbortController()
  generation += 1
  return controller.signal
}

export function selectPrefetchIds(
  ids: Iterable<number>,
  max = MAX_PREFETCH,
): number[] {
  const selected: number[] = []
  const seen = new Set<number>()
  for (const id of ids) {
    if (!Number.isInteger(id) || id <= 0 || seen.has(id)) continue
    seen.add(id)
    selected.push(id)
    if (selected.length >= max) break
  }
  return selected
}

export function prefetchArticleDetails(ids: Iterable<number>): number {
  const signal = bumpPrefetch()
  for (const id of selectPrefetchIds(ids)) {
    void Promise.try(() => loadDetail(id, signal)).catch(() => undefined)
  }
  return generation
}

export function cancelArticlePrefetch(): void {
  bumpPrefetch()
}

export function articlePrefetchGeneration(): number {
  return generation
}

/** Tests only. */
export function setArticlePrefetchLoader(
  loader: PrefetchLoader | null,
): void {
  loadDetail = loader ?? loadItem
  bumpPrefetch()
}
