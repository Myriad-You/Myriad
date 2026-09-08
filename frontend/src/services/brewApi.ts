/**
 * Brew 阅读 API 服务
 */

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
import { getCSRFToken } from '../utils/csrf'
import { notifyHttpRateLimit } from '../utils/httpRateLimitToast'
import { requestCache } from '../utils/requestCache'
import { httpStatusMessage, isUselessErrorText } from '../utils/userFacingError'
import { ApiError, parseApiErrorBody } from './api'

const API_BASE = `${API_URL}/api/brew`

function brewHttpError(status: number, data: unknown): ApiError {
  const parsed = parseApiErrorBody(data, status)
  const message = isUselessErrorText(parsed.message)
    ? httpStatusMessage(status)
    : parsed.message
  return new ApiError(
    message,
    status,
    parsed.code,
    parsed.details,
    parsed.hint,
  )
}

// 缓存 TTL 配置（毫秒）
const CACHE_TTL = {
  SOURCES: 30 * 1000, // 订阅源列表 30 秒
  CATEGORIES: 60 * 1000, // 分类列表 1 分钟
  STATS: 30 * 1000, // 统计信息 30 秒
  ITEM: 5 * 60 * 1000, // 单篇文章 5 分钟
}

/**
 * 通用 API 请求（带 CSRF token 自动重试）
 */
async function request<T>(
  endpoint: string,
  options: RequestInit = {},
  retryOnCSRFError: boolean = true,
): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }

  // 对于状态变更操作添加 CSRF Token
  const method = options.method?.toUpperCase() || 'GET'
  const needsCSRF = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method)

  if (needsCSRF) {
    const csrfToken = await getCSRFToken()
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken
    }
  }

  const response = await fetch(`${API_BASE}${endpoint}`, {
    ...options,
    headers,
    credentials: 'include',
  })

  if (!response.ok) {
    notifyHttpRateLimit(response)
  }

  const data = await response.json()

  if (!response.ok) {
    // 如果是 CSRF 错误且允许重试，刷新 token 后重试一次
    if (response.status === 403 && needsCSRF && retryOnCSRFError) {
      const errorMsg = data.error || ''
      if (errorMsg.toLowerCase().includes('csrf')) {
        console.warn('CSRF token invalid, refreshing and retrying...')
        // 强制刷新 CSRF token
        const newToken = await getCSRFToken(true)
        if (newToken) {
          headers['X-CSRF-Token'] = newToken
          // 重试请求（不再重试）
          const retryResponse = await fetch(`${API_BASE}${endpoint}`, {
            ...options,
            headers,
            credentials: 'include',
          })
          const retryData = await retryResponse.json()
          if (!retryResponse.ok) {
            throw brewHttpError(retryResponse.status, retryData)
          }
          return retryData
        }
      }
    }
    throw brewHttpError(response.status, data)
  }

  return data
}

/**
 * 附加请求头（Tapp 沙箱调用时携带 Runtime Grant，用于服务端归因与权限强制）
 */
export type BrewAttributionHeaders = Record<string, string>

// 订阅源管理

/**
 * 获取所有订阅源（带缓存；Tapp 归因调用绕过缓存以保证服务端强制）
 */
export async function getSources(
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewSource[]> {
  const fetchSources = async () => {
    const data = await request<BrewSourcesResponse>('/sources', {
      headers: attributionHeaders,
    })
    return data.sources
  }
  if (attributionHeaders) return fetchSources()
  return requestCache.fetch('brew:sources', fetchSources, CACHE_TTL.SOURCES)
}

/**
 * 清除订阅源缓存（订阅源变更后调用）
 */
export function invalidateSourcesCache(): void {
  requestCache.delete('brew:sources')
  requestCache.delete('brew:stats')
}

/**
 * 添加订阅源
 */
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
  return data.source
}

/**
 * 更新订阅源
 */
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
  return data.source
}

/**
 * 删除订阅源
 */
