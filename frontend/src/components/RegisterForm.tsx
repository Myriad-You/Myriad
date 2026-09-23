import type { FC, SubmitEvent } from 'react'
import { useState } from 'react'
import { FaLock, FaUser } from 'react-icons/fa'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { fetchJson } from '../utils/apiHelper'
import { emitAppEvent } from '../utils/appEvents'
import { messageForRegisterError } from '../utils/authErrorMessages'
import { sanitizeUsername } from '../utils/inputSanitizer'
import { setSessionHint } from '../utils/sessionDetection'
import { Spinner } from './Spinner'
import './LoginForm.css'

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
    // 与后端 validate_password / SetupWizard 一致：≥8 + Unicode 字母 + 数字。
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
        // 硬跳转前同步入队并立刻 flush。
        trackProductEvent(AnalyticsEvents.REGISTER_SUCCESS, { flush: true })
      } catch {
      }

      setSessionHint()
      emitAppEvent('auth-login-success', { user: data.user, isAdmin: false })
      emitAppEvent('auth-state-changed', { isAuthenticated: true, isAdmin: false })

      setTimeout(() => {
        window.location.href = '/'
      }, 100)
    } catch (err: any) {
      setError(messageForRegisterError(err, t))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="login-form">
      <h2 className="login-form-title">{t.auth.register}</h2>

      {error && (
        <div className="login-form-error" role="alert">
          {error}
        </div>
      )}

      <form onSubmit={handleSubmit} className="login-form-fields">
        <div>
          <label className="login-form-label" htmlFor="register-username">
            {t.auth.username}
          </label>
          <div className="login-form-input-wrap">
            <span className="login-form-input-icon" aria-hidden>
              <FaUser />
            </span>
            <input
              id="register-username"
              type="text"
              value={formData.username}
              onChange={(e) => {
                const sanitized = sanitizeUsername(e.target.value)
                setFormData({ ...formData, username: sanitized })
              }}
              className="login-form-input"
              placeholder={t.auth.enterUsername}
              maxLength={20}
              autoComplete="username"
              required
            />
          </div>
        </div>

        <div>
          <label className="login-form-label" htmlFor="register-email">
            {t.auth.emailOptional}
          </label>
          <input
            id="register-email"
            type="email"
            value={formData.email}
            onChange={(e) =>
              setFormData({ ...formData, email: e.target.value })
            }
            className="login-form-input login-form-input--bare"
            placeholder={t.auth.emailPlaceholder}
            maxLength={255}
            autoComplete="email"
          />
        </div>

        <div>
          <label className="login-form-label" htmlFor="register-password">
            {t.auth.password}
          </label>
          <div className="login-form-input-wrap">
            <span className="login-form-input-icon" aria-hidden>
              <FaLock />
            </span>
            <input
              id="register-password"
              type="password"
              value={formData.password}
              onChange={(e) =>
                setFormData({ ...formData, password: e.target.value })
              }
              className="login-form-input"
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
          className="login-form-submit"
        >
          {submitting ? <Spinner size="xs" color="white" /> : t.auth.register}
        </button>
      </form>

      <p className="login-form-footer">
        {t.auth.haveAccount}
        <a href="/login">{t.auth.backToLogin}</a>
      </p>
    </div>
  )
}

export default RegisterForm
