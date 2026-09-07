/**
 * 头像来源选择器 —— 用户中心与设置页用户管理共用同一份。
 *
 * 来源全部保留、显式二选一：
 * - 自动：站长优先平台画像，其余人用账号头像（即历史行为）
 * - 账号：`users.avatar_url`
 * - 每个已绑定 OAuth 身份
 * - 站长的每个平台画像（B站 / GitHub / YouTube / Steam）
 * - 站点人设的 Q 版贴纸头像（生成过才出现，人设关掉就收回）
 *
 * 同站合并由后端 `list_avatar_sources` 完成（如 GitHub OAuth + 站长 GitHub
 * 抓取 → 一行 `kind=platform`）；本组件只消费列表，不二次去重。
 *
 * 选定后后端把解析结果落成快照，`/api/auth/me`、`/api/profile/user-info`、
 * `/api/tapp/context/user`、`/api/admin/users` 读到的是同一张脸；本组件切换成功
 * 后广播 `notifyAvatarChanged()`，让同页其它头像位置立即跟上。
 */

import type {
  AvatarSourceItem,
  AvatarSourceKind,
} from '../services/avatarSourceApi'

import { useCallback, useEffect, useState } from 'react'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import avatarSourceApi from '../services/avatarSourceApi'
import { userFacingError } from '../utils/userFacingError'
import { Avatar } from './Avatar'
import { Spinner } from './Spinner'
import './AvatarSourcePicker.css'

interface AvatarSourcePickerProps {
  /** 省略 = 改自己；传 id = 管理员改他人 */
  userId?: number
  /**
   * 管理员改他人时：目标是否为站长。
   * 仅 viewer / 站长切换才广播全局 avatar-changed（首页信息条）。
   */
  targetIsSiteOwner?: boolean
  /** 切换成功后的回调（刷新外层头像 / 关闭弹窗） */
  onApplied?: () => void
}

/** 选中项的稳定标识：kind 单独不够（identity/platform 有多个） */
function sourceKey(kind: AvatarSourceKind, ref: string | null): string {
  return `${kind}:${ref ?? ''}`
}

export function AvatarSourcePicker({
  userId,
  targetIsSiteOwner = false,
  onApplied,
}: AvatarSourcePickerProps) {
  const { t } = useI18n()
  const { user: viewer } = useAuth()
  const [sources, setSources] = useState<AvatarSourceItem[]>([])
  const [currentKey, setCurrentKey] = useState<string>('')
  const [loading, setLoading] = useState(true)
  const [applyingKey, setApplyingKey] = useState<string | null>(null)
  const [error, setError] = useState('')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const data =
        userId == null
          ? await avatarSourceApi.listMine()
          : await avatarSourceApi.listForUser(userId)
      setSources(data.sources ?? [])
      // Prefer a listed source with is_current so merged rows (e.g. GitHub OAuth
      // + platform scrape → kind=platform) still highlight when the DB still
      // stores the underlying identity selection.
      const currentFromList = (data.sources ?? []).find((s) => s.is_current)
      setCurrentKey(
        currentFromList
          ? sourceKey(currentFromList.kind, currentFromList.ref || null)
          : sourceKey(data.current.kind, data.current.ref),
      )
      setError('')
    } catch (error) {
      setError(userFacingError(error, t.userModal.profileSourceFailed))
    } finally {
      setLoading(false)
    }
  }, [userId, t])

  useEffect(() => {
    void load()
  }, [load])

  const apply = async (kind: AvatarSourceKind, ref: string | null) => {
    const key = sourceKey(kind, ref)
    if (key === currentKey || applyingKey != null) return
    setApplyingKey(key)
    setError('')
    try {
      if (userId == null) {
        await avatarSourceApi.setMine(kind, ref)
      } else {
        const isViewer = viewer?.id === userId
        await avatarSourceApi.setForUser(userId, kind, ref, {
          broadcast: isViewer || targetIsSiteOwner,
        })
      }
      setCurrentKey(key)
      onApplied?.()
    } catch (e) {
      setError(userFacingError(e, t.userModal.profileSourceFailed))
    } finally {
      setApplyingKey(null)
    }
  }

  if (loading && sources.length === 0) {
    return <p className="user-modal-oauth-empty">…</p>
  }

  // 「自动」不是后端列出的来源，是取消显式选择；始终排在最前
  const rows: Array<{
    key: string
    kind: AvatarSourceKind
    ref: string | null
    label: string
    sublabel: string | null
    avatarUrl: string | null
  }> = [
    {
      key: sourceKey('auto', null),
      kind: 'auto',
      ref: null,
      label: t.userModal.profileSourceAuto,
      sublabel: t.userModal.profileSourceAutoDesc,
      avatarUrl: null,
    },
    ...sources.map((source) => ({
      key: sourceKey(source.kind, source.ref || null),
      kind: source.kind,
      ref: source.ref || null,
      // account / persona 的 label 由后端固定返回标识串，文案在前端本地化
      label:
        source.kind === 'account'
          ? t.userModal.profileSourceAccount
          : source.kind === 'persona'
            ? t.userModal.profileSourcePersona
            : source.label,
      sublabel: source.sublabel,
      avatarUrl: source.avatar_url,
    })),
  ]

  return (
    <>
      <p className="user-modal-profile-source-hint">
        {t.userModal.profileSourceHint}
      </p>
      {rows.length === 1 ? (
        <p className="user-modal-oauth-empty">
          {t.userModal.profileSourceEmpty}
        </p>
      ) : (
        <ul className="user-modal-profile-source-list">
          {rows.map((row) => {
            const isCurrent = row.key === currentKey
            const isApplying = applyingKey === row.key
            return (
              <li key={row.key}>
                <button
                  type="button"
                  className={`user-modal-profile-source-row ${isCurrent ? 'is-primary' : ''}`}
                  disabled={applyingKey != null || isCurrent}
                  onClick={() => void apply(row.kind, row.ref)}
                >
                  <Avatar
                    src={row.avatarUrl}
                    name={row.sublabel || row.label}
                    className="user-modal-profile-source-avatar"
                    decorative
                  />
                  <span className="user-modal-profile-source-info">
                    <span className="user-modal-profile-source-name">
                      {row.label}
                      {isCurrent && (
                        <span className="user-modal-profile-source-current">
                          {t.userModal.profileSourceCurrent}
                        </span>
                      )}
                    </span>
                    {row.sublabel && (
                      <span className="user-modal-profile-source-sub">
                        {row.kind === 'identity'
                          ? row.sublabel.startsWith('@')
                            ? row.sublabel
                            : `@${row.sublabel}`
                          : row.sublabel}
                      </span>
                    )}
                  </span>
                  {isApplying ? (
                    <Spinner size="sm" />
                  ) : isCurrent ? (
                    <span className="user-modal-profile-source-check">✓</span>
                  ) : null}
                </button>
              </li>
            )
          })}
        </ul>
      )}
      {error && <p className="user-modal-oauth-error">{error}</p>}
    </>
  )
}

export default AvatarSourcePicker
