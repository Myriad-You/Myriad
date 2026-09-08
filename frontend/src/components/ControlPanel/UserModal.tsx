import type { FC, Ref, SubmitEvent } from 'react'

import type { User } from '../../contexts/AuthContext'
import type {
  RecentTappItem,
  TappListItem,
} from '../../tapp/services/TappLifecycleApi'
import {
  FaGithub,
  LuChevronLeft,
  LuCrown,
  LuLink,
  LuUser,
  LuX,
  MyriadStoreIcon,
} from '@lib/icons'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useNavigate } from 'react-router-dom'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { isChannelPairingProvider } from '../channel/channelPairing'
import { ChannelPairingPanel } from '../channel/ChannelPairingPanel'
import { TappIconBadge } from '../../tapp/components/TappIconBadge'
import { getRecentTapps, listTapps } from '../../tapp/services/TappLifecycleApi'
import { resolveManifestText } from '../../tapp/utils/manifestLocale'
import { getTappIconStyle } from '../../tapp/utils/tappColors'
import { TAPP_LIST_PATH, tappRunPath } from '../../tapp/utils/tappPaths'
import { getCSRFToken } from '../../utils/csrf'
import { normalizeOAuthIconUrl } from '../../utils/oauthIcons'
import { showError } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import { Avatar } from '../Avatar'
import { AvatarSourcePicker } from '../AvatarSourcePicker'
import OAuthIconImage from '../OAuthIconImage'
import { ProfileTextSourcePicker } from '../ProfileTextSourcePicker'
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
  /** 可能为空：<Avatar> 负责本地兜底，后端不再编造 ui-avatars 地址 */
  avatar: string | null
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
 * 二级页顶栏：左返回、中当前页标题、右关闭。两颗圆钮同一套交互。
 */
