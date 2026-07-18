import type { FC, SubmitEvent } from 'react'
import { FaGithub, FaLock, FaUser } from '@lib/icons'
import { useEffect, useState } from 'react'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { fetchJson } from '../utils/apiHelper'
import {
  messageForLocalLoginError,
  messageForOAuthError,
} from '../utils/authErrorMessages'
import { sanitizeUsername } from '../utils/inputSanitizer'
import { normalizeOAuthIconUrl, preloadOAuthIcons } from '../utils/oauthIcons'
import { RateLimitError } from '../utils/rateLimiter'
import { setSessionHint } from '../utils/sessionDetection'
import OAuthIconImage from './OAuthIconImage'
import { Spinner } from './Spinner'
import './LoginForm.css'

// PR #2/#3：后端返回的 OAuth provider 描述
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

  useEffect(() => {
    // Surface OAuth callback failures redirected as `/login?oauth_error=...&desc=...`
    try {
      const params = new URLSearchParams(window.location.search)
      const oauthError = params.get('oauth_error')?.trim()
      if (oauthError) {
        const desc = params.get('desc')
        setError(messageForOAuthError(oauthError, desc, t, format))
        // Drop query so refresh does not re-show the same banner forever
        params.delete('oauth_error')
        params.delete('desc')
        const next = params.toString()
        const path = window.location.pathname
        window.history.replaceState(
          {},
          '',
          next ? `${path}?${next}` : path,
        )
      }
    } catch {
      // ignore (SSR / non-browser)
    }
  }, [t, format])

  useEffect(() => {
    // 并发拉 provider 列表 + setup config（注册开关）
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
      } catch (_err) {
        // 静默：provider 列表不可用时仅显示本地登录
      }
      try {
        const data = await fetchJson(`${API_URL}/api/setup/config`)
        setAllowRegister(Boolean(data?.allow_local_registration))
      } catch (_err) {
        // 静默：取不到时默认关闭注册
      }
    })()
  }, [])

  const handleSubmit = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault()
    setError('')

    // 输入验证
    if (!formData.username || !formData.password) {
      setError(t.auth.fillUsernameAndPassword)
      return
    }

    // 验证用户名格式
    if (formData.username.length < 3 || formData.username.length > 50) {
      setError(t.auth.usernameLengthError)
      return
    }

    // 验证用户名只包含字母、数字、下划线
    if (!/^\w+$/.test(formData.username)) {
      setError(t.auth.usernameFormatError)
      return
    }

    // 验证密码长度
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

      // The JWT is intentionally available only through the HttpOnly cookie.

      // 🔒 安全修复 P1: 只存储会话提示标志，不存储用户信息
      // 用户信息（包括 is_admin）将通过后端 API 实时验证
      setSessionHint()

      // 触发自定义事件通知Layout更新用户信息（携带管理员状态）
      window.dispatchEvent(
        new CustomEvent('auth-login-success', {
          detail: {
            user: data.user,
            isAdmin: data.user?.is_admin || false,
          },
        }),
      )

      // 同时触发认证状态变化事件
      window.dispatchEvent(
        new CustomEvent('auth-state-changed', {
          detail: {
            isAuthenticated: true,
            isAdmin: data.user?.is_admin || false,
          },
        }),
      )

      // 延迟一下再跳转，让事件处理器先执行
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
    <div className="w-full max-w-md">
      <div className="glass rounded-2xl shadow-xl p-8">
        {/* Error Message */}
        {error && (
          <div className="mb-6 bg-red-50 border border-red-200 rounded-lg p-4">
            <p className="text-red-800 text-sm">{error}</p>
          </div>
        )}

        {/* Local Login Form */}
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              {t.auth.username}
            </label>
            <div className="relative">
              <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                <FaUser className="text-gray-400" />
              </div>
              <input
                type="text"
                value={formData.username}
                onChange={(e) => {
                  const sanitized = sanitizeUsername(e.target.value)
                  setFormData({ ...formData, username: sanitized })
                }}
                className="w-full pl-10 pr-3 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                placeholder={t.auth.enterUsername}
                maxLength={50}
                autoComplete="username"
                required
              />
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              {t.auth.password}
            </label>
            <div className="relative">
              <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                <FaLock className="text-gray-400" />
              </div>
              <input
                type="password"
                value={formData.password}
                onChange={(e) =>
                  setFormData({ ...formData, password: e.target.value })
                }
                className="w-full pl-10 pr-3 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
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
            className="w-full py-3 bg-indigo-600 hover:bg-indigo-700 text-white rounded-lg transition-all flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed font-semibold shadow-lg"
          >
            {submitting ? (
              <>
                <Spinner size="sm" variant="white" />
                <span>{t.auth.loggingIn}</span>
              </>
            ) : (
              <span>{t.auth.login}</span>
            )}
          </button>
        </form>

        {/* OAuth Providers — 动态从 /api/auth/oauth/providers 拉取 */}
        {providers.length > 0 && (
          <>
            <div className="relative my-6">
              <div className="absolute inset-0 flex items-center">
                <div className="w-full border-t border-gray-300"></div>
              </div>
              <div className="relative flex justify-center text-sm">
                <span className="px-2 bg-white text-gray-500">
                  {t.common.or}
                </span>
              </div>
            </div>

            <div className="flex flex-col gap-2.5">
              {providers.map((p) => (
                <a
                  key={p.slug}
                  href={`${API_URL}/api/auth/oauth/${p.slug}/login`}
                  className={
                    p.slug === 'github'
                      ? 'oauth-provider-btn oauth-provider-btn-github'
                      : 'oauth-provider-btn oauth-provider-btn-oidc'
                  }
                >
                  <span className="oauth-provider-icon">
                    {p.kind === 'github' || p.slug === 'github' || p.icon === 'github' ? (
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

        {/* Register link — PR #4 */}
        {allowRegister && (
          <div className="mt-6 text-center text-sm text-gray-600">
            {t.auth.noAccount}
            <a
              href="/register"
              className="text-indigo-600 hover:underline ml-1"
            >
              {t.auth.registerHere}
            </a>
          </div>
        )}
      </div>
    </div>
  )
}

export default LoginForm
