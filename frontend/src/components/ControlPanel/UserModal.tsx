import type { FC, SubmitEvent } from 'react'

import type { User } from '../../contexts/AuthContext'
import type {
  RecentTappItem,
  TappListItem,
} from '../../tapp/services/TappLifecycleApi'
import { FaGithub, LuCrown, LuLink, LuUser, MyriadStoreIcon } from '@lib/icons'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useNavigate } from 'react-router-dom'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { TappIcon } from '../../tapp/components/TappIcon'
import { getRecentTapps, listTapps } from '../../tapp/services/TappLifecycleApi'
import { resolveManifestText } from '../../tapp/utils/manifestLocale'
import { getCSRFToken } from '../../utils/csrf'
import { normalizeOAuthIconUrl } from '../../utils/oauthIcons'
import OAuthIconImage from '../OAuthIconImage'
import { Spinner } from '../Spinner'
import '../UserModal.css'

interface OAuthProviderInfo {
  slug: string
  display_name: string
  icon?: string | null
}

interface OAuthIdentity {
  id: number
  provider: string
  provider_username: string | null
  is_primary: boolean
  linked_at: string | null
  avatar_url?: string | null
  email?: string | null
}

interface UserInfo {
  name: string
  avatar: string
  bio: string
  platform: string
}

interface UserModalProps {
  user: User
  userInfo: UserInfo
  isClosing: boolean
  canAnimate: boolean
  onClose: () => void
  onLogout: () => void
  /**
   * 从控制面板内导航（收起面板并替换 GCP 历史哨兵）。
   * 必须用此路径跳转 Tapp 等页，不能直接 navigate——否则关面板时 history.back() 会退回打开面板前的路由。
   */
  onNavigateFromPanel?: (path: string) => void
  /** 切换画像源后刷新外侧头像/名称 */
  onProfileApplied?: () => void
}

/**
 * 用户信息弹窗组件（已登录状态）
 * 全新设计：头像居中、信息整合、浮动关闭按钮
 */
