/**
 * 设置页「用户管理」区块
 *
 * - 管理员账户：账号信息 + OAuth 绑定状态
 * - 已注册用户：OAuth 状态、安装应用、在线时间；支持展开详情、
 *   授予/撤销管理员、启停本地登录、解绑 OAuth、删除用户、创建用户
 */

import type { AdminUser, AdminUserIdentity, AdminUserUpdate } from '../../services/adminUsersApi'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import {
  FaGithub,
  LuClock,
  LuPackage,
  LuPlus,
  LuRefreshCw,
  LuSearch,
  LuShieldCheck,
  LuUser,
  LuX,
} from '../../lib/icons'
import adminUsersApi from '../../services/adminUsersApi'
import { messageForAdminUserError } from '../../utils/authErrorMessages'
import { getOAuthIconAsset } from '../../utils/oauthIcons'
import OAuthIconImage from '../OAuthIconImage'
import {
  InputItem,
  SettingGroup,
  SettingSection,
  SwitchItem,
  ToggleSwitch,
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
  /** 允许公开注册本地账号（存于 OAuth 设置，由 ConfigForm 统一保存） */
  allowRegister: boolean
  allowRegisterLoading?: boolean
  onAllowRegisterChange: (allow: boolean) => void
}

const ONLINE_ICON_SIZE = 14

/** Case-insensitive match against username, display name, email, OAuth identity fields. */
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

