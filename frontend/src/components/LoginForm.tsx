import type { FC, SubmitEvent } from 'react'
import { FaLock, FaUser } from '@lib/faChromeIcons'
import { FaGithub } from '@lib/platformBrandIcons'
import { useEffect, useState } from 'react'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { fetchJson } from '../utils/apiHelper'
import { messageForLocalLoginError } from '../utils/authErrorMessages'
import { sanitizeUsername } from '../utils/inputSanitizer'
import { normalizeOAuthIconUrl, preloadOAuthIcons } from '../utils/oauthIcons'
import { RateLimitError } from '../utils/rateLimiter'
import { setSessionHint } from '../utils/sessionDetection'
import OAuthIconImage from './OAuthIconImage'
import { Spinner } from './Spinner'
import './LoginForm.css'

interface ProviderInfo {
  slug: string
  kind: 'github' | 'oidc'
  display_name: string
  icon?: string | null
}

function normalizeProviderInfo(provider: ProviderInfo): ProviderInfo {
  if (provider.icon === 'github') return provider

  return {
    ...provider,
    icon: normalizeOAuthIconUrl(provider.icon),
  }
}

const LoginForm: FC = () => {
  const { t, format } = useI18n()
  const [formData, setFormData] = useState({
    username: '',
    password: '',
  })
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState('')
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [allowRegister, setAllowRegister] = useState(false)

  // oauth_error 由 useAuthUrlFeedback 以 toast 展示，GuestOnly 从 /login 重定向后仍能看见。

  useEffect(() => {
    void (async () => {
      try {
        const data = await fetchJson(`${API_URL}/api/auth/oauth/providers`)
        if (Array.isArray(data?.providers)) {
          const normalizedProviders = (data.providers as ProviderInfo[]).map(
            normalizeProviderInfo,
          )
          preloadOAuthIcons(normalizedProviders.map((provider) => provider.icon))
          setProviders(normalizedProviders)
        }
      } catch {
      }
      try {
        const data = await fetchJson(`${API_URL}/api/setup/config`)
        setAllowRegister(Boolean(data?.allow_local_registration))
      } catch {
      }
    })()
  }, [])

  const handleSubmit = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault()
    setError('')

    if (!formData.username || !formData.password) {
      setError(t.auth.fillUsernameAndPassword)
      return
    }

    if (formData.username.length < 3 || formData.username.length > 50) {
      setError(t.auth.usernameLengthError)
      return
    }

    if (!/^\w+$/.test(formData.username)) {
      setError(t.auth.usernameFormatError)
      return
    }

    if (formData.password.length < 8 || formData.password.length > 128) {
      setError(t.auth.passwordLengthError)
      return
    }

    setSubmitting(true)

    try {
      const data = await fetchJson(
        `${API_URL}/api/auth/login`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
          body: JSON.stringify(formData),
        },
        t.auth.loginFailed,
      )

      if (!data.user || typeof data.user !== 'object') {
        throw new Error(t.auth.userInfoIncomplete)
      }

      // JWT 只走 HttpOnly cookie。

      // 只存会话提示标志，用户信息（含 is_admin）走后端实时校验。
      setSessionHint()

      try {
        const { setAnalyticsStaffSession } = await import(
          '../utils/siteAnalytics',
        )
        const { trackProductEvent, AnalyticsEvents } = await import(
          '../utils/analyticsEvents',
        )
        const isStaff = Boolean(data.user?.is_admin || data.user?.is_owner)
        if (isStaff) {
          setAnalyticsStaffSession({
            isAdmin: Boolean(data.user?.is_admin),
            isOwner: Boolean(data.user?.is_owner),
          })
        } else {
          // 硬跳转约 100ms，先同步入队并立刻 flush。
          trackProductEvent(AnalyticsEvents.LOGIN_SUCCESS, { flush: true })
        }
      } catch {
      }

      window.dispatchEvent(
        new CustomEvent('auth-login-success', {
          detail: {
            user: data.user,
            isAdmin: data.user?.is_admin || false,
          },
        }),
      )

      window.dispatchEvent(
        new CustomEvent('auth-state-changed', {
          detail: {
            isAuthenticated: true,
            isAdmin: data.user?.is_admin || false,
          },
        }),
      )

      setTimeout(() => {
        window.location.href = '/'
      }, 100)
    } catch (err: unknown) {
      if (err instanceof RateLimitError) {
        const seconds = Math.ceil(err.retryAfter / 1000)
        setError(format(t.auth.rateLimitError, { seconds }))
      } else {
        setError(messageForLocalLoginError(err, t, format))
      }
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="login-form">
      <h2 id="login-form-title" className="login-form-title">
        {t.auth.login}
      </h2>

      {error && (
        <div className="login-form-error" role="alert">
          {error}
        </div>
      )}

      <form onSubmit={handleSubmit} className="login-form-fields">
        <div>
          <label className="login-form-label" htmlFor="login-username">
            {t.auth.username}
          </label>
          <div className="login-form-input-wrap">
            <span className="login-form-input-icon" aria-hidden>
              <FaUser />
            </span>
            <input
              id="login-username"
              type="text"
              value={formData.username}
              onChange={(e) => {
                const sanitized = sanitizeUsername(e.target.value)
                setFormData({ ...formData, username: sanitized })
              }}
              className="login-form-input"
              placeholder={t.auth.enterUsername}
              maxLength={50}
              autoComplete="username"
              required
            />
          </div>
        </div>

        <div>
          <label className="login-form-label" htmlFor="login-password">
            {t.auth.password}
          </label>
          <div className="login-form-input-wrap">
            <span className="login-form-input-icon" aria-hidden>
              <FaLock />
            </span>
            <input
              id="login-password"
              type="password"
              value={formData.password}
              onChange={(e) =>
                setFormData({ ...formData, password: e.target.value })
              }
              className="login-form-input"
              placeholder={t.auth.enterPassword}
              maxLength={128}
              autoComplete="current-password"
              required
            />
          </div>
        </div>

        <button
          type="submit"
          disabled={submitting}
          className="login-form-submit"
        >
          {submitting ? <Spinner size="xs" color="white" /> : t.auth.login}
        </button>
      </form>

      {providers.length > 0 && (
        <>
          <div className="login-form-divider">
            <span>{t.common.or}</span>
          </div>

          <div className="login-form-oauth">
            {providers.map((p) => (
              <a
                key={p.slug}
                href={`${API_URL}/api/auth/oauth/${p.slug}/login`}
                onClick={() => {
                  void import('../utils/analyticsEvents').then(
                    ({ trackProductEvent, AnalyticsEvents }) => {
                      trackProductEvent(AnalyticsEvents.LOGIN_OAUTH_CLICK, {
                        target: p.slug,
                        flush: true,
                      })
                    },
                  )
                }}
                className={
                  p.slug === 'github'
                    ? 'oauth-provider-btn oauth-provider-btn-github'
                    : 'oauth-provider-btn oauth-provider-btn-oidc'
                }
              >
                <span className="oauth-provider-icon">
                  {p.kind === 'github' ||
                  p.slug === 'github' ||
                  p.icon === 'github' ? (
                    <FaGithub />
                  ) : p.icon ? (
                    <OAuthIconImage
                      src={p.icon}
                      size={20}
                      loading="eager"
                      fetchPriority="high"
                    />
                  ) : (
                    <span className="oauth-provider-icon-fallback">
                      {p.display_name?.[0]?.toUpperCase() || '?'}
                    </span>
                  )}
                </span>
                <span className="oauth-provider-label">
                  {p.slug === 'github'
                    ? t.auth.loginWithGithub
                    : format(t.auth.loginWith, { name: p.display_name })}
                </span>
              </a>
            ))}
          </div>
        </>
      )}

      {allowRegister && (
        <p className="login-form-footer">
          {t.auth.noAccount}
          <a href="/register">{t.auth.registerHere}</a>
        </p>
      )}
    </div>
  )
}

export default LoginForm