function UserModalPageHead({
  title,
  backLabel,
  closeLabel,
  onBack,
  onClose,
  headRef,
}: {
  title: string
  backLabel: string
  closeLabel: string
  onBack: () => void
  onClose: () => void
  headRef?: Ref<HTMLDivElement>
}) {
  return (
    <div className="user-modal-page-head" ref={headRef}>
      <div className="user-modal-page-back">
        <button
          type="button"
          className="user-modal-chrome-hit"
          aria-label={backLabel}
          title={backLabel}
          onClick={onBack}
        >
          <LuChevronLeft aria-hidden />
        </button>
        <h3 className="user-modal-page-title">{title}</h3>
      </div>
      <button
        type="button"
        className="user-modal-chrome-hit"
        aria-label={closeLabel}
        title={closeLabel}
        onClick={onClose}
      >
        <LuX aria-hidden />
      </button>
    </div>
  )
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
  const { t, locale, format } = useI18n()
  const navigate = useNavigate()
  /** 自然高度测量目标：不受外层钉住 height / 滚动容器 max-height 约束 */
  const contentRef = useRef<HTMLDivElement>(null)
  const pageHeadRef = useRef<HTMLDivElement>(null)
  const modalRef = useRef<HTMLDivElement>(null)
  const [modalHeight, setModalHeight] = useState<number>()

  // 与 UserModal.css 的 max-height（85vh / 移动端 90vh）保持一致
  const getModalMaxHeightPx = useCallback(() => {
    if (typeof window === 'undefined') return Number.POSITIVE_INFINITY
    const mobile = window.matchMedia('(max-width: 640px)').matches
    return window.innerHeight * (mobile ? 0.9 : 0.85)
  }, [])

  // 与 UserModal.css --user-modal-min-height / min-height 保持一致（外层 shell 地板）
  const getModalMinHeightPx = useCallback(() => {
    if (typeof window === 'undefined') return 0
    const el = modalRef.current
    if (el) {
      // Computed min-height is already min(designedMin, maxvh) resolved to px
      const minH = Number.parseFloat(getComputedStyle(el).minHeight)
      if (Number.isFinite(minH) && minH > 0) return minH
      const raw = getComputedStyle(el).getPropertyValue('--user-modal-min-height').trim()
      const n = Number.parseFloat(raw)
      if (Number.isFinite(n) && n > 0) {
        if (raw.endsWith('rem')) {
          const rootFs =
            Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
          return n * rootFs
        }
        return n
      }
    }
    // Fallback before ref attach — keep in sync with UserModal.css
    const mobile = window.matchMedia('(max-width: 640px)').matches
    const rem = mobile ? 18 : 20
    const rootFs =
      Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
    return rem * rootFs
  }, [])

  // height = clamp(natural, effectiveMin, max) where effectiveMin = min(designedMin, max)
  const clampModalHeight = useCallback(
    (natural: number) => {
      const maxH = getModalMaxHeightPx()
      const minH = Math.min(getModalMinHeightPx(), maxH)
      return Math.max(minH, Math.min(natural, maxH))
    },
    [getModalMaxHeightPx, getModalMinHeightPx],
  )

  // .user-modal 全局 border-box：钉 height 时边框/内边距占额度，需加回壳层 chrome
  const getShellChromePx = useCallback(() => {
    const modalEl = modalRef.current
    if (!modalEl) return 2 // 与 CSS border: 1px 上下合计兜底
    const style = getComputedStyle(modalEl)
    return (
      (Number.parseFloat(style.borderTopWidth) || 0) +
      (Number.parseFloat(style.borderBottomWidth) || 0) +
      (Number.parseFloat(style.paddingTop) || 0) +
      (Number.parseFloat(style.paddingBottom) || 0)
    )
  }, [])

  // 内容自然高度 → 外层 shell 应钉的 height（含 chrome）。
  // 用 offsetHeight：不受 pre-animate 的 scale(0.92) 影响（getBoundingClientRect 会缩水）。
  const measureShellHeightFromContent = useCallback(() => {
    const el = contentRef.current
    if (!el) return 0
    const contentH = el.offsetHeight
    if (contentH <= 0) return 0
    // 顶栏已提出滚动层，量高时加回它的占位
    const headH = pageHeadRef.current?.offsetHeight ?? 0
    return Math.ceil(contentH + headH + getShellChromePx())
  }, [getShellChromePx])

  // 跟随内容自然高度，让主页/二级页切换（及内容加载）时的高度变化有过渡动画。
  // 测量 .user-modal-content（非滚动层），避免外层 height 钉住时 scrollHeight 卡在旧高度。
  // 外层高度 clamp 到 [min, max]；短内容时 shell 落在 min，内层 .user-modal-inner 填满。
  useEffect(() => {
    const el = contentRef.current
    if (!el) return
    const updateHeight = () => {
      const natural = measureShellHeightFromContent()
      if (natural <= 0) return
      setModalHeight(clampModalHeight(natural))
    }
    updateHeight()
    const observer = new ResizeObserver(updateHeight)
    observer.observe(el)
    const head = pageHeadRef.current
    if (head) observer.observe(head)
    window.addEventListener('resize', updateHeight)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', updateHeight)
    }
  }, [clampModalHeight, measureShellHeightFromContent, page])

  // 换页后等 DOM 绘制再量一次，确保从当前外层高度过渡到新内容 clamp 后高度
  useEffect(() => {
    const el = contentRef.current
    if (!el) return
    // 先钉住当前渲染高度，避免内容瞬间变矮时外层还没 transition 就塌掉
    const modalEl = modalRef.current
    if (modalEl) {
      // offsetHeight：布局高度（含 border），不受 scale 变换影响
      const current = modalEl.offsetHeight
      if (current > 0) {
        setModalHeight(clampModalHeight(current))
      }
    }
    let raf2 = 0
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        const natural = measureShellHeightFromContent()
        if (natural > 0) {
          setModalHeight(clampModalHeight(natural))
        }
      })
    })
    return () => {
      cancelAnimationFrame(raf1)
      cancelAnimationFrame(raf2)
    }
  }, [page, clampModalHeight, measureShellHeightFromContent])

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
        setOAuthProviders(
          (Array.isArray(data?.providers) ? data.providers : []).filter(
            (provider: { slug?: string }) =>
              !isChannelPairingProvider(provider.slug),
          ),
        )
      }
      if (identitiesRes.ok) {
        const data = await identitiesRes.json()
        const list = Array.isArray(data?.identities) ? data.identities : []
        setIdentities(
          list
            .filter(
              (row: Record<string, unknown>) =>
                !isChannelPairingProvider(
                  typeof row.provider === 'string' ? row.provider : '',
                ),
            )
            .map(
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
      setOAuthError(userFacingError(error, t.userModal.oauthLoadFailed))
    } finally {
      setOAuthLoading(false)
    }
  }, [t.userModal.oauthLoadFailed])

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
      .filter(
        (p): p is string =>
          !!p && p.trim().length > 0 && !isChannelPairingProvider(p),
      )
    if (fromLive.length > 0) {
      return [...new Set(fromLive.map((p) => p.trim().toLowerCase()))]
    }
    const fromUser = (user.identities ?? [])
      .map((i) => i.provider)
      .filter(
        (p): p is string =>
          !!p && p.trim().length > 0 && !isChannelPairingProvider(p),
      )
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
    const count = linkedProviders.length
    const onlyGithub = count === 1 && linkedProviders[0] === 'github'

    // 单个平台写名字；多个只标数量，避免徽章被平台名撑开。
    if (onlyGithub || (count === 0 && user.auth_provider === 'github')) {
      return { kind: 'github' as const, text: 'GitHub' }
    }

    if (count === 1) {
      return {
        kind: 'oauth' as const,
        text: providerDisplayName(linkedProviders[0]),
      }
    }

    if (count > 1) {
      return {
        kind: 'oauth' as const,
        text: format(t.userModal.linkedProviderCount, { count }),
      }
    }

    if (user.auth_provider === 'oidc') {
      return {
        kind: 'oauth' as const,
        text: t.userModal.oauthAccount || 'OAuth',
      }
    }

    return {
      kind: 'local' as const,
      text: t.userModal.localAccount || 'Local',
    }
  }, [
    user.auth_provider,
    linkedProviders,
    providerDisplayName,
    format,
    t.userModal,
  ])

  // 返回主页面，清空二级页面的临时状态
  const backToMain = () => {
    setPage('main')
    setOAuthError('')
    setPasswordError('')
  }

  const openProfileSource = () => {
    setPage('profileSource')
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
          userFacingError(
            (typeof body?.message === 'string' && body.message) ||
              (typeof body?.error === 'string' && body.error) ||
              `HTTP ${response.status}`,
            t.userModal.oauthUnbindFailed,
          ),
        )
        return
      }
      await loadOAuthBindings()
    } catch (error) {
      setOAuthError(userFacingError(error, t.userModal.networkError))
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
        showError(userFacingError(error, t.tapp.listLoadFailed))
      } finally {
        setTappsLoading(false)
      }
    }
    loadData()
  }, [t.tapp.listLoadFailed])

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
        setPasswordError(
          userFacingError(
            result.message || result.error || `HTTP ${response.status}`,
            t.errors.passwordChangeFailed,
          ),
        )
      }
    } catch (error) {
      setPasswordError(userFacingError(error, t.userModal.networkError))
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
        setPasswordError(
          userFacingError(
            result.message || result.error || `HTTP ${response.status}`,
            t.errors.passwordSetFailed,
          ),
        )
      }
    } catch (error) {
      setPasswordError(userFacingError(error, t.userModal.networkError))
    } finally {
      setPasswordSubmitting(false)
    }
  }

  const secondaryTitle =
    page === 'oauth'
      ? t.userModal.oauthBindings
      : page === 'profileSource'
        ? t.userModal.profileDisplaySourcesTitle
        : hasPassword
          ? t.userModal.changePassword
          : t.userModal.setPassword

  const goFromPanel = (path: string) => {
    onClose()
    if (onNavigateFromPanel) {
      onNavigateFromPanel(path)
    } else {
      navigate(path)
    }
  }

  const handleTappClick = (tappId: string) => {
    goFromPanel(tappRunPath(tappId))
  }

  const handleViewAllTapps = () => {
    goFromPanel(TAPP_LIST_PATH)
  }

  return (
    <div
      ref={modalRef}
      className={`user-modal ${canAnimate ? 'animate-in' : 'pre-animate'} ${isClosing ? 'closing' : ''}`}
      style={modalHeight !== undefined ? { height: modalHeight } : undefined}
    >
      {page === 'main' ? (
        <button
          type="button"
          onClick={onClose}
          className="user-modal-chrome-hit user-modal-close-float"
          aria-label={t.common.close}
          title={t.common.close}
        >
          <LuX aria-hidden />
        </button>
      ) : (
        <UserModalPageHead
          title={secondaryTitle}
          backLabel={t.common.back}
          closeLabel={t.common.close}
          onBack={backToMain}
          onClose={onClose}
          headRef={pageHeadRef}
        />
      )}

      {/* 滚动容器：外层 height 封顶时在此滚动；不参与自然高度测量 */}
      <div className="user-modal-inner">
        {/* 测量目标：height auto，不受外层钉高影响，供 ResizeObserver 读自然高度 */}
        <div className="user-modal-content" ref={contentRef}>
        {page !== 'main' ? (
          /* 二级页面正文：顶栏已提出滚动层 */
          <div className="user-modal-page">
            {page === 'profileSource' ? (
              <div className="user-modal-page-body">
                {/* 两套来源分开管理：切头像不改文案，切文案不改头像 */}
                <section className="user-modal-profile-source-section">
                  <h4 className="user-modal-profile-source-section-title">
                    {t.userModal.profileSourceTitle}
                  </h4>
                  <AvatarSourcePicker
                    onApplied={() => {
                      void loadOAuthBindings()
                      onProfileApplied?.()
                    }}
                  />
                </section>
                <section className="user-modal-profile-source-section">
                  <h4 className="user-modal-profile-source-section-title">
                    {t.userModal.profileTextSourceTitle}
                  </h4>
                  <ProfileTextSourcePicker
                    onApplied={() => {
                      onProfileApplied?.()
                    }}
                  />
                </section>
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
                          !isChannelPairingProvider(identity.provider) &&
                          !oauthProviders.some(
                            (p) =>
                              p.slug.trim().toLowerCase() ===
                              identity.provider.trim().toLowerCase(),
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
                <div className="user-modal-qq-pairing">
                  <ChannelPairingPanel channel="qq" />
                </div>
                <div className="user-modal-qq-pairing">
                  <ChannelPairingPanel channel="telegram" />
                </div>
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

              {/* 头像：点击选择画像源。
                  这里不再要求「已绑定 OAuth」—— 账号本身就是一个可选来源，
                  站长还多出平台画像，所以任何人都有得选。 */}
              <div className="user-modal-avatar-wrapper">
                <button
                  type="button"
                  className="user-modal-avatar-btn"
                  onClick={openProfileSource}
                  title={t.userModal.profileSourceTitle}
                  aria-label={t.userModal.profileSourceTitle}
                >
                  <Avatar
                    src={userInfo.avatar}
                    name={userInfo.name}
                    className="user-modal-avatar-lg"
                  />
                  <span className="user-modal-avatar-edit-hint">
                    {t.userModal.profileSourceAvatarHint}
                  </span>
                </button>
                {/* 在线状态指示器 */}
                <div className="user-modal-online-dot" />
              </div>

              {/* 用户名和角色 */}
              <div className="user-modal-identity">
                <h3 className="user-modal-username">{userInfo.name}</h3>
                {/* 真实本地账号名：面板上方显示的是平台昵称（站点形象），
                    和登录用的账号不是一回事，这里明确标出来 */}
                {user.username && user.username !== userInfo.name && (
                  <p className="user-modal-account-name">@{user.username}</p>
                )}
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
                  {/* 账户类型：有绑定时只标平台，不再单独写「本地 +」 */}
                  {accountBadge.kind === 'github' ? (
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
                        <TappIconBadge
                          icon={tapp.icon}
                          iconSvg={tapp.iconSvg}
                          name={tappName}
                          id={tapp.id}
                          themeColor={tapp.themeColor}
                          iconStyle={getTappIconStyle({
                            icon: tapp.icon,
                            iconSvg: tapp.iconSvg,
                            themeColor: tapp.themeColor,
                            id: tapp.id,
                          })}
                          shellClassName="user-modal-tapp-icon"
                          glyphSizeClass="w-4 h-4"
                          glyphTextClass="text-base"
                        />
                        <span className="user-modal-tapp-name">
                          {tappName}
                        </span>
                      </button>
                      )
                    })
                  ) : (
                    // 无最近使用时显示空状态
                    <span className="user-modal-recent-empty">
                      {t.userModal.noRecentTapps}
                    </span>
                  )}
                </div>
              </div>
            </div>
          </>
        )}
        </div>
      </div>
    </div>
  )
}

export default UserModal
