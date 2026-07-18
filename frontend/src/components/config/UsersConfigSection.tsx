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
  LuShieldCheck,
  LuUser,
} from '../../lib/icons'
import adminUsersApi from '../../services/adminUsersApi'
import { getOAuthIconAsset } from '../../utils/oauthIcons'
import OAuthIconImage from '../OAuthIconImage'
import { SettingGroup, SettingSection, SwitchItem } from '../settings'
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

  const notifyError = useCallback(
    (error: unknown, fallback: string) => {
      const message =
        error instanceof Error && error.message ? error.message : fallback
      onMessage?.(message, 'error')
    },
    [onMessage],
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

  const admins = useMemo(() => users.filter((u) => u.is_admin), [users])
  const registered = useMemo(() => users.filter((u) => !u.is_admin), [users])

  /** 站点主管理员为 user id = 1；非主管理员不能改其他管理员权限或删除管理员 */
  const isPrimaryAdmin = currentUser?.id === 1
  const canManageAdminPrivileges = (target: AdminUser) =>
    isPrimaryAdmin || (!target.is_admin && target.id !== 1)
  const canDeleteUser = (target: AdminUser) =>
    target.id !== currentUser?.id &&
    (isPrimaryAdmin || (!target.is_admin && target.id !== 1))

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
        applyUpdated(await adminUsersApi.update(userId, update))
        return true
      } catch (error) {
        notifyError(error, t.config.usersActionError)
        return false
      } finally {
        setBusy(false)
      }
    },
    [applyUpdated, notifyError, t],
  )

  const handleToggleAdmin = useCallback(
    async (user: AdminUser) => {
      if (user.is_admin && !window.confirm(t.config.usersRevokeAdminConfirm)) {
        return
      }
      await runUpdate(user.id, { is_admin: !user.is_admin })
    },
    [runUpdate, t],
  )

  const handleToggleLocalLogin = useCallback(
    (user: AdminUser) =>
      runUpdate(user.id, { local_login_disabled: !user.local_login_disabled }),
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
      await adminUsersApi.create({
        username: createDraft.username.trim(),
        password: createDraft.password,
        email: createDraft.email.trim() || undefined,
        is_admin: createDraft.is_admin,
      })
      setCreating(false)
      setCreateDraft({ username: '', password: '', email: '', is_admin: false })
      await loadUsers()
    } catch (error) {
      notifyError(error, t.config.usersActionError)
    } finally {
      setBusy(false)
    }
  }, [createDraft, loadUsers, notifyError, t])

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
                  aria-label={t.config.usersRoleAdmin}
                />
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
                    className="users-button"
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
                  {/* 非主管理员：对已是管理员 / id=1 的行隐藏 is_admin 切换；可提升普通用户 */}
                  {(canManageAdminPrivileges(shown) ||
                    shown.id === currentUser?.id) && (
                    <button
                      type="button"
                      className={`users-button${shown.is_admin ? ' danger' : ''}`}
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
                      className="users-button danger"
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

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      <SettingGroup
        title={t.config.usersAdminGroup}
        description={
          isPrimaryAdmin
            ? t.config.usersAdminGroupDesc
            : `${t.config.usersAdminGroupDesc}. ${t.config.usersPrimaryAdminOnly}`
        }
      >
        <div className="users-list">
          {loading && users.length === 0 ? (
            <div className="users-muted">…</div>
          ) : (
            admins.map(renderUserRow)
          )}
        </div>
      </SettingGroup>

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

        <div className="users-toolbar">
          <button
            type="button"
            className="users-button"
            disabled={loading}
            onClick={loadUsers}
          >
            <LuRefreshCw aria-hidden className={loading ? 'spinning' : ''} />
            {t.config.usersRefresh}
          </button>
          <button
            type="button"
            className="users-button primary"
            onClick={() => setCreating((v) => !v)}
          >
            <LuPlus aria-hidden />
            {t.config.usersCreateUser}
          </button>
        </div>

        {creating && (
          <div className="users-edit-form users-create-form">
            <label>
              {t.config.usersCreateUsername}
              <input
                type="text"
                autoComplete="off"
                value={createDraft.username}
                onChange={(e) =>
                  setCreateDraft((d) => ({ ...d, username: e.target.value }))
                }
              />
            </label>
            <label>
              {t.config.usersCreatePassword}
              <input
                type="password"
                autoComplete="new-password"
                value={createDraft.password}
                onChange={(e) =>
                  setCreateDraft((d) => ({ ...d, password: e.target.value }))
                }
              />
            </label>
            <label>
              {t.config.usersEmail}
              <input
                type="email"
                autoComplete="off"
                value={createDraft.email}
                onChange={(e) =>
                  setCreateDraft((d) => ({ ...d, email: e.target.value }))
                }
              />
            </label>
            <label className="users-checkbox">
              <input
                type="checkbox"
                checked={createDraft.is_admin}
                onChange={(e) =>
                  setCreateDraft((d) => ({ ...d, is_admin: e.target.checked }))
                }
              />
              {t.config.usersCreateIsAdmin}
            </label>
            <div className="users-actions">
              <button
                type="button"
                className="users-button primary"
                disabled={
                  busy || !createDraft.username.trim() || !createDraft.password
                }
                onClick={handleCreate}
              >
                {t.config.usersCreateSubmit}
              </button>
              <button
                type="button"
                className="users-button"
                disabled={busy}
                onClick={() => setCreating(false)}
              >
                {t.config.usersCancel}
              </button>
            </div>
          </div>
        )}

        <div className="users-list">
          {loading && users.length === 0 ? (
            <div className="users-muted">…</div>
          ) : registered.length === 0 ? (
            <div className="users-muted">{t.config.usersEmpty}</div>
          ) : (
            registered.map(renderUserRow)
          )}
        </div>
      </SettingGroup>
    </SettingSection>
  )
}

export default UsersConfigSection
