import type { FC, SubmitEvent } from 'react'
import { FaLock, FaUser } from '@lib/icons'
import { useState } from 'react'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { fetchJson } from '../utils/apiHelper'
import { sanitizeUsername } from '../utils/inputSanitizer'
import { setSessionHint } from '../utils/sessionDetection'
import { Spinner } from './Spinner'

/**
 * 公开本地账号注册表单（PR #4）
 *
 * 后端开关：`DynamicConfig.allow_local_registration`
 * 端点：POST /api/auth/register
 */
const RegisterForm: FC = () => {
  const { t } = useI18n()
  const [formData, setFormData] = useState({
    username: '',
    password: '',
    email: '',
  })
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState('')

  const handleSubmit = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault()
    setError('')

    if (!formData.username || !formData.password) {
      setError(t.auth.fillUsernameAndPassword)
      return
    }
    if (formData.username.length < 3 || formData.username.length > 20) {
      setError(t.auth.usernameRange3to20)
      return
    }
    if (!/^\w+$/.test(formData.username)) {
      setError(t.auth.usernameFormatError)
      return
    }
    // Align with backend validate_password + SetupWizard: ≥8 + Unicode letter + digit
    if (
      formData.password.length < 8 ||
      !/\p{L}/u.test(formData.password) ||
      !/\p{N}/u.test(formData.password)
    ) {
      setError(t.auth.passwordRule)
      return
    }

    setSubmitting(true)
    try {
      const body: Record<string, string> = {
        username: formData.username,
        password: formData.password,
      }
      if (formData.email) body.email = formData.email

      const data = await fetchJson(
        `${API_URL}/api/auth/register`,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        },
        t.auth.registerFailed,
      )

      if (!data?.user) {
        throw new Error(t.auth.registerResponseIncomplete)
      }

      try {
        const { trackProductEvent, AnalyticsEvents } = await import(
          '../utils/analyticsEvents',
        )
        // Sync enqueue + immediate flush before hard redirect (~100ms)
        trackProductEvent(AnalyticsEvents.REGISTER_SUCCESS, { flush: true })
      } catch {
        /* ignore */
      }

      setSessionHint()
      window.dispatchEvent(
        new CustomEvent('auth-login-success', {
          detail: { user: data.user, isAdmin: false },
        }),
      )
      window.dispatchEvent(
        new CustomEvent('auth-state-changed', {
          detail: { isAuthenticated: true, isAdmin: false },
        }),
      )

      setTimeout(() => {
        window.location.href = '/'
      }, 100)
    } catch (err: any) {
      setError(err?.message || t.auth.registerFailed)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="w-full max-w-md">
      <div className="glass rounded-2xl shadow-xl p-8">
        {error && (
          <div className="mb-6 bg-red-50 border border-red-200 rounded-lg p-4">
            <p className="text-red-800 text-sm">{error}</p>
          </div>
        )}

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
                maxLength={20}
                autoComplete="username"
                required
              />
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              {t.auth.emailOptional}
            </label>
            <input
              type="email"
              value={formData.email}
              onChange={(e) =>
                setFormData({ ...formData, email: e.target.value })
              }
              className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
              placeholder={t.auth.emailPlaceholder}
              maxLength={255}
              autoComplete="email"
            />
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
                autoComplete="new-password"
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
              <Spinner size="xs" color="white" />
            ) : (
              <span>{t.auth.register}</span>
            )}
          </button>
        </form>

        <div className="mt-6 text-center text-sm text-gray-600">
          {t.auth.haveAccount}
          <a href="/login" className="text-indigo-600 hover:underline ml-1">
            {t.auth.backToLogin}
          </a>
        </div>
      </div>
    </div>
  )
}

export default RegisterForm
