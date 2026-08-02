/**
 * AraelHeartbeatSection - 定时任务（Heartbeat）
 *
 * 卡片列表 + 频率预设 + 内联创建/编辑表单。
 */

import type { HeartbeatTask } from '../../../services/agent'
import { LuClock, LuEdit3, LuPlus, LuTrash2, LuX } from '@lib/icons'
import React, { useCallback, useMemo, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { isImeComposing } from '../../../utils/ime'

export interface AraelHeartbeatSectionProps {
  tasks: HeartbeatTask[]
  isAdmin: boolean
  onTasksChange: React.Dispatch<React.SetStateAction<HeartbeatTask[]>>
}

interface HbDraft {
  name: string
  schedule: string
  action: string
  enabled: boolean
}

const EMPTY_DRAFT: HbDraft = {
  name: '',
  schedule: '0 9 * * *',
  action: '',
  enabled: true,
}

/** Known schedule presets (cron value → id) */
const PRESET_CRONS = [
  { id: '15m', cron: '*/15 * * * *' },
  { id: '30m', cron: '*/30 * * * *' },
  { id: '1h', cron: '0 * * * *' },
  { id: '6h', cron: '0 */6 * * *' },
  { id: 'daily9', cron: '0 9 * * *' },
] as const

type PresetId = (typeof PRESET_CRONS)[number]['id'] | 'custom'

function matchPreset(cron: string): PresetId {
  const hit = PRESET_CRONS.find((p) => p.cron === cron.trim())
  return hit?.id ?? 'custom'
}

/** Cron → human readable */
export function humanizeCron(
  cron: string,
  fmt: (template: string, params: Record<string, string | number>) => string,
  arael: {
    cronEveryMinutes: string
    cronEveryHours: string
    cronHourly: string
    cronDaily: string
  },
): string {
  const raw = cron.trim()
  // */n * * * * → every n minutes
  const everyMin = raw.match(/^\*\/(\d+)\s+\*\s+\*\s+\*\s+\*$/)
  if (everyMin) {
    const n = Number(everyMin[1])
    if (n > 0) return fmt(arael.cronEveryMinutes, { n })
  }
  // 0 */n * * * → every n hours
  const everyHour = raw.match(/^0\s+\*\/(\d+)\s+\*\s+\*\s+\*$/)
  if (everyHour) {
    const n = Number(everyHour[1])
    if (n > 0) return fmt(arael.cronEveryHours, { n })
  }
  // 0 * * * * → hourly
  if (raw === '0 * * * *') return arael.cronHourly

  const parts = raw.split(/\s+/)
  if (parts.length >= 5) {
    const [min, hour, dom, mon, dow] = parts
    // daily at HH:MM
    if (
      hour &&
      min &&
      hour !== '*' &&
      min !== '*' &&
      !hour.includes('/') &&
      !min.includes('/') &&
      (dom === '*' || dom === '?') &&
      (mon === '*' || mon === '?') &&
      (dow === '*' || dow === '?')
    ) {
      return fmt(arael.cronDaily, {
        time: `${hour.padStart(2, '0')}:${min.padStart(2, '0')}`,
      })
    }
  }
  return raw
}

function formatRelativeTime(
  dateStr: string,
  arael: {
    timeJustNow: string
    timeMinutesAgo: string
    timeHoursAgo: string
    timeDaysAgo: string
  },
  fmt: (template: string, params: Record<string, string | number>) => string,
): string {
  const date = new Date(dateStr)
  if (Number.isNaN(date.getTime())) return dateStr
  const diffMin = Math.floor((Date.now() - date.getTime()) / 60000)
  if (diffMin < 1) return arael.timeJustNow
  if (diffMin < 60) return fmt(arael.timeMinutesAgo, { n: diffMin })
  const diffHour = Math.floor(diffMin / 60)
  if (diffHour < 24) return fmt(arael.timeHoursAgo, { n: diffHour })
  const diffDay = Math.floor(diffHour / 24)
  if (diffDay < 7) return fmt(arael.timeDaysAgo, { n: diffDay })
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function resultLooksError(result: string): boolean {
  const s = result.trim().toLowerCase()
  return (
    s.startsWith('error') ||
    s.startsWith('fail') ||
    s.includes('timed out') ||
    s.includes('timeout') ||
    s.includes('失败') ||
    s.includes('错误')
  )
}

const HeartbeatForm: React.FC<{
  mode: 'create' | 'edit'
  draft: HbDraft
  saving: boolean
  onChange: (next: HbDraft) => void
  onSave: () => void
  onCancel: () => void
}> = ({ mode, draft, saving, onChange, onSave, onCancel }) => {
  const { t: i18n } = useI18n()
  const a = i18n.arael
  const activePreset = matchPreset(draft.schedule)

  const presets = useMemo(
    () =>
      [
        { id: '15m' as const, label: a.cronPreset15m },
        { id: '30m' as const, label: a.cronPreset30m },
        { id: '1h' as const, label: a.cronPreset1h },
        { id: '6h' as const, label: a.cronPreset6h },
        { id: 'daily9' as const, label: a.cronPresetDaily9 },
        { id: 'custom' as const, label: a.heartbeatScheduleCustom },
      ] as const,
    [
      a.cronPreset15m,
      a.cronPreset30m,
      a.cronPreset1h,
      a.cronPreset6h,
      a.cronPresetDaily9,
      a.heartbeatScheduleCustom,
    ],
  )

  const canSave =
    draft.name.trim().length > 0 &&
    draft.schedule.trim().length > 0 &&
    draft.action.trim().length > 0 &&
    !saving

  const cronInputRef = React.useRef<HTMLInputElement>(null)

  const applyPreset = (id: PresetId) => {
    if (id === 'custom') {
      // Focus cron field so user can type a free expression
      cronInputRef.current?.focus()
      cronInputRef.current?.select()
      return
    }
    const found = PRESET_CRONS.find((p) => p.id === id)
    if (found) onChange({ ...draft, schedule: found.cron })
  }

  return (
    <div className="arael-hb-form" role="form">
      <div className="arael-hb-form-head">
        <span className="arael-hb-form-title">
          {mode === 'create' ? a.createHeartbeat : a.editHeartbeat}
        </span>
        <button
          type="button"
          className="arael-hb-icon-btn"
          onClick={onCancel}
          disabled={saving}
          aria-label={i18n.common.cancel}
        >
          <LuX size={14} />
        </button>
      </div>

      <label className="arael-hb-field">
        <span className="arael-hb-field-label">{a.heartbeatName}</span>
        <input
          type="text"
          className="arael-hb-input"
          value={draft.name}
          placeholder={a.heartbeatNamePlaceholder}
          autoFocus
          disabled={saving}
          onChange={(e) => onChange({ ...draft, name: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === 'Escape') onCancel()
          }}
        />
      </label>

      <div className="arael-hb-field">
        <span className="arael-hb-field-label">{a.heartbeatSchedule}</span>
        <div
          className="arael-hb-presets"
          role="group"
          aria-label={a.heartbeatPresetsAria}
        >
          {presets.map((p) => (
            <button
              key={p.id}
              type="button"
              className={`arael-hb-preset${activePreset === p.id ? ' is-active' : ''}`}
              disabled={saving}
              onClick={() => applyPreset(p.id)}
            >
              {p.label}
            </button>
          ))}
        </div>
        <input
          ref={cronInputRef}
          type="text"
          className="arael-hb-input arael-hb-input--mono"
          value={draft.schedule}
          placeholder="0 9 * * *"
          spellCheck={false}
          disabled={saving}
          aria-label={a.heartbeatCronHint}
          onChange={(e) => onChange({ ...draft, schedule: e.target.value })}
        />
        <span className="arael-hb-field-hint">{a.heartbeatCronHint}</span>
      </div>

      <label className="arael-hb-field">
        <span className="arael-hb-field-label">{a.heartbeatAction}</span>
        <textarea
          className="arael-hb-input arael-hb-textarea"
          rows={3}
          value={draft.action}
          placeholder={a.heartbeatActionPlaceholder}
          disabled={saving}
          onChange={(e) => onChange({ ...draft, action: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && !isImeComposing(e)) {
              e.preventDefault()
              if (canSave) onSave()
            }
            if (e.key === 'Escape') onCancel()
          }}
        />
      </label>

      {mode === 'create' ? (
        <label className="arael-hb-check">
          <input
            type="checkbox"
            checked={draft.enabled}
            disabled={saving}
            onChange={(e) =>
              onChange({ ...draft, enabled: e.target.checked })
            }
          />
          <span>{a.heartbeatEnabled}</span>
        </label>
      ) : null}

      <div className="arael-hb-form-actions">
        <button
          type="button"
          className="arael-hb-btn arael-hb-btn--ghost"
          disabled={saving}
          onClick={onCancel}
        >
          {i18n.common.cancel}
        </button>
        <button
          type="button"
          className="arael-hb-btn arael-hb-btn--primary"
          disabled={!canSave}
          onClick={onSave}
        >
          {saving ? a.loading : a.saveHeartbeat}
        </button>
      </div>
    </div>
  )
}

const TaskCard: React.FC<{
  task: HeartbeatTask
  isAdmin: boolean
  busy: boolean
  onToggle: () => void
  onEdit: () => void
  onDelete: () => void
}> = ({ task, isAdmin, busy, onToggle, onEdit, onDelete }) => {
  const { t: i18n, format } = useI18n()
  const a = i18n.arael
  const scheduleLabel = humanizeCron(task.schedule, format, a)
  const lastRunLabel = task.lastRun
    ? format(a.heartbeatLastRun, {
        time: formatRelativeTime(task.lastRun, a, format),
      })
    : a.heartbeatNeverRun
  const resultError = task.lastResult
    ? resultLooksError(task.lastResult)
    : false

  return (
    <article
      className={`arael-hb-card${task.enabled ? ' is-on' : ' is-off'}`}
    >
      <div className="arael-hb-card-accent" aria-hidden />
      <div className="arael-hb-card-body">
        <div className="arael-hb-card-top">
          <div className="arael-hb-card-title-row">
            <h4 className="arael-hb-card-name">{task.name}</h4>
            <span
              className={`arael-hb-status${task.enabled ? ' is-on' : ' is-off'}`}
            >
              {task.enabled ? a.heartbeatActive : a.heartbeatPaused}
            </span>
          </div>
          {isAdmin ? (
            <button
              type="button"
              className={`arael-hb-switch${task.enabled ? ' is-on' : ''}`}
              role="switch"
              aria-checked={task.enabled}
              aria-label={
                task.enabled ? a.toggleOff : a.toggleOn
              }
              disabled={busy}
              onClick={onToggle}
            >
              <span className="arael-hb-switch-thumb" />
            </button>
          ) : null}
        </div>

        <div className="arael-hb-card-meta">
          <span className="arael-hb-chip" title={task.schedule}>
            <LuClock size={11} aria-hidden />
            {scheduleLabel}
          </span>
          <span className="arael-hb-meta-sep" aria-hidden>
            ·
          </span>
          <span className="arael-hb-meta-text">{lastRunLabel}</span>
        </div>

        {task.action ? (
          <p className="arael-hb-card-action">{task.action}</p>
        ) : null}

        {task.lastResult ? (
          <p
            className={`arael-hb-card-result${resultError ? ' is-error' : ' is-ok'}`}
            title={task.lastResult}
          >
            {task.lastResult}
          </p>
        ) : null}

        {isAdmin ? (
          <div className="arael-hb-card-actions">
            <button
              type="button"
              className="arael-hb-icon-btn"
              onClick={onEdit}
              disabled={busy}
              title={a.editHeartbeat}
              aria-label={a.editHeartbeat}
            >
              <LuEdit3 size={13} />
            </button>
            <button
              type="button"
              className="arael-hb-icon-btn arael-hb-icon-btn--danger"
              onClick={onDelete}
              disabled={busy}
              title={a.deleteHeartbeat}
              aria-label={a.deleteHeartbeat}
            >
              <LuTrash2 size={13} />
            </button>
          </div>
        ) : null}
      </div>
    </article>
  )
}

export const AraelHeartbeatSection: React.FC<AraelHeartbeatSectionProps> = ({
  tasks,
  isAdmin,
  onTasksChange,
}) => {
  const { t: i18n, format } = useI18n()
  const a = i18n.arael

  const [editingId, setEditingId] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [draft, setDraft] = useState<HbDraft>(EMPTY_DRAFT)
  const [saving, setSaving] = useState(false)
  const [togglingId, setTogglingId] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)

  const enabledCount = useMemo(
    () => tasks.filter((t) => t.enabled).length,
    [tasks],
  )

  const startCreate = useCallback(() => {
    setEditingId(null)
    setCreating(true)
    setDraft({ ...EMPTY_DRAFT })
  }, [])

  const startEdit = useCallback((task: HeartbeatTask) => {
    setCreating(false)
    setEditingId(task.id)
    setDraft({
      name: task.name,
      schedule: task.schedule,
      action: task.action,
      enabled: task.enabled,
    })
  }, [])

  const cancelForm = useCallback(() => {
    setCreating(false)
    setEditingId(null)
    setDraft({ ...EMPTY_DRAFT })
  }, [])

  const handleToggle = useCallback(
    async (taskId: string) => {
      const prev = tasks.find((t) => t.id === taskId)
      if (!prev) return
      const rolledBackEnabled = prev.enabled
      onTasksChange((list) =>
        list.map((t) =>
          t.id === taskId ? { ...t, enabled: !t.enabled } : t,
        ),
      )
      setTogglingId(taskId)
      try {
        const result = await agentService.toggleHeartbeat(taskId)
        onTasksChange((list) =>
          list.map((t) =>
            t.id === taskId ? { ...t, enabled: result.enabled } : t,
          ),
        )
      } catch (e) {
        onTasksChange((list) =>
          list.map((t) =>
            t.id === taskId ? { ...t, enabled: rolledBackEnabled } : t,
          ),
        )
        setActionError(e instanceof Error ? e.message : a.manageActionError)
      } finally {
        setTogglingId(null)
      }
    },
    [tasks, onTasksChange, a.manageActionError],
  )

  const handleSave = useCallback(async () => {
    const name = draft.name.trim()
    const schedule = draft.schedule.trim()
    const action = draft.action.trim()
    if (!name || !schedule || !action) return
    setSaving(true)
    setActionError(null)
    try {
      if (creating) {
        const created = await agentService.createHeartbeat({
          name,
          schedule,
          action,
          enabled: draft.enabled,
        })
        onTasksChange((list) => [...list, created])
      } else if (editingId) {
        const updated = await agentService.updateHeartbeat(editingId, {
          name,
          schedule,
          action,
        })
        onTasksChange((list) =>
          list.map((t) => (t.id === editingId ? { ...t, ...updated } : t)),
        )
      }
      cancelForm()
    } catch (e) {
      setActionError(e instanceof Error ? e.message : a.manageActionError)
    } finally {
      setSaving(false)
    }
  }, [
    draft,
    creating,
    editingId,
    onTasksChange,
    cancelForm,
    a.manageActionError,
  ])

  const handleDelete = useCallback(
    async (task: HeartbeatTask) => {
      const ok = window.confirm(
        format(a.confirmDeleteHeartbeat, { name: task.name }),
      )
      if (!ok) return
      setActionError(null)
      let snapshot: HeartbeatTask[] | null = null
      onTasksChange((list) => {
        snapshot = list
        return list.filter((t) => t.id !== task.id)
      })
      if (editingId === task.id) cancelForm()
      try {
        await agentService.deleteHeartbeat(task.id)
      } catch (e) {
        if (snapshot) onTasksChange(snapshot)
        setActionError(e instanceof Error ? e.message : a.manageActionError)
      }
    },
    [
      editingId,
      onTasksChange,
      cancelForm,
      format,
      a.confirmDeleteHeartbeat,
      a.manageActionError,
    ],
  )

  const formOpen = creating || editingId != null

  return (
    <div className="arael-hb">
      {(isAdmin && !formOpen) || tasks.length > 0 ? (
        <div className="arael-hb-header">
          {tasks.length > 0 ? (
            <span className="arael-hb-header-count">
              {format(a.heartbeatCount, {
                n: tasks.length,
                active: enabledCount,
              })}
            </span>
          ) : (
            <span />
          )}
          {isAdmin && !formOpen ? (
            <button
              type="button"
              className="arael-hb-btn arael-hb-btn--primary arael-hb-btn--sm"
              onClick={startCreate}
            >
              <LuPlus size={13} aria-hidden />
              {a.createHeartbeat}
            </button>
          ) : null}
        </div>
      ) : null}

      {actionError ? (
        <div className="arael-hb-action-error" role="alert">
          <span>{actionError}</span>
          <button
            type="button"
            className="arael-hb-icon-btn"
            onClick={() => setActionError(null)}
            aria-label={i18n.common.close}
          >
            <LuX size={12} />
          </button>
        </div>
      ) : null}

      {isAdmin && formOpen ? (
        <HeartbeatForm
          mode={creating ? 'create' : 'edit'}
          draft={draft}
          saving={saving}
          onChange={setDraft}
          onSave={() => void handleSave()}
          onCancel={cancelForm}
        />
      ) : null}

      {tasks.length === 0 && !formOpen ? (
        <div className="arael-hb-empty">
          <div className="arael-hb-empty-icon" aria-hidden>
            <LuClock size={22} />
          </div>
          <p className="arael-hb-empty-title">{a.emptyHeartbeat}</p>
          <p className="arael-hb-empty-hint">{a.emptyHeartbeatHint}</p>
        </div>
      ) : (
        <div className="arael-hb-list">
          {tasks.map((task) =>
            editingId === task.id ? null : (
              <TaskCard
                key={task.id}
                task={task}
                isAdmin={isAdmin}
                busy={saving || togglingId === task.id}
                onToggle={() => void handleToggle(task.id)}
                onEdit={() => startEdit(task)}
                onDelete={() => void handleDelete(task)}
              />
            ),
          )}
        </div>
      )}
    </div>
  )
}

export default AraelHeartbeatSection
