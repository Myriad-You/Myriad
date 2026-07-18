import type { FC, SubmitEvent } from 'react'

import type { User } from '../../contexts/AuthContext'
import type {
  RecentTappItem,
  TappListItem,
} from '../../tapp/services/TappLifecycleApi'
import { FaGithub, LuCrown, LuLink, LuUser, MyriadStoreIcon } from '@lib/icons'
import { useCallback, useEffect, useRef, useState } from 'react'

import { useNavigate } from 'react-router-dom'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { TappIcon } from '../../tapp/components/TappIcon'
import { getRecentTapps, listTapps } from '../../tapp/services/TappLifecycleApi'
import { getCSRFToken } from '../../utils/csrf'
import { normalizeOAuthIconUrl } from '../../utils/oauthIcons'
import OAuthIconImage from '../OAuthIconImage'
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
}) => {
  const [page, setPage] = useState<'main' | 'oauth' | 'password'>('main')
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
  const { t } = useI18n()
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
        setIdentities(Array.isArray(data?.identities) ? data.identities : [])
      }
    } catch (error) {
      console.error('Failed to load OAuth bindings:', error)
    } finally {
      setOAuthLoading(false)
    }
  }, [])

  // 打开二级页面时才加载
  useEffect(() => {
    if (page === 'oauth') {
      loadOAuthBindings()
    }
  }, [page, loadOAuthBindings])

  // 返回主页面，清空二级页面的临时状态
  const backToMain = () => {
    setPage('main')
    setOAuthError('')
    setPasswordError('')
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

  const handleTappClick = (tappId: string) => {
    onClose()
    navigate(`/tapp/run/${tappId}`)
  }

  const handleViewAllTapps = () => {
    onClose()
    navigate('/tapp')
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
                  : hasPassword
                    ? t.userModal.changePassword
                    : t.userModal.setPassword}
              </h3>
            </div>

            {page === 'oauth' ? (
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
                          <span className="user-modal-oauth-provider">
                            {provider.slug === 'github' ? (
                              <FaGithub size={16} aria-hidden />
                            ) : iconSrc ? (
                              <OAuthIconImage src={iconSrc} size={16} />
                            ) : (
                              <LuUser size={16} aria-hidden />
                            )}
                            <span className="user-modal-oauth-name">
                              {provider.display_name}
                            </span>
                            {bound && (
                              <span className="user-modal-oauth-username">
                                {bound.provider_username ||
                                  t.userModal.githubLinked}
                              </span>
                            )}
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
                          <span className="user-modal-oauth-provider">
                            {identity.provider === 'github' ? (
                              <FaGithub size={16} aria-hidden />
                            ) : (
                              <LuUser size={16} aria-hidden />
                            )}
                            <span className="user-modal-oauth-name">
                              {identity.provider}
                            </span>
                            <span className="user-modal-oauth-username">
                              {identity.provider_username || ''}
                            </span>
                            <span className="user-modal-oauth-disabled-tag">
                              {t.userModal.oauthNotConfigured}
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

              {/* 头像 - 居中 */}
              <div className="user-modal-avatar-wrapper">
                <img
                  src={userInfo.avatar}
                  alt={userInfo.name}
                  className="user-modal-avatar-lg"
                  onError={(e) => {
                    e.currentTarget.src = `https://ui-avatars.com/api/?name=${encodeURIComponent(userInfo.name)}&size=128&background=6366f1&color=fff`
                  }}
                />
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
                  {/* 账户类型徽章 - 根据数据库记录正确判断 */}
                  {/* 混合账户：auth_provider='local' + linked_github_id 存在 = 本地管理员绑定了 GitHub */}
                  {user.auth_provider === 'local' && user.linked_github_id ? (
                    // 混合账户（本地+GitHub绑定）- 显示特殊的混合标识
                    <span className="user-modal-badge badge-hybrid">
                      <svg
                        className="w-3.5 h-3.5"
                        fill="currentColor"
                        viewBox="0 0 20 20"
                      >
                        <path
                          fillRule="evenodd"
                          d="M10 0C4.477 0 0 4.484 0 10.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0110 4.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.203 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.942.359.31.678.921.678 1.856 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0020 10.017C20 4.484 15.522 0 10 0z"
                          clipRule="evenodd"
                        />
                      </svg>
                      {t.userModal.hybridAccount || 'Local + GitHub'}
                    </span>
                  ) : user.auth_provider === 'github' ? (
                    // 纯 GitHub 用户
                    <span className="user-modal-badge badge-github">
                      <svg
                        className="w-3.5 h-3.5"
                        fill="currentColor"
                        viewBox="0 0 20 20"
                      >
                        <path
                          fillRule="evenodd"
                          d="M10 0C4.477 0 0 4.484 0 10.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0110 4.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.203 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.942.359.31.678.921.678 1.856 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0020 10.017C20 4.484 15.522 0 10 0z"
                          clipRule="evenodd"
                        />
                      </svg>
                      GitHub
                    </span>
                  ) : user.auth_provider === 'oidc' ? (
                    // OIDC 提供方（Google / Microsoft 等）创建的账户
                    <span className="user-modal-badge badge-oauth">
                      <LuLink size={13} className="inline" />
                      {t.userModal.oauthAccount}
                    </span>
                  ) : (
                    // 纯本地用户（未绑定 GitHub）
                    <span className="user-modal-badge badge-local">
                      <svg
                        className="w-3.5 h-3.5"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"
                        />
                      </svg>
                      Local
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
                    <span className="user-modal-tapps-loading" />
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
                    recentTapps.map((tapp) => (
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
                            name={tapp.name}
                            sizeClass="w-4 h-4"
                            textSizeClass="text-base"
                            svgColor={tapp.themeColor || undefined}
                          />
                        </div>
                        <span className="user-modal-tapp-name">
                          {tapp.name}
                        </span>
                      </button>
                    ))
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