/** Compact segmented chip control (radiogroup pattern). */
function FilterChipGroup<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string
  value: T
  options: Array<{ value: T; label: string }>
  onChange: (next: T) => void
}) {
  return (
    <div className="users-chip-group" role="radiogroup" aria-label={label}>
      {options.map((option) => {
        const selected = value === option.value
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            className={`users-chip${selected ? ' active' : ''}`}
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </button>
        )
      })}
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
}) => {
  const { t } = useI18n()
  const { user: currentUser } = useAuth()
  const [users, setUsers] = useState<AdminUser[]>([])
  const [loading, setLoading] = useState(true)
  const [expandedId, setExpandedId] = useState<number | null>(null)
  const [detail, setDetail] = useState<AdminUser | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [creating, setCreating] = useState(false)
  const [createDraft, setCreateDraft] = useState({
    username: '',
    password: '',
    email: '',
    is_admin: false,
  })
  const [busy, setBusy] = useState(false)
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
      notifyError(error, t.config.usersLoadError)
    } finally {
      setLoading(false)
    }
  }, [notifyError, t])

  useEffect(() => {
    loadUsers()
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

  const admins = useMemo(
    () => filteredUsers.filter((u) => u.is_admin),
    [filteredUsers],
  )
  const registered = useMemo(
    () => filteredUsers.filter((u) => !u.is_admin),
    [filteredUsers],
  )

  /**
   * Site owner (`is_owner`); was heuristic id === 1.
   * Prefer /api/auth/me and list payload flags over id.
   */
  const isPrimaryAdmin = Boolean(
    currentUser?.is_owner ||
      users.find((u) => u.id === currentUser?.id)?.is_owner,
  )
  const canDeleteUser = (target: AdminUser) =>
    target.id !== currentUser?.id &&
    !target.is_owner &&
    (isPrimaryAdmin || !target.is_admin)

  const formatDateTime = useCallback(
    (value: string | null) => {
      if (!value) return t.config.usersNever
      const date = new Date(value)
      return Number.isNaN(date.getTime())
        ? t.config.usersNever
        : date.toLocaleString()
    },
    [t],
  )

  const formatOnlineTotal = useCallback(
    (seconds: number) => {
      const hours = Math.floor(seconds / 3600)
      const minutes = Math.floor((seconds % 3600) / 60)
      if (hours <= 0 && minutes <= 0) return `0 ${t.config.usersMinutes}`
      const parts: string[] = []
      if (hours > 0) parts.push(`${hours} ${t.config.usersHours}`)
      if (minutes > 0) parts.push(`${minutes} ${t.config.usersMinutes}`)
      return parts.join(' ')
    },
    [t],
  )

  /** 原位替换列表和详情中的同一用户 */
  const applyUpdated = useCallback((updated: AdminUser) => {
    setUsers((prev) => prev.map((u) => (u.id === updated.id ? updated : u)))
    setDetail((prev) => (prev?.id === updated.id ? updated : prev))
  }, [])

  const toggleExpand = useCallback(
    async (user: AdminUser) => {
      if (expandedId === user.id) {
        setExpandedId(null)
        setDetail(null)
        return
      }
      setExpandedId(user.id)
      setDetail(null)
      setDetailLoading(true)
      try {
        setDetail(await adminUsersApi.get(user.id))
      } catch (error) {
        notifyError(error, t.config.usersLoadError)
      } finally {
        setDetailLoading(false)
      }
    },
    [expandedId, notifyError, t],
  )

  const runUpdate = useCallback(
    async (userId: number, update: AdminUserUpdate) => {
      setBusy(true)
      try {
        const { user: updated, notice } = await adminUsersApi.update(
          userId,
          update,
        )
        applyUpdated(updated)
        return { ok: true as const, notice, updated }
      } catch (error) {
        notifyError(error, t.config.usersActionError)
        return { ok: false as const }
      } finally {
        setBusy(false)
      }
    },
    [applyUpdated, notifyError, t],
  )

  const handleToggleAdmin = useCallback(
    async (user: AdminUser) => {
      if (user.is_owner && user.is_admin) {
        onMessage?.(t.config.usersErrorCannotDemoteOwner, 'error')
        return
      }
      if (user.is_admin && !window.confirm(t.config.usersRevokeAdminConfirm)) {
        return
      }
      const promoting = !user.is_admin
      const result = await runUpdate(user.id, { is_admin: !user.is_admin })
      if (!result.ok) return
      if (promoting) {
        onMessage?.(t.config.usersPromoteReLoginNotice, 'info')
      } else {
        onMessage?.(t.config.usersDemoteImmediateNotice, 'info')
      }
    },
    [onMessage, runUpdate, t],
  )

  const handleToggleLocalLogin = useCallback(
    async (user: AdminUser) => {
      await runUpdate(user.id, {
        local_login_disabled: !user.local_login_disabled,
      })
    },
    [runUpdate],
  )

  const handleUnlinkIdentity = useCallback(
    async (user: AdminUser, identity: AdminUserIdentity) => {
      if (!window.confirm(t.config.usersUnlinkConfirm)) return
      setBusy(true)
      try {
        applyUpdated(await adminUsersApi.unlinkIdentity(user.id, identity.id))
      } catch (error) {
        notifyError(error, t.config.usersActionError)
      } finally {
        setBusy(false)
      }
    },
    [applyUpdated, notifyError, t],
  )

  const handleDeleteUser = useCallback(
    async (user: AdminUser) => {
      if (user.id === currentUser?.id) return
      if (!window.confirm(t.config.usersDeleteConfirm)) return
      setBusy(true)
      try {
        await adminUsersApi.delete(user.id)
        setUsers((prev) => prev.filter((u) => u.id !== user.id))
        if (expandedId === user.id) {
          setExpandedId(null)
          setDetail(null)
        }
        onMessage?.(t.config.usersDeleteSuccess, 'success')
      } catch (error) {
        notifyError(error, t.config.usersActionError)
      } finally {
        setBusy(false)
      }
    },
    [currentUser?.id, expandedId, notifyError, onMessage, t],
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
        // 仅站点 owner 可创建管理员账号
        is_admin: createAsAdmin,
      })
      setCreating(false)
      setCreateDraft({ username: '', password: '', email: '', is_admin: false })
      await loadUsers()
      if (createAsAdmin) {
        onMessage?.(t.config.usersPromoteReLoginNotice, 'info')
      }
    } catch (error) {
      notifyError(error, t.config.usersActionError)
    } finally {
      setBusy(false)
    }
  }, [createDraft, isPrimaryAdmin, loadUsers, notifyError, onMessage, t])

  const renderIdentities = (user: AdminUser, allowUnlink: boolean) =>
    user.identities.length === 0 ? (
      <span className="users-muted">{t.config.usersNoIdentities}</span>
    ) : (
      <span className="users-identities">
        {user.identities.map((identity) => (
          <span key={identity.id} className="users-identity-item">
            <ProviderBadge identity={identity} />
            {allowUnlink && (
              <button
                type="button"
                className="users-link-button danger"
                disabled={busy}
                onClick={() => handleUnlinkIdentity(user, identity)}
              >
                {t.config.usersUnlinkIdentity}
              </button>
            )}
          </span>
        ))}
      </span>
    )

  const renderUserRow = (user: AdminUser) => {
    const expanded = expandedId === user.id
    const shown = detail?.id === user.id ? detail : user
    return (
      <div
        key={user.id}
        className={`users-row${expanded ? ' expanded' : ''}${user.online ? ' online' : ''}`}
      >
        <button
          type="button"
          className="users-row-main"
          onClick={() => toggleExpand(user)}
          aria-expanded={expanded}
          title={expanded ? t.config.usersHideDetail : t.config.usersShowDetail}
        >
          {user.avatar_url ? (
            <img className="users-avatar" src={user.avatar_url} alt="" />
          ) : (
            <span className="users-avatar placeholder">
              <LuUser aria-hidden />
            </span>
          )}
          <span className="users-row-name">
            <span className="users-username">
              {user.display_name || user.username}
              {user.is_admin && (
                <LuShieldCheck
                  className="users-admin-mark"
                  aria-label={
                    user.is_owner
                      ? t.config.usersRoleOwner
                      : t.config.usersRoleAdmin
                  }
                />
              )}
              {user.is_owner && (
                <span className="users-self-mark" title={t.config.usersRoleOwner}>
                  {t.config.usersRoleOwner}
                </span>
              )}
              {user.id === currentUser?.id && (
                <span className="users-self-mark">·</span>
              )}
            </span>
            <span className="users-muted">@{user.username}</span>
          </span>
          <span
            className={`users-online-pill${user.online ? ' online' : ''}`}
            title={`${t.config.usersLastSeen}: ${formatDateTime(user.last_seen_at)}`}
          >
            {user.online ? t.config.usersOnline : t.config.usersOffline}
          </span>
          <span className="users-row-meta">
            <span className="users-meta-item" title={t.config.usersOnlineTotal}>
              <LuClock aria-hidden />
              {formatOnlineTotal(user.online_seconds)}
            </span>
            <span
              className="users-meta-item"
              title={t.config.usersInstalledTapps}
            >
              <LuPackage aria-hidden />
              {t.config.usersTappCount.replace(
                '{count}',
                String(user.tapp_count),
              )}
            </span>
          </span>
        </button>

        {expanded && (
          <div className="users-row-detail">
            {detailLoading && !detail ? (
              <div className="users-muted">…</div>
            ) : (
              <>
                <dl className="users-detail-grid">
                  <dt>{t.config.usersOAuthIdentities}</dt>
                  <dd>{renderIdentities(shown, true)}</dd>
                  <dt>{t.config.usersLocalPassword}</dt>
                  <dd>
                    {shown.has_password
                      ? t.config.usersPasswordSet
                      : t.config.usersPasswordUnset}
                    {shown.local_login_disabled && (
                      <span className="users-tag warning">
                        {t.config.usersLocalLoginDisabled}
                      </span>
                    )}
                  </dd>
                  <dt>{t.config.usersLastLogin}</dt>
                  <dd>{formatDateTime(shown.last_login_at)}</dd>
                  <dt>{t.config.usersLastSeen}</dt>
                  <dd>{formatDateTime(shown.last_seen_at)}</dd>
                  <dt>{t.config.usersCreatedAt}</dt>
                  <dd>{formatDateTime(shown.created_at)}</dd>
                  <dt>{t.config.usersInstalledTapps}</dt>
                  <dd>
                    {!shown.tapps || shown.tapps.length === 0 ? (
                      <span className="users-muted">
                        {t.config.usersNoTapps}
                      </span>
                    ) : (
                      <ul className="users-tapp-list">
                        {shown.tapps.map((tapp) => (
                          <li key={tapp.tapp_id}>
                            <span className="users-tapp-name">
                              {tapp.name}
                            </span>
                            <span className="users-muted">
                              v{tapp.version} · {tapp.status}
                            </span>
                          </li>
                        ))}
                      </ul>
                    )}
                  </dd>
                </dl>

                <div className="users-actions">
                  <button
                    type="button"
                    className="btn-base btn-secondary"
                    disabled={
                      busy ||
                      (!shown.local_login_disabled &&
                        shown.identities.length === 0)
                    }
                    title={
                      !shown.local_login_disabled &&
                      shown.identities.length === 0
                        ? t.config.usersLocalLoginRequiresOAuth
                        : undefined
                    }
                    onClick={() => handleToggleLocalLogin(shown)}
                  >
                    {shown.local_login_disabled
                      ? t.config.usersEnableLocalLogin
                      : t.config.usersDisableLocalLogin}
                  </button>
                  {/* 仅站点 owner 可授予/撤销 is_admin；owner 自身与 is_owner 目标不可降级 */}
                  {isPrimaryAdmin && !shown.is_owner && (
                    <button
                      type="button"
                      className={`btn-base ${shown.is_admin ? 'btn-danger' : 'btn-secondary'}`}
                      disabled={busy || shown.id === currentUser?.id}
                      onClick={() => handleToggleAdmin(shown)}
                    >
                      {shown.is_admin
                        ? t.config.usersRevokeAdmin
                        : t.config.usersMakeAdmin}
                    </button>
                  )}
                  {canDeleteUser(shown) && (
                    <button
                      type="button"
                      className="btn-base btn-danger"
                      disabled={busy}
                      onClick={() => handleDeleteUser(shown)}
                    >
                      {t.config.usersDelete}
                    </button>
                  )}
                </div>
              </>
            )}
          </div>
        )}
      </div>
    )
  }

  const clearFilters = useCallback(() => {
    setSearchQuery('')
    setRoleFilter('all')
    setOnlineFilter('all')
  }, [])

  const matchCountLabel = t.config.usersResultCount.replace(
    '{count}',
    String(filteredUsers.length),
  )

  // Hide empty groups while filtering; keep registered group for the switch.
  const showAdminGroup = !hasActiveFilter || admins.length > 0
  const showRegisteredList =
    !hasActiveFilter || registered.length > 0 || admins.length === 0

  const emptyListMessage = (isAdminGroup: boolean) => {
    if (loading && users.length === 0) {
      return <div className="users-muted">…</div>
    }
    if (hasActiveFilter) {
      // Only surface the no-match copy once (in the section that remains visible).
      if (isAdminGroup) return null
      if (filteredUsers.length > 0) return null
      return (
        <div className="users-muted users-empty-state" role="status">
          {t.config.usersNoMatch}
        </div>
      )
    }
    if (!isAdminGroup) {
      return (
        <div className="users-muted users-empty-state">
          {t.config.usersEmpty}
        </div>
      )
    }
    return null
  }

  const roleOptions: Array<{ value: RoleFilter; label: string }> = [
    { value: 'all', label: t.config.usersFilterAll },
    { value: 'admin', label: t.config.usersRoleAdmin },
    { value: 'user', label: t.config.usersRoleUser },
  ]

  const onlineOptions: Array<{ value: OnlineFilter; label: string }> = [
    { value: 'all', label: t.config.usersFilterAll },
    { value: 'online', label: t.config.usersOnline },
    { value: 'offline', label: t.config.usersOffline },
  ]

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      <div className="users-control-strip">
        <div className="users-control-main" role="search">
          <label className="users-search-field">
            <span className="visually-hidden">{t.config.usersSearchLabel}</span>
            <LuSearch className="users-search-icon" aria-hidden />
            <input
              type="search"
              className="field-input users-search-input"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={t.config.usersSearchPlaceholder}
              autoComplete="off"
              spellCheck={false}
            />
            {searchQuery && (
              <button
                type="button"
                className="users-search-clear"
                onClick={() => setSearchQuery('')}
                aria-label={t.config.usersSearchClear}
                title={t.config.usersSearchClear}
              >
                <LuX aria-hidden size={14} />
              </button>
            )}
          </label>

          <div className="users-filter-chips">
            <FilterChipGroup
              label={t.config.usersFilterRole}
              value={roleFilter}
              options={roleOptions}
              onChange={setRoleFilter}
            />
            <FilterChipGroup
              label={t.config.usersFilterStatus}
              value={onlineFilter}
              options={onlineOptions}
              onChange={setOnlineFilter}
            />
          </div>

          <div className="users-control-actions">
            <span
              className={`users-match-count${hasActiveFilter ? ' visible' : ''}`}
              role="status"
              aria-live="polite"
            >
              {hasActiveFilter ? matchCountLabel : '\u00A0'}
            </span>
            <button
              type="button"
              className={`btn-base btn-secondary users-filter-reset${hasActiveFilter ? ' visible' : ''}`}
              onClick={clearFilters}
              disabled={!hasActiveFilter}
              aria-hidden={!hasActiveFilter}
              tabIndex={hasActiveFilter ? 0 : -1}
            >
              {t.config.usersFilterClear}
            </button>
            <button
              type="button"
              className="btn-base btn-secondary"
              disabled={loading}
              onClick={loadUsers}
            >
              <LuRefreshCw aria-hidden className={loading ? 'spinning' : ''} />
              {t.config.usersRefresh}
            </button>
            <button
              type="button"
              className="btn-base btn-primary"
              onClick={() => setCreating((v) => !v)}
            >
              <LuPlus aria-hidden />
              {t.config.usersCreateUser}
            </button>
          </div>
        </div>
      </div>

      {creating && (
        <div className="users-edit-form users-create-form">
          <InputItem
            itemKey="users-create-username"
            label={t.config.usersCreateUsername}
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
            label={t.config.usersCreatePassword}
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
            label={t.config.usersEmail}
            value={createDraft.email}
            onChange={(email) => setCreateDraft((d) => ({ ...d, email }))}
            inputType="email"
            autoComplete="off"
            layout="vertical"
            size="sm"
          />
          {isPrimaryAdmin && (
            <div className="users-create-admin-row">
              <span className="setting-label-text">
                {t.config.usersCreateIsAdmin}
              </span>
              <ToggleSwitch
                checked={createDraft.is_admin}
                onChange={(is_admin) =>
                  setCreateDraft((d) => ({ ...d, is_admin }))
                }
                aria-label={t.config.usersCreateIsAdmin}
              />
            </div>
          )}
          <div className="users-actions">
            <button
              type="button"
              className="btn-base btn-primary"
              disabled={
                busy || !createDraft.username.trim() || !createDraft.password
              }
              onClick={handleCreate}
            >
              {t.config.usersCreateSubmit}
            </button>
            <button
              type="button"
              className="btn-base btn-secondary"
              disabled={busy}
              onClick={() => setCreating(false)}
            >
              {t.config.usersCancel}
            </button>
          </div>
        </div>
      )}

      {showAdminGroup && (
        <SettingGroup
          title={t.config.usersAdminGroup}
          description={
            isPrimaryAdmin
              ? t.config.usersAdminGroupDesc
              : `${t.config.usersAdminGroupDesc}. ${t.config.usersPrimaryAdminOnly}`
          }
        >
          <div className="users-list">
            {admins.length > 0
              ? admins.map(renderUserRow)
              : emptyListMessage(true)}
          </div>
        </SettingGroup>
      )}

      <SettingGroup
        title={t.config.usersRegisteredGroup}
        description={t.config.usersRegisteredGroupDesc}
      >
        {/* 本地注册开关（从第三方登录区搬入；仍随 ConfigForm 全局保存） */}
        <SwitchItem
          itemKey="allow_local_registration"
          label={t.config.allowRegisterTitle}
          description={t.config.allowRegisterDesc}
          value={allowRegister}
          loading={allowRegisterLoading}
          onChange={onAllowRegisterChange}
        />

        {showRegisteredList && (
          <div className="users-list">
            {registered.length > 0
              ? registered.map(renderUserRow)
              : emptyListMessage(false)}
          </div>
        )}
      </SettingGroup>
    </SettingSection>
  )
}

export default UsersConfigSection
