/**
 * 输入清洗和验证工具
 * 防止 XSS、SQL 注入等攻击
 */

/**
 * HTML 实体编码
 */
export function escapeHtml(unsafe: string): string {
  return unsafe
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;')
}

/**
 * 移除 HTML 标签
 */
export function stripHtmlTags(html: string): string {
  return html.replace(/<[^>]*>/g, '')
}

/**
 * 清洗用户名输入
 */
export function sanitizeUsername(username: string): string {
  // 只保留字母、数字、下划线
  return username.replace(/\W/g, '').slice(0, 50)
}

/**
 * 清洗 URL 输入
 */
export function sanitizeUrl(url: string): string {
  try {
    const parsed = new URL(url)
    // 只允许 http 和 https 协议
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      throw new Error('Invalid protocol')
    }
    return parsed.toString()
  } catch {
    return ''
  }
}

/**
 * 清洗文件名
 */
export function sanitizeFilename(filename: string): string {
  return filename.replace(/[<>:"/\\|?*\x00-\x1F]/g, '_').slice(0, 255)
}