export async function deleteSource(
  id: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  await request(`/sources/${id}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
  invalidateSourcesCache()
}

/**
 * 刷新订阅源
 */
export async function refreshSource(
  id: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<number> {
  const data = await request<{ success: boolean; new_items: number }>(
    `/sources/${id}/refresh`,
    { method: 'POST', headers: attributionHeaders },
  )
  invalidateSourcesCache()
  return data.new_items
}

/**
 * 探测订阅源信息
 */
export async function discoverSource(
  url: string,
  attributionHeaders?: BrewAttributionHeaders,
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
    },
  )
  return data.feed
}

// OPML 导入导出

/**
 * 导入 OPML
 */
export async function importOpml(
  opml: string,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<{ imported: number; skipped: number }> {
  const data = await request<{
    success: boolean
    imported: number
    skipped: number
  }>('/import-opml', {
    method: 'POST',
    body: JSON.stringify({ opml }),
    headers: attributionHeaders,
  })
  invalidateSourcesCache()
  invalidateCategoriesCache()
  return { imported: data.imported, skipped: data.skipped }
}

/**
 * 导出 OPML
 */
export async function exportOpml(
  attributionHeaders?: BrewAttributionHeaders,
): Promise<string> {
  const response = await fetch(`${API_BASE}/export-opml`, {
    credentials: 'include',
    headers: attributionHeaders,
  })
  return response.text()
}

// 分类管理

/**
 * 获取所有分类（带缓存）
 */
export async function getCategories(
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewCategoriesResponse['categories']> {
  const fetchCategories = async () => {
    const data = await request<BrewCategoriesResponse>('/categories', {
      headers: attributionHeaders,
    })
    return data.categories
  }
  if (attributionHeaders) return fetchCategories()
  return requestCache.fetch(
    'brew:categories',
    fetchCategories,
    CACHE_TTL.CATEGORIES,
  )
}

/**
 * 清除分类缓存（分类变更后调用）
 */
export function invalidateCategoriesCache(): void {
  requestCache.delete('brew:categories')
}

/**
 * 创建分类
 */
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
): Promise<RsshubInstance[]> {
  const data = await request<{
    success: boolean
    instances: RsshubInstance[]
  }>('/rsshub/instances', { headers: attributionHeaders })
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

/**
 * 删除分类
 */
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

// 文章获取

/**
 * 获取文章列表
 */
export async function getItems(
  query: BrewItemsQuery = {},
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewItemsResponse> {
  const params = new URLSearchParams()
  if (query.source_id) params.set('source_id', String(query.source_id))
  if (query.category) params.set('category', query.category)
  if (query.topic) params.set('topic', query.topic)
  if (query.filter) params.set('filter', query.filter)
  if (query.sort_order) params.set('sort_order', query.sort_order)
  if (query.page) params.set('page', String(query.page))
  if (query.per_page) params.set('per_page', String(query.per_page))

  const queryString = params.toString()
  const endpoint = queryString ? `/items?${queryString}` : '/items'

  return request<BrewItemsResponse>(endpoint, { headers: attributionHeaders })
}

/**
 * 获取单篇文章（带缓存）
 */
export async function getItem(
  id: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewItem> {
  const fetchItem = async () => {
    const data = await request<{ success: boolean; item: BrewItem }>(
      `/items/${id}`,
      { headers: attributionHeaders },
    )
    return data.item
  }
  if (attributionHeaders) return fetchItem()
  return requestCache.fetch(`brew:item:${id}`, fetchItem, CACHE_TTL.ITEM)
}

/**
 * 清除单篇文章缓存
 */
/**
 * 写一篇手记。返回新条目 id 与站内链接。
 */
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
  return { id: data.id, link: data.link }
}

/**
 * 改一篇手记。改完必须让这篇文章的缓存失效，否则阅读器还显示旧正文。
 */
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
  return { id: data.id, link: data.link }
}

/**
 * 删一篇手记。
 */
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
}

/**
 * 取回原文供编辑。阅读器拿到的是渲染后的 HTML，改稿要的是 Markdown。
 */
export async function getNoteDraft(id: number): Promise<BrewNoteDraft> {
  const data = await request<{ success: boolean; note: BrewNoteDraft }>(
    `/notes/${id}`,
  )
  return data.note
}

/**
 * 编辑器预览。与发布走同一个后端渲染函数 —— 前端不自己解析 Markdown，
 * 预览里看到的就是发出去之后的那份 HTML。
 */
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

// 阅读状态

/**
 * 标记为已读
 */
export async function markRead(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  await request(`/items/${itemId}/read`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  invalidateItemCache(itemId)
  invalidateSourcesCache() // 更新未读计数
}

/**
 * 标记为未读
 */
export async function markUnread(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  await request(`/items/${itemId}/unread`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  invalidateItemCache(itemId)
  invalidateSourcesCache() // 更新未读计数
}

/**
 * 收藏文章
 */
export async function starItem(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  await request(`/items/${itemId}/star`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  invalidateItemCache(itemId)
}

/**
 * 取消收藏
 */
export async function unstarItem(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<void> {
  await request(`/items/${itemId}/unstar`, {
    method: 'POST',
    headers: attributionHeaders,
  })
  invalidateItemCache(itemId)
}

/**
 * 全部标记为已读
 */
export async function markAllRead(
  options: {
    source_id?: number
    category?: string
    before?: number
  } = {},
  attributionHeaders?: BrewAttributionHeaders,
): Promise<number> {
  const data = await request<{ success: boolean; marked: number }>(
    '/mark-all-read',
    {
      method: 'POST',
      body: JSON.stringify(options),
      headers: attributionHeaders,
    },
  )
  invalidateSourcesCache() // 更新未读计数
  return data.marked
}

// 统计信息

/**
 * 获取统计信息（带缓存）
 */
export async function getStats(
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewStats> {
  const fetchStats = async () => {
    const data = await request<BrewStatsResponse>('/stats', {
      headers: attributionHeaders,
    })
    return data.stats
  }
  if (attributionHeaders) return fetchStats()
  return requestCache.fetch('brew:stats', fetchStats, CACHE_TTL.STATS)
}

// 阅读进度同步

export interface BrewSyncStateItem {
  item_id: number
  is_read?: boolean
  is_starred?: boolean
  /** 0–100 scroll progress */
  read_progress?: number
  /** client epoch ms */
  updated_at: number
}

export interface BrewSyncStatesResponse {
  synced: number
  conflicts: Array<{
    item_id: number
    server_updated_at: number
    client_updated_at: number
  }>
}

/**
 * 批量同步阅读状态 / 进度（对应 POST /api/brew/sync-states）
 */
export async function syncReadingStates(
  states: BrewSyncStateItem[],
  attributionHeaders?: BrewAttributionHeaders,
): Promise<BrewSyncStatesResponse> {
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

/**
 * 上报单篇阅读进度（包装 sync-states）
 */
export async function updateReadProgress(
  itemId: number,
  progress: number,
  opts?: { isRead?: boolean; attributionHeaders?: BrewAttributionHeaders },
): Promise<void> {
  const clamped = Math.max(0, Math.min(100, Math.round(progress)))
  await syncReadingStates(
    [
      {
        item_id: itemId,
        read_progress: clamped,
        is_read: opts?.isRead,
        updated_at: Date.now(),
      },
    ],
    opts?.attributionHeaders,
  )
  invalidateItemCache(itemId)
}

// WebSocket

/**
 * 创建 WebSocket 连接（登录用户：新源/新文章推送）
 */
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

// 用户评论（批注）

/**
 * 评论项
 */
export interface CommentItem {
  id: number
  item_id: number
  user_id: number
  /** 用户名 */
  user_name?: string
  /** 用户显示名称 */
  user_display_name?: string
  /** 用户头像 */
  user_avatar?: string
  /** 选中的原文文本 */
  selected_text: string
  /** 评论内容 */
  comment: string
  /** 选中文本在原文中的起始位置 */
  start_offset?: number
  /** 选中文本在原文中的结束位置 */
  end_offset?: number
  /** 前文上下文 */
  context_before?: string
  /** 后文上下文 */
  context_after?: string
  /** 评论颜色 */
  color?: string
  /** 是否公开 */
  is_public: boolean
  /** 父评论 ID（回复时指定） */
  parent_id?: number
  created_at: number
  updated_at: number
  /** 回复列表（可选，仅详情时返回） */
  replies?: CommentItem[]
  /** 回复数量 */
  reply_count?: number
}

/**
 * 评论列表响应
 */
export interface CommentsResponse {
  success: boolean
  comments: CommentItem[]
  has_comments: boolean
  error?: string
}

/**
 * 创建评论请求
 */
export interface CreateCommentRequest {
  selected_text: string
  comment: string
  start_offset?: number
  end_offset?: number
  context_before?: string
  context_after?: string
  color?: string
  is_public?: boolean
  /** 父评论 ID（回复时指定） */
  parent_id?: number
}

/**
 * 更新评论请求
 */
export interface UpdateCommentRequest {
  comment?: string
  color?: string
  /** Visibility toggle (aligned with create). */
  is_public?: boolean
}

/**
 * 获取文章的用户评论列表
 */
export async function getComments(
  itemId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<CommentsResponse> {
  return request<CommentsResponse>(`/items/${itemId}/comments`, {
    headers: attributionHeaders,
  })
}

/**
 * 创建评论
 */
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

/**
 * 更新评论
 */
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

/**
 * 删除评论
 */
export async function deleteComment(
  commentId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<{ success: boolean; error?: string }> {
  return request(`/comments/${commentId}`, {
    method: 'DELETE',
    headers: attributionHeaders,
  })
}

/**
 * 回复列表响应
 */
export interface RepliesResponse {
  success: boolean
  replies: CommentItem[]
  error?: string
}

/**
 * 获取评论的回复列表
 */
export async function getCommentReplies(
  commentId: number,
  attributionHeaders?: BrewAttributionHeaders,
): Promise<RepliesResponse> {
  return request<RepliesResponse>(`/comments/${commentId}/replies`, {
    headers: attributionHeaders,
  })
}

/**
 * 创建评论回复
 *
 * Color / is_public are optional — when omitted, BE inherits from the parent
 * comment so replies match the highlight thread.
 */
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
      selected_text: '', // 回复不需要选中文本
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

// AI 风格标签

export type { StyleTagsResponse } from './brewliaApi'
export { generateStyleTags } from './brewliaApi'
