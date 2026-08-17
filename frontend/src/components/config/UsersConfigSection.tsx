/**
 * 设置页「用户管理」区块
 *
 * 基于 ManagedList：
 * - 搜索 / 角色 / 在线筛选（折叠查询栏）
 * - 创建用户（折叠表单）
 * - 列表行「详情」走选项指南同款浮窗；浮窗内 tab 切换（账号+活动 / 来源 / 应用）
 */

import type {
  AdminUser,
  AdminUserIdentity,
  AdminUserUpdate,
} from '../../services/adminUsersApi'
import type {
  ManagedListAction,
  ManagedListFilterOption,
  ManagedListItem,
  ManagedListStat,
} from '../settings'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import {
  FaGithub,
  FaPlus,
  FaSearch,
  FaSyncAlt,
  FaUser,
  LuClock,
  LuPackage,
  LuShieldCheck,
  LuTrash2,
  LuUser,
} from '../../lib/icons'
import adminUsersApi from '../../services/adminUsersApi'
import { TappIconBadge } from '../../tapp/components/TappIconBadge'
import { messageForAdminUserError } from '../../utils/authErrorMessages'
import { getOAuthIconAsset } from '../../utils/oauthIcons'
import { Avatar } from '../Avatar'
import { AvatarSourcePicker } from '../AvatarSourcePicker'
import OAuthIconImage from '../OAuthIconImage'
import { ProfileTextSourcePicker } from '../ProfileTextSourcePicker'
import {
  guideDomProps,
  InputItem,
  ManagedList,
  SegmentedControl,
  SettingsButton,
  SettingSection,
  SettingTitleGuideEntry,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import './UsersConfigSection.css'

const KNOWN_PROVIDER_ICONS: Record<string, string> = {
  google: getOAuthIconAsset('google'),
  microsoft: getOAuthIconAsset('microsoft'),
  gitlab: getOAuthIconAsset('gitlab'),
  discord: getOAuthIconAsset('discord'),
  authentik: getOAuthIconAsset('authentik'),
  keycloak: getOAuthIconAsset('keycloak'),
  auth0: getOAuthIconAsset('auth0'),
}

type RoleFilter = 'all' | 'admin' | 'user'
type OnlineFilter = 'all' | 'online' | 'offline'

interface UsersConfigSectionProps {
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
  onMessage?: (message: string, type?: 'success' | 'error' | 'info') => void
  allowRegister: boolean
  allowRegisterLoading?: boolean
  onAllowRegisterChange: (allow: boolean) => void
  /**
   * Private Tapp install cleanup preset:
   * - '7' / '14': prune after that many days inactive
   * - 'logout': wipe on logout
   * - other numeric string (e.g. '30'): preserve custom inactivity days from API
   */
  privateTappInstallPreset: string
  privateTappInstallLoading?: boolean
  onPrivateTappInstallPresetChange: (preset: string) => void
}

const ONLINE_ICON_SIZE = 14

function userMatchesQuery(user: AdminUser, query: string): boolean {
  if (!query) return true
  const q = query.toLowerCase()
  const fields: Array<string | null | undefined> = [
    user.username,
    user.display_name,
    user.email,
  ]
  for (const identity of user.identities) {
    fields.push(
      identity.provider_username,
      identity.email,
      identity.provider,
    )
  }
  return fields.some((value) => value?.toLowerCase().includes(q))
}

function DetailField({
  label,
  value,
}: {
  label: string
  value: React.ReactNode
}) {
  return (
    <div className="users-expand-field">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  )
}

type UserDetailTab = 'account' | 'avatar' | 'text' | 'tapps'

function UserDetailPanel({
  accountLabel,
  avatarLabel,
  textLabel,
  tappsLabel,
  tabsAriaLabel,
  account,
  avatar,
  text,
  tapps,
}: {
  accountLabel: string
  avatarLabel: string
  textLabel: string
  tappsLabel: string
  tabsAriaLabel: string
  account: React.ReactNode
  avatar: React.ReactNode
  text: React.ReactNode
  tapps: React.ReactNode
}) {
  const [tab, setTab] = useState<UserDetailTab>('account')
  return (
    <div className="users-expand-stack">
      <SegmentedControl
        size="sm"
        columns="auto"
        ariaLabel={tabsAriaLabel}
        value={tab}
        onChange={setTab}
        className="users-detail-tabs"
        options={[
          { value: 'account', label: accountLabel },
          { value: 'avatar', label: avatarLabel },
          { value: 'text', label: textLabel },
          { value: 'tapps', label: tappsLabel },
        ]}
      />
      <div className={`users-expand-extra${tab === 'account' ? ' is-account' : ' is-pane'}`}>
        {tab === 'account'
          ? account
          : tab === 'avatar'
            ? avatar
            : tab === 'text'
              ? text
              : tapps}
      </div>
    </div>
  )
}

function ProviderBadge({ identity }: { identity: AdminUserIdentity }) {
  const { t } = useI18n()
  const iconSrc = KNOWN_PROVIDER_ICONS[identity.provider.toLowerCase()]
  const label = identity.provider_username || identity.provider
  return (
    <span
      className={`users-provider-badge${identity.is_primary ? ' primary' : ''}`}
      title={
        identity.is_primary
          ? `${identity.provider} · ${t.config.usersPrimaryIdentity}`
          : identity.provider
      }
    >
      {identity.provider === 'github' ? (
        <FaGithub size={ONLINE_ICON_SIZE} aria-hidden />
      ) : iconSrc ? (
        <OAuthIconImage src={iconSrc} size={ONLINE_ICON_SIZE} />
      ) : (
        <LuUser size={ONLINE_ICON_SIZE} aria-hidden />
      )}
      <span className="users-provider-name">{label}</span>
    </span>
  )
}

export const UsersConfigSection: React.FC<UsersConfigSectionProps> = ({
  title,
  icon,
  description,
  sectionId,
  onMessage,
  allowRegister,
  allowRegisterLoading = false,
  onAllowRegisterChange,
  privateTappInstallPreset,
  privateTappInstallLoading = false,
  onPrivateTappInstallPresetChange,
}) => {
  const { t } = useI18n()
  const c = t.config
  const { catalog: g, renderGuide, bindGuide } = useSettingGuide()
  const { user: currentUser } = useAuth()
  const [users, setUsers] = useState<AdminUser[]>([])
  const [loading, setLoading] = useState(true)
  const [detailUserId, setDetailUserId] = useState<number | null>(null)
  const [detail, setDetail] = useState<AdminUser | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [createDraft, setCreateDraft] = useState({
    username: '',
    password: '',
    email: '',
    is_admin: false,
  })
  const [formOpen, setFormOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [rowBusyId, setRowBusyId] = useState<number | null>(null)
  const [searchQuery, setSearchQuery] = useState('')
  const [roleFilter, setRoleFilter] = useState<RoleFilter>('all')
  const [onlineFilter, setOnlineFilter] = useState<OnlineFilter>('all')

  const notifyError = useCallback(
    (error: unknown, fallback: string) => {
      const message = messageForAdminUserError(error, t, fallback)
      onMessage?.(message, 'error')
    },
    [onMessage, t],
  )

  const loadUsers = useCallback(async () => {
    setLoading(true)
    try {
      setUsers(await adminUsersApi.list())
    } catch (error) {
      notifyError(error, c.usersLoadError)
    } finally {
      setLoading(false)
    }
  }, [notifyError, c.usersLoadError])

  useEffect(() => {
    void loadUsers()
  }, [loadUsers])

  const hasActiveFilter =
    searchQuery.trim().length > 0 ||
    roleFilter !== 'all' ||
    onlineFilter !== 'all'

  const filteredUsers = useMemo(() => {
    const query = searchQuery.trim()
    return users.filter((user) => {
      if (!userMatchesQuery(user, query)) return false
      if (roleFilter === 'admin' && !user.is_admin) return false
      if (roleFilter === 'user' && user.is_admin) return false
      if (onlineFilter === 'online' && !user.online) return false
      if (onlineFilter === 'offline' && user.online) return false
      return true
    })
  }, [users, searchQuery, roleFilter, onlineFilter])

  const isPrimaryAdmin = Boolean(
    currentUser?.is_owner ||
      users.find((u) => u.id === currentUser?.id)?.is_owner,
  )

  const canDeleteUser = useCallback(
    (target: AdminUser) =>
      target.id !== currentUser?.id &&
      !target.is_owner &&
      (isPrimaryAdmin || !target.is_admin),
    [currentUser?.id, isPrimaryAdmin],
  )

  const formatDateTime = useCallback(
    (value: string | null) => {
      if (!value) return c.usersNever
      const date = new Date(value)
      return Number.isNaN(date.getTime()) ? c.usersNever : date.toLocaleString()
    },
    [c.usersNever],
  )

  const formatOnlineTotal = useCallback(
    (seconds: number) => {
      const hours = Math.floor(seconds / 3600)
      const minutes = Math.floor((seconds % 3600) / 60)
      if (hours <= 0 && minutes <= 0) return `0 ${c.usersMinutes}`
      const parts: string[] = []
      if (hours > 0) parts.push(`${hours} ${c.usersHours}`)
      if (minutes > 0) parts.push(`${minutes} ${c.usersMinutes}`)
      return parts.join(' ')
    },
    [c.usersHours, c.usersMinutes],
  )

  const applyUpdated = useCallback((updated: AdminUser) => {
    setUsers((prev) => prev.map((u) => (u.id === updated.id ? updated : u)))
    setDetail((prev) => (prev?.id === updated.id ? updated : prev))
  }, [])

  const ensureDetail = useCallback(
    async (user: AdminUser) => {
      if (detail?.id === user.id) return
      setDetailUserId(user.id)
      setDetail(null)
      setDetailLoading(true)
      try {
        setDetail(await adminUsersApi.get(user.id))
      } catch (error) {
        notifyError(error, c.usersLoadError)
      } finally {
        setDetailLoading(false)
      }
    },
    [detail?.id, notifyError, c.usersLoadError],
  )

  const runUpdate = useCallback(
    async (userId: number, update: AdminUserUpdate) => {
      setBusy(true)
      setRowBusyId(userId)
      try {
        const { user: updated } = await adminUsersApi.update(userId, update)
        applyUpdated(updated)
        return { ok: true as const, updated }
      } catch (error) {
        notifyError(error, c.usersActionError)
        return { ok: false as const }
      } finally {
        setBusy(false)
        setRowBusyId(null)
      }
    },
    [applyUpdated, notifyError, c.usersActionError],
  )

  const handleToggleAdmin = useCallback(
    async (user: AdminUser) => {
      if (user.is_owner && user.is_admin) {
        onMessage?.(c.usersErrorCannotDemoteOwner, 'error')
        return
      }
      if (user.is_admin && !window.confirm(c.usersRevokeAdminConfirm)) return
      const promoting = !user.is_admin
      const result = await runUpdate(user.id, { is_admin: !user.is_admin })
      if (!result.ok) return
      onMessage?.(
        promoting ? c.usersPromoteReLoginNotice : c.usersDemoteImmediateNotice,
        'info',
      )
    },
    [onMessage, runUpdate, c],
  )

  const handleToggleLocalLogin = useCallback(
    async (user: AdminUser) => {
      await runUpdate(user.id, {
        local_login_disabled: !user.local_login_disabled,
      })
    },
    [runUpdate],
  )

  const handleToggleTappInstall = useCallback(
    async (user: AdminUser) => {
      await runUpdate(user.id, {
        tapp_install_disabled: !user.tapp_install_disabled,
      })
    },
    [runUpdate],
  )

  const handleUninstallTapp = useCallback(
    async (user: AdminUser, tapp: { tapp_id: string; name: string }) => {
      if (
        !window.confirm(
          c.usersUninstallTappConfirm.replace('{name}', tapp.name || tapp.tapp_id),
        )
      ) {
        return
      }
      setBusy(true)
      setRowBusyId(user.id)
      try {
        applyUpdated(await adminUsersApi.uninstallTapp(user.id, tapp.tapp_id))
      } catch (error) {
        notifyError(error, c.usersActionError)
      } finally {
        setBusy(false)
        setRowBusyId(null)
      }
    },
    [applyUpdated, notifyError, c],
  )

  const handleUnlinkIdentity = useCallback(
    async (user: AdminUser, identity: AdminUserIdentity) => {
      if (!window.confirm(c.usersUnlinkConfirm)) return
      setBusy(true)
      setRowBusyId(user.id)
      try {
        applyUpdated(await adminUsersApi.unlinkIdentity(user.id, identity.id))
      } catch (error) {
        notifyError(error, c.usersActionError)
      } finally {
        setBusy(false)
        setRowBusyId(null)
      }
    },
    [applyUpdated, notifyError, c],
  )

  const handleDeleteUser = useCallback(
    async (user: AdminUser) => {
      if (user.id === currentUser?.id) return
      if (!window.confirm(c.usersDeleteConfirm)) return
      setBusy(true)
      setRowBusyId(user.id)
      try {
        await adminUsersApi.delete(user.id)
        setUsers((prev) => prev.filter((u) => u.id !== user.id))
        if (detail?.id === user.id || detailUserId === user.id) {
          setDetailUserId(null)
          setDetail(null)
        }
        onMessage?.(c.usersDeleteSuccess, 'success')
      } catch (error) {
        notifyError(error, c.usersActionError)
      } finally {
        setBusy(false)
        setRowBusyId(null)
      }
    },
    [currentUser?.id, detail?.id, detailUserId, notifyError, onMessage, c],
  )

  const handleCreate = useCallback(async () => {
    if (!createDraft.username.trim() || !createDraft.password) return
    setBusy(true)
    try {
      const createAsAdmin = isPrimaryAdmin && createDraft.is_admin
      await adminUsersApi.create({
        username: createDraft.username.trim(),
        password: createDraft.password,
        email: createDraft.email.trim() || undefined,
        is_admin: createAsAdmin,
      })
      setFormOpen(false)
      setCreateDraft({ username: '', password: '', email: '', is_admin: false })
      await loadUsers()
      if (createAsAdmin) {
        onMessage?.(c.usersPromoteReLoginNotice, 'info')
      }
    } catch (error) {
      notifyError(error, c.usersActionError)
    } finally {
      setBusy(false)
    }
  }, [createDraft, isPrimaryAdmin, loadUsers, notifyError, onMessage, c])

  const listStats = useMemo((): ManagedListStat[] => {
    let admins = 0
    let online = 0
    for (const u of users) {
      if (u.is_admin) admins++
      if (u.online) online++
    }
    // Metrics first; switch chips (CheckboxCard) always after data.
    return [
      {
        key: 'total',
        label: c.usersFilterAll,
        value: users.length,
      },
      {
        key: 'admin',
        label: c.usersRoleAdmin,
        value: admins,
      },
      {
        key: 'online',
        label: c.usersOnline,
        value: online,
      },
      {
        key: 'allow-register',
        kind: 'switch',
        label: c.allowRegisterTitle,
        description: c.allowRegisterDesc,
        icon: <FaUser aria-hidden />,
        checked: allowRegister,
        onChange: onAllowRegisterChange,
        disabled: allowRegisterLoading,
        loading: allowRegisterLoading,
        title: c.allowRegisterDesc,
        guide: renderGuide(g.users.allowLocalRegister),
        guidePath: 'users.allowLocalRegister',
      },
      {
        key: 'private-tapp-cleanup',
        kind: 'choice',
        label: c.privateTappInstallCleanupTitle,
        description: c.privateTappInstallCleanupDesc,
        icon: <LuTrash2 aria-hidden size={14} />,
        value: privateTappInstallPreset,
        options: (() => {
          const base = [
            { value: '7', label: c.privateTappInstallPreset7Short },
            { value: '14', label: c.privateTappInstallPreset14Short },
          ]
          // Preserve non-preset inactivity days (API allows 1–365) instead of
          // silently showing them as 14.
          if (
            privateTappInstallPreset !== 'logout' &&
            privateTappInstallPreset !== '7' &&
            privateTappInstallPreset !== '14'
          ) {
            const days = Number(privateTappInstallPreset)
            if (Number.isFinite(days) && days >= 1) {
              base.push({
                value: String(days),
                // Dedicated {n} template — avoid brittle replace(/7/) on i18n
                label: c.privateTappInstallPresetNShort.replace(
                  '{n}',
                  String(days),
                ),
              })
            }
          }
          base.push({
            value: 'logout',
            label: c.privateTappInstallPresetLogoutShort,
          })
          return base
        })(),
        onChange: (v) => onPrivateTappInstallPresetChange(String(v)),
        disabled: privateTappInstallLoading,
        loading: privateTappInstallLoading,
        className: 'users-private-tapp-choice',
      },
    ]
  }, [
    users,
    allowRegister,
    allowRegisterLoading,
    onAllowRegisterChange,
    privateTappInstallPreset,
    privateTappInstallLoading,
    onPrivateTappInstallPresetChange,
    c.allowRegisterTitle,
    c.allowRegisterDesc,
    c.privateTappInstallCleanupTitle,
    c.privateTappInstallCleanupDesc,
    c.privateTappInstallPreset7Short,
    c.privateTappInstallPreset14Short,
    c.privateTappInstallPresetLogoutShort,
    c.usersFilterAll,
    c.usersRoleAdmin,
    c.usersOnline,
    g.users.allowLocalRegister,
    renderGuide,
  ])

  const roleFilterOptions = useMemo((): ManagedListFilterOption[] => {
    let admin = 0
    let regular = 0
    for (const u of users) {
      if (u.is_admin) admin++
      else regular++
    }
    return [
      { key: 'all', label: c.usersFilterAll, count: users.length },
      { key: 'admin', label: c.usersRoleAdmin, count: admin },
      { key: 'user', label: c.usersRoleUser, count: regular },
    ]
  }, [users, c.usersFilterAll, c.usersRoleAdmin, c.usersRoleUser])

  const onlineFilterOptions = useMemo((): ManagedListFilterOption[] => {
    let online = 0
    let offline = 0
    for (const u of users) {
      if (u.online) online++
      else offline++
    }
    return [
      { key: 'all', label: c.usersFilterAll, count: users.length },
      { key: 'online', label: c.usersOnline, count: online },
      { key: 'offline', label: c.usersOffline, count: offline },
    ]
  }, [users, c.usersFilterAll, c.usersOnline, c.usersOffline])

  const toolbar = useMemo((): ManagedListAction[] => {
    return [
      {
        key: 'refresh',
        label: c.usersRefresh,
        description: c.usersRefreshDesc,
        icon: <FaSyncAlt aria-hidden />,
        onClick: () => void loadUsers(),
        loading,
        disabled: busy,
        variant: 'secondary',
      },
    ]
  }, [c.usersRefresh, c.usersRefreshDesc, loadUsers, loading, busy])

  const renderExpandContent = useCallback(
    (user: AdminUser) => {
      const shown = detail?.id === user.id ? detail : user
      const loadingDetail = detailLoading && detailUserId === user.id && !detail

      if (loadingDetail) {
        return (
          <div className="users-detail-loading" role="status">
            …
          </div>
        )
      }

      const passwordValue = shown.has_password
        ? shown.local_login_disabled
          ? `${c.usersPasswordSet} · ${c.usersLocalLoginDisabled}`
          : c.usersPasswordSet
        : c.usersPasswordUnset

      const identityBlock =
        shown.identities.length === 0 ? (
          <span className="users-muted">{c.usersNoIdentities}</span>
        ) : (
          <div className="users-identities">
            {shown.identities.map((identity) => (
              <span key={identity.id} className="users-identity-item">
                <ProviderBadge identity={identity} />
                <SettingsButton
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => void handleUnlinkIdentity(shown, identity)}
                >
                  {c.usersUnlinkIdentity}
                </SettingsButton>
              </span>
            ))}
          </div>
        )

      const tappBlock =
        !shown.tapps || shown.tapps.length === 0 ? (
          <span className="users-muted">{c.usersNoTapps}</span>
        ) : (
          <ul className="users-tapp-grid">
            {shown.tapps.map((tapp) => (
              <li key={tapp.tapp_id} className="users-tapp-tile">
                <TappIconBadge
                  icon={tapp.icon ?? undefined}
                  iconSvg={tapp.icon_svg ?? undefined}
                  iconShell={tapp.icon_shell ?? undefined}
                  themeColor={tapp.theme_color ?? undefined}
                  name={tapp.name}
                  id={tapp.tapp_id}
                  shellClassName="users-tapp-tile-icon"
                  glyphSizeClass="w-4 h-4"
                  glyphTextClass="text-sm"
                />
                <span className="users-tapp-tile-text">
                  <span className="users-tapp-name">{tapp.name}</span>
                  <span className="users-muted">
                    v{tapp.version} · {tapp.status}
                  </span>
                </span>
                <SettingsButton
                  size="sm"
                  variant="danger"
                  block
                  disabled={busy}
                  onClick={() => void handleUninstallTapp(shown, tapp)}
                >
                  {c.usersUninstallTapp}
                </SettingsButton>
              </li>
            ))}
          </ul>
        )

      const actions: ManagedListAction[] = [
        {
          key: 'local-login',
          label: shown.local_login_disabled
            ? c.usersEnableLocalLogin
            : c.usersDisableLocalLogin,
          onClick: () => void handleToggleLocalLogin(shown),
          disabled:
            busy ||
            (!shown.local_login_disabled && shown.identities.length === 0),
          title:
            !shown.local_login_disabled && shown.identities.length === 0
              ? c.usersLocalLoginRequiresOAuth
              : undefined,
          variant: 'secondary',
        },
      ]

      if (isPrimaryAdmin && !shown.is_owner) {
        actions.push({
          key: 'admin',
          label: shown.is_admin ? c.usersRevokeAdmin : c.usersMakeAdmin,
          onClick: () => void handleToggleAdmin(shown),
          disabled: busy || shown.id === currentUser?.id,
          variant: shown.is_admin ? 'danger' : 'secondary',
        })
      }

      if (canDeleteUser(shown)) {
        actions.push({
          key: 'delete',
          label: c.usersDelete,
          onClick: () => void handleDeleteUser(shown),
          disabled: busy,
          variant: 'danger',
        })
      }

      return (
        <UserDetailPanel
          accountLabel={c.usersAccountSection}
          avatarLabel={t.userModal.profileSourceTitle}
          textLabel={t.userModal.profileTextSourceTitle}
          tappsLabel={c.usersInstalledTapps}
          tabsAriaLabel={c.usersDetail}
          account={
            <div className="users-account-sheet">
              <div className="users-account-facts">
                <DetailField
                  label={c.usersEmail}
                  value={shown.email || '—'}
                />
                <DetailField
                  label={c.usersLocalPassword}
                  value={passwordValue}
                />
              </div>
              <div className="users-account-bindings">
                <span className="users-account-kicker">
                  {c.usersOAuthIdentities}
                </span>
                {identityBlock}
              </div>
              <div className="users-account-stats">
                <DetailField
                  label={c.usersLastLogin}
                  value={formatDateTime(shown.last_login_at)}
                />
                <DetailField
                  label={c.usersLastSeen}
                  value={formatDateTime(shown.last_seen_at)}
                />
                <DetailField
                  label={c.usersCreatedAt}
                  value={formatDateTime(shown.created_at)}
                />
                <DetailField
                  label={c.usersOnlineTotal}
                  value={formatOnlineTotal(shown.online_seconds)}
                />
              </div>
              {actions.length > 0 ? (
                <div className="users-expand-actions">
                  {actions.map((a) => (
                    <SettingsButton
                      key={a.key}
                      size="sm"
                      variant={a.variant ?? 'secondary'}
                      disabled={a.disabled}
                      loading={a.loading}
                      title={a.title}
                      onClick={a.onClick}
                    >
                      {a.label}
                    </SettingsButton>
                  ))}
                </div>
              ) : null}
            </div>
          }
          avatar={
            <div className="users-expand-pane users-expand-pane--full">
              {/* 管理员替他人换头像来源；后端只接受该用户已有的来源，
                  塞不进任意 URL，且会记一条审计日志 */}
              <AvatarSourcePicker
                userId={shown.id}
                targetIsSiteOwner={shown.is_owner}
                onApplied={() => void loadUsers()}
              />
            </div>
          }
          text={
            <div className="users-expand-pane users-expand-pane--full">
              {/* 名称/简介来源与头像独立；同站合并仅影响列表展示 */}
              <ProfileTextSourcePicker
                userId={shown.id}
                targetIsSiteOwner={shown.is_owner}
                onApplied={() => void loadUsers()}
              />
            </div>
          }
          tapps={
            <div className="users-expand-pane users-expand-pane--full users-tapp-manage">
              {!shown.is_owner ? (
                <div
                  className={`users-tapp-policy${shown.tapp_install_disabled ? ' is-blocked' : ''}`}
                >
                  {shown.tapp_install_disabled ? (
                    <span className="users-account-kicker">
                      {c.usersTappInstallDisabled}
                    </span>
                  ) : null}
                  <SettingsButton
                    size="sm"
                    variant={shown.tapp_install_disabled ? 'secondary' : 'danger'}
                    disabled={busy}
                    onClick={() => void handleToggleTappInstall(shown)}
                  >
                    {shown.tapp_install_disabled
                      ? c.usersEnableTappInstall
                      : c.usersDisableTappInstall}
                  </SettingsButton>
                </div>
              ) : null}
              {tappBlock}
            </div>
          }
        />
      )
    },
    [
      detail,
      detailLoading,
      detailUserId,
      busy,
      c,
      t.userModal.profileSourceTitle,
      t.userModal.profileTextSourceTitle,
      loadUsers,
      formatDateTime,
      formatOnlineTotal,
      handleUnlinkIdentity,
      handleToggleLocalLogin,
      handleToggleTappInstall,
      handleUninstallTapp,
      handleToggleAdmin,
      handleDeleteUser,
      isPrimaryAdmin,
      currentUser?.id,
      canDeleteUser,
    ],
  )

  const listItems = useMemo((): ManagedListItem[] => {
    return filteredUsers.map((user) => {
      const isSelf = user.id === currentUser?.id
      const roleLabel = user.is_owner
        ? c.usersRoleOwner
        : user.is_admin
          ? c.usersRoleAdmin
          : null
      const badges: ManagedListItem['badges'] = []
      if (roleLabel) {
        badges.push({
          label: roleLabel,
          tone: 'muted',
        })
      }
      if (user.tapp_install_disabled) {
        badges.push({
          label: c.usersTappInstallDisabled,
          tone: 'warn',
        })
      }

      return {
        id: user.id,
        className: [
          'users-row',
          user.online ? 'is-online' : 'is-offline',
          isSelf ? 'is-self' : '',
        ]
          .filter(Boolean)
          .join(' '),
        leading: (
          <span
            className={`users-avatar-wrap${user.online ? ' is-online' : ''}`}
            title={
              user.online
                ? `${c.usersOnline} · ${c.usersLastSeen}: ${formatDateTime(user.last_seen_at)}`
                : `${c.usersOffline} · ${c.usersLastSeen}: ${formatDateTime(user.last_seen_at)}`
            }
          >
            <Avatar
              className="managed-list-avatar"
              src={user.avatar_url}
              name={user.display_name || user.username}
              decorative
            />
            <span className="users-presence-dot" aria-hidden />
          </span>
        ),
        title: (
          <span className="users-row-title">
            <span className="users-display-name">
              {user.display_name || user.username}
            </span>
            <span className="users-handle">@{user.username}</span>
            {user.is_admin ? (
              <LuShieldCheck className="users-admin-mark" aria-hidden />
            ) : null}
          </span>
        ),
        meta: (
          <span className="users-row-meta-inline">
            <span title={c.usersOnlineTotal}>
              <LuClock aria-hidden size={13} />
              {formatOnlineTotal(user.online_seconds)}
            </span>
            <span title={c.usersInstalledTapps}>
              <LuPackage aria-hidden size={13} />
              {user.tapp_count}
            </span>
          </span>
        ),
        badges: badges.length > 0 ? badges : undefined,
        busy: rowBusyId === user.id,
        renderHit: ({ leading: hitLeading, main: hitMain }) => (
          <SettingTitleGuideEntry
            requireShowDetails={false}
            title={user.display_name || user.username}
            openLabel={c.usersDetail}
            closeLabel={c.usersHideDetail}
            panelClassName="users-detail-guide-float"
            guide={renderExpandContent(user)}
            renderTrigger={(api) => (
              <button
                type="button"
                className={`managed-list-row-hit${api.open || api.closing ? ' is-active' : ''}`}
                aria-label={
                  api.open
                    ? c.usersHideDetail
                    : `${c.usersDetail} · ${user.display_name || user.username}`
                }
                aria-expanded={api.open}
                aria-controls={api.mounted ? api.panelId : undefined}
                title={api.open ? c.usersHideDetail : c.usersDetail}
                onClick={(event) => {
                  if (!api.open) void ensureDetail(user)
                  api.toggle(event)
                }}
              >
                {hitLeading}
                {hitMain}
              </button>
            )}
          />
        ),
      }
    })
  }, [
    filteredUsers,
    currentUser?.id,
    c,
    formatOnlineTotal,
    formatDateTime,
    rowBusyId,
    ensureDetail,
    renderExpandContent,
  ])

  const emptyText =
    users.length === 0
      ? c.usersEmpty
      : hasActiveFilter
        ? c.usersNoMatch
        : c.usersEmpty

  const USERS_LIST_CAP = 80
  const usersListTruncated =
    filteredUsers.length > 12 && filteredUsers.length > USERS_LIST_CAP

  const footer =
    hasActiveFilter && users.length > 0 && !usersListTruncated
      ? c.usersResultCount.replace(
          '{count}',
          String(filteredUsers.length),
        )
      : undefined

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      {...bindGuide('users.section', g.users.section)}
      sectionId={sectionId}
    >
      <div
        className="users-managed-list-host"
        {...guideDomProps('users.list')}
      >
        <ManagedList
          className="users-managed-list"
          stats={listStats}
          loading={loading}
          working={busy}
          maxHeight={filteredUsers.length > 12 ? '28rem' : null}
          maxVisibleItems={
            filteredUsers.length > 12 ? USERS_LIST_CAP : null
          }
          truncateFooter={(shown, total) =>
            c.usersShowing
              .replace('{shown}', String(shown))
              .replace('{total}', String(total))
          }
          emptyText={emptyText}
          footer={footer}
          queryDefaultOpen
          queryChrome="plain"
          queryCollapsible={false}
          queryToggleLabel={c.federationListQueryToggle}
          queryToggleDescription={c.federationListQueryToggleDesc}
          queryToggleIcon={<FaSearch aria-hidden />}
          queryCollapseLabel={c.federationListQueryCollapse}
          queryCollapseDescription={c.federationListQueryCollapseDesc}
          search={{
            value: searchQuery,
            onChange: setSearchQuery,
            placeholder: c.usersSearchPlaceholder,
            ariaLabel: c.usersSearchLabel,
          }}
          filterGroups={[
            {
              options: roleFilterOptions,
              value: roleFilter,
              onChange: (k) => setRoleFilter(k as RoleFilter),
              ariaLabel: c.usersFilterRole,
            },
            {
              options: onlineFilterOptions,
              value: onlineFilter,
              onChange: (k) => setOnlineFilter(k as OnlineFilter),
              ariaLabel: c.usersFilterStatus,
            },
          ]}
          toolbar={toolbar}
          formTitle={c.usersCreateUser}
          formDescription={c.usersCreateUserDesc}
          formIcon={<FaPlus aria-hidden />}
          formCollapseLabel={c.usersCancel}
          formCollapseDescription={c.usersCancelDesc}
          formOpen={formOpen}
          onFormOpenChange={setFormOpen}
          formOpenTitle={
            <span className="managed-list-form-title-with-guide">
              <span>{c.usersCreateUser}</span>
              <SettingTitleGuideEntry
                title={c.usersCreateUser}
                guide={renderGuide(g.users.create)}
              />
            </span>
          }
          form={
            <>
              <div
                id="cfg-g-users-create"
                data-guide-path="users.create"
                className="has-guide-anchor managed-list-guide-anchor"
                hidden
                aria-hidden
              />
              <InputItem
                itemKey="users-create-username"
                label={c.usersCreateUsername}
                value={createDraft.username}
                onChange={(username) =>
                  setCreateDraft((d) => ({ ...d, username }))
                }
                inputType="text"
                autoComplete="off"
                layout="vertical"
                size="sm"
                required
              />
              <InputItem
                itemKey="users-create-password"
                label={c.usersCreatePassword}
                value={createDraft.password}
                onChange={(password) =>
                  setCreateDraft((d) => ({ ...d, password }))
                }
                inputType="password"
                autoComplete="new-password"
                layout="vertical"
                size="sm"
                required
              />
              <InputItem
                itemKey="users-create-email"
                label={c.usersEmail}
                value={createDraft.email}
                onChange={(email) => setCreateDraft((d) => ({ ...d, email }))}
                inputType="email"
                autoComplete="off"
                layout="vertical"
                size="sm"
              />
              {isPrimaryAdmin && (
                <div
                  className="users-create-admin-row"
                  role="presentation"
                  onClick={() =>
                    setCreateDraft((d) => ({ ...d, is_admin: !d.is_admin }))
                  }
                >
                  <span className="setting-label-text">
                    {c.usersCreateIsAdmin}
                  </span>
                  <ToggleSwitch
                    checked={createDraft.is_admin}
                    onChange={(is_admin) =>
                      setCreateDraft((d) => ({ ...d, is_admin }))
                    }
                    aria-label={c.usersCreateIsAdmin}
                  />
                </div>
              )}
              <div className="managed-list-form-actions">
                <SettingsButton
                  size="sm"
                  variant="primary"
                  disabled={
                    busy ||
                    !createDraft.username.trim() ||
                    !createDraft.password
                  }
                  loading={busy && formOpen}
                  onClick={() => void handleCreate()}
                >
                  {c.usersCreateSubmit}
                </SettingsButton>
              </div>
            </>
          }
          items={listItems}
        />
      </div>
    </SettingSection>
  )
}

export default UsersConfigSection
