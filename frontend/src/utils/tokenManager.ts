/**
 * Token 管理器 - 统一管理认证 Token 的存储和访问
 * 支持 localStorage 和 HttpOnly Cookie 双模式
 */

export class TokenManager {
  private static readonly TOKEN_KEY = 'auth_token'
  private static readonly COOKIE_NAME = 'auth_token'

  /**
   * 验证 JWT Token 格式是否有效
   */
  private static isValidToken(token: string): boolean {
    if (!token || typeof token !== 'string') return false
    const parts = token.split('.')
    return parts.length === 3 && parts.every((part) => part.length > 0)
  }

  /**
   * 从 Cookie 中读取 Token
   * ⚠️ 注意：HttpOnly Cookie 无法被 JavaScript 读取（这是安全特性）
   * 如果后端设置了 HttpOnly 标志，此方法将返回 null
   * 在这种情况下，应使用 credentials: 'include' 让浏览器自动发送 Cookie
   */
  private static getTokenFromCookie(): string | null {
    try {
      const cookies = document.cookie.split(';')
      for (const cookie of cookies) {
        const [name, value] = cookie.trim().split('=')
        if (name === this.COOKIE_NAME && value) {
          return decodeURIComponent(value)
        }
      }
      return null
    } catch {
      return null
    }
  }

  /**
   * 获取 Token（仅从 HttpOnly Cookie 读取）
   * ✅ 安全修复 P0: 移除 localStorage 回退，强制使用 HttpOnly Cookie
   */
  static getToken(): string | null {
    try {
      // 仅从 HttpOnly Cookie 读取（XSS 无法窃取）
      const cookieToken = this.getTokenFromCookie()
      if (cookieToken && this.isValidToken(cookieToken)) {
        return cookieToken
      }

      // 不再回退到 localStorage
      // 如果 Cookie 中没有 Token，说明用户未登录或 Token 已过期
      return null
    } catch {
      return null
    }
  }

  /**
   * 移除 Token
   */
  static removeToken(): void {
    try {
      // 清除 localStorage
      localStorage.removeItem(this.TOKEN_KEY)

      // 清除 Cookie（设置过期时间为过去）
      document.cookie = `${this.COOKIE_NAME}=; expires=Thu, 01 Jan 1970 00:00:00 UTC; path=/; SameSite=Strict`
    } catch (e) {
      console.error('Failed to remove token:', e)
    }
  }

  /**
   * 检查用户是否已认证
   */
  static isAuthenticated(): boolean {
    const token = this.getToken()
    return token !== null && this.isValidToken(token)
  }

  /**
   * 解析 Token 获取 Payload（不验证签名）
   */
  static decodeToken(token?: string): any {
    try {
      const t = token || this.getToken()
      if (!t) return null

      const parts = t.split('.')
      if (parts.length !== 3) return null

      const payload = parts[1]
      const decoded = atob(payload.replace(/-/g, '+').replace(/_/g, '/'))
      return JSON.parse(decoded)
    } catch {
      return null
    }
  }

  /**
   * 检查 Token 是否即将过期（默认 5 分钟内）
   */
  static isTokenExpiringSoon(thresholdMs: number = 300000): boolean {
    try {
      const payload = this.decodeToken()
      if (!payload || !payload.exp) return true

      const expirationTime = payload.exp * 1000 // JWT exp 是秒，转换为毫秒
      const now = Date.now()
      return expirationTime - now < thresholdMs
    } catch {
      return true
    }
  }

  /**
   * 获取 Token 过期时间
   */
  static getTokenExpiration(): Date | null {
    try {
      const payload = this.decodeToken()
      if (!payload || !payload.exp) return null
      return new Date(payload.exp * 1000)
    } catch {
      return null
    }
  }
}

export default TokenManager
