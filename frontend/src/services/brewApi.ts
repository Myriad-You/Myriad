import type {
  AddRsshubInstanceRequest,
  AddSourceRequest,
  BrewCategoriesResponse,
  BrewCategory,
  BrewItem,
  BrewItemsQuery,
  BrewItemsResponse,
  BrewNoteDraft,
  BrewNoteInput,
  BrewSource,
  BrewSourcesResponse,
  BrewStats,
  BrewStatsResponse,
  CreateCategoryRequest,
  RsshubInstance,
  UpdateCategoryRequest,
  UpdateRsshubInstanceRequest,
  UpdateSourceRequest,
} from '../types/brew'
import { API_URL } from '../config'
import { hostLocaleHeaders } from '../i18n/hostLocaleHeaders'
import { brewItemState } from '../utils/brewItemState'
import { BrewRevisionChain } from '../utils/brewRevisionChain'
import { brewSubject } from '../utils/brewSubject'
import { BrewSyncConflictError } from '../utils/brewSyncConflict'
import { getCSRFToken } from '../utils/csrf'
import { notifyHttpRateLimit } from '../utils/httpRateLimitToast'
import { KeyedWrites } from '../utils/keyedWrites'
import { requestCache } from '../utils/requestCache'
import { httpStatusMessage, isUselessErrorText } from '../utils/userFacingError'
import { ApiError, parseApiErrorBody } from './api'

const API_BASE = `${API_URL}/api/brew`

function brewHttpError(status: number, data: unknown): ApiError {
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

/** CSRF: retry once. */
async function request<T>(
  endpoint: string,
  options: RequestInit = {},
  retryOnCSRFError: boolean = true,
): Promise<T> {
  const subject = brewSubject.capture()
  const stateRevision = brewItemState.getSnapshot()
  options = {
    ...options,
    signal: options.signal
      ? AbortSignal.any([options.signal, subject.signal])
      : subject.signal,
  }
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...hostLocaleHeaders(),
    ...(options.headers as Record<string, string>),
  }

  const method = options.method?.toUpperCase() || 'GET'
  const needsCSRF = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method)

  if (needsCSRF) {
    const csrfToken = await getCSRFToken()
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken
    }
  }

  brewSubject.assert(subject)
  const response = await fetch(`${API_BASE}${endpoint}`, {
    ...options,
    headers,
    credentials: 'include',
  })

  if (!response.ok) {
    notifyHttpRateLimit(response)
  }

  const data = await response.json()
  brewSubject.assert(subject)

  if (!response.ok) {
    if (response.status === 403 && needsCSRF && retryOnCSRFError) {
      const errorMsg = data.error || ''
      if (errorMsg.toLowerCase().includes('csrf')) {
        console.warn('CSRF token invalid, refreshing and retrying...')
        const newToken = await getCSRFToken(true)
        brewSubject.assert(subject)
        if (newToken) {
          headers['X-CSRF-Token'] = newToken
          const retryResponse = await fetch(`${API_BASE}${endpoint}`, {
            ...options,
            headers,
            credentials: 'include',
          })
          const retryData = await retryResponse.json()
          brewSubject.assert(subject)
          if (!retryResponse.ok) {
            throw brewHttpError(retryResponse.status, retryData)
          }
          return retryData
        }
      }
    }
    throw brewHttpError(response.status, data)
  }

  if (method === 'GET') {
    const items = [
      ...(Array.isArray(data.items) ? data.items : []),
      ...(data.item ? [data.item] : []),
      ...(Array.isArray(data.sources) ? data.sources.flatMap((source: BrewSource) => source.recent_items ?? []) : []),
    ].filter(item => typeof item?.id === 'number')
    brewItemState.observeMany(items, stateRevision)
  }
  return data
}

/** Tapp sandbox: send Runtime Grant. */
export type BrewAttributionHeaders = Record<string, string>

