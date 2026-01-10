/**
 * 认证上下文
 * 统一管理用户登录状态，避免重复的认证请求
 */

import type { ReactNode } from 'react'
import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { API_URL } from '../config'
import { clearSessionHint } from '../utils/sessionDetection'

interface User {
  username: string
  display_name?: string
  is_admin: boolean
  auth_provider?: string
  linked_github_id?: string
  github_id?: number
  avatar_url?: string
  bio?: string
}

interface AuthContextType {
  isAuthenticated: boolean
  isAdmin: boolean
  user: User | null
  isLoading: boolean
  hasChecked: boolean
  checkAuth: () => Promise<void>
  logout: () => void
}

const AuthContext = createContext<AuthContextType | undefined>(undefined)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(false)
  const [isAdmin, setIsAdmin] = useState(false)
  const [user, setUser] = useState<User | null>(null)
  const [isLoading, setIsLoading] = useState(false) // 初始不加载
  const [hasChecked, setHasChecked] = useState(false) // 是否已检查过

  const checkAuth = useCallback(async () => {
    // 如果已经在检查中，避免重复
    if (isLoading)
      return

    setIsLoading(true)
    try {
      const response = await fetch(`${API_URL}/api/auth/me`, {
        credentials: 'include',
        signal: AbortSignal.timeout(5000),
      })

      if (response.ok) {
        const userData = await response.json()
        setUser(userData)
        setIsAuthenticated(true)
        setIsAdmin(userData.is_admin || false)
      }
      else {
        // 401 是正常的未登录状态，静默处理
        setUser(null)
        setIsAuthenticated(false)
        setIsAdmin(false)
      }
    }
    catch (error) {
      // 网络错误时静默处理
      setUser(null)
      setIsAuthenticated(false)
      setIsAdmin(false)
    }
    finally {
      setIsLoading(false)
      setHasChecked(true)
    }
  }, [isLoading])

  const logout = useCallback(() => {
    setUser(null)
    setIsAuthenticated(false)
    setIsAdmin(false)
    // 清除会话提示标志
    clearSessionHint()
  }, [])

  // 页面加载时检查认证状态，包括：
  // 1. OAuth 回调（auth=success 或 link=success）
  // 2. 页面刷新时恢复登录状态（通过 Cookie 持久化）
  useEffect(() => {
    const urlParams = new URLSearchParams(window.location.search)
    const authSuccess = urlParams.get('auth') === 'success'
    const linkSuccess = urlParams.get('link') === 'success'

    if (authSuccess || linkSuccess) {
      // OAuth 登录/绑定成功，立即检查认证状态
      console.debug('[AuthContext] OAuth callback detected, checking auth...')
      checkAuth()

      // 清理 URL 参数，避免刷新时重复触发
      const newUrl = window.location.pathname
      window.history.replaceState({}, '', newUrl)
    }
    else {
      // 页面加载时自动检查认证状态（恢复登录会话）
      // 这确保了刷新页面后登录状态能够持久化
      console.debug('[AuthContext] Page load, checking auth session...')
      checkAuth()
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // 🔧 性能优化：使用 useMemo 缓存 context value，避免不必要的重渲染
  const value = useMemo(() => ({
    isAuthenticated,
    isAdmin,
    user,
    isLoading,
    hasChecked,
    checkAuth,
    logout,
  }), [isAuthenticated, isAdmin, user, isLoading, hasChecked, checkAuth, logout])

  return (
    <AuthContext.Provider value={value}>
      {children}
    </AuthContext.Provider>
  )
}

export function useAuth() {
  const context = useContext(AuthContext)
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider')
  }
  return context
}