export const UserModal: FC<UserModalProps> = ({
  user,
  userInfo,
  isClosing,
  canAnimate,
  onClose,
  onLogout,
  onNavigateFromPanel,
  onProfileApplied,
}) => {
  const [page, setPage] = useState<
    'main' | 'oauth' | 'password' | 'profileSource'
  >('main')
  const [profileSelectingId, setProfileSelectingId] = useState<number | null>(
    null,
  )
  const [profileSourceError, setProfileSourceError] = useState('')
  // 是否已有本地密码：有 → 修改密码；没有（纯 OAuth 账户）→ 设置密码
  // 旧版后端没有 has_password 字段时按 auth_provider 兜底
  const [hasPassword, setHasPassword] = useState(
    user.has_password ?? user.auth_provider === 'local',
  )
  const [passwordError, setPasswordError] = useState('')
  const [passwordSubmitting, setPasswordSubmitting] = useState(false)
  const [tapps, setTapps] = useState<TappListItem[]>([])
  const [recentTapps, setRecentTapps] = useState<RecentTappItem[]>([])
  const [tappsLoading, setTappsLoading] = useState(true)
  const [oauthProviders, setOAuthProviders] = useState<OAuthProviderInfo[]>([])
  const [identities, setIdentities] = useState<OAuthIdentity[]>([])
  const [oauthLoading, setOAuthLoading] = useState(false)
  const [oauthError, setOAuthError] = useState('')
  const [unbindingId, setUnbindingId] = useState<number | null>(null)
  const { t, locale } = useI18n()
  const navigate = useNavigate()
  const contentRef = useRef<HTMLDivElement>(null)
  const [modalHeight, setModalHeight] = useState<number>()

  // 跟随内容高度，让主页/二级页切换（及内容加载）时的高度变化有过渡动画
  useEffect(() => {
    const el = contentRef.current
    if (!el) return
    const observer = new ResizeObserver(() => {
      setModalHeight(el.offsetHeight)
    })
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  // 加载可用 provider 与当前用户已绑定的 identities
  const loadOAuthBindings = useCallback(async () => {
    setOAuthLoading(true)
    try {
      const [providersRes, identitiesRes] = await Promise.all([
        fetch(`${API_URL}/api/auth/oauth/providers`, {
          credentials: 'include',
        }),
        fetch(`${API_URL}/api/auth/identities`, { credentials: 'include' }),
      ])
      if (providersRes.ok) {
        const data = await providersRes.json()
        setOAuthProviders(Array.isArray(data?.providers) ? data.providers : [])
      }
      if (identitiesRes.ok) {
        const data = await identitiesRes.json()
        const list = Array.isArray(data?.identities) ? data.identities : []
        setIdentities(
          list.map(
            (row: Record<string, unknown>): OAuthIdentity => ({
              id: Number(row.id) || 0,
              provider: String(row.provider ?? ''),
              provider_username:
                typeof row.provider_username === 'string'
                  ? row.provider_username
                  : null,
              is_primary: row.is_primary === true,
              linked_at:
                typeof row.linked_at === 'string' ? row.linked_at : null,
              avatar_url:
                typeof row.avatar_url === 'string' ? row.avatar_url : null,
              email: typeof row.email === 'string' ? row.email : null,
            }),
          ),
        )
      }
    } catch (error) {
      console.error('Failed to load OAuth bindings:', error)
    } finally {
      setOAuthLoading(false)
    }
  }, [])

  // 主页徽章需要 identities；进入 OAuth 页再拉一次以同步解绑/绑定
  useEffect(() => {
    void loadOAuthBindings()
  }, [loadOAuthBindings])

  useEffect(() => {
    if (page === 'oauth') {
      void loadOAuthBindings()
    }
  }, [page, loadOAuthBindings])

  /** Prefer live bindings list; fall back to /me identities + legacy linked_github_id */
  const linkedProviders = useMemo(() => {
    const fromLive = identities
      .map((i) => i.provider)
      .filter((p): p is string => !!p && p.trim().length > 0)
    if (fromLive.length > 0) {
      return [...new Set(fromLive.map((p) => p.trim().toLowerCase()))]
    }
    const fromUser = (user.identities ?? [])
      .map((i) => i.provider)
      .filter((p): p is string => !!p && p.trim().length > 0)
      .map((p) => p.trim().toLowerCase())
    if (fromUser.length > 0) {
      return [...new Set(fromUser)]
    }
    if (user.linked_github_id || user.github_id) {
      return ['github']
    }
    return [] as string[]
  }, [identities, user.identities, user.linked_github_id, user.github_id])

  const providerDisplayName = useCallback(
    (slug: string): string => {
      const key = slug.toLowerCase()
      if (key === 'github') return 'GitHub'
      const match = oauthProviders.find(
        (p) => p.slug.toLowerCase() === key,
      )
      if (match?.display_name?.trim()) return match.display_name.trim()
      // oidc-google → Google-style fallback
      const bare = key.replace(/^oidc[-_]?/, '')
      if (bare.length === 0) return slug
      return bare.charAt(0).toUpperCase() + bare.slice(1)
    },
    [oauthProviders],
  )

  const accountBadge = useMemo(() => {
    const hasLocalPassword =
      hasPassword ||
      user.has_password === true ||
      user.auth_provider === 'local'
    const labels = linkedProviders.map(providerDisplayName)
    const join = (names: string[]) =>
      names.join(locale.startsWith('zh') ? '、' : ', ')

    if (hasLocalPassword && linkedProviders.length > 0) {
      const providersText = join(labels)
      const text =
        t.userModal.hybridAccountWithProviders?.replace(
          '{providers}',
          providersText,
        ) ||
        t.userModal.hybridAccount ||
        `Local + ${providersText}`
      const onlyGithub =
        linkedProviders.length === 1 && linkedProviders[0] === 'github'
      return {
        kind: 'hybrid' as const,
        text,
        onlyGithub,
      }
    }

    if (
      user.auth_provider === 'github' ||
      (linkedProviders.length === 1 &&
        linkedProviders[0] === 'github' &&
        !hasLocalPassword)
    ) {
      return { kind: 'github' as const, text: 'GitHub', onlyGithub: true }
    }

    if (
      user.auth_provider === 'oidc' ||
      (linkedProviders.length > 0 && !hasLocalPassword)
    ) {
      const text =
        labels.length > 0
          ? join(labels)
          : t.userModal.oauthAccount || 'OAuth'
      return { kind: 'oauth' as const, text, onlyGithub: false }
    }

    return {
      kind: 'local' as const,
      text: t.userModal.localAccount || 'Local',
      onlyGithub: false,
    }
  }, [
    hasPassword,
    user.has_password,
    user.auth_provider,
    linkedProviders,
    providerDisplayName,
    locale,
    t.userModal,
  ])

  // 返回主页面，清空二级页面的临时状态
  const backToMain = () => {
    setPage('main')
    setOAuthError('')
    setPasswordError('')
    setProfileSourceError('')
    setProfileSelectingId(null)
  }

  const openProfileSource = () => {
    if (identities.length === 0 && linkedProviders.length === 0) return
    setProfileSourceError('')
    setPage('profileSource')
    void loadOAuthBindings()
  }

  const handleSelectProfileSource = async (identity: OAuthIdentity) => {
    if (identity.is_primary || profileSelectingId != null) return
    setProfileSourceError('')
    setProfileSelectingId(identity.id)
    try {
      const csrfToken = await getCSRFToken()
      if (!csrfToken) {
        setProfileSourceError(t.userModal.cannotGetCsrf)
        return
      }
      const response = await fetch(
        `${API_URL}/api/auth/identities/${identity.id}/primary`,
        {
          method: 'POST',
          credentials: 'include',
          headers: { 'X-CSRF-Token': csrfToken },
        },
      )
      if (!response.ok) {
        const body = await response.json().catch(() => null)
        setProfileSourceError(
          (typeof body?.message === 'string' && body.message) ||
            (typeof body?.error === 'string' && body.error) ||
            t.userModal.profileSourceFailed,
        )
        return
      }
      await loadOAuthBindings()
      onProfileApplied?.()
      setPage('main')
    } catch {
      setProfileSourceError(t.userModal.networkError)
    } finally {
      setProfileSelectingId(null)
    }
  }

  // 解绑某个 OAuth identity
  const handleUnbind = async (identity: OAuthIdentity) => {
    if (!window.confirm(t.userModal.oauthUnbindConfirm)) return
    setOAuthError('')
    setUnbindingId(identity.id)
    try {
      const csrfToken = await getCSRFToken()
      if (!csrfToken) {
        setOAuthError(t.userModal.cannotGetCsrf)
        return
      }
      const response = await fetch(
        `${API_URL}/api/auth/oauth/${encodeURIComponent(identity.provider)}/unlink/${identity.id}`,
        {
          method: 'DELETE',
          credentials: 'include',
          headers: { 'X-CSRF-Token': csrfToken },
        },
      )
      if (!response.ok) {
        const body = await response.json().catch(() => null)
        setOAuthError(
          (typeof body?.message === 'string' && body.message) ||
            (typeof body?.error === 'string' && body.error) ||
            t.userModal.oauthUnbindFailed,
        )
        return
      }
      await loadOAuthBindings()
    } catch {
      setOAuthError(t.userModal.networkError)
    } finally {
      setUnbindingId(null)
    }
  }

  // 加载 Tapp 列表和最近使用记录
  useEffect(() => {
    const loadData = async () => {
      try {
        // 并行加载 Tapp 列表和最近使用记录
        const [tappList, recentList] = await Promise.all([
          listTapps(),
          getRecentTapps(3).catch(() => [] as RecentTappItem[]), // 如果获取失败返回空数组
        ])
        setTapps(tappList)
        setRecentTapps(recentList)
      } catch (error) {
        console.error('Failed to load tapps:', error)
      } finally {
        setTappsLoading(false)
      }
    }
    loadData()
  }, [])

  // 处理修改密码
  const handleChangePassword = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault()
    setPasswordError('')

    const formData = new FormData(e.currentTarget)
    const oldPassword = formData.get('old-password') as string
    const newPassword = formData.get('new-password') as string
    const confirmPassword = formData.get('confirm-password') as string

    if (newPassword.length < 8) {
      setPasswordError(t.userModal.newPasswordMinLength)
      return
    }

    // 与后端 validate_password 一致：必须同时包含字母和数字（Unicode 语义）
    if (!/\p{L}/u.test(newPassword) || !/\p{N}/u.test(newPassword)) {
      setPasswordError(t.userModal.passwordNeedsLetterAndDigit)
      return
    }

    if (newPassword !== confirmPassword) {
      setPasswordError(t.userModal.passwordMismatch)
      return
    }

    if (oldPassword === newPassword) {
      setPasswordError(t.userModal.passwordSameAsOld)
      return
    }

    setPasswordSubmitting(true)

    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) {
        setPasswordError(t.userModal.cannotGetCsrf)
        setPasswordSubmitting(false)
        return
      }

      const response = await fetch(`${API_URL}/api/auth/change-password`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({
          old_password: oldPassword,
          new_password: newPassword,
        }),
      })

      const result = await response.json()

      if (response.ok && result.success) {
        alert(t.userModal.passwordChanged)
        setPage('main')
      } else {
        setPasswordError(result.message || result.error || t.common.error)
      }
    } catch (_error) {
      setPasswordError(t.userModal.networkError)
    } finally {
      setPasswordSubmitting(false)
    }
  }

  // 处理设置密码（纯 OAuth 账户后补本地密码）
  const handleSetPassword = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault()
    setPasswordError('')

    const formData = new FormData(e.currentTarget)
    const newPassword = formData.get('new-password') as string
    const confirmPassword = formData.get('confirm-password') as string

    if (newPassword.length < 8) {
      setPasswordError(t.userModal.newPasswordMinLength)
      return
    }

    // 与后端 validate_password 一致：必须同时包含字母和数字（Unicode 语义）
    if (!/\p{L}/u.test(newPassword) || !/\p{N}/u.test(newPassword)) {
      setPasswordError(t.userModal.passwordNeedsLetterAndDigit)
      return
    }

    if (newPassword !== confirmPassword) {
      setPasswordError(t.userModal.passwordMismatch)
      return
    }

    setPasswordSubmitting(true)

    try {
      const csrfToken = await getCSRFToken(true)
      if (!csrfToken) {
        setPasswordError(t.userModal.cannotGetCsrf)
        setPasswordSubmitting(false)
        return
      }

      const response = await fetch(`${API_URL}/api/auth/me/set-password`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({ new_password: newPassword }),
      })

      const result = await response.json()

      if (response.ok && result.success) {
        alert(t.userModal.passwordSet)
        setHasPassword(true)
        setPage('main')
      } else {
        setPasswordError(result.message || result.error || t.common.error)
      }
    } catch (_error) {
      setPasswordError(t.userModal.networkError)
    } finally {
      setPasswordSubmitting(false)
    }
  }

  const goFromPanel = (path: string) => {
    onClose()
    if (onNavigateFromPanel) {
      onNavigateFromPanel(path)
    } else {
      navigate(path)
    }
  }

  const handleTappClick = (tappId: string) => {
    goFromPanel(`/tapp/run/${tappId}`)
  }

  const handleViewAllTapps = () => {
    goFromPanel('/tapp')
  }

  return (
    <div
      className={`user-modal ${canAnimate ? 'animate-in' : 'pre-animate'} ${isClosing ? 'closing' : ''}`}
      style={modalHeight !== undefined ? { height: modalHeight } : undefined}
    >
      {/* 浮动关闭按钮 */}
      <button
        onClick={onClose}
        className="user-modal-close-float"
        aria-label={t.common.close}
      >
        <svg
          className="w-5 h-5"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M6 18L18 6M6 6l12 12"
          />
        </svg>
      </button>

      {/* 内容包裹层：ResizeObserver 测量自然高度，驱动弹窗高度过渡 */}
      <div className="user-modal-inner" ref={contentRef}>
        {page !== 'main' ? (
          /* 二级页面：整体替换弹窗内容，左上角返回 */
          <div className="user-modal-page">
            <div className="user-modal-page-head">
              <button
                type="button"
                className="user-modal-page-back"
                aria-label={t.common.back}
                onClick={backToMain}
              >
                <svg
                  className="w-5 h-5"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M15 19l-7-7 7-7"
                  />
                </svg>
              </button>
              <h3 className="user-modal-page-title">
                {page === 'oauth'
                  ? t.userModal.oauthBindings
                  : page === 'profileSource'
                    ? t.userModal.profileSourceTitle
                    : hasPassword
                      ? t.userModal.changePassword
                      : t.userModal.setPassword}
              </h3>
            </div>

            {page === 'profileSource' ? (
              <div className="user-modal-page-body">
                <p className="user-modal-profile-source-hint">
                  {t.userModal.profileSourceHint}
                </p>
                {oauthLoading && identities.length === 0 ? (
                  <p className="user-modal-oauth-empty">…</p>
                ) : identities.length === 0 ? (
                  <p className="user-modal-oauth-empty">
                    {t.userModal.profileSourceEmpty}
                  </p>
                ) : (
                  <ul className="user-modal-profile-source-list">
                    {identities.map((identity) => {
                      const name = providerDisplayName(identity.provider)
                      const avatar =
                        identity.avatar_url ||
                        `https://ui-avatars.com/api/?name=${encodeURIComponent(
                          identity.provider_username || name,
                        )}&size=64&background=6366f1&color=fff`
                      const selecting = profileSelectingId === identity.id
                      return (
                        <li key={identity.id}>
                          <button
                            type="button"
                            className={`user-modal-profile-source-row ${identity.is_primary ? 'is-primary' : ''}`}
                            disabled={selecting || profileSelectingId != null}
                            onClick={() =>
                              void handleSelectProfileSource(identity)
                            }
                          >
                            <img
                              src={avatar}
                              alt=""
                              className="user-modal-profile-source-avatar"
                              onError={(e) => {
                                e.currentTarget.src = `https://ui-avatars.com/api/?name=${encodeURIComponent(name)}&size=64&background=6366f1&color=fff`
                              }}
                            />
                            <span className="user-modal-profile-source-info">
                              <span className="user-modal-profile-source-name">
                                {name}
                                {identity.is_primary && (
                                  <span className="user-modal-profile-source-current">
                                    {t.userModal.profileSourceCurrent}
                                  </span>
                                )}
                              </span>
                              {identity.provider_username && (
                                <span className="user-modal-profile-source-sub">
                                  @{identity.provider_username}
                                </span>
                              )}
                            </span>
                            {selecting ? (
                              <Spinner size="sm" />
                            ) : identity.is_primary ? (
                              <span className="user-modal-profile-source-check">
                                ✓
                              </span>
                            ) : null}
                          </button>
                        </li>
                      )
                    })}
                  </ul>
                )}
                {profileSourceError && (
                  <p className="user-modal-oauth-error">{profileSourceError}</p>
                )}
              </div>
            ) : page === 'oauth' ? (
              <div className="user-modal-page-body">
                {oauthLoading ? (
                  <p className="user-modal-oauth-empty">…</p>
                ) : oauthProviders.length === 0 && identities.length === 0 ? (
                  <p className="user-modal-oauth-empty">
                    {t.userModal.oauthNoProviders}
                  </p>
                ) : (
                  <ul className="user-modal-oauth-list">
                    {/* 已启用的 provider */}
                    {oauthProviders.map((provider) => {
                      const bound = identities.find(
                        (identity) => identity.provider === provider.slug,
                      )
                      const iconSrc = normalizeOAuthIconUrl(provider.icon)
                      return (
                        <li
                          key={provider.slug}
                          className="user-modal-oauth-row"
                        >
                          <span className="user-modal-oauth-icon">
                            {provider.slug === 'github' ? (
                              <FaGithub size={20} aria-hidden />
                            ) : iconSrc ? (
                              <OAuthIconImage src={iconSrc} size={20} />
                            ) : (
                              <LuUser size={20} aria-hidden />
                            )}
                          </span>
                          <span className="user-modal-oauth-info">
                            <span className="user-modal-oauth-name">
                              {provider.display_name}
                            </span>
                            <span
                              className={`user-modal-oauth-sub ${bound ? 'bound' : ''}`}
                            >
                              {bound
                                ? bound.provider_username ||
                                  t.userModal.githubLinked
                                : t.userModal.githubNotLinked}
                            </span>
                          </span>
                          {bound ? (
                            <button
                              type="button"
                              className="user-modal-oauth-btn danger"
                              disabled={unbindingId !== null}
                              onClick={() => handleUnbind(bound)}
                            >
                              {unbindingId === bound.id
                                ? '…'
                                : t.userModal.oauthUnbind}
                            </button>
                          ) : (
                            <a
                              href={`${API_URL}/api/auth/oauth/${encodeURIComponent(provider.slug)}/link`}
                              className="user-modal-oauth-btn"
                            >
                              {t.userModal.oauthBind}
                            </a>
                          )}
                        </li>
                      )
                    })}
                    {/* 已绑定但 provider 已被停用/删除的 identity：仍允许解绑 */}
                    {identities
                      .filter(
                        (identity) =>
                          !oauthProviders.some(
                            (p) => p.slug === identity.provider,
                          ),
                      )
                      .map((identity) => (
                        <li
                          key={`orphan-${identity.id}`}
                          className="user-modal-oauth-row orphan"
                        >
                          <span className="user-modal-oauth-icon">
                            {identity.provider === 'github' ? (
                              <FaGithub size={20} aria-hidden />
                            ) : (
                              <LuUser size={20} aria-hidden />
                            )}
                          </span>
                          <span className="user-modal-oauth-info">
                            <span className="user-modal-oauth-name">
                              {identity.provider}
                              <span className="user-modal-oauth-disabled-tag">
                                {t.userModal.oauthNotConfigured}
                              </span>
                            </span>
                            <span className="user-modal-oauth-sub">
                              {identity.provider_username ||
                                t.userModal.githubLinked}
                            </span>
                          </span>
                          <button
                            type="button"
                            className="user-modal-oauth-btn danger"
                            disabled={unbindingId !== null}
                            onClick={() => handleUnbind(identity)}
                          >
                            {unbindingId === identity.id
                              ? '…'
                              : t.userModal.oauthUnbind}
                          </button>
                        </li>
                      ))}
                  </ul>
                )}
                {oauthError && (
                  <p className="user-modal-oauth-error">{oauthError}</p>
                )}
              </div>
            ) : (
              /* 二级页面：修改密码 / 设置密码（纯 OAuth 账户后补本地密码） */
              <div className="user-modal-page-body">
                {!hasPassword && (
                  <div className="user-modal-password-intro">
                    <p className="user-modal-password-hint">
                      {t.userModal.setPasswordHint}
                    </p>
                    <p className="user-modal-password-username">
                      {t.userModal.localLoginUsername}
                      <code>{user.username}</code>
                    </p>
                  </div>
                )}
                <form
                  onSubmit={
                    hasPassword ? handleChangePassword : handleSetPassword
                  }
                  className="user-modal-password-form space-y-3"
                >
                  {hasPassword && (
                    <input
                      type="password"
                      name="old-password"
                      required
                      className="user-modal-input"
                      placeholder={t.userModal.currentPassword}
                      autoComplete="current-password"
                    />
                  )}
                  <input
                    type="password"
                    name="new-password"
                    required
                    minLength={8}
                    className="user-modal-input"
                    placeholder={t.userModal.newPassword}
                    autoComplete="new-password"
                  />
                  <input
                    type="password"
                    name="confirm-password"
                    required
                    minLength={8}
                    className="user-modal-input"
                    placeholder={t.userModal.confirmNewPassword}
                    autoComplete="new-password"
                  />
                  {passwordError && (
                    <p className="text-red-500 text-xs text-center">
                      {passwordError}
                    </p>
                  )}
                  <button
                    type="submit"
                    disabled={passwordSubmitting}
                    className="w-full user-modal-action-btn action-confirm"
                  >
                    {passwordSubmitting
                      ? hasPassword
                        ? t.userModal.changing
                        : t.userModal.setting
                      : hasPassword
                        ? t.userModal.confirmChange
                        : t.userModal.confirmSet}
                  </button>
                </form>
              </div>
            )}
          </div>
        ) : (
          <>
            {/* 上部区域：用户信息（约60%） */}
            <div className="user-modal-hero">
              {/* 装饰背景 */}
              <div className="user-modal-hero-bg" />

              {/* 头像：有 OAuth 绑定时可点击选择画像源 */}
              <div className="user-modal-avatar-wrapper">
                {identities.length > 0 || linkedProviders.length > 0 ? (
                  <button
                    type="button"
                    className="user-modal-avatar-btn"
                    onClick={openProfileSource}
                    title={t.userModal.profileSourceTitle}
                    aria-label={t.userModal.profileSourceTitle}
                  >
                    <img
                      src={userInfo.avatar}
                      alt={userInfo.name}
                      className="user-modal-avatar-lg"
                      onError={(e) => {
                        e.currentTarget.src = `https://ui-avatars.com/api/?name=${encodeURIComponent(userInfo.name)}&size=128&background=6366f1&color=fff`
                      }}
                    />
                    <span className="user-modal-avatar-edit-hint">
                      {t.userModal.profileSourceAvatarHint}
                    </span>
                  </button>
                ) : (
                  <img
                    src={userInfo.avatar}
                    alt={userInfo.name}
                    className="user-modal-avatar-lg"
                    onError={(e) => {
                      e.currentTarget.src = `https://ui-avatars.com/api/?name=${encodeURIComponent(userInfo.name)}&size=128&background=6366f1&color=fff`
                    }}
                  />
                )}
                {/* 在线状态指示器 */}
                <div className="user-modal-online-dot" />
              </div>

              {/* 用户名和角色 */}
              <div className="user-modal-identity">
                <h3 className="user-modal-username">{userInfo.name}</h3>
                <div className="user-modal-badges">
                  {/* 角色徽章 */}
                  <span
                    className={`user-modal-badge ${user.is_admin ? 'badge-admin' : 'badge-user'}`}
                  >
                    {user.is_admin ? (
                      <>
                        <LuCrown size={12} className="inline" /> Admin
                      </>
                    ) : (
                      <>
                        <LuUser size={12} className="inline" /> User
                      </>
                    )}
                  </span>
                  {/* 账户类型：本地密码 + 任意 OAuth/OIDC 绑定 → 混合；否则按主 provider */}
                  {accountBadge.kind === 'hybrid' ? (
                    <span className="user-modal-badge badge-hybrid">
                      {accountBadge.onlyGithub ? (
                        <FaGithub size={13} className="inline" />
                      ) : (
                        <LuLink size={13} className="inline" />
                      )}
                      {accountBadge.text}
                    </span>
                  ) : accountBadge.kind === 'github' ? (
                    <span className="user-modal-badge badge-github">
                      <FaGithub size={13} className="inline" />
                      {accountBadge.text}
                    </span>
                  ) : accountBadge.kind === 'oauth' ? (
                    <span className="user-modal-badge badge-oauth">
                      <LuLink size={13} className="inline" />
                      {accountBadge.text}
                    </span>
                  ) : (
                    <span className="user-modal-badge badge-local">
                      <LuUser size={13} className="inline" />
                      {accountBadge.text}
                    </span>
                  )}
                </div>
              </div>

              {/* 简介 */}
              {userInfo.bio && userInfo.bio !== t.userModal.defaultBio && (
                <p className="user-modal-bio">{userInfo.bio}</p>
              )}

              {/* 操作按钮组 */}
              <div className="user-modal-actions">
                {/* 第三方账号绑定入口（管理面板为二级页面） */}
                <button
                  onClick={() => setPage('oauth')}
                  className="user-modal-action-btn action-oauth"
                >
                  <LuLink size={15} />
                  {t.userModal.oauthBindings}
                </button>

                {/* 已有本地密码 → 修改密码；纯 OAuth 账户 → 设置密码（补本地登录通道） */}
                <button
                  onClick={() => setPage('password')}
                  className="user-modal-action-btn action-password"
                >
                  <svg
                    className="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"
                    />
                  </svg>
                  {hasPassword
                    ? t.userModal.changePassword
                    : t.userModal.setPassword}
                </button>

                {/* 退出登录 - 所有用户显示 */}
                <button
                  onClick={onLogout}
                  className="user-modal-action-btn action-logout"
                >
                  <svg
                    className="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1"
                    />
                  </svg>
                  {t.userModal.logout}
                </button>
              </div>
            </div>

            {/* 下部区域：Tapp 信息（约40%） */}
            <div className="user-modal-tapps">
              <div className="user-modal-tapps-header">
                <div className="user-modal-tapps-title">
                  <MyriadStoreIcon className="w-4 h-4" />
                  <span>Tapp</span>
                </div>
                {/* 已安装数 + 查看全部合并 */}
                <button
                  onClick={handleViewAllTapps}
                  className="user-modal-tapps-count-btn"
                  title={t.userModal.viewAllTapps || 'View all Tapps'}
                >
                  {tappsLoading ? (
                    <Spinner size="sm" color="primary" />
                  ) : (
                    <>
                      <span className="user-modal-tapps-number">
                        {tapps.length}
                      </span>
                      <span className="user-modal-tapps-label">
                        {t.userModal.installedApps || 'installed'}
                      </span>
                      <svg
                        className="user-modal-tapps-arrow"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M9 5l7 7-7 7"
                        />
                      </svg>
                    </>
                  )}
                </button>
              </div>

              {/* 最近使用的 Tapp - 始终渲染容器，避免高度跳变 */}
              <div className="user-modal-recent-tapps">
                <p className="user-modal-recent-label">
                  {t.userModal.recentlyUsed || 'Recently used'}
                </p>
                <div className="user-modal-recent-list">
                  {tappsLoading ? (
                    // 加载中显示骨架屏
                    <>
                      <div className="user-modal-tapp-item user-modal-tapp-skeleton" />
                      <div className="user-modal-tapp-item user-modal-tapp-skeleton" />
                      <div className="user-modal-tapp-item user-modal-tapp-skeleton" />
                    </>
                  ) : recentTapps.length > 0 ? (
                    recentTapps.map((tapp) => {
                      const tappName = resolveManifestText(tapp, locale).name
                      return (
                      <button
                        key={tapp.id}
                        onClick={() => handleTappClick(tapp.id)}
                        className="user-modal-tapp-item"
                      >
                        <div
                          className="user-modal-tapp-icon"
                          style={
                            tapp.themeColor
                              ? {
                                  background: `linear-gradient(135deg, ${tapp.themeColor}30 0%, ${tapp.themeColor}40 100%)`,
                                }
                              : undefined
                          }
                        >
                          <TappIcon
                            icon={tapp.icon}
                            iconSvg={tapp.iconSvg}
                            name={tappName}
                            sizeClass="w-4 h-4"
                            textSizeClass="text-base"
                            svgColor={tapp.themeColor || undefined}
                          />
                        </div>
                        <span className="user-modal-tapp-name">
                          {tappName}
                        </span>
                      </button>
                      )
                    })
                  ) : (
                    // 无最近使用时显示空状态
                    <span className="user-modal-recent-empty">
                      {t.userModal.noRecentTapps || 'No recent apps'}
                    </span>
                  )}
                </div>
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  )
}

export default UserModal
