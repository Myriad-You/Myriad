import type { ManagedListItem, ManagedListTone } from '../../settings/ManagedList'
import {
  LuChevronDown as ChevronDown,
  LuKey as Key,
  LuEdit3 as Pencil,
  LuPlus as Plus,
  LuRefreshCw as RefreshCw,
  LuTrash2 as Trash2,
} from '@lib/icons'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../../contexts/I18nContext'
import { ApiError, apiService } from '../../../services/api'
import { showError } from '../../../utils/toastManager'
import { CheckboxCard } from '../../settings/items/CheckboxCard'
import { InputItem } from '../../settings/items/InputItem'
import { NumberItem } from '../../settings/items/NumberItem'
import { SettingsButton } from '../../settings/items/SettingsButton'
import { ToggleSwitch } from '../../settings/items/ToggleSwitch'
import { ManagedList } from '../../settings/ManagedList'
import { SettingTitleTag } from '../../settings/SettingTitleTag'
import { Spinner } from '../../Spinner'
import './RSSHubInstances.css'

export interface RsshubInstance {
  id: number
  user_id: number | null
  name: string
  url: string
  has_access_key: boolean
  priority: number
  enabled: boolean
  health_status: 'healthy' | 'degraded' | 'unhealthy' | 'unknown'
  last_health_check: number | null
  last_response_time_ms: number | null
  consecutive_failures: number
  success_rate: number
  created_at: number
}

const HEALTH_TONE: Record<RsshubInstance['health_status'], ManagedListTone> = {
  healthy: 'success',
  degraded: 'warn',
  unhealthy: 'danger',
  unknown: 'muted',
}

const HEALTH_LABEL = {
  healthy: 'rsshubHealthy',
  degraded: 'rsshubDegraded',
  unhealthy: 'rsshubUnhealthy',
  unknown: 'rsshubUnknown',
} as const

/** These admin endpoints answer `{ success, error, ... }` on every status. */
interface RsshubEnvelope {
  success?: boolean
  error?: string
  /** Present whenever a single-instance call reports success. */
  instance: RsshubInstance
  instances?: RsshubInstance[]
}

/** A rejection's body is the same envelope, so callers keep one success check; only transport failures throw. */
async function rsshubRequest(
  path: string,
  init: { method?: 'POST' | 'PUT' | 'DELETE', body?: unknown } = {},
): Promise<RsshubEnvelope> {
  try {
    return await apiService.request<RsshubEnvelope>(`/phantasi/rsshub${path}`, {
      method: init.method ?? 'GET',
      body: init.body === undefined ? undefined : JSON.stringify(init.body),
    })
  } catch (error) {
    if (error instanceof ApiError && error.status > 0 && error.body && typeof error.body === 'object') {
      return error.body as RsshubEnvelope
    }
    throw error
  }
}

function emptyDraft() {
  return { name: '', url: '', accessKey: '', priority: 100 }
}

