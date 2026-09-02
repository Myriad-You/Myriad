/**
 * 安全的会话检测工具
 *
 * 🔒 安全说明：
 * - 只存储布尔标志，不存储敏感信息（username, is_admin 等）
 * - 用于性能优化的智能检测，不用于权限判断
 * - 所有权限判断必须通过后端 API 验证
 */

// 使用简单的标志键，不包含任何敏感信息
const SESSION_HINT_KEY = 'myriad_session_hint'

/**
 * 设置会话提示标志
 * 在登录成功后调用
 */
export function setSessionHint(): void {
  try {
    localStorage.setItem(SESSION_HINT_KEY, 'true')
  } catch {
    // 忽略 localStorage 错误（私密浏览模式等）
  }
}

/**
 * 检查是否存在会话提示
 * 用于智能检测是否需要调用 checkAuth()
 *
 * @returns true 如果可能存在活跃会话（需要验证）
 */
export function hasSessionHint(): boolean {
  try {
    return localStorage.getItem(SESSION_HINT_KEY) === 'true'
  } catch (_e) {
    // localStorage 访问失败时，返回 false
    return false
  }
}

/**
 * 清除会话提示标志
 * 在登出或会话失效时调用
 */
export function clearSessionHint(): void {
  try {
    localStorage.removeItem(SESSION_HINT_KEY)
  } catch {
    // 忽略错误
  }
}
