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
  userId?: number
  targetIsSiteOwner?: boolean
  onApplied?: () => void
}

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