export function RSSHubInstances({
  disabled = false,
  defaultOpen = false,
  layout = 'embed',
  initialUrl,
  onChange,
}: {
  disabled?: boolean
  defaultOpen?: boolean
  layout?: 'embed' | 'page'
  initialUrl?: string
  onChange: (instance: RsshubInstance | null) => void
}) {
  const compact = layout === 'embed'
  const { t, locale } = useI18n()
  const phantasi = t.phantasi
  const onChangeRef = useRef(onChange)
  onChangeRef.current = onChange
  const pickedRef = useRef(false)

  const [instances, setInstances] = useState<RsshubInstance[]>([])
  const [loading, setLoading] = useState(true)
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [openId, setOpenId] = useState<number | null>(null)
  const [panelOpen, setPanelOpen] = useState(defaultOpen || layout === 'page')
  const [formOpen, setFormOpen] = useState(false)
  const [draft, setDraft] = useState(emptyDraft)
  const [adding, setAdding] = useState(false)
  const [editingId, setEditingId] = useState<number | null>(null)
  const [edit, setEdit] = useState(emptyDraft)
  const [checkingId, setCheckingId] = useState<number | null>(null)
  const [checkingAll, setCheckingAll] = useState(false)
  const [savingId, setSavingId] = useState<number | null>(null)

  const current = useMemo(
    () =>
      instances.find((item) => item.id === selectedId) ??
      instances.find((item) => item.enabled) ??
      instances[0] ??
      null,
    [instances, selectedId],
  )

  useEffect(() => {
    onChangeRef.current(current)
  }, [current])

  const load = useCallback(async () => {
    try {
      setLoading(true)
      const data = await rsshubRequest('/instances')
      if (!data.success) {
        showError(data.error || phantasi.errorLoadFailed)
        return
      }
      const next = (data.instances || []) as RsshubInstance[]
      setInstances(next)
      if (pickedRef.current) return
      pickedRef.current = true
      const matched = initialUrl
        ? next.find((item) => item.url === initialUrl)
        : undefined
      const enabled = next.find((item) => item.enabled)
      setSelectedId(matched?.id ?? enabled?.id ?? next[0]?.id ?? null)
    } catch {
      showError(phantasi.errorNetworkRetry)
    } finally {
      setLoading(false)
    }
  }, [phantasi.errorLoadFailed, phantasi.errorNetworkRetry, initialUrl])

  useEffect(() => {
    void load()
  }, [load])

  const formatTime = (timestamp: number | null) => {
    if (!timestamp) return phantasi.rsshubNever
    return new Date(timestamp).toLocaleString(locale, {
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  const handleAdd = async () => {
    if (!draft.name.trim() || !draft.url.trim()) return
    try {
      setAdding(true)
      const data = await rsshubRequest('/instances', {
        method: 'POST',
        body: {
          name: draft.name.trim(),
          url: draft.url.trim().replaceAll(/\/$/g, ''),
          access_key: draft.accessKey.trim() || null,
          priority: draft.priority,
        },
      })
      if (!data.success) {
        showError(data.error || phantasi.errorAddFailed)
        return
      }
      setInstances((prev) => [...prev, data.instance])
      setSelectedId(data.instance.id)
      setFormOpen(false)
      setDraft(emptyDraft())
    } catch {
      showError(phantasi.errorNetworkRetry)
    } finally {
      setAdding(false)
    }
  }

  const handleUpdate = async (id: number) => {
    try {
      setSavingId(id)
      const data = await rsshubRequest(`/instances/${id}`, {
        method: 'PUT',
        body: {
          name: edit.name.trim() || undefined,
          url: edit.url.trim().replaceAll(/\/$/g, '') || undefined,
          access_key: edit.accessKey.trim() || undefined,
          priority: edit.priority,
        },
      })
      if (!data.success) {
        showError(data.error || phantasi.errorUpdateFailed)
        return
      }
      setInstances((prev) =>
        prev.map((item) => (item.id === id ? data.instance : item)),
      )
      setEditingId(null)
    } catch {
      showError(phantasi.errorNetworkRetry)
    } finally {
      setSavingId(null)
    }
  }

  const handleDelete = async (id: number) => {
    try {
      const data = await rsshubRequest(`/instances/${id}`, { method: 'DELETE' })
      if (!data.success) {
        showError(data.error || phantasi.errorDeleteFailed)
        return
      }
      setInstances((prev) => {
        const next = prev.filter((item) => item.id !== id)
        if (selectedId === id) {
          setSelectedId(next.find((item) => item.enabled)?.id ?? next[0]?.id ?? null)
        }
        return next
      })
      if (openId === id) setOpenId(null)
      if (editingId === id) setEditingId(null)
    } catch {
      showError(phantasi.errorNetworkRetry)
    }
  }

  const handleToggle = async (instance: RsshubInstance) => {
    try {
      setSavingId(instance.id)
      const data = await rsshubRequest(`/instances/${instance.id}`, { method: 'PUT', body: { enabled: !instance.enabled } })
      if (!data.success) {
        showError(data.error || phantasi.errorSaveFailed)
        return
      }
      setInstances((prev) =>
        prev.map((item) =>
          item.id === instance.id ? data.instance : item,
        ),
      )
    } catch {
      showError(phantasi.errorNetworkRetry)
    } finally {
      setSavingId(null)
    }
  }

  const handleCheck = async (id: number) => {
    try {
      setCheckingId(id)
      const data = await rsshubRequest(`/instances/${id}/health-check`, { method: 'POST' })
      if (data.success) await load()
    } catch {
      showError(phantasi.errorHealthCheckFailed)
    } finally {
      setCheckingId(null)
    }
  }

  const handleCheckAll = async () => {
    try {
      setCheckingAll(true)
      const data = await rsshubRequest('/health-check-all', { method: 'POST' })
      if (data.success) await load()
    } catch {
      showError(phantasi.errorHealthCheckFailed)
    } finally {
      setCheckingAll(false)
    }
  }

  const handleReset = async (id: number) => {
    try {
      const data = await rsshubRequest(`/instances/${id}/reset`, { method: 'POST' })
      if (data.success) await load()
    } catch {
      showError(phantasi.errorResetFailed)
    }
  }

  const startEdit = (instance: RsshubInstance) => {
    setEditingId(instance.id)
    setOpenId(instance.id)
    setSelectedId(instance.id)
    setEdit({
      name: instance.name,
      url: instance.url,
      accessKey: '',
      priority: instance.priority,
    })
    setFormOpen(false)
    setPanelOpen(true)
  }

  const closePanel = () => {
    setPanelOpen(false)
    setFormOpen(false)
    setEditingId(null)
    setOpenId(null)
  }

  const items: ManagedListItem[] = instances.map((instance) => {
    const selected = selectedId === instance.id
    const editing = editingId === instance.id
    const busy = savingId === instance.id || checkingId === instance.id
    return {
      id: instance.id,
      title: (
        <span className="phantasi-rsshub-instances__name">
          {instance.name}
          {instance.has_access_key ? (
            <Key aria-label={phantasi.rsshubHasAccessKey} />
          ) : null}
        </span>
      ),
      subtitle: instance.url,
      className: selected ? 'is-on' : instance.enabled ? '' : 'is-off',
      busy,
      badge: {
        label: phantasi[HEALTH_LABEL[instance.health_status]],
        tone: instance.enabled
          ? HEALTH_TONE[instance.health_status]
          : 'muted',
      },
      trailing: (
        <ToggleSwitch
          checked={instance.enabled}
          disabled={disabled || busy}
          aria-label={
            instance.enabled ? phantasi.rsshubDisable : phantasi.rsshubEnable
          }
          onChange={() => {
            void handleToggle(instance)
          }}
        />
      ),
      expanded: openId === instance.id,
      onToggleExpand: () => {
        if (disabled) return
        setSelectedId(instance.id)
        setOpenId((cur) => (cur === instance.id ? null : instance.id))
      },
      expandContent: editing ? (
        <div className="phantasi-rsshub-instances__form">
          <InputItem
            itemKey={`rsshub-edit-name-${instance.id}`}
            size="sm"
            label={phantasi.rsshubInstanceName}
            value={edit.name}
            onChange={(name) => setEdit((prev) => ({ ...prev, name }))}
            disabled={busy}
          />
          <InputItem
            itemKey={`rsshub-edit-url-${instance.id}`}
            size="sm"
            label={phantasi.rsshubInstanceUrl}
            inputType="url"
            value={edit.url}
            onChange={(url) => setEdit((prev) => ({ ...prev, url }))}
            disabled={busy}
          />
          <InputItem
            itemKey={`rsshub-edit-key-${instance.id}`}
            size="sm"
            label={phantasi.rsshubAccessKey}
            inputType="password"
            value={edit.accessKey}
            onChange={(accessKey) =>
              setEdit((prev) => ({ ...prev, accessKey }))
            }
            placeholder={phantasi.rsshubAccessKeyKeep}
            disabled={busy}
          />
          <NumberItem
            itemKey={`rsshub-edit-priority-${instance.id}`}
            size="sm"
            label={phantasi.rsshubPriority}
            hint={phantasi.rsshubPriorityHint}
            value={edit.priority}
            onChange={(priority) =>
              setEdit((prev) => ({ ...prev, priority }))
            }
            min={0}
            max={999}
            disabled={busy}
          />
          <div className="managed-list-form-actions">
            <SettingsButton
              size="sm"
              onClick={() => setEditingId(null)}
              disabled={busy}
            >
              {phantasi.cancel}
            </SettingsButton>
            <SettingsButton
              size="sm"
              variant="primary"
              loading={busy}
              disabled={!edit.name.trim() || !edit.url.trim()}
              onClick={() => void handleUpdate(instance.id)}
            >
              {phantasi.save}
            </SettingsButton>
          </div>
        </div>
      ) : (
        <div className="phantasi-rsshub-instances__detail">
          <div className="phantasi-rsshub-instances__metrics">
            <span>
              <small>{phantasi.rsshubPriority}</small>
              {instance.priority}
            </span>
            <span>
              <small>{phantasi.rsshubSuccessRate}</small>
              {instance.success_rate.toFixed(0)}%
            </span>
            <span>
              <small>{phantasi.rsshubResponse}</small>
              {instance.last_response_time_ms
                ? `${instance.last_response_time_ms}ms`
                : '—'}
            </span>
            <span>
              <small>{phantasi.rsshubLastCheck}</small>
              {formatTime(instance.last_health_check)}
            </span>
          </div>
          <div className="phantasi-rsshub-instances__detail-actions">
            <SettingsButton
              size="sm"
              variant="ghost"
              icon={<RefreshCw />}
              loading={checkingId === instance.id}
              disabled={disabled || busy}
              onClick={() => void handleCheck(instance.id)}
            >
              {phantasi.rsshubCheck}
            </SettingsButton>
            <SettingsButton
              size="sm"
              variant="ghost"
              icon={<Pencil />}
              disabled={disabled || busy}
              onClick={() => startEdit(instance)}
            >
              {phantasi.edit}
            </SettingsButton>
            <SettingsButton
              size="sm"
              variant="ghost"
              disabled={disabled || busy}
              onClick={() => void handleReset(instance.id)}
            >
              {phantasi.rsshubReset}
            </SettingsButton>
            <SettingsButton
              size="sm"
              variant="danger"
              icon={<Trash2 />}
              confirm={phantasi.rsshubConfirmDelete}
              disabled={disabled || busy}
              onClick={() => void handleDelete(instance.id)}
            >
              {phantasi.delete}
            </SettingsButton>
          </div>
        </div>
      ),
    }
  })

  const healthVariant =
    !current?.enabled || current.health_status === 'unknown'
      ? 'muted'
      : current.health_status === 'healthy'
        ? 'default'
        : 'danger'

  const showPanel = !compact || panelOpen
  const [headerHost, setHeaderHost] = useState<HTMLElement | null>(null)

  useLayoutEffect(() => {
    if (compact) {
      setHeaderHost(null)
      return
    }
    setHeaderHost(document.getElementById('workbench-rsshub-actions'))
  }, [compact])

  const toggleForm = () => {
    setFormOpen((open) => !open)
    setEditingId(null)
    if (formOpen) setDraft(emptyDraft())
  }

  const bar = compact ? (
    <div className="phantasi-rsshub-instances__bar">
      <SettingsButton
        size="sm"
        variant="ghost"
        icon={<RefreshCw />}
        loading={checkingAll}
        disabled={disabled || loading || instances.length === 0}
        onClick={() => void handleCheckAll()}
      >
        {phantasi.rsshubCheckAll}
      </SettingsButton>
      <SettingsButton
        size="sm"
        icon={<Plus />}
        disabled={disabled}
        variant={formOpen ? 'ghost' : 'primary'}
        onClick={toggleForm}
      >
        {formOpen ? phantasi.cancel : phantasi.rsshubAddInstance}
      </SettingsButton>
    </div>
  ) : (
    <div className="phantasi-rsshub-instances__bar">
      <CheckboxCard
        variant="action"
        tone="primary"
        label={phantasi.rsshubCheckAll}
        description={phantasi.workbenchRsshub}
        icon={<RefreshCw />}
        showIndicator={false}
        checked={false}
        loading={checkingAll}
        disabled={disabled || loading || instances.length === 0}
        className="setting-section-header-action"
        onChange={() => {
          void handleCheckAll()
        }}
      />
      <CheckboxCard
        variant="action"
        tone="primary"
        label={formOpen ? phantasi.cancel : phantasi.rsshubAddInstance}
        description={phantasi.workbenchRsshub}
        icon={<Plus />}
        showIndicator={false}
        checked={false}
        disabled={disabled}
        className="setting-section-header-action"
        onChange={toggleForm}
      />
    </div>
  )

  return (
    <div
      className={`phantasi-rsshub-instances${showPanel ? ' is-open' : ''}${compact ? '' : ' is-page'}${disabled ? ' is-disabled' : ''}`}
    >
      {compact ? (
        <button
          type="button"
          className="phantasi-rsshub-instances__summary"
          disabled={disabled}
          aria-expanded={panelOpen}
          onClick={() => {
            if (disabled) return
            if (panelOpen) closePanel()
            else setPanelOpen(true)
          }}
        >
          <span className="phantasi-rsshub-instances__summary-main">
            <span className="phantasi-rsshub-instances__summary-name">
              {loading ? phantasi.loading : current?.name ?? phantasi.rsshubNoInstance}
              {current?.has_access_key ? <Key /> : null}
            </span>
            <span className="phantasi-rsshub-instances__summary-url">
              {current?.url ?? phantasi.rsshubClickToAdd}
            </span>
          </span>
          <span className="phantasi-rsshub-instances__summary-side">
            {loading ? (
              <Spinner size="xs" />
            ) : current ? (
              <SettingTitleTag variant={healthVariant}>
                {phantasi[HEALTH_LABEL[current.health_status]]}
              </SettingTitleTag>
            ) : null}
            {instances.length > 0 ? (
              <SettingTitleTag variant="muted">
                {instances.length}
              </SettingTitleTag>
            ) : null}
            <ChevronDown className="phantasi-rsshub-instances__chevron" />
          </span>
        </button>
      ) : null}

      {!compact && headerHost ? createPortal(bar, headerHost) : null}

      {showPanel ? (
        <div className="phantasi-rsshub-instances__panel">
          {compact || !headerHost ? bar : null}

          {formOpen ? (
            <div className="phantasi-rsshub-instances__form">
              <InputItem
                itemKey="rsshub-new-name"
                size="sm"
                label={phantasi.rsshubInstanceName}
                value={draft.name}
                onChange={(name) => setDraft((prev) => ({ ...prev, name }))}
                disabled={adding}
              />
              <InputItem
                itemKey="rsshub-new-url"
                size="sm"
                label={phantasi.rsshubInstanceUrl}
                inputType="url"
                value={draft.url}
                onChange={(url) => setDraft((prev) => ({ ...prev, url }))}
                placeholder="https://rsshub.app"
                disabled={adding}
              />
              <InputItem
                itemKey="rsshub-new-key"
                size="sm"
                label={phantasi.rsshubAccessKey}
                inputType="password"
                value={draft.accessKey}
                onChange={(accessKey) =>
                  setDraft((prev) => ({ ...prev, accessKey }))
                }
                disabled={adding}
              />
              <NumberItem
                itemKey="rsshub-new-priority"
                size="sm"
                label={phantasi.rsshubPriority}
                hint={phantasi.rsshubPriorityHint}
                value={draft.priority}
                onChange={(priority) =>
                  setDraft((prev) => ({ ...prev, priority }))
                }
                min={0}
                max={999}
                disabled={adding}
              />
              <div className="managed-list-form-actions">
                <SettingsButton
                  size="sm"
                  variant="primary"
                  loading={adding}
                  disabled={!draft.name.trim() || !draft.url.trim()}
                  onClick={() => void handleAdd()}
                >
                  {phantasi.add}
                </SettingsButton>
              </div>
            </div>
          ) : null}

          <ManagedList
            className="phantasi-rsshub-instances__list"
            loading={loading && instances.length === 0}
            working={adding || checkingAll}
            maxHeight={compact ? '11rem' : undefined}
            items={items}
            emptyText={loading ? phantasi.loading : phantasi.rsshubNoInstances}
          />
        </div>
      ) : null}
    </div>
  )
}
