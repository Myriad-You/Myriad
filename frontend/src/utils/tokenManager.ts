export class TokenManager {
  private static readonly TOKEN_KEY = 'auth_token'
  private static readonly COOKIE_NAME = 'auth_token'

  private static isValidToken(token: string): boolean {
    if (!token || typeof token !== 'string') return false
    const parts = token.split('.')
    return parts.length === 3 && parts.every((part) => part.length > 0)
  }

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

  /** HttpOnly cookie only; no localStorage. */
  static getToken(): string | null {
    try {
      const cookieToken = this.getTokenFromCookie()
      if (cookieToken && this.isValidToken(cookieToken)) {
        return cookieToken
      }

      return null
    } catch {
      return null
    }
  }

  static removeToken(): void {
    try {
      localStorage.removeItem(this.TOKEN_KEY)

      document.cookie = `${this.COOKIE_NAME}=; expires=Thu, 01 Jan 1970 00:00:00 UTC; path=/; SameSite=Strict`
    } catch (e) {
      console.error('Failed to remove token:', e)
    }
  }

  static isAuthenticated(): boolean {
    const token = this.getToken()
    return token !== null && this.isValidToken(token)
  }

  static decodeToken(token?: string): any {
    try {
      const t = token || this.getToken()
      if (!t) return null

      const parts = t.split('.')
      if (parts.length !== 3) return null

      const payload = parts[1]
      const decoded = atob(payload.replaceAll('-', '+').replaceAll('_', '/'))
      return JSON.parse(decoded)
    } catch {
      return null
    }
  }

  static isTokenExpiringSoon(thresholdMs: number = 300000): boolean {
    try {
      const payload = this.decodeToken()
      if (!payload || !payload.exp) return true

      const expirationTime = payload.exp * 1000 // JWT exp is seconds.
      const now = Date.now()
      return expirationTime - now < thresholdMs
    } catch {
      return true
    }
  }

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