/** Tapp attribution skips cache. */
export async function getSources(
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<BrewSource[]> {
  const fetchSources = async () => {
    const data = await request<BrewSourcesResponse>('/sources', {
      headers: attributionHeaders,
      signal: options?.signal,
    })
    return data.sources
  }
  if (attributionHeaders) return fetchSources()
  if (options?.signal) {
    const sources = await fetchSources()
    if (!options.signal.aborted) {
      requestCache.set('brew:sources', sources, CACHE_TTL.SOURCES)
    }
    return sources
  }
  return requestCache.fetch('brew:sources', fetchSources, CACHE_TTL.SOURCES)
}

export function invalidateSourcesCache(): void {
  requestCache.delete('brew:sources')
  requestCache.delete('brew:stats')
}

/** Source/note mutations only; not read/star. */
export function invalidateBoardPageCache(): void {
  requestCache.deleteByPrefix('brew:feed-stories:')
  requestCache.deleteByPrefix('brew:home-notes:')
}

export async function addSource(
  req: AddSourceRequest,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewSource> {
  const data = await request<{
    success: boolean
    source: BrewSource
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
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewSource> {
  const data = await request<{ success: boolean; source: BrewSource }>(
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
  attributionHeaders?: BrewAttributionHeaders,
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
  attributionHeaders?: BrewAttributionHeaders,
): Promise<number> {
  const data = await request<{ success: boolean; new_items: number }>(
    `/sources/${id}/refresh`,
    { method: 'POST', headers: attributionHeaders },
  )
  invalidateSourcesCache()
  invalidateBoardPageCache()
  return data.new_items
}

export async function discoverSource(
  url: string,
  attributionHeaders?: BrewAttributionHeaders,
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
  attributionHeaders?: BrewAttributionHeaders,
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
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<string> {
  const subject = brewSubject.capture()
  const response = await fetch(`${API_BASE}/export-opml`, {
    credentials: 'include',
    headers: attributionHeaders,
    signal: options?.signal
      ? AbortSignal.any([options.signal, subject.signal])
      : subject.signal,
  })
  const content = await response.text()
  brewSubject.assert(subject)
  if (options?.signal?.aborted) {
    throw new DOMException('Aborted', 'AbortError')
  }
  if (!response.ok) throw brewHttpError(response.status, null)
  return content
}

export async function getCategories(
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<BrewCategoriesResponse['categories']> {
  const fetchCategories = async () => {
    const data = await request<BrewCategoriesResponse>('/categories', {
      headers: attributionHeaders,
      signal: options?.signal,
    })
    return data.categories
  }
  if (attributionHeaders) return fetchCategories()
  if (options?.signal) {
    const categories = await fetchCategories()
    if (!options.signal.aborted) {
      requestCache.set('brew:categories', categories, CACHE_TTL.CATEGORIES)
    }
    return categories
  }
  return requestCache.fetch(
    'brew:categories',
    fetchCategories,
    CACHE_TTL.CATEGORIES,
  )
}

export function invalidateCategoriesCache(): void {
  requestCache.delete('brew:categories')
}

export async function createCategory(
  req: CreateCategoryRequest,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewCategory> {
  const data = await request<{ success: boolean; category: BrewCategory }>(
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
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewCategory> {
  const data = await request<{ success: boolean; category: BrewCategory }>(
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
  attributionHeaders?: BrewAttributionHeaders,
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
  attributionHeaders?: BrewAttributionHeaders,
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
  attributionHeaders?: BrewAttributionHeaders,
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
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  await request(`/categories/${id}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
  invalidateCategoriesCache()
}

export type BrewItemListEntry = Omit<BrewItem, 'content'>
export type BrewItemPreviewsResponse = Omit<BrewItemsResponse, 'items'> & { items: BrewItemListEntry[] }

export function getItems(
  query: BrewItemsQuery = {},
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<BrewItemsResponse> {
  return queryItems(query, attributionHeaders, undefined, options?.signal)
}

export function getItemPreviews(
  query: BrewItemsQuery = {},
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<BrewItemPreviewsResponse> {
  return queryItems(query, attributionHeaders, 'preview', options?.signal)
}

async function queryItems<T>(
  query: BrewItemsQuery = {},
  attributionHeaders?: BrewAttributionHeaders,
  projection?: 'preview',
  signal?: AbortSignal,
): Promise<T> {
  const params = new URLSearchParams()
  if (projection) params.set('projection', projection)
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

  return request<T>(endpoint, { headers: attributionHeaders, signal })
}

export async function getItem(
  id: number,
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<BrewItem> {
  const fetchItem = async () => {
    const data = await request<{ success: boolean; item: BrewItem }>(
      `/items/${id}`,
      { headers: attributionHeaders, signal: options?.signal },
    )
    return data.item
  }
  if (attributionHeaders) return fetchItem()
  if (options?.signal) {
    const item = await fetchItem()
    if (!options.signal.aborted) {
      requestCache.set(`brew:item:${id}`, item, CACHE_TTL.ITEM)
    }
    return item
  }
  return requestCache.fetch(`brew:item:${id}`, fetchItem, CACHE_TTL.ITEM)
}

export async function createNote(
  req: BrewNoteInput,
  attributionHeaders?: BrewAttributionHeaders,
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
  req: BrewNoteInput,
  attributionHeaders?: BrewAttributionHeaders,
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
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  await request(`/notes/${id}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
  invalidateItemCache(id)
  invalidateSourcesCache()
  invalidateBoardPageCache()
}

/** Reader HTML is rendered; edit needs Markdown. */
export async function getNoteDraft(
  id: number,
  signal?: AbortSignal,
): Promise<BrewNoteDraft> {
  const data = await request<{ success: boolean; note: BrewNoteDraft }>(
    `/notes/${id}`,
    { signal },
  )
  return data.note
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

export function invalidateItemCache(id: number): void {
  requestCache.delete(`brew:item:${id}`)
}

let localRevisions = new BrewRevisionChain()
let itemWrites = new KeyedWrites()
let writesGeneration = -1
function currentWrites() {
  const subject = brewSubject.capture()
  if (subject.generation !== writesGeneration) {
    writesGeneration = subject.generation
    itemWrites = new KeyedWrites()
    localRevisions = new BrewRevisionChain()
  }
  return { subject, writes: itemWrites }
}

function writeItem<T>(itemId: number, task: () => Promise<T>): Promise<T> {
  const { subject, writes } = currentWrites()
  return writes.run(`${subject.generation}:${itemId}`, subject.signal, async () => {
    brewSubject.assert(subject)
    return task()
  })
}

export async function markRead(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  return writeItem(itemId, async () => {
  const saved = await request<{ previous_revision?: number; revision?: number }>(`/items/${itemId}/read`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  localRevisions.record(itemId, saved.previous_revision, saved.revision)
  brewItemState.commit(itemId, { is_read: true })
  invalidateItemCache(itemId)
  invalidateSourcesCache()
  })
}

export async function markUnread(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  return writeItem(itemId, async () => {
  const saved = await request<{ previous_revision?: number; revision?: number }>(`/items/${itemId}/unread`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  localRevisions.record(itemId, saved.previous_revision, saved.revision)
  brewItemState.commit(itemId, { is_read: false })
  invalidateItemCache(itemId)
  invalidateSourcesCache()
  })
}

export async function starItem(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  return writeItem(itemId, async () => {
  const saved = await request<{ previous_revision?: number; revision?: number }>(`/items/${itemId}/star`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  localRevisions.record(itemId, saved.previous_revision, saved.revision)
  invalidateSourcesCache()
  brewItemState.commit(itemId, { is_starred: true })
  invalidateItemCache(itemId)
  })
}

export async function unstarItem(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  return writeItem(itemId, async () => {
  const saved = await request<{ previous_revision?: number; revision?: number }>(`/items/${itemId}/unstar`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  localRevisions.record(itemId, saved.previous_revision, saved.revision)
  invalidateSourcesCache()
  brewItemState.commit(itemId, { is_starred: false })
  invalidateItemCache(itemId)
  })
}

export async function markAllRead(
  options: {
    source_id?: number
    category?: string
    before?: number
  } = {},
  attributionHeaders?: BrewAttributionHeaders,
): Promise<number> {
  const { subject, writes } = currentWrites()
  return writes.barrier(subject.signal, async () => {
    brewSubject.assert(subject)

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
    brewItemState.commit(change.item_id, { is_read: true })
    invalidateItemCache(change.item_id)
  }
  if (options.source_id === undefined && options.category === undefined && options.before === undefined) {
    brewItemState.markAllRead()
  }
  invalidateSourcesCache()
  return data.marked
  })
}

export async function getStats(
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<BrewStats> {
  const fetchStats = async () => {
    const data = await request<BrewStatsResponse>('/stats', {
      headers: attributionHeaders,
      signal: options?.signal,
    })
    return data.stats
  }
  if (attributionHeaders) return fetchStats()
  if (options?.signal) {
    const stats = await fetchStats()
    if (!options.signal.aborted) {
      requestCache.set('brew:stats', stats, CACHE_TTL.STATS)
    }
    return stats
  }
  return requestCache.fetch('brew:stats', fetchStats, CACHE_TTL.STATS)
}

export interface BrewSyncStateItem {
  expected_revision?: number
  item_id: number
  is_read?: boolean
  is_starred?: boolean
  /** 0–100 */
  read_progress?: number
  /** epoch ms */
  updated_at: number
}

export interface BrewSyncStatesResponse {
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
  states: BrewSyncStateItem[],
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewSyncStatesResponse> {
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
  return request<BrewSyncStatesResponse>('/sync-states', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...attributionHeaders,
    },
    body: JSON.stringify({ states }),
  })
}

export async function updateReadProgress(
  itemId: number,
  progress: number,
  opts?: { isRead?: boolean; expectedRevision?: number; observedAt?: number; attributionHeaders?: BrewAttributionHeaders },
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
  if (conflict) throw new BrewSyncConflictError(itemId, conflict.server_revision)
  if (result.synced !== 1 || result.conflicts.length > 0 || (result.failed?.length ?? 0) > 0 || (result.confirmed !== undefined && !result.confirmed.includes(itemId))) {
    throw new Error('Reading progress was not saved')
  }
  if (typeof opts?.isRead === 'boolean') {
    brewItemState.commit(itemId, { is_read: opts.isRead })
  }
  invalidateItemCache(itemId)
  return result.revisions?.[itemId]
  })
}

export function createBrewWebSocket(
  onMessage: (notification: any) => void,
  onError?: (error: Event) => void,
): WebSocket {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  const ws = new WebSocket(`${protocol}//${window.location.host}/api/brew/ws`)

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

export interface CommentItem {
  id: number
  item_id: number
  user_id: number
  user_name?: string
  user_display_name?: string
  user_avatar?: string
  selected_text: string
  comment: string
  start_offset?: number
  end_offset?: number
  context_before?: string
  context_after?: string
  color?: string
  is_public: boolean
  parent_id?: number
  content_revision?: number
  created_at: number
  updated_at: number
  replies?: CommentItem[]
  reply_count?: number
}

export interface CommentsResponse {
  success: boolean
  comments: CommentItem[]
  has_comments: boolean
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

export interface UpdateCommentRequest {
  comment?: string
  color?: string
  is_public?: boolean
}

export async function getComments(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<CommentsResponse> {
  return request<CommentsResponse>(`/items/${itemId}/comments`, {
    headers: attributionHeaders,
    signal: options?.signal,
  })
}

export async function createComment(
  itemId: number,
  req: CreateCommentRequest,
  attributionHeaders?: BrewAttributionHeaders,
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
  attributionHeaders?: BrewAttributionHeaders,
): Promise<{ success: boolean; comment: CommentItem; error?: string }> {
  return request(`/comments/${commentId}`, {
    method: 'PUT',
    body: JSON.stringify(req),
    headers: attributionHeaders,
  })
}

export async function deleteComment(
  commentId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<{ success: boolean; error?: string }> {
  return request(`/comments/${commentId}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
}

export interface RepliesResponse {
  success: boolean
  replies: CommentItem[]
  error?: string
}

export async function getCommentReplies(
  commentId: number,
  attributionHeaders?: BrewAttributionHeaders,
  options?: { signal?: AbortSignal },
): Promise<RepliesResponse> {
  return request<RepliesResponse>(`/comments/${commentId}/replies`, {
    headers: attributionHeaders,
    signal: options?.signal,
  })
}

/** Omitted color/is_public inherit from the parent. */
export async function createReply(
  itemId: number,
  parentId: number,
  comment: string,
  attributionHeaders?: BrewAttributionHeaders,
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

export type { StyleTagsResponse } from './brewliaApi'
export { generateStyleTags } from './brewliaApi'
