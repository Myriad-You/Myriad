import type {
  AddRsshubInstanceRequest,
  AddSourceRequest,
  ApplySourceApplicationInput,
  CommentItem,
  CreateCategoryRequest,
  PhantasiCategoriesResponse,
  PhantasiCategory,
  PhantasiItem,
  PhantasiItemsQuery,
  PhantasiItemsResponse,
  PhantasiNoteAuthor,
  PhantasiNoteDoc,
  PhantasiNoteDocInput,
  PhantasiNoteInput,
  PhantasiSource,
  PhantasiSourceApplication,
  PhantasiSourcesResponse,
  PhantasiStats,
  PhantasiStatsResponse,
  RsshubInstance,
  UpdateCategoryRequest,
  UpdateRsshubInstanceRequest,
  UpdateSourceRequest,
} from '../types/phantasi'
import { parseFeedTopicCards } from '../components/phantasi/logic/feedTopicCards'
import { API_URL } from '../config'
import { KeyedWrites } from '../utils/keyedWrites'
import { phantasiItemState } from '../utils/phantasiItemState'
import { PhantasiRevisionChain } from '../utils/phantasiRevisionChain'
import { phantasiSubject } from '../utils/phantasiSubject'
import { PhantasiSyncConflictError } from '../utils/phantasiSyncConflict'
import { requestCache } from '../utils/requestCache'
import { httpStatusMessage, isUselessErrorText } from '../utils/userFacingError'
import { ApiError, apiService, parseApiErrorBody } from './api'
import {
  invalidatePhantasiBoardCache,
  invalidatePhantasiNoteDocsCache,
  invalidatePhantasiSourcesCache,
  invalidatePhantasiStatsCache,
  phantasiCacheKeys,
} from './phantasiCache'

export type { CommentItem }

const API_BASE = `${API_URL}/api/phantasi`

function phantasiHttpError(status: number, data: unknown): ApiError {
  const parsed = parseApiErrorBody(data, status)
  const message = isUselessErrorText(parsed.message)
    ? httpStatusMessage(status)
    : parsed.message
  return new ApiError(message, status, parsed.code, parsed.details, parsed.hint)
}

const CACHE_TTL = {
  SOURCES: 30 * 1000, // 30s
  CATEGORIES: 60 * 1000, // 1 min
  STATS: 30 * 1000, // 30s
  ITEM: 5 * 60 * 1000, // 5 min
}

/** 429 后最多等这么久再重试一次；更长的窗口就直接报错，别让页面挂着转圈。 */

/** CSRF: retry once. */
/**
 * Phantasi over the shared client (CSRF, session failure, transient and short
 * 429 retries). This layer owns the domain rules: requests die with the
 * Phantasi subject, and every item a read returns is reported to the shared
 * item state.
 */
async function request<T>(
  endpoint: string,
  options: RequestInit = {},
): Promise<T> {
  const subject = phantasiSubject.capture()
  const stateRevision = phantasiItemState.getSnapshot()
  const signal = options.signal
    ? AbortSignal.any([options.signal, subject.signal])
    : subject.signal
  const method = options.method?.toUpperCase() || 'GET'
  signal.throwIfAborted()
  phantasiSubject.assert(subject)

  let data: any
  try {
    data = await apiService.request<any>(`/phantasi${endpoint}`, { ...options, signal, timeout: 0 })
  } catch (error) {
    if (!(error instanceof ApiError)) throw error
    throw phantasiHttpError(error.status, error.body)
  }
  phantasiSubject.assert(subject)
  signal.throwIfAborted()

  if (method === 'GET') {
    const items = [
      ...(Array.isArray(data.items) ? data.items : []),
      ...(data.item ? [data.item] : []),
      ...(Array.isArray(data.sources) ? data.sources.flatMap((source: PhantasiSource) => source.recent_items ?? []) : []),
    ].filter(item => typeof item?.id === 'number')
    phantasiItemState.observeMany(items, stateRevision)
  }
  return data
}

/** Tapp sandbox: send Runtime Grant. */
type PhantasiAttributionHeaders = Record<string, string>

/** Tapp attribution skips cache. */
export async function getSources(
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: {
    signal?: AbortSignal
    view?: 'catalog'
    category?: string
    board?: 'feeds' | 'notes' | 'sites'
    forceRefresh?: boolean
  },
): Promise<PhantasiSource[]> {
  options?.signal?.throwIfAborted()
  const catalog = options?.view === 'catalog'
  const category = options?.category?.trim() || undefined
  const board = options?.board
  const params = new URLSearchParams()
  if (catalog) params.set('view', 'catalog')
  if (category) params.set('category', category)
  if (board) params.set('board', board)
  const query = params.toString()
  const path = query ? `/sources?${query}` : '/sources'
  const cacheKey = phantasiCacheKeys.sourceList(
    catalog ? 'catalog' : undefined,
    category,
    board,
  )
  const fetchSources = async (signal?: AbortSignal) => {
    const data = await request<PhantasiSourcesResponse>(path, {
      headers: attributionHeaders,
      signal,
    })
    return data.sources
  }
  if (attributionHeaders) return fetchSources(options?.signal)
  if (options?.forceRefresh) requestCache.delete(cacheKey)
  const sources = await requestCache.fetch(
    cacheKey,
    fetchSources,
    CACHE_TTL.SOURCES,
    false,
    options?.signal,
  )
  options?.signal?.throwIfAborted()
  return sources
}

