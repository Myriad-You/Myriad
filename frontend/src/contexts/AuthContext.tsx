/**
 * 认证上下文
 * 统一管理用户登录状态，避免重复的认证请求
 */

import type { ReactNode } from 'react'

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { API_URL } from '../config'
import {
  clearSessionHint,
  hasSessionHint,
  setSessionHint,
} from '../utils/sessionDetection'

export interface User {
  id: number
  username: string
  display_name?: string
  is_admin: boolean
  /** Durable site owner (was: heuristic id === 1). */
  is_owner?: boolean
  auth_provider?: string
  linked_github_id?: string
  github_id?: number
  avatar_url?: string
  bio?: string
  has_password?: boolean
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

  // tapp runtime 动态加载：Auth 上下文是全站首屏必经之路，静态 import 会把
  // runtime/调度器拖进每个页面的关键路径。身份切换是低频操作，多付一次
  // chunk 加载换取首屏不含 runtime。
  //
  // 必须 await 后再拉新身份：reset 会 destroy 所有 TappRuntimeGrant，而
  // destroy 是不可逆的（getToken 之后永远抛 'Tapp runtime has already
  // stopped'）。若 reset 落在新会话之后，刚挂载的 tapp 会被打成 guest——
  // 宿主 user.getRole / context.getUser 都吞掉该异常并回落 guest，
  // 表现为联邦客户端加载不出用户信息。
  const resetTappSubjectState = useCallback(async () => {
    const [{ TappScheduler }, { TappRuntimeGrant }, { TappRuntime }] =
      await Promise.all([
        import('../tapp/runtime/TappScheduler'),
        import('../tapp/runtime/TappRuntimeGrant'),
        import('../tapp/runtime/TappRuntime'),
      ])
    TappScheduler.reset()
    TappRuntimeGrant.destroyAll()
    TappRuntime.reset()
  }, [])

  // Serialize concurrent checkAuth calls. Wait for any in-flight check, then
  // always run a fresh /auth/me — login after page-load check must not no-op.
  const checkAuthInflight = useRef<Promise<void> | null>(null)

  const checkAuth = useCallback(async () => {
    while (checkAuthInflight.current) {
      try {
        await checkAuthInflight.current
      } catch {
        // previous attempt failed; still run a fresh probe
      }
    }

    setIsLoading(true)
    // Holder so the async body can compare against the same Promise without
    // TS "used before assigned" / ESLint prefer-const friction.
    const inflight = { current: null as Promise<void> | null }
    inflight.current = (async () => {
      try {
        const response = await fetch(`${API_URL}/api/auth/me`, {
          credentials: 'include',
          signal: AbortSignal.timeout(5000),
        })

        if (response.ok) {
          const userData = await response.json()
          setSessionHint()
          setUser(userData)
          setIsAuthenticated(true)
          setIsAdmin(userData.is_admin || false)
        } else {
          // 401 是正常的未登录状态，静默处理（不 console.error）
          clearSessionHint()
          setUser(null)
          setIsAuthenticated(false)
          setIsAdmin(false)
        }
      } catch (_error) {
        // 网络错误时静默处理
        setUser(null)
        setIsAuthenticated(false)
        setIsAdmin(false)
      } finally {
        setIsLoading(false)
        setHasChecked(true)
        if (checkAuthInflight.current === inflight.current) {
          checkAuthInflight.current = null
        }
      }
    })()
    checkAuthInflight.current = inflight.current
    await inflight.current
  }, [])

  const logout = useCallback(() => {
    // 登出不必等待：清空身份后没有新 tapp 会以已登录状态挂载，且这里的 reset
    // 与随后可能的登录 reset 共享同一份 import 缓存，解析顺序即调用顺序。
    void resetTappSubjectState()
    setUser(null)
    setIsAuthenticated(false)
    setIsAdmin(false)
    // 清除会话提示标志
    clearSessionHint()
  }, [resetTappSubjectState])

  // 页面加载时检查认证状态，包括：
  // 1. OAuth 回调（auth=success 或 link=success）— 始终探测
  // 2. 有 session hint 时恢复登录（Cookie 持久化）
  // 3. 纯游客（无 hint）跳过 /api/auth/me，避免预期内的 401 网络红字
  // link=* query params are cleaned by useAuthUrlFeedback (toasts need them first).
  useEffect(() => {
    const urlParams = new URLSearchParams(window.location.search)
    const authSuccess = urlParams.get('auth') === 'success'
    const linkSuccess = urlParams.get('link') === 'success'

    if (authSuccess || linkSuccess) {
      // OAuth 登录/绑定成功，立即检查认证状态
      console.debug('[AuthContext] OAuth callback detected, checking auth...')
      void checkAuth()

      // Strip only auth=success; leave link=* for the feedback toast hook
      if (authSuccess) {
        urlParams.delete('auth')
        const next = urlParams.toString()
        const path = window.location.pathname
        window.history.replaceState({}, '', next ? `${path}?${next}` : path)
      }
    } else if (hasSessionHint()) {
      // 可能有活跃会话：探测 Cookie / 恢复登录态
      console.debug('[AuthContext] Session hint present, checking auth...')
      void checkAuth()
    } else {
      // 纯游客：不打 /api/auth/me，消除浏览器 Network 上的预期 401
      console.debug('[AuthContext] No session hint — guest, skip auth probe')
      setUser(null)
      setIsAuthenticated(false)
      setIsAdmin(false)
      setIsLoading(false)
      setHasChecked(true)
    }
  }, [])

  // 监听全局认证状态变化事件（由 LoginForm、api.ts、UserSection 触发）
  useEffect(() => {
    const handleAuthChange = (e: Event) => {
      const isAuth = (e as CustomEvent).detail?.isAuthenticated ?? false
      if (isAuth) {
        // 1) destroyAll old grants (irreversible)
        // 2) refresh session identity
        // 3) tell open sandboxes to remount AFTER grants are cleared and
        //    user is known — otherwise Aro keeps a dead grant and stays guest.
        void (async () => {
          try {
            await resetTappSubjectState()
          } catch (error) {
            console.warn('[AuthContext] tapp runtime reset failed:', error)
          }
          try {
            await checkAuth()
          } finally {
            window.dispatchEvent(
              new CustomEvent('tapp-subject-ready', {
                detail: { isAuthenticated: true },
              }),
            )
          }
        })()
      } else {
        logout()
        window.dispatchEvent(
          new CustomEvent('tapp-subject-ready', {
            detail: { isAuthenticated: false },
          }),
        )
      }
    }
    window.addEventListener('auth-state-changed', handleAuthChange)
    return () =>
      window.removeEventListener('auth-state-changed', handleAuthChange)
  }, [checkAuth, logout, resetTappSubjectState])

  // 🔧 性能优化：使用 useMemo 缓存 context value，避免不必要的重渲染
  const value = useMemo(
    () => ({
      isAuthenticated,
      isAdmin,
      user,
      isLoading,
      hasChecked,
      checkAuth,
      logout,
    }),
    [isAuthenticated, isAdmin, user, isLoading, hasChecked, checkAuth, logout],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  const context = useContext(AuthContext)
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider')
  }
  return context
}
