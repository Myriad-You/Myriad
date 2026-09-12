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
import { isLocale } from '../i18n'
import { isAuthMeHttpOk, parseAuthMeResponse } from '../utils/authMe'
import { setKnownAuthState } from '../utils/authState'
import { authSubject, authSubjectKey } from '../utils/authSubject'
import { brewSubject, brewSubjectKey } from '../utils/brewSubject'
import {
  clearSessionHint,
  hasSessionHint,
  setSessionHint,
} from '../utils/sessionDetection'

export interface AuthIdentity {
  id: number
  provider: string
  provider_username?: string | null
  is_primary?: boolean
  linked_at?: string | null
}

export interface User {
  id: number
  username: string
  display_name?: string
  is_admin: boolean
  /** Durable site owner. */
  is_owner?: boolean
  auth_provider?: string
  linked_github_id?: string
  github_id?: number
  avatar_url?: string
  bio?: string
  has_password?: boolean
  last_login_at?: string | null
  identities?: AuthIdentity[]
  /** Account UI language; null if never set. */
  locale?: import('../i18n').Locale | null
}

interface AuthContextType {
  isAuthenticated: boolean
  isAdmin: boolean
  user: User | null
  isLoading: boolean
  hasChecked: boolean
  /** Probe session; true if authenticated after this probe. */
  checkAuth: () => Promise<boolean>
  logout: () => void
}

