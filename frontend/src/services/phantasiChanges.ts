import type { DataTopic } from './dataChangeEvents'

/** 不改数据的写请求：渲染预览、编辑器偏好、笔记 RSS 开关。 */
const NOT_A_CHANGE = new Set(['/notes/preview', '/notes/editor-preference', '/notes/rss'])

/** 一次成功的 Phantasi 写请求改了哪类数据；跟创作台窗口无关的（订阅源、已读、收藏）给 null。 */
export function phantasiWriteTopic(method: string, endpoint: string): DataTopic | null {
  if (method.toUpperCase() === 'GET') return null
  const path = endpoint.split('?')[0] ?? ''
  if (NOT_A_CHANGE.has(path)) return null
  if (path === '/notes' || path.startsWith('/notes/')) return 'notes'
  if (path === '/categories' || path.startsWith('/categories/')) return 'categories'
  if (path.startsWith('/comments/') || /^\/items\/\d+\/comments$/.test(path)) return 'comments'
  if (path === '/applications' || path.startsWith('/applications/')) return 'reviews'
  return null
}