export function invalidateStatsCache(): void {
  invalidatePhantasiStatsCache()
}

export function invalidateSourcesCache(): void {
  invalidatePhantasiSourcesCache()
}

/** Source/note mutations only; not read/star. */
function invalidateBoardPageCache(): void {
  invalidatePhantasiBoardCache()
}

export async function addSource(
  req: AddSourceRequest,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<PhantasiSource> {
  const data = await request<{
    success: boolean
    source: PhantasiSource
    error?: string
  }>('/sources', {
    method: 'POST',
    body: JSON.stringify(req),
    headers: attributionHeaders,
  })
  invalidateSourcesCache()
  invalidateBoardPageCache()
  return data.source
}

export async function updateSource(
  id: number,
  req: UpdateSourceRequest,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<PhantasiSource> {
  const data = await request<{ success: boolean; source: PhantasiSource }>(
    `/sources/${id}`,
    {
      method: 'PUT',
      body: JSON.stringify(req),
      headers: attributionHeaders,
    },
  )
  invalidateSourcesCache()
  invalidateBoardPageCache()
  return data.source
}

export async function deleteSource(
  id: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<void> {
  await request(`/sources/${id}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
  invalidateSourcesCache()
  invalidateBoardPageCache()
}

export async function refreshSource(
  id: number,
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { skipCache?: boolean },
): Promise<number> {
  const data = await request<{ success: boolean; new_items: number }>(
    `/sources/${id}/refresh`,
    { method: 'POST', headers: attributionHeaders },
  )
  if (!options?.skipCache) {
    invalidateSourcesCache()
    invalidateBoardPageCache()
  }
  return data.new_items
}

export async function refreshSources(
  ids: number[],
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<number> {
  if (ids.length === 0) return 0
  const results = await Promise.allSettled(
    ids.map((id) => refreshSource(id, attributionHeaders, { skipCache: true })),
  )
  invalidateSourcesCache()
  invalidateBoardPageCache()
  const failed = results.find(result => result.status === 'rejected')
  if (failed?.status === 'rejected') throw failed.reason
  return results.reduce((sum, result) => sum + (result.status === 'fulfilled' ? result.value : 0), 0)
}

export async function discoverSource(
  url: string,
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<{
  url: string
  autocompleted: boolean
  title: string
  description: string | null
  site_url: string | null
  icon: string | null
  feed_type: string
  item_count: number
}> {
  const data = await request<{ success: boolean; feed: any }>(
    '/sources/discover',
    {
      method: 'POST',
      body: JSON.stringify({ url }),
      headers: attributionHeaders,
      signal: options?.signal,
    },
  )
  return data.feed
}

export async function importOpml(
  opml: string,
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<{ imported: number; skipped: number }> {
  const data = await request<{
    success: boolean
    imported: number
    skipped: number
  }>('/import-opml', {
    method: 'POST',
    body: JSON.stringify({ opml }),
    headers: attributionHeaders,
    signal: options?.signal,
  })
  invalidateSourcesCache()
  invalidateBoardPageCache()
  invalidateCategoriesCache()
  return { imported: data.imported, skipped: data.skipped }
}

export async function exportOpml(
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<string> {
  const subject = phantasiSubject.capture()
  const response = await fetch(`${API_BASE}/export-opml`, {
    credentials: 'include',
    headers: attributionHeaders,
    signal: options?.signal
      ? AbortSignal.any([options.signal, subject.signal])
      : subject.signal,
  })
  const content = await response.text()
  phantasiSubject.assert(subject)
  if (options?.signal?.aborted) {
    throw new DOMException('Aborted', 'AbortError')
  }
  if (!response.ok) throw phantasiHttpError(response.status, null)
  return content
}

export async function getCategories(
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal; forceRefresh?: boolean },
): Promise<PhantasiCategoriesResponse['categories']> {
  options?.signal?.throwIfAborted()
  const fetchCategories = async (signal?: AbortSignal) => {
    const data = await request<PhantasiCategoriesResponse>('/categories', {
      headers: attributionHeaders,
      signal,
    })
    return data.categories
  }
  if (attributionHeaders) return fetchCategories(options?.signal)
  if (options?.forceRefresh) requestCache.delete(phantasiCacheKeys.categories)
  const categories = await requestCache.fetch(
    phantasiCacheKeys.categories,
    fetchCategories,
    CACHE_TTL.CATEGORIES,
    false,
    options?.signal,
  )
  options?.signal?.throwIfAborted()
  return categories
}

function invalidateCategoriesCache(): void {
  requestCache.delete(phantasiCacheKeys.categories)
}

export async function createCategory(
  req: CreateCategoryRequest,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<PhantasiCategory> {
  const data = await request<{ success: boolean; category: PhantasiCategory }>(
    '/categories',
    {
      method: 'POST',
      body: JSON.stringify(req),
      headers: attributionHeaders,
    },
  )
  invalidateCategoriesCache()
  return data.category
}

export async function updateCategory(
  id: number,
  req: UpdateCategoryRequest,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<PhantasiCategory> {
  const data = await request<{ success: boolean; category: PhantasiCategory }>(
    `/categories/${id}`,
    {
      method: 'PUT',
      body: JSON.stringify(req),
      headers: attributionHeaders,
    },
  )
  invalidateCategoriesCache()
  return data.category
}

export async function listRsshubInstances(
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<RsshubInstance[]> {
  const data = await request<{
    success: boolean
    instances: RsshubInstance[]
  }>('/rsshub/instances', {
    headers: attributionHeaders,
    signal: options?.signal,
  })
  return data.instances || []
}

export async function addRsshubInstance(
  req: AddRsshubInstanceRequest,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<RsshubInstance> {
  const data = await request<{ success: boolean; instance: RsshubInstance }>(
    '/rsshub/instances',
    {
      method: 'POST',
      body: JSON.stringify(req),
      headers: attributionHeaders,
    },
  )
  return data.instance
}

export async function updateRsshubInstance(
  id: number,
  req: UpdateRsshubInstanceRequest,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<RsshubInstance> {
  const data = await request<{ success: boolean; instance: RsshubInstance }>(
    `/rsshub/instances/${id}`,
    {
      method: 'PUT',
      body: JSON.stringify(req),
      headers: attributionHeaders,
    },
  )
  return data.instance
}

export async function deleteCategory(
  id: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<void> {
  await request(`/categories/${id}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
  invalidateCategoriesCache()
}

export function getItemPreviews(
  query: PhantasiItemsQuery = {},
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<PhantasiItemsResponse> {
  const params = new URLSearchParams()
  if (query.source_id) params.set('source_id', String(query.source_id))
  if (query.category) params.set('category', query.category)
  if (query.topic) params.set('topic', query.topic)
  if (query.filter) params.set('filter', query.filter)
  if (query.sort_order) params.set('sort_order', query.sort_order)
  if (query.cursor) params.set('cursor', query.cursor)
  else if (query.page) params.set('page', String(query.page))
  if (query.per_page) params.set('per_page', String(query.per_page))

  const queryString = params.toString()
  const endpoint = queryString ? `/items?${queryString}` : '/items'

  return request<PhantasiItemsResponse>(endpoint, { headers: attributionHeaders, signal: options?.signal })
}

export async function getItem(
  id: number,
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal; forceRefresh?: boolean },
): Promise<PhantasiItem> {
  options?.signal?.throwIfAborted()
  const fetchItem = async (signal?: AbortSignal) => {
    const data = await request<{ success: boolean; item: PhantasiItem }>(
      `/items/${id}`,
      {
        headers: attributionHeaders,
        signal,
      },
    )
    return data.item
  }
  if (attributionHeaders) return fetchItem(options?.signal)
  if (options?.forceRefresh) requestCache.delete(phantasiCacheKeys.item(id))
  const item = await requestCache.fetch(
    phantasiCacheKeys.item(id),
    fetchItem,
    CACHE_TTL.ITEM,
    false,
    options?.signal,
  )
  options?.signal?.throwIfAborted()
  return item
}

export interface SubscriptionTopicCatalog {
  topics: string[]
  cards: string[]
}

export async function listSubscriptionTopicCatalog(
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal; forceRefresh?: boolean },
): Promise<SubscriptionTopicCatalog> {
  options?.signal?.throwIfAborted()
  const fetchCatalog = async (signal?: AbortSignal) => {
    const data = await request<{
      success: boolean
      topics?: unknown
      cards?: unknown
    }>('/topics', {
      headers: attributionHeaders,
      signal,
    })
    return {
      topics: parseFeedTopicCards(data.topics),
      cards: parseFeedTopicCards(data.cards),
    }
  }
  if (attributionHeaders) return fetchCatalog(options?.signal)
  if (options?.forceRefresh) requestCache.delete(phantasiCacheKeys.topicCatalog)
  const catalog = await requestCache.fetch(
    phantasiCacheKeys.topicCatalog,
    fetchCatalog,
    CACHE_TTL.CATEGORIES,
    false,
    options?.signal,
  )
  options?.signal?.throwIfAborted()
  return catalog
}

export async function listSubscriptionTopics(
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<string[]> {
  return (await listSubscriptionTopicCatalog(attributionHeaders)).topics
}

export async function setFeedTopicCards(
  cards: string[],
): Promise<string[]> {
  const data = await request<{ success: boolean; cards?: unknown }>(
    '/topics/cards',
    { method: 'PUT', body: JSON.stringify({ cards }) },
  )
  requestCache.delete(phantasiCacheKeys.topicCatalog)
  return parseFeedTopicCards(data.cards)
}

function rememberItemTopic(id: number, topic: string | null): string | null {
  requestCache.delete(phantasiCacheKeys.item(id))
  requestCache.delete(phantasiCacheKeys.topicCatalog)
  invalidateSourcesCache()
  invalidateBoardPageCache()
  return topic
}

export async function setItemTopic(
  id: number,
  topic: string | null,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<string | null> {
  const data = await request<{ success: boolean; topic: string | null }>(
    `/items/${id}/topic`,
    {
      method: 'PUT',
      body: JSON.stringify({ topic }),
      headers: attributionHeaders,
    },
  )
  return rememberItemTopic(id, data.topic ?? null)
}

export async function suggestItemTopic(
  id: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<string | null> {
  const data = await request<{ success: boolean; topic: string | null }>(
    `/items/${id}/suggest-topic`,
    {
      method: 'POST',
      headers: attributionHeaders,
    },
  )
  return rememberItemTopic(id, data.topic ?? null)
}

export async function getNotesRssSettings(): Promise<{ enabled: boolean }> {
  const data = await request<{ success: boolean; enabled: boolean }>(
    '/notes/rss',
  )
  return { enabled: Boolean(data.enabled) }
}

export async function setNotesRssEnabled(enabled: boolean): Promise<boolean> {
  const data = await request<{ success: boolean; enabled: boolean }>(
    '/notes/rss',
    {
      method: 'PUT',
      body: JSON.stringify({ enabled }),
    },
  )
  return Boolean(data.enabled)
}

export async function createNote(
  req: PhantasiNoteInput,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<{ id: number; link: string }> {
  const data = await request<{ success: boolean; id: number; link: string }>(
    '/notes',
    {
      method: 'POST',
      body: JSON.stringify(req),
      headers: attributionHeaders,
    },
  )
  invalidateSourcesCache()
  invalidateBoardPageCache()
  return { id: data.id, link: data.link }
}

/** Must invalidate this article's cache. */
export async function updateNote(
  id: number,
  req: PhantasiNoteInput,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<{ id: number; link: string }> {
  const data = await request<{ success: boolean; id: number; link: string }>(
    `/notes/${id}`,
    {
      method: 'PUT',
      body: JSON.stringify(req),
      headers: attributionHeaders,
    },
  )
  invalidateItemCache(id)
  invalidateSourcesCache()
  invalidateBoardPageCache()
  return { id: data.id, link: data.link }
}

export async function deleteNote(
  id: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<void> {
  await request(`/notes/${id}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
  invalidateItemCache(id)
  invalidateSourcesCache()
  invalidateBoardPageCache()
}

export async function listNoteDocs(
  signal?: AbortSignal,
): Promise<PhantasiNoteDoc[]> {
  const data = await request<{ success: boolean; docs: PhantasiNoteDoc[] }>(
    '/notes/docs',
    { signal },
  )
  return data.docs
}

export async function createNoteDoc(
  req: PhantasiNoteDocInput = {},
): Promise<PhantasiNoteDoc> {
  const data = await request<{ success: boolean; doc: PhantasiNoteDoc }>(
    '/notes/docs',
    {
      method: 'POST',
      body: JSON.stringify(req),
    },
  )
  invalidatePhantasiNoteDocsCache()
  return data.doc
}

export async function getNoteDoc(
  id: number,
  signal?: AbortSignal,
): Promise<PhantasiNoteDoc> {
  const data = await request<{ success: boolean; doc: PhantasiNoteDoc }>(
    `/notes/docs/${id}`,
    { signal },
  )
  return data.doc
}

export async function getNoteDocForItem(
  itemId: number,
  signal?: AbortSignal,
): Promise<PhantasiNoteDoc> {
  const data = await request<{ success: boolean; doc: PhantasiNoteDoc }>(
    `/notes/docs/for-item/${itemId}`,
    { signal },
  )
  return data.doc
}

export async function listNoteDocAuthors(
  id: number,
  signal?: AbortSignal,
): Promise<PhantasiNoteAuthor[]> {
  const data = await request<{ success: boolean; authors: PhantasiNoteAuthor[] }>(
    `/notes/docs/${id}/authors`,
    { signal },
  )
  return data.authors
}

export async function updateNoteDoc(
  id: number,
  req: PhantasiNoteDocInput,
): Promise<PhantasiNoteDoc> {
  const data = await request<{ success: boolean; doc: PhantasiNoteDoc }>(
    `/notes/docs/${id}`,
    {
      method: 'PUT',
      body: JSON.stringify(req),
    },
  )
  invalidatePhantasiNoteDocsCache()
  return data.doc
}

export async function deleteNoteDoc(id: number): Promise<void> {
  await request(`/notes/docs/${id}`, { method: 'DELETE' })
  invalidatePhantasiNoteDocsCache()
}

/** Change category without publishing the document's in-progress body. */
export async function updateNoteDocTopic(
  id: number,
  req: { topic: string | null; revision: number },
): Promise<PhantasiNoteDoc> {
  const data = await request<{ success: boolean; doc: PhantasiNoteDoc }>(
    `/notes/docs/${id}/topic`,
    { method: 'PUT', body: JSON.stringify(req) },
  ).finally(invalidatePhantasiNoteDocsCache)
  if (data.doc.item_id != null) invalidateItemCache(data.doc.item_id)
  invalidateSourcesCache()
  invalidateBoardPageCache()
  return data.doc
}

export async function publishNoteDoc(
  id: number,
  req: PhantasiNoteDocInput = {},
): Promise<{ id: number; link: string; doc: PhantasiNoteDoc }> {
  const data = await request<{
    success: boolean
    id: number
    link: string
    doc: PhantasiNoteDoc
  }>(`/notes/docs/${id}/publish`, {
    method: 'POST',
    body: JSON.stringify(req),
  })
  if (data.id) invalidateItemCache(data.id)
  invalidateSourcesCache()
  invalidateBoardPageCache()
  return { id: data.id, link: data.link, doc: data.doc }
}

export async function scheduleNoteDoc(
  id: number,
  req: PhantasiNoteDocInput,
): Promise<PhantasiNoteDoc> {
  const data = await request<{ success: boolean; doc: PhantasiNoteDoc }>(
    `/notes/docs/${id}/schedule`,
    {
      method: 'POST',
      body: JSON.stringify(req),
    },
  )
  invalidatePhantasiNoteDocsCache()
  return data.doc
}

export async function unscheduleNoteDoc(
  id: number,
  req: PhantasiNoteDocInput = {},
): Promise<PhantasiNoteDoc> {
  const data = await request<{ success: boolean; doc: PhantasiNoteDoc }>(
    `/notes/docs/${id}/unschedule`,
    {
      method: 'POST',
      body: JSON.stringify(req),
    },
  )
  invalidatePhantasiNoteDocsCache()
  return data.doc
}

export async function listNoteAuthorCandidates(
  signal?: AbortSignal,
): Promise<PhantasiNoteAuthor[]> {
  const data = await request<{
    success: boolean
    candidates: PhantasiNoteAuthor[]
  }>('/notes/author-candidates', { signal })
  return data.candidates
}

export async function addNoteAuthor(
  docId: number,
  userId: number,
): Promise<PhantasiNoteAuthor[]> {
  const data = await request<{ success: boolean; authors: PhantasiNoteAuthor[] }>(
    `/notes/docs/${docId}/authors`,
    {
      method: 'POST',
      body: JSON.stringify({ user_id: userId }),
    },
  )
  return data.authors
}

export async function removeNoteAuthor(
  docId: number,
  userId: number,
): Promise<PhantasiNoteAuthor[]> {
  const data = await request<{ success: boolean; authors: PhantasiNoteAuthor[] }>(
    `/notes/docs/${docId}/authors/${userId}`,
    { method: 'DELETE' },
  )
  return data.authors
}

export function noteDocWsUrl(id: number): string {
  const proto = window.location.protocol === 'https:' ? 'wss' : 'ws'
  return `${proto}://${window.location.host}/api/phantasi/notes/docs/${id}/ws`
}

export async function previewNote(
  contentMd: string,
  signal?: AbortSignal,
): Promise<string> {
  const data = await request<{ success: boolean; html: string }>(
    '/notes/preview',
    {
      method: 'POST',
      body: JSON.stringify({ content_md: contentMd }),
      signal,
    },
  )
  return data.html
}

function invalidateItemCache(id: number): void {
  requestCache.delete(phantasiCacheKeys.item(id))
}

let localRevisions = new PhantasiRevisionChain()
let itemWrites = new KeyedWrites()
let writesGeneration = -1
function currentWrites() {
  const subject = phantasiSubject.capture()
  if (subject.generation !== writesGeneration) {
    writesGeneration = subject.generation
    itemWrites = new KeyedWrites()
    localRevisions = new PhantasiRevisionChain()
  }
  return { subject, writes: itemWrites }
}

function writeItem<T>(itemId: number, task: () => Promise<T>): Promise<T> {
  const { subject, writes } = currentWrites()
  return writes.run(`${subject.generation}:${itemId}`, subject.signal, async () => {
    phantasiSubject.assert(subject)
    return task()
  })
}

export async function markRead(
  itemId: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<void> {
  return writeItem(itemId, async () => {
  const saved = await request<{ previous_revision?: number; revision?: number }>(`/items/${itemId}/read`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  localRevisions.record(itemId, saved.previous_revision, saved.revision)
  phantasiItemState.commit(itemId, { is_read: true })
  invalidateItemCache(itemId)
  invalidateStatsCache()
  })
}

export async function markUnread(
  itemId: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<void> {
  return writeItem(itemId, async () => {
  const saved = await request<{ previous_revision?: number; revision?: number }>(`/items/${itemId}/unread`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  localRevisions.record(itemId, saved.previous_revision, saved.revision)
  phantasiItemState.commit(itemId, { is_read: false })
  invalidateItemCache(itemId)
  invalidateStatsCache()
  })
}

export async function starItem(
  itemId: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<void> {
  return writeItem(itemId, async () => {
  const saved = await request<{ previous_revision?: number; revision?: number }>(`/items/${itemId}/star`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  localRevisions.record(itemId, saved.previous_revision, saved.revision)
  invalidateStatsCache()
  phantasiItemState.commit(itemId, { is_starred: true })
  invalidateItemCache(itemId)
  })
}

export async function unstarItem(
  itemId: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<void> {
  return writeItem(itemId, async () => {
  const saved = await request<{ previous_revision?: number; revision?: number }>(`/items/${itemId}/unstar`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  localRevisions.record(itemId, saved.previous_revision, saved.revision)
  invalidateStatsCache()
  phantasiItemState.commit(itemId, { is_starred: false })
  invalidateItemCache(itemId)
  })
}

export async function markAllRead(
  options: {
    source_id?: number
    category?: string
    before?: number
  } = {},
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<number> {
  const { subject, writes } = currentWrites()
  return writes.barrier(subject.signal, async () => {
    phantasiSubject.assert(subject)

  const data = await request<{ success: boolean; marked: number; changes?: Array<{ item_id: number; previous_revision: number; revision: number }> }>(
    '/mark-all-read',
    {
      method: 'POST',
      body: JSON.stringify(options),
      headers: attributionHeaders,
    },
  )
  for (const change of data.changes ?? []) {
    localRevisions.record(change.item_id, change.previous_revision, change.revision)
    phantasiItemState.commit(change.item_id, { is_read: true })
    invalidateItemCache(change.item_id)
  }
  if (options.source_id === undefined && options.category === undefined && options.before === undefined) {
    phantasiItemState.markAllRead()
  }
  invalidateStatsCache()
  return data.marked
  })
}

export async function getStats(
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal; forceRefresh?: boolean },
): Promise<PhantasiStats> {
  options?.signal?.throwIfAborted()
  const fetchStats = async (signal?: AbortSignal) => {
    const data = await request<PhantasiStatsResponse>('/stats', {
      headers: attributionHeaders,
      signal,
    })
    return data.stats
  }
  if (attributionHeaders) return fetchStats(options?.signal)
  if (options?.forceRefresh) requestCache.delete(phantasiCacheKeys.stats)
  const stats = await requestCache.fetch(
    phantasiCacheKeys.stats,
    fetchStats,
    CACHE_TTL.STATS,
    false,
    options?.signal,
  )
  options?.signal?.throwIfAborted()
  return stats
}

interface PhantasiSyncStateItem {
  expected_revision?: number
  item_id: number
  is_read?: boolean
  is_starred?: boolean
  /** 0–100 */
  read_progress?: number
  /** epoch ms */
  updated_at: number
}

interface PhantasiSyncStatesResponse {
  revisions?: Record<number, number>
  confirmed?: number[]
  failed?: number[]
  synced: number
  conflicts: Array<{
    item_id: number
    server_revision?: number
    server_updated_at: number
    client_updated_at: number
  }>
}

export async function syncReadingStates(
  states: PhantasiSyncStateItem[],
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<PhantasiSyncStatesResponse> {
  const subject = phantasiSubject.capture()
  const ids = new Set<number>()
  for (const state of states) {
    if (!Number.isInteger(state.item_id) || state.item_id <= 0 || ids.has(state.item_id)) {
      throw new Error('Invalid or duplicate article ID in sync batch')
    }
    if (state.expected_revision !== undefined && (!Number.isSafeInteger(state.expected_revision) || state.expected_revision < 0)) {
      throw new Error('Invalid state revision')
    }
    ids.add(state.item_id)
  }
  if (states.length === 0) {
    return { synced: 0, conflicts: [] }
  }
  // Keep transactions bounded; validate the entire input above before the first write.
  const merged: PhantasiSyncStatesResponse = { synced: 0, conflicts: [] }
  for (let offset = 0; offset < states.length; offset += 100) {
    phantasiSubject.assert(subject)
    let result: PhantasiSyncStatesResponse
    try {
      result = await request<PhantasiSyncStatesResponse>('/sync-states', {
        method: 'POST',
        signal: subject.signal,
        headers: { 'Content-Type': 'application/json', ...attributionHeaders },
        body: JSON.stringify({ states: states.slice(offset, offset + 100) }),
      })
      phantasiSubject.assert(subject)
    } catch (error) {
      phantasiSubject.assert(subject)
      if (error instanceof Error && error.name === 'AbortError') throw error
      if (offset === 0) throw error
      // Earlier batches committed. Keep their confirmations and leave the rest retryable.
      merged.failed = [...(merged.failed ?? []), ...states.slice(offset).map(state => state.item_id)]
      return merged
    }
    merged.synced += result.synced
    merged.conflicts.push(...result.conflicts)
    if (result.confirmed) merged.confirmed = [...(merged.confirmed ?? []), ...result.confirmed]
    if (result.failed) merged.failed = [...(merged.failed ?? []), ...result.failed]
    if (result.revisions) merged.revisions = { ...merged.revisions, ...result.revisions }
  }
  phantasiSubject.assert(subject)
  return merged
}

export async function updateReadProgress(
  itemId: number,
  progress: number,
  opts?: { isRead?: boolean; expectedRevision?: number; observedAt?: number; attributionHeaders?: PhantasiAttributionHeaders },
): Promise<number | undefined> {
  return writeItem(itemId, async () => {
  const clamped = Math.max(0, Math.min(100, Math.round(progress)))
  const result = await syncReadingStates(
    [
      {
        item_id: itemId,
        read_progress: clamped,
        expected_revision: localRevisions.advance(itemId, opts?.expectedRevision),
        is_read: opts?.isRead,
        updated_at: opts?.observedAt ?? Date.now(),
      },
    ],
    opts?.attributionHeaders,
  )
  const conflict = result.conflicts.find(value => value.item_id === itemId)
  if (conflict) throw new PhantasiSyncConflictError(itemId, conflict.server_revision)
  if (result.synced !== 1 || result.conflicts.length > 0 || (result.failed?.length ?? 0) > 0 || (result.confirmed !== undefined && !result.confirmed.includes(itemId))) {
    throw new Error('Reading progress was not saved')
  }
  if (typeof opts?.isRead === 'boolean') {
    phantasiItemState.commit(itemId, { is_read: opts.isRead })
  }
  invalidateItemCache(itemId)
  return result.revisions?.[itemId]
  })
}

export function createPhantasiWebSocket(
  onMessage: (notification: any) => void,
  onError?: (error: Event) => void,
): WebSocket {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  const ws = new WebSocket(`${protocol}//${window.location.host}/api/phantasi/ws`)

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data)
      onMessage(data)
    } catch (e) {
      console.error('Failed to parse WebSocket message:', e)
    }
  }

  ws.onerror = (error) => {
    console.error('WebSocket error:', error)
    onError?.(error)
  }

  return ws
}

interface CommentsResponse {
  next_cursor?: number | null
  success: boolean
  comments: CommentItem[]
  has_comments: boolean
  can_write?: boolean
  error?: string
}

export interface CreateCommentRequest {
  selected_text: string
  comment: string
  start_offset?: number
  end_offset?: number
  context_before?: string
  context_after?: string
  color?: string
  is_public?: boolean
  parent_id?: number
}

interface UpdateCommentRequest {
  comment?: string
  color?: string
  is_public?: boolean
}

export async function listAdminComments(
  opts?: { q?: string; sourceId?: number; itemId?: number; signal?: AbortSignal },
): Promise<{ success: boolean; comments: CommentItem[] }> {
  const query = new URLSearchParams()
  const q = opts?.q?.trim()
  if (q) query.set('q', q)
  if (opts?.sourceId != null) query.set('source_id', String(opts.sourceId))
  if (opts?.itemId != null) query.set('item_id', String(opts.itemId))
  const suffix = query.size > 0 ? `?${query}` : ''
  return request(`/comments${suffix}`, { signal: opts?.signal })
}

// Readers need all anchors for highlighting; fetch bounded pages without changing that contract.
async function collectCommentPages<T extends CommentsResponse | RepliesResponse>(
  path: string,
  key: 'comments' | 'replies',
  headers?: PhantasiAttributionHeaders,
  signal?: AbortSignal,
): Promise<T> {
  const subject = phantasiSubject.capture()
  signal = signal ? AbortSignal.any([signal, subject.signal]) : subject.signal
  let cursor = 0
  let first: T | undefined
  const rows = new Map<number, CommentItem>()
  while (true) {
    phantasiSubject.assert(subject)
    signal.throwIfAborted()
    const page = await request<T>(`${path}${cursor ? `?after_id=${cursor}` : ''}`, { headers, signal })
    phantasiSubject.assert(subject)
    signal.throwIfAborted()
    first ??= page
    const items = key === 'comments' ? (page as CommentsResponse).comments : (page as RepliesResponse).replies
    for (const item of items) rows.set(item.id, item)
    if (page.next_cursor == null) break
    if (!Number.isSafeInteger(page.next_cursor) || page.next_cursor <= cursor) {
      throw new Error('Invalid comment pagination cursor')
    }
    cursor = page.next_cursor
  }
  phantasiSubject.assert(subject)
  signal.throwIfAborted()
  return { ...first, [key]: [...rows.values()], next_cursor: null } as T
}

export async function getComments(
  itemId: number,
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<CommentsResponse> {
  const result = await collectCommentPages<CommentsResponse>(`/items/${itemId}/comments`, 'comments', attributionHeaders, options?.signal)
  result.comments.sort((a, b) => (a.start_offset ?? Infinity) - (b.start_offset ?? Infinity) || a.id - b.id)
  return result
}

export async function createComment(
  itemId: number,
  req: CreateCommentRequest,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<{ success: boolean; comment: CommentItem; error?: string }> {
  return request(`/items/${itemId}/comments`, {
    method: 'POST',
    body: JSON.stringify(req),
    headers: attributionHeaders,
  })
}

export async function updateComment(
  commentId: number,
  req: UpdateCommentRequest,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<{ success: boolean; comment: CommentItem; error?: string }> {
  return request(`/comments/${commentId}`, {
    method: 'PUT',
    body: JSON.stringify(req),
    headers: attributionHeaders,
  })
}

export async function deleteComment(
  commentId: number,
  attributionHeaders?: PhantasiAttributionHeaders,
): Promise<{ success: boolean; error?: string }> {
  return request(`/comments/${commentId}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
}

interface RepliesResponse {
  next_cursor?: number | null
  success: boolean
  replies: CommentItem[]
  error?: string
}

export async function getCommentReplies(
  commentId: number,
  attributionHeaders?: PhantasiAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<RepliesResponse> {
  const result = await collectCommentPages<RepliesResponse>(`/comments/${commentId}/replies`, 'replies', attributionHeaders, options?.signal)
  result.replies.sort((a, b) => a.created_at - b.created_at || a.id - b.id)
  return result
}

/** Omitted color/is_public inherit from the parent. */
export async function createReply(
  itemId: number,
  parentId: number,
  comment: string,
  attributionHeaders?: PhantasiAttributionHeaders,
  opts?: { color?: string; is_public?: boolean },
): Promise<{ success: boolean; comment: CommentItem; error?: string }> {
  return request(`/items/${itemId}/comments`, {
    method: 'POST',
    body: JSON.stringify({
      selected_text: '',
      comment,
      parent_id: parentId,
      ...(opts?.color ? { color: opts.color } : {}),
      ...(typeof opts?.is_public === 'boolean'
        ? { is_public: opts.is_public }
        : {}),
    }),
    headers: attributionHeaders,
  })
}

export { generateStyleTags } from './phantasiaiApi'

export async function applySourceApplication(
  req: ApplySourceApplicationInput,
): Promise<{ success: boolean; application: { id: number; status: string } }> {
  return request('/applications', {
    method: 'POST',
    body: JSON.stringify(req),
  })
}

export async function listSourceApplications(opts?: {
  q?: string
  status?: string
  signal?: AbortSignal
}): Promise<{ success: boolean; applications: PhantasiSourceApplication[] }> {
  const query = new URLSearchParams()
  const q = opts?.q?.trim()
  if (q) query.set('q', q)
  if (opts?.status?.trim()) query.set('status', opts.status.trim())
  const suffix = query.size > 0 ? `?${query}` : ''
  return request(`/applications${suffix}`, { signal: opts?.signal })
}

export async function approveSourceApplication(
  id: number,
  req?: { review_note?: string },
): Promise<{
  success: boolean
  application: PhantasiSourceApplication
  source?: PhantasiSource
}> {
  return request(`/applications/${id}/approve`, {
    method: 'POST',
    body: JSON.stringify(req ?? {}),
  })
}

export async function rejectSourceApplication(
  id: number,
  req?: { review_note?: string },
): Promise<{ success: boolean; application: PhantasiSourceApplication }> {
  return request(`/applications/${id}/reject`, {
    method: 'POST',
    body: JSON.stringify(req ?? {}),
  })
}

export async function deleteSourceApplication(
  id: number,
): Promise<{ success: boolean }> {
  return request(`/applications/${id}`, { method: 'DELETE' })
}

export type NoteEditorDefaultView = 'write' | 'visual' | 'preview'
export interface NoteHistoryEntry {
  revision: number
  actor_id: number | null
  actor_name: string | null
  saved_at: number
  snapshot: {
    title: string
    content_md: string
    topic: string | null
    image: string | null
    published_at: number | null
  }
}
export type NoteHistorySummary = Omit<NoteHistoryEntry, 'snapshot'> & {
  snapshot: Omit<NoteHistoryEntry['snapshot'], 'content_md'>
}
export async function getNoteEditorPreference(signal?: AbortSignal): Promise<NoteEditorDefaultView> {
  const data = await request<{ default_view: NoteEditorDefaultView }>('/notes/editor-preference', { signal })
  return data.default_view
}
export async function saveNoteEditorPreference(defaultView: NoteEditorDefaultView): Promise<void> {
  await request('/notes/editor-preference', { method: 'PUT', body: JSON.stringify({ default_view: defaultView }) })
}
export async function getNoteHistory(id: number, signal?: AbortSignal): Promise<NoteHistorySummary[]> {
  const data = await request<{ history: NoteHistorySummary[] }>(`/notes/docs/${id}/history`, { signal })
  return data.history
}
export async function getNoteHistoryEntry(id: number, version: number, signal?: AbortSignal): Promise<NoteHistoryEntry> {
  const data = await request<{ entry: NoteHistoryEntry }>(`/notes/docs/${id}/history/${version}`, { signal })
  return data.entry
}
export async function restoreNoteHistory(id: number, version: number, revision: number, clientRequestId: string, current: NoteHistoryEntry['snapshot']): Promise<PhantasiNoteDoc> {
  const data = await request<{ doc: PhantasiNoteDoc }>(`/notes/docs/${id}/history/${version}/restore`, {
    method: 'POST', body: JSON.stringify({ revision, client_request_id: clientRequestId, current }),
  })
  invalidatePhantasiNoteDocsCache()
  return data.doc
}
