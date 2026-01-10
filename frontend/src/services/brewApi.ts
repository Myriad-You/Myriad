/**
 * Brew 阅读 API 服务
 */

import type {
  AddSourceRequest,
  BrewCategoriesResponse,
  BrewItem,
  BrewItemsQuery,
  BrewItemsResponse,
  BrewSource,
  BrewSourcesResponse,
  BrewStats,
  BrewStatsResponse,
  CreateCategoryRequest,
  UpdateSourceRequest,
} from '../types/brew'
import { getCSRFToken } from '../utils/csrf'
import { requestCache } from '../utils/requestCache'

const API_BASE = '/api/brew'

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
            throw new Error(retryData.error || `HTTP ${retryResponse.status}`)
          }
          return retryData
        }
      }
    }
    throw new Error(data.error || `HTTP ${response.status}`)
  }

  return data
}

// ==================== 订阅源管理 ====================

/**
 * 获取所有订阅源（带缓存）
 */
export async function getSources(): Promise<BrewSource[]> {
  return requestCache.fetch(
    'brew:sources',
    async () => {
      const data = await request<BrewSourcesResponse>('/sources')
      return data.sources
    },
    CACHE_TTL.SOURCES,
  )
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
export async function addSource(req: AddSourceRequest): Promise<BrewSource> {
  const data = await request<{ success: boolean, source: BrewSource, error?: string }>(
    '/sources',
    {
      method: 'POST',
      body: JSON.stringify(req),
    },
  )
  invalidateSourcesCache()
  return data.source
}

/**
 * 更新订阅源
 */
export async function updateSource(
  id: number,
  req: UpdateSourceRequest,
): Promise<BrewSource> {
  const data = await request<{ success: boolean, source: BrewSource }>(
    `/sources/${id}`,
    {
      method: 'PUT',
      body: JSON.stringify(req),
    },
  )
  invalidateSourcesCache()
  return data.source
}

/**
 * 删除订阅源
 */
export async function deleteSource(id: number): Promise<void> {
  await request(`/sources/${id}`, { method: 'DELETE' })
  invalidateSourcesCache()
}

/**
 * 刷新订阅源
 */
export async function refreshSource(id: number): Promise<number> {
  const data = await request<{ success: boolean, new_items: number }>(
    `/sources/${id}/refresh`,
    { method: 'POST' },
  )
  invalidateSourcesCache()
  return data.new_items
}

/**
 * 探测订阅源信息
 */
export async function discoverSource(url: string): Promise<{
  title: string
  description: string | null
  site_url: string | null
  icon: string | null
  feed_type: string
  item_count: number
}> {
  const data = await request<{ success: boolean, feed: any }>('/sources/discover', {
    method: 'POST',
    body: JSON.stringify({ url }),
  })
  return data.feed
}

// ==================== OPML 导入导出 ====================

/**
 * 导入 OPML
 */
export async function importOpml(opml: string): Promise<{ imported: number, skipped: number }> {
  const data = await request<{ success: boolean, imported: number, skipped: number }>(
    '/import-opml',
    {
      method: 'POST',
      body: JSON.stringify({ opml }),
    },
  )
  invalidateSourcesCache()
  invalidateCategoriesCache()
  return { imported: data.imported, skipped: data.skipped }
}

/**
 * 导出 OPML
 */
export async function exportOpml(): Promise<string> {
  const response = await fetch(`${API_BASE}/export-opml`, {
    credentials: 'include',
  })
  return response.text()
}

// ==================== 分类管理 ====================

/**
 * 获取所有分类（带缓存）
 */
export async function getCategories(): Promise<BrewCategoriesResponse['categories']> {
  return requestCache.fetch(
    'brew:categories',
    async () => {
      const data = await request<BrewCategoriesResponse>('/categories')
      return data.categories
    },
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
export async function createCategory(req: CreateCategoryRequest): Promise<void> {
  await request('/categories', {
    method: 'POST',
    body: JSON.stringify(req),
  })
  invalidateCategoriesCache()
}

/**
 * 删除分类
 */
export async function deleteCategory(id: number): Promise<void> {
  await request(`/categories/${id}`, { method: 'DELETE' })
  invalidateCategoriesCache()
}

// ==================== 文章获取 ====================

/**
 * 获取文章列表
 */
export async function getItems(query: BrewItemsQuery = {}): Promise<BrewItemsResponse> {
  const params = new URLSearchParams()
  if (query.source_id)
    params.set('source_id', String(query.source_id))
  if (query.category)
    params.set('category', query.category)
  if (query.filter)
    params.set('filter', query.filter)
  if (query.sort_order)
    params.set('sort_order', query.sort_order)
  if (query.page)
    params.set('page', String(query.page))
  if (query.per_page)
    params.set('per_page', String(query.per_page))

  const queryString = params.toString()
  const endpoint = queryString ? `/items?${queryString}` : '/items'

  return request<BrewItemsResponse>(endpoint)
}

/**
 * 获取单篇文章（带缓存）
 */
export async function getItem(id: number): Promise<BrewItem> {
  return requestCache.fetch(
    `brew:item:${id}`,
    async () => {
      const data = await request<{ success: boolean, item: BrewItem }>(`/items/${id}`)
      return data.item
    },
    CACHE_TTL.ITEM,
  )
}

/**
 * 清除单篇文章缓存
 */
export function invalidateItemCache(id: number): void {
  requestCache.delete(`brew:item:${id}`)
}

// ==================== 阅读状态 ====================

/**
 * 标记为已读
 */
export async function markRead(itemId: number): Promise<void> {
  await request(`/items/${itemId}/read`, { method: 'POST' })
  invalidateItemCache(itemId)
  invalidateSourcesCache() // 更新未读计数
}

/**
 * 标记为未读
 */
export async function markUnread(itemId: number): Promise<void> {
  await request(`/items/${itemId}/unread`, { method: 'POST' })
  invalidateItemCache(itemId)
  invalidateSourcesCache() // 更新未读计数
}

/**
 * 收藏文章
 */
export async function starItem(itemId: number): Promise<void> {
  await request(`/items/${itemId}/star`, { method: 'POST' })
  invalidateItemCache(itemId)
}

/**
 * 取消收藏
 */
export async function unstarItem(itemId: number): Promise<void> {
  await request(`/items/${itemId}/unstar`, { method: 'POST' })
  invalidateItemCache(itemId)
}

/**
 * 全部标记为已读
 */
export async function markAllRead(options: {
  source_id?: number
  category?: string
  before?: number
} = {}): Promise<number> {
  const data = await request<{ success: boolean, marked: number }>('/mark-all-read', {
    method: 'POST',
    body: JSON.stringify(options),
  })
  invalidateSourcesCache() // 更新未读计数
  return data.marked
}

// ==================== 统计信息 ====================

/**
 * 获取统计信息（带缓存）
 */
export async function getStats(): Promise<BrewStats> {
  return requestCache.fetch(
    'brew:stats',
    async () => {
      const data = await request<BrewStatsResponse>('/stats')
      return data.stats
    },
    CACHE_TTL.STATS,
  )
}

// ==================== WebSocket ====================

/**
 * 创建 WebSocket 连接
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
    }
    catch (e) {
      console.error('Failed to parse WebSocket message:', e)
    }
  }

  ws.onerror = (error) => {
    console.error('WebSocket error:', error)
    onError?.(error)
  }

  return ws
}

// ==================== 用户评论（批注）====================

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
  /** 创建时间 */
  created_at: number
  /** 更新时间 */
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
}

/**
 * 获取文章的用户评论列表
 */
export async function getComments(itemId: number): Promise<CommentsResponse> {
  return request<CommentsResponse>(`/items/${itemId}/comments`)
}

/**
 * 创建评论
 */
export async function createComment(
  itemId: number,
  req: CreateCommentRequest,
): Promise<{ success: boolean, comment: CommentItem, error?: string }> {
  return request(`/items/${itemId}/comments`, {
    method: 'POST',
    body: JSON.stringify(req),
  })
}

/**
 * 更新评论
 */
export async function updateComment(
  commentId: number,
  req: UpdateCommentRequest,
): Promise<{ success: boolean, comment: CommentItem, error?: string }> {
  return request(`/comments/${commentId}`, {
    method: 'PUT',
    body: JSON.stringify(req),
  })
}

/**
 * 删除评论
 */
export async function deleteComment(
  commentId: number,
): Promise<{ success: boolean, error?: string }> {
  return request(`/comments/${commentId}`, {
    method: 'DELETE',
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
export async function getCommentReplies(commentId: number): Promise<RepliesResponse> {
  return request<RepliesResponse>(`/comments/${commentId}/replies`)
}

/**
 * 创建评论回复
 */
export async function createReply(
  itemId: number,
  parentId: number,
  comment: string,
): Promise<{ success: boolean, comment: CommentItem, error?: string }> {
  return request(`/items/${itemId}/comments`, {
    method: 'POST',
    body: JSON.stringify({
      selected_text: '', // 回复不需要选中文本
      comment,
      parent_id: parentId,
    }),
  })
}

// ==================== AI 风格标签 ====================

/**
 * 风格标签响应
 */
export interface StyleTagsResponse {
  success: boolean
  tags: string[]
  from_cache: boolean
  error?: string
}

/**
 * 生成订阅源的 AI 风格标签
 * 基于最近 10 篇文章的标题和前 100 字正文分析
 */
export async function generateStyleTags(sourceId: number): Promise<StyleTagsResponse> {
  const csrfToken = await getCSRFToken(true) // 强制刷新获取最新 token
  if (!csrfToken) {
    throw new Error('CSRF token not found')
  }

  const response = await fetch(`/api/brewlia/sources/${sourceId}/style-tags`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-CSRF-Token': csrfToken,
    },
    credentials: 'include',
  })

  const data = await response.json()

  if (!response.ok) {
    // 如果是 CSRF 错误，尝试刷新 token 重试
    if (response.status === 403) {
      const errorMsg = data.error || ''
      if (errorMsg.toLowerCase().includes('csrf')) {
        console.warn('CSRF token invalid, refreshing and retrying...')
        const newToken = await getCSRFToken(true)
        if (newToken) {
          const retryResponse = await fetch(`/api/brewlia/sources/${sourceId}/style-tags`, {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'X-CSRF-Token': newToken,
            },
            credentials: 'include',
          })
          const retryData = await retryResponse.json()
          if (!retryResponse.ok) {
            throw new Error(retryData.error || `HTTP ${retryResponse.status}`)
          }
          return retryData
        }
      }
    }
    throw new Error(data.error || `HTTP ${response.status}`)
  }

  return data
}
