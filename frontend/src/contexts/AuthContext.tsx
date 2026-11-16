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
import { authSubjectKey } from '../utils/authSubject'
import { clearCSRFToken } from '../utils/csrf'
import { HOST_SESSION_RECHECK_EVENT } from '../utils/hostSessionFailure'
import { beginIdentityChange, finishIdentityChange, settleIdentity } from '../utils/identity'
import { consumeLoginReturn } from '../utils/safeReturnPath'
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
  // The embed build never starts a Tapp runtime, so there is nothing to
  // reset (typeof: unit tests run without Vite's define).
  const resetTappSubjectState = useCallback(async () => {
    if (typeof __EMBED_ENTRY__ !== 'undefined' && __EMBED_ENTRY__) return
    const [{ TappScheduler }, { tappKernel }, { TappRuntime }] =
      await Promise.all([
        import('../tapp/runtime/TappScheduler'),
        import('../tapp/tappKernel'),
        import('../tapp/runtime/TappRuntime'),
      ])
    TappScheduler.reset()
    tappKernel.grants.destroyAll()
    TappRuntime.reset()
  }, [])

  // Wait for in-flight checkAuth, then always re-probe (login after mount must not reuse a stale result).
  const checkAuthInflight = useRef<Promise<boolean> | null>(null)
  /** Monotonic generation so a stale probe cannot clear a fresher login hint. */
  const checkAuthGeneration = useRef(0)
  const authTransition = useRef(0)
  const probeController = useRef<AbortController | null>(null)
  const checkAuthRef = useRef<(() => void) | null>(null)
  const authRetryTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const authRetryAttempt = useRef(0)
  const mounted = useRef(true)

  const clearAuthRetry = useCallback(() => {
    if (authRetryTimer.current) {
      clearTimeout(authRetryTimer.current)
      authRetryTimer.current = null
    }
    authRetryAttempt.current = 0
  }, [])

  const scheduleAuthRetry = useCallback(() => {
    if (!hasSessionHint() || authRetryTimer.current || authRetryAttempt.current >= 3) return
    const delay = 1_000 * 2 ** authRetryAttempt.current
    authRetryAttempt.current += 1
    authRetryTimer.current = setTimeout(() => {
      authRetryTimer.current = null
      void checkAuthRef.current?.()
    }, delay)
  }, [])

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      checkAuthGeneration.current++
      authTransition.current++
      probeController.current?.abort()
      clearAuthRetry()
    }
  }, [clearAuthRetry])

  const confirmedSubject = useRef('guest')
  const cleanupQueue = useRef<Promise<void>>(Promise.resolve())
  const pendingSubject = useRef<{ epoch: number; cleanup: Promise<void> } | null>(null)
  const beginSubjectChange = useCallback(() => {
    const epoch = beginIdentityChange()
    const cleanup = cleanupQueue.current.catch(() => {}).then(resetTappSubjectState)
    cleanupQueue.current = cleanup
    pendingSubject.current = { epoch, cleanup }
    return pendingSubject.current
  }, [resetTappSubjectState])

  const prepareSubject = useCallback(async (key: string, generation: number) => {
    if (confirmedSubject.current === key && !pendingSubject.current) return true
    const transition = pendingSubject.current ?? beginSubjectChange()
    await transition.cleanup
    if (!mounted.current || generation !== checkAuthGeneration.current || pendingSubject.current !== transition) return false
    return true
  }, [beginSubjectChange])

  const finishSubject = useCallback((key: string, authenticated: boolean) => {
    if (!authenticated) clearCSRFToken()
    confirmedSubject.current = key
    const transition = pendingSubject.current
    pendingSubject.current = null
    if (transition) finishIdentityChange(transition.epoch, authenticated)
  }, [])

  const checkAuth = useCallback(async (): Promise<boolean> => {
    if (!mounted.current) return false
    while (checkAuthInflight.current) {
      try {
        await checkAuthInflight.current
      } catch {
        // Failed probe; still run a fresh one.
      }
      if (!mounted.current) return false
    }

    const generation = ++checkAuthGeneration.current
    const controller = new AbortController()
    probeController.current = controller
    setIsLoading(true)
    const inflight = { current: null as Promise<boolean> | null }
    inflight.current = (async (): Promise<boolean> => {
      try {
        const response = await fetch(`${API_URL}/api/auth/me`, {
          credentials: 'include',
          signal: AbortSignal.any([controller.signal, AbortSignal.timeout(5000)]),
        })

        if (generation !== checkAuthGeneration.current) return false

        // Guest/expired → HTTP 200 + authenticated:false (never 401). Do not treat status alone as logged in.
        if (isAuthMeHttpOk(response.status)) {
          const parsed = parseAuthMeResponse(await response.json())
          if (generation !== checkAuthGeneration.current) return false
          if (parsed.authenticated) {
            const u = parsed.user
            const key = authSubjectKey(u)
            if (!await prepareSubject(key, generation) || generation !== checkAuthGeneration.current) return false
            settleIdentity(u)
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
            clearAuthRetry()
            finishSubject(key, true)
            return true
          }
          if (!await prepareSubject('guest', generation) || generation !== checkAuthGeneration.current) return false
          // Drop the session hint only on a definitive guest body.
          clearSessionHint()
          settleIdentity(null)
          setUser(null)
          setIsAuthenticated(false)
          setIsAdmin(false)
          setKnownAuthState(false)
          clearAuthRetry()
          finishSubject('guest', false)
          return false
        }

        if (response.status === 401 || response.status === 403) {
          if (!await prepareSubject('guest', generation) || generation !== checkAuthGeneration.current) return false
          settleIdentity(null)
          clearSessionHint()
          setUser(null)
          setIsAuthenticated(false)
          setIsAdmin(false)
          setKnownAuthState(false)
          clearAuthRetry()
          finishSubject('guest', false)
        } else {
          scheduleAuthRetry()
        }
        // 5xx is not a definitive guest; do not let the sandbox block on it. Hint may remain.
        return false
      } catch {
        // Network/timeout is inconclusive. Preserve the last confirmed identity;
        // only a definitive guest body or 401/403 may clear admin state.
        if (generation !== checkAuthGeneration.current) return false
        scheduleAuthRetry()
        return false
      } finally {
        if (generation === checkAuthGeneration.current) {
          setIsLoading(false)
          setHasChecked(true)
        }
        if (probeController.current === controller) probeController.current = null
        if (checkAuthInflight.current === inflight.current) {
          checkAuthInflight.current = null
        }
      }
    })()
    checkAuthInflight.current = inflight.current
    return await inflight.current
  }, [clearAuthRetry, scheduleAuthRetry, prepareSubject, finishSubject])
  checkAuthRef.current = checkAuth

  const logout = useCallback(() => {
    authTransition.current++
    checkAuthGeneration.current++
    probeController.current?.abort()
    const transition = beginSubjectChange()
    setIsLoading(false)
    void transition.cleanup.then(() => {
      if (!mounted.current || pendingSubject.current !== transition) return
      settleIdentity(null)
      finishSubject('guest', false)
    }).catch(error => console.warn('[AuthContext] tapp runtime reset failed:', error))
    setUser(null)
    setIsAuthenticated(false)
    setIsAdmin(false)
    setKnownAuthState(false)
    clearSessionHint()
    clearAuthRetry()
  }, [clearAuthRetry, beginSubjectChange, finishSubject])

  // Cookie is the session. localStorage is only a retry hint, not identity.
  // /api/auth/me is 200 + authenticated:false for guests (never 401).
  // link=* is cleaned by useAuthUrlFeedback after toasts.
  useEffect(() => {
    const urlParams = new URLSearchParams(window.location.search)
    const authSuccess = urlParams.get('auth') === 'success'

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
      const stored = consumeLoginReturn(window.location.origin)
      if (stored) {
        window.location.assign(stored)
        return
      }
      urlParams.delete('auth')
      const next = urlParams.toString()
      const path = window.location.pathname
      window.history.replaceState({}, '', next ? `${path}?${next}` : path)
    }
  }, [])

  useEffect(() => {
    const handleAuthChange = (e: Event) => {
      const isAuth = (e as CustomEvent).detail?.isAuthenticated ?? false
      if (isAuth) {
        const transition = ++authTransition.current
        checkAuthGeneration.current++
        probeController.current?.abort()
        clearAuthRetry()
        const isCurrent = () => mounted.current && transition === authTransition.current
        const subject = beginSubjectChange()
        void subject.cleanup.then(async () => {
          if (!isCurrent()) return
          await checkAuth()
        }).catch(error => console.warn('[AuthContext] tapp runtime reset failed:', error))
      } else {
        logout()
      }
    }
    window.addEventListener('auth-state-changed', handleAuthChange)
    return () =>
      window.removeEventListener('auth-state-changed', handleAuthChange)
  }, [checkAuth, clearAuthRetry, logout, beginSubjectChange])

  useEffect(() => {
    let pending = false
    const recheck = () => {
      if (pending || pendingSubject.current) return
      pending = true
      void checkAuth().finally(() => { pending = false })
    }
    window.addEventListener(HOST_SESSION_RECHECK_EVENT, recheck)
    return () => window.removeEventListener(HOST_SESSION_RECHECK_EVENT, recheck)
  }, [checkAuth])

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