const AuthContext = createContext<AuthContextType | undefined>(undefined)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(false)
  const [isAdmin, setIsAdmin] = useState(false)
  const [user, setUser] = useState<User | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [hasChecked, setHasChecked] = useState(false)

  // Dynamic-import tapp runtime (Auth is on the first-paint path).
  // Await reset before the new identity: destroyAll is irreversible; reset after
  // a new session would leave sandboxes guest (host APIs swallow the error).
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

  // Wait for in-flight checkAuth, then always re-probe (login after mount must not reuse a stale result).
  const checkAuthInflight = useRef<Promise<boolean> | null>(null)
  /** Monotonic generation so a stale probe cannot clear a fresher login hint. */
  const checkAuthGeneration = useRef(0)

  const checkAuth = useCallback(async (): Promise<boolean> => {
    while (checkAuthInflight.current) {
      try {
        await checkAuthInflight.current
      } catch {
        // Failed probe; still run a fresh one.
      }
    }

    const generation = ++checkAuthGeneration.current
    setIsLoading(true)
    const inflight = { current: null as Promise<boolean> | null }
    inflight.current = (async (): Promise<boolean> => {
      try {
        const response = await fetch(`${API_URL}/api/auth/me`, {
          credentials: 'include',
          signal: AbortSignal.timeout(5000),
        })

        if (generation !== checkAuthGeneration.current) return false

        // Guest/expired → HTTP 200 + authenticated:false (never 401). Do not treat status alone as logged in.
        if (isAuthMeHttpOk(response.status)) {
          const parsed = parseAuthMeResponse(await response.json())
          if (generation !== checkAuthGeneration.current) return false
          if (parsed.authenticated) {
            const u = parsed.user
            authSubject.change(authSubjectKey(u))
            brewSubject.change(brewSubjectKey(u))
            setSessionHint()
            const rawIdentities = (u as { identities?: unknown }).identities
            const identities = Array.isArray(rawIdentities)
              ? rawIdentities
                  .filter(
                    (row): row is Record<string, unknown> =>
                      !!row && typeof row === 'object',
                  )
                  .map((row) => ({
                    id: Number(row.id) || 0,
                    provider: String(row.provider ?? ''),
                    provider_username:
                      typeof row.provider_username === 'string'
                        ? row.provider_username
                        : null,
                    is_primary: row.is_primary === true,
                    linked_at:
                      typeof row.linked_at === 'string' ? row.linked_at : null,
                  }))
                  .filter((row) => row.provider)
              : undefined
            setUser({
              id: u.id,
              username: u.username,
              display_name: u.display_name,
              is_admin: u.is_admin,
              is_owner: u.is_owner,
              auth_provider: u.auth_provider,
              linked_github_id: u.linked_github_id,
              github_id: u.github_id,
              avatar_url: u.avatar_url,
              bio: u.bio,
              has_password: u.has_password,
              last_login_at:
                typeof u.last_login_at === 'string' ? u.last_login_at : null,
              identities,
              locale: isLocale(u.locale) ? u.locale : null,
            })
            setIsAuthenticated(true)
            setIsAdmin(u.is_admin || false)
            setKnownAuthState(true)
            return true
          }
          // Drop the session hint only on a definitive guest body.
          clearSessionHint()
          authSubject.change('guest')
          brewSubject.change('guest')
          setUser(null)
          setIsAuthenticated(false)
          setIsAdmin(false)
          setKnownAuthState(false)
          return false
        }

        if (response.status === 401 || response.status === 403) {
          authSubject.change('guest')
          brewSubject.change('guest')
          clearSessionHint()
          setUser(null)
          setIsAuthenticated(false)
          setIsAdmin(false)
          setKnownAuthState(false)
        }
        // 5xx is not a definitive guest; do not let the sandbox block on it. Hint may remain.
        return false
      } catch {
        // Network/timeout: keep the session hint; do not claim authenticated.
        if (generation !== checkAuthGeneration.current) return false
        authSubject.change('unknown')
        brewSubject.change('unknown', false)
        setUser(null)
        setIsAuthenticated(false)
        setIsAdmin(false)
        // Missed probe; do not write knownAuthState.
        return false
      } finally {
        if (generation === checkAuthGeneration.current) {
          setIsLoading(false)
          setHasChecked(true)
        }
        if (checkAuthInflight.current === inflight.current) {
          checkAuthInflight.current = null
        }
      }
    })()
    checkAuthInflight.current = inflight.current
    return await inflight.current
  }, [])

  const logout = useCallback(() => {
    checkAuthGeneration.current++
    authSubject.change('guest', true)
    brewSubject.change('guest', true, true)
    setIsLoading(false)
    // Logout need not await; this reset and a later login share the import cache.
    void resetTappSubjectState()
    setUser(null)
    setIsAuthenticated(false)
    setIsAdmin(false)
    setKnownAuthState(false)
    clearSessionHint()
  }, [resetTappSubjectState])

  // Probe on OAuth callback or session hint; skip for a hintless guest.
  // /api/auth/me is 200 + authenticated:false for guests (never 401).
  // link=* is cleaned by useAuthUrlFeedback after toasts.
  useEffect(() => {
    const urlParams = new URLSearchParams(window.location.search)
    const authSuccess = urlParams.get('auth') === 'success'
    const linkSuccess = urlParams.get('link') === 'success'

    if (authSuccess || linkSuccess) {
      void checkAuth()

      // Strip only auth=success; leave link=* for the feedback toast hook.
      if (authSuccess) {
        void import('../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.LOGIN_OAUTH_SUCCESS, {
              flush: true,
            })
          },
        )
        urlParams.delete('auth')
        const next = urlParams.toString()
        const path = window.location.pathname
        window.history.replaceState({}, '', next ? `${path}?${next}` : path)
      }
    } else if (hasSessionHint()) {
      // Probe is safe (200 guest body) even if the hint is stale.
      console.debug('[AuthContext] Session hint present, checking auth...')
      void checkAuth()
    } else {
      console.debug('[AuthContext] No session hint — guest, skip auth probe')
      setUser(null)
      setIsAuthenticated(false)
      setIsAdmin(false)
      setIsLoading(false)
      setHasChecked(true)
      // Host already rendered as guest; sandbox must match. A later checkAuth can flip this.
      setKnownAuthState(false)
    }
  }, [])

  useEffect(() => {
    const handleAuthChange = (e: Event) => {
      const isAuth = (e as CustomEvent).detail?.isAuthenticated ?? false
      if (isAuth) {
        authSubject.change('changing', true)
        brewSubject.change('changing', false, true)
        // Remount sandboxes only after destroyAll and a known user, or Aro keeps a dead grant.
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
