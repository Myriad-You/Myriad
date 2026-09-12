/** 不调 brewApi。 */

export interface AgentOpenHint {
  articleId?: string
  articleLink?: string
  openLatest?: boolean
}

export const PENDING_OPEN_KEY = 'brew_pending_open_article'
export const PENDING_READING_KEY = 'brew_pending_reading_list'
export const PENDING_TTL_MS = 10_000

export function itemMatchesAgentHint(
  item: { id: number; link?: string | null; guid?: string | null },
  hint: Pick<AgentOpenHint, 'articleId' | 'articleLink'>,
): boolean {
  if (hint.articleLink && item.link === hint.articleLink) return true
  if (hint.articleId && String(item.id) === hint.articleId) return true
  if (hint.articleId && item.guid === hint.articleId) return true
  return false
}

export function numericArticleId(articleId?: string): number | null {
  if (!articleId) return null
  const n = Number.parseInt(articleId, 10)
  return Number.isNaN(n) ? null : n
}

export function wantsLatestOnly(hint: AgentOpenHint): boolean {
  return Boolean(hint.openLatest) && !hint.articleId && !hint.articleLink
}

/** 最多 5 页 × 20，找到就停，不把整页灌进列表。 */
export const AGENT_SCAN_MAX_PAGES = 5
export const AGENT_SCAN_PER_PAGE = 20

export interface AgentArticle {
  id: number
  title?: string
  link?: string | null
  guid?: string | null
}

export async function findAgentArticle<T extends AgentArticle>(
  hint: Pick<AgentOpenHint, 'articleId' | 'articleLink'>,
  local: readonly T[],
  io: {
    getById: (id: number) => Promise<T | null | undefined>
    getPage: (
      page: number,
      perPage: number,
    ) => Promise<{ items: T[]; total: number }>
  },
): Promise<T | null> {
  const numericId = numericArticleId(hint.articleId)
  if (numericId != null) {
    try {
      console.log('[Brew] Fetching article by ID:', numericId)
      const article = await io.getById(numericId)
      if (article) {
        console.log('[Brew] Got article by ID:', article.title)
        return article
      }
    } catch (err) {
      console.warn('[Brew] Failed to fetch article by ID:', err)
    }
  }

  const localHit = local.find((item) => itemMatchesAgentHint(item, hint))
  if (localHit) {
    console.log('[Brew] Found article in loaded items:', localHit.title)
    return localHit
  }

  console.log('[Brew] Article not found locally, loading from API...')
  for (let page = 1; page <= AGENT_SCAN_MAX_PAGES; page++) {
    const data = await io.getPage(page, AGENT_SCAN_PER_PAGE)
    console.log('[Brew] Loaded page', page, 'items:', data.items.length)

    const hit = data.items.find((item) => itemMatchesAgentHint(item, hint))
    if (hit) {
      console.log('[Brew] Found article from API:', hit.title)
      return hit
    }

    if (data.items.length < AGENT_SCAN_PER_PAGE) break
    if (data.total > 0 && page * AGENT_SCAN_PER_PAGE >= data.total) break
  }

  console.warn('[Brew] Article not found in API response. Looking for:', hint)
  return null
}

export type PendingTake<T> =
  | { ok: true; value: T }
  | { ok: false; reason: 'missing' | 'bad' | 'expired' }

export function takeFreshPending<T extends { timestamp?: number }>(
  raw: string | null,
  now: number,
  ttlMs = PENDING_TTL_MS,
): PendingTake<T> {
  if (raw == null) return { ok: false, reason: 'missing' }
  try {
    const value = JSON.parse(raw) as T
    if (!value.timestamp || now - value.timestamp >= ttlMs) {
      return { ok: false, reason: 'expired' }
    }
    return { ok: true, value }
  } catch {
    return { ok: false, reason: 'bad' }
  }
}
