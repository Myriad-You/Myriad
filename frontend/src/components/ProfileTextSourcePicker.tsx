/**
 * 名称与简介来源选择器 —— 与 AvatarSourcePicker 分开，互不改对方存储。
 *
 * 同站合并（GitHub OAuth + 抓取）由后端 list 完成；本组件只消费列表。
 * 切换后广播 `notifyProfileDisplayChanged()`，首页 name/bio 立即跟上。
 */

import type {
  ProfileTextSourceItem,
  ProfileTextSourceKind,
} from '../services/profileTextSourceApi'

import { useCallback, useEffect, useState } from 'react'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { userFacingError } from '../utils/userFacingError'
import profileTextSourceApi from '../services/profileTextSourceApi'
import { Spinner } from './Spinner'
import './AvatarSourcePicker.css'

interface ProfileTextSourcePickerProps {
  /** 省略 = 改自己；传 id = 管理员改他人 */
  userId?: number
  /**
   * 管理员改他人时：目标是否为站长。
   * 仅 viewer / 站长切换才广播全局 profile-display-changed（首页信息条）。
   */
  targetIsSiteOwner?: boolean
  onApplied?: () => void
}

function sourceKey(kind: ProfileTextSourceKind, ref: string | null): string {
  return `${kind}:${ref ?? ''}`
}

export function ProfileTextSourcePicker({
  userId,
  targetIsSiteOwner = false,
  onApplied,
}: ProfileTextSourcePickerProps) {
  const { t } = useI18n()
  const { user: viewer } = useAuth()
  const [sources, setSources] = useState<ProfileTextSourceItem[]>([])
  const [currentKey, setCurrentKey] = useState<string>('')
  const [loading, setLoading] = useState(true)
  const [applyingKey, setApplyingKey] = useState<string | null>(null)
  const [error, setError] = useState('')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const data =
        userId == null
          ? await profileTextSourceApi.listMine()
          : await profileTextSourceApi.listForUser(userId)
      setSources(data.sources ?? [])
      const currentFromList = (data.sources ?? []).find((s) => s.is_current)
      setCurrentKey(
        currentFromList
          ? sourceKey(currentFromList.kind, currentFromList.ref || null)
          : sourceKey(data.current.kind, data.current.ref),
      )
      setError('')
    } catch {
      setError(t.userModal.profileTextSourceFailed)
    } finally {
      setLoading(false)
    }
  }, [userId, t])

  useEffect(() => {
    void load()
  }, [load])

  const apply = async (kind: ProfileTextSourceKind, ref: string | null) => {
    const key = sourceKey(kind, ref)
    if (key === currentKey || applyingKey != null) return
    setApplyingKey(key)
    setError('')
    try {
      if (userId == null) {
        await profileTextSourceApi.setMine(kind, ref)
      } else {
        const isViewer = viewer?.id === userId
        await profileTextSourceApi.setForUser(userId, kind, ref, {
          broadcast: isViewer || targetIsSiteOwner,
        })
      }
      setCurrentKey(key)
      onApplied?.()
    } catch (e) {
      setError(userFacingError(e, t.userModal.profileTextSourceFailed))
    } finally {
      setApplyingKey(null)
    }
  }

  if (loading && sources.length === 0) {
    return <p className="user-modal-oauth-empty">…</p>
  }

  const rows: Array<{
    key: string
    kind: ProfileTextSourceKind
    ref: string | null
    label: string
    sublabel: string | null
  }> = [
    {
      key: sourceKey('auto', null),
      kind: 'auto',
      ref: null,
      label: t.userModal.profileTextSourceAuto,
      sublabel: t.userModal.profileTextSourceAutoDesc,
    },
    ...sources.map((source) => {
      // identity：sublabel 是 provider 用户名（handle），与 AvatarSourcePicker 一致；
      // 勿用 preview_name（显示名）再强行加 @，会得到 `@Alice Smith`。
      let sublabel: string | null
      if (source.kind === 'identity') {
        sublabel = source.sublabel
      } else {
        sublabel =
          source.preview_name ||
          source.sublabel ||
          (source.preview_bio
            ? source.preview_bio.length > 48
              ? `${source.preview_bio.slice(0, 48)}…`
              : source.preview_bio
            : null)
      }
      return {
        key: sourceKey(source.kind, source.ref || null),
        kind: source.kind,
        ref: source.ref || null,
        label:
          source.kind === 'account'
            ? t.userModal.profileTextSourceAccount
            : source.label,
        sublabel,
      }
    }),
  ]

  return (
    <>
      <p className="user-modal-profile-source-hint">
        {t.userModal.profileTextSourceHint}
      </p>
      {rows.length === 1 ? (
        <p className="user-modal-oauth-empty">
          {t.userModal.profileTextSourceEmpty}
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

export default ProfileTextSourcePicker
