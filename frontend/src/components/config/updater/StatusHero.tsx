/**
 * Updater status hero, progress card, and auto-check prefs.
 */

import type { Job, ReleaseManifest, UpdaterStatus } from '../../../services/updaterApi'
import type { ChannelKey, Mood, Toast, Tone, U } from './helpers'
import { LuRefreshCw } from '@lib/icons'
import React from 'react'
import {
  FieldSelect,
  SettingsButton,
  SettingTitleGuideEntry,
  ToggleSwitch,
  useSettingGuide,
} from '../../settings'
import { Spinner } from '../../Spinner'
import {

  channelLabel,
  COMMIT_URL,
  format,
  formatAgo,

  moodText,

} from './helpers'

export function StatusHero({
  mood,
  status,
  available,
  sel,
  busy,
  loading,
  tokenRequired,
  requiresSelfUpdate,
  stale,
  autoRechecking,
  pendingConfirm,
  nowTick,
  toast,
  linkDown,
  u,
  onCheck,
  onUpdate,
  onSelfUpdate,
  onRetry,
  onSaveAutoPrefs,
}: {
  mood: Mood
  status: UpdaterStatus | null
  available: ReleaseManifest | null
  sel: ChannelKey
  busy: string | null
  loading: boolean
  tokenRequired: boolean
  requiresSelfUpdate: boolean
  stale: boolean
  autoRechecking: boolean
  pendingConfirm: boolean
  nowTick: number
  toast: Toast
  /** Brief admin↔updater/proxy blip (self-update / proxy recreate). */
  linkDown: boolean
  u: U
  onCheck: () => void
  onUpdate: () => void
  onSelfUpdate: () => void
  onRetry: () => void
  onSaveAutoPrefs: (prefs: {
    check_interval_secs?: number
    auto_install?: boolean
  }) => Promise<void>
}) {
  const { title: moodTitle, hint, tone: moodTone } = moodText(mood, u)
  const latest = status?.latest_available
  const targetVersion = available?.version ?? latest?.version
  const relation = available?.relation ?? latest?.relation
  const aheadBy = available?.ahead_by ?? latest?.ahead_by
  const behindBy = available?.behind_by ?? latest?.behind_by
  const notesUrl = available?.notes_url || latest?.notes_url || null
  const source = available?.source ?? latest?.source
  const irreversible = available?.migrations?.irreversible === true
  const showUpdateDetails =
    !stale && (mood === 'available' || mood === 'downgrade') && !!targetVersion

  let freshness: string | null = null
  if (relation === 'ahead' && aheadBy != null) {
    freshness = format(u.updaterFreshnessAhead, { n: String(aheadBy) })
  } else if (relation === 'behind' && behindBy != null) {
    freshness = format(u.updaterFreshnessBehind, { n: String(behindBy) })
  } else if (relation === 'identical') {
    freshness = u.updaterFreshnessIdentical
  } else if (relation === 'diverged') {
    freshness = format(u.updaterFreshnessDiverged, {
      ahead: String(aheadBy ?? 0),
      behind: String(behindBy ?? 0),
    })
  } else if (relation === 'unknown') {
    freshness = u.updaterFreshnessUnknown
  }
  // While cache is stale, prefer recheck over acting on cached “update available”.
  const showCheckPrimary =
    mood === 'healthy' ||
    mood === 'firstRun' ||
    pendingConfirm ||
    (stale &&
      mood !== 'offline' &&
      mood !== 'updating' &&
      mood !== 'maintenance' &&
      mood !== 'needsManual')

  let title = moodTitle
  let tone: Tone = moodTone
  if (pendingConfirm) {
    title = `${moodTitle} · ${u.updaterStatusUnconfirmed}`
    tone = 'warn'
  } else if (stale && (mood === 'healthy' || mood === 'firstRun')) {
    tone = 'warn'
  }

  // direct 模式没填 token 时连不上是意料之中——提示填 token，而不是让用户去查 backend 配置。
  let effectiveHint: string | null =
    mood === 'offline' && tokenRequired ? u.updaterTokenRequiredDirect : hint
  if (stale && mood !== 'offline' && mood !== 'updating') {
    if (autoRechecking || busy === 'check') {
      effectiveHint = u.updaterCheckStale
    } else {
      effectiveHint = u.updaterCheckStaleAction
    }
  }
  // Prefer reconnect copy while infra is recreating — stronger than stale check.
  if (linkDown && mood !== 'offline') {
    if (busy === 'proxy-update') {
      effectiveHint = u.updaterProxyUpdateReconnecting
    } else if (busy === 'self-update') {
      effectiveHint = u.updaterSelfUpdateReconnecting
    } else {
      effectiveHint = u.updaterSelfUpdateReconnecting
    }
  }

  let action: React.ReactNode = null
  if (showCheckPrimary) {
    action = (
      <SettingsButton
        variant="primary"
        onClick={onCheck}
        disabled={busy === 'check' || loading || tokenRequired}
        loading={busy === 'check' || autoRechecking}
        icon={<LuRefreshCw size={13} />}
      >
        {u.updaterCheckNow}
      </SettingsButton>
    )
  } else if (mood === 'available' || mood === 'downgrade') {
    action = requiresSelfUpdate ? (
      <SettingsButton
        variant="primary"
        onClick={onSelfUpdate}
        disabled={busy === 'self-update' || tokenRequired}
        loading={busy === 'self-update'}
      >
        {u.updaterSelfUpdateButton}
      </SettingsButton>
    ) : (
      <SettingsButton
        variant={mood === 'downgrade' ? 'secondary' : 'primary'}
        onClick={onUpdate}
        disabled={busy === 'update' || busy === 'check' || tokenRequired}
        loading={busy === 'update' || busy === 'check'}
      >
        {mood === 'downgrade'
          ? format(u.updaterDowngradeNow, {
              version: targetVersion ?? '…',
            })
          : u.updaterUpdateNow}
      </SettingsButton>
    )
  } else if (mood === 'offline') {
    action = (
      <SettingsButton
        variant="secondary"
        onClick={onRetry}
        disabled={loading || tokenRequired}
        loading={loading}
        icon={<LuRefreshCw size={13} />}
      >
        {u.updaterRetry}
      </SettingsButton>
    )
  }

  const lastCheckedAbs = status?.last_checked_at
    ? new Date(status.last_checked_at).toLocaleString()
    : null
  const checking = busy === 'check' || autoRechecking

  return (
    <div
      className={`updater-hero tone-${tone}${stale ? ' stale' : ''}${busy ? ' is-busy' : ''}`}
    >
      <div className="updater-hero-main">
        <span className="updater-status-dot" aria-hidden="true" />
        <div className="updater-hero-text">
          <div className="updater-hero-status">
            {title}
            {(mood === 'available' || mood === 'downgrade') &&
              targetVersion && (
                <>
                  {' '}
                  <code>{targetVersion}</code>
                </>
              )}
          </div>
          <div className={`updater-hero-subtitle${stale ? ' stale' : ''}`}>
            {effectiveHint && (
              <span className="updater-hero-hint">{effectiveHint}</span>
            )}
            <span className="updater-hero-context">
              <span>
                {u.updaterCurrentVersion}{' '}
                {status?.current_version ? (
                  <>
                    <code>{status.current_version}</code>
                    {status.current_commit_sha && (
                      <a
                        className="updater-commit-link"
                        href={`${COMMIT_URL}${status.current_commit_sha}`}
                        target="_blank"
                        rel="noreferrer noopener"
                        title={status.current_commit_sha}
                      >
                        {status.current_commit_sha.slice(0, 7)}
                      </a>
                    )}
                  </>
                ) : (
                  <em>{u.updaterUnknown}</em>
                )}
              </span>
              <span aria-hidden="true">·</span>
              <span>
                {u.updaterChannelLabel} {channelLabel(sel, u)}
              </span>
              {status?.last_checked_at && (
                <>
                  <span aria-hidden="true">·</span>
                  <span title={lastCheckedAbs ?? undefined}>
                    {u.updaterLastChecked}{' '}
                    {formatAgo(status.last_checked_at, u, nowTick)}
                  </span>
                </>
              )}
              {!showCheckPrimary && mood !== 'offline' && (
                <button
                  type="button"
                  className="updater-hero-recheck"
                  onClick={onCheck}
                  disabled={busy === 'check' || loading || tokenRequired}
                >
                  {checking ? (
                    <Spinner size="xs" color="current" />
                  ) : (
                    <LuRefreshCw size={12} />
                  )}
                  <span>{u.updaterCheckNow}</span>
                </button>
              )}
            </span>
          </div>
        </div>
        {action && <div className="updater-hero-action">{action}</div>}
      </div>
      {showUpdateDetails &&
        (freshness ||
          source === 'dockerhub' ||
          requiresSelfUpdate ||
          irreversible ||
          notesUrl) && (
          <div className="updater-hero-details">
            {freshness && <p>{freshness}</p>}
            {source === 'dockerhub' && (
              <p className="updater-hero-warning">
                {u.updaterDockerHubFallback}
              </p>
            )}
            {requiresSelfUpdate && (
              <p className="updater-hero-warning">
                {format(u.updaterSelfUpdateNeeded, {
                  version: targetVersion,
                  minVersion: latest?.min_updater_version ?? '—',
                })}
              </p>
            )}
            {irreversible && (
              <p className="updater-hero-warning">
                {u.updaterIrreversibleWarn}
              </p>
            )}
            {notesUrl && (
              <a
                className="updater-hero-notes"
                href={notesUrl}
                target="_blank"
                rel="noreferrer noopener"
              >
                {u.updaterReleaseNotes} ↗
              </a>
            )}
          </div>
        )}
      <AutoUpdatePrefs
        status={status}
        disabled={!!busy || tokenRequired}
        u={u}
        onSave={onSaveAutoPrefs}
      />
      {toast && (
        <div
          className={`updater-hero-feedback ${toast.kind}`}
          role={toast.kind === 'error' ? 'alert' : 'status'}
          aria-live="polite"
        >
          <span className="updater-feedback-mark" aria-hidden="true" />
          <span>{toast.text}</span>
        </div>
      )}
    </div>
  )
}

// ===== 进度卡 =====

export function ProgressCard({ job, u }: { job: Job; u: U }) {
  const done = job.steps.filter((s) => s.ok === true).length
  const total = Math.max(job.steps.length, done + 1)
  const pct = Math.min(99, Math.round((done / total) * 100))
  const currentStep = job.steps.at(-1)
  return (
    <div className="updater-progress">
      <div className="updater-progress-head">
        <h4 className="updater-progress-title">
          {u.updaterStatusUpdating}
          {job.to_version && (
            <>
              {' '}
              · <code>{job.to_version}</code>
            </>
          )}
        </h4>
        <span className="updater-progress-counts">
          {done} / {total}
        </span>
      </div>
      <p className="updater-progress-hint">{u.updaterHintUpdating}</p>
      <p className="updater-progress-hint muted">
        {u.updaterProgressOnMaintenance}
      </p>
      <p className="updater-progress-phase">
        {currentStep?.phase ?? job.status}
      </p>
      <div className="updater-progress-bar">
        <div
          className={`updater-progress-bar-fill ${currentStep?.finished_at ? '' : 'indeterminate'}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <details className="updater-progress-details">
        <summary>{u.updaterStepLog}</summary>
        <ol className="updater-progress-steps">
          {job.steps.map((s, i) => (
            <li key={i}>
              <span
                className={
                  s.ok === true
                    ? 'updater-step-ok'
                    : s.ok === false
                      ? 'updater-step-err'
                      : ''
                }
              >
                <code>{s.phase}</code>
              </span>
              {s.error && (
                <span className="updater-step-err-msg">{s.error}</span>
              )}
            </li>
          ))}
        </ol>
      </details>
    </div>
  )
}

// ===== 自动检查频率 + 自动安装 =====

const INTERVAL_OPTIONS: Array<{
  value: number
  labelKey:
    | 'updaterCheckIntervalOff'
    | 'updaterCheckInterval1h'
    | 'updaterCheckInterval6h'
    | 'updaterCheckInterval12h'
    | 'updaterCheckInterval24h'
}> = [
  { value: 0, labelKey: 'updaterCheckIntervalOff' },
  { value: 3600, labelKey: 'updaterCheckInterval1h' },
  { value: 21600, labelKey: 'updaterCheckInterval6h' },
  { value: 43200, labelKey: 'updaterCheckInterval12h' },
  { value: 86400, labelKey: 'updaterCheckInterval24h' },
]

export function AutoUpdatePrefs({
  status,
  disabled,
  u,
  onSave,
}: {
  status: UpdaterStatus | null
  disabled: boolean
  u: U
  onSave: (prefs: {
    check_interval_secs?: number
    auto_install?: boolean
  }) => Promise<void>
}) {
  const { catalog: g, bindGuide } = useSettingGuide()
  const checkIntervalBinding = bindGuide(
    'updater.checkInterval',
    g.updater.checkInterval,
  )
  const autoInstallBinding = bindGuide(
    'updater.autoInstall',
    g.updater.autoInstall,
  )
  const checkIntervalGuide = checkIntervalBinding.guide
  const autoInstallGuide = autoInstallBinding.guide

  const effectiveInterval = status?.check_interval_secs ?? 3600
  const known = INTERVAL_OPTIONS.some((o) => o.value === effectiveInterval)
  const intervalValue = known ? effectiveInterval : 3600
  const autoInstall = status?.auto_install === true
  const intervalOptions = INTERVAL_OPTIONS.map((o) => ({
    value: String(o.value),
    label: u[o.labelKey],
  }))

  return (
    <div className="updater-hero-auto">
      <div className="updater-auto-prefs">
        {/* 频率行用 div：自定义下拉不能包在 label 里，否则会误触 */}
        <div
          id="cfg-g-updater-checkInterval"
          data-guide-path="updater.checkInterval"
          className="updater-auto-row updater-auto-frequency has-guide-anchor"
        >
          <span className="updater-auto-label">
            <span className="updater-auto-title">
              {u.updaterCheckInterval}
              <SettingTitleGuideEntry
                title={u.updaterCheckInterval}
                guide={checkIntervalGuide}
              />
            </span>
            <span className="updater-auto-desc">
              {u.updaterCheckIntervalDesc}
            </span>
          </span>
          <FieldSelect
            value={String(intervalValue)}
            options={intervalOptions}
            disabled={disabled || !status}
            aria-label={u.updaterCheckInterval}
            size="sm"
            onChange={(secs) => {
              void onSave({ check_interval_secs: Number(secs) })
            }}
          />
        </div>
        <div
          id="cfg-g-updater-autoInstall"
          data-guide-path="updater.autoInstall"
          className="updater-auto-row updater-auto-install has-guide-anchor"
          role="presentation"
          onClick={() => {
            if (disabled || !status) return
            void onSave({ auto_install: !autoInstall })
          }}
        >
          <span className="updater-auto-label">
            <span className="updater-auto-title">
              {u.updaterAutoInstall}
              <SettingTitleGuideEntry
                title={u.updaterAutoInstall}
                guide={autoInstallGuide}
              />
            </span>
            <span className="updater-auto-desc">
              {u.updaterAutoInstallDesc}
            </span>
          </span>
          <ToggleSwitch
            checked={autoInstall}
            disabled={disabled || !status}
            aria-label={u.updaterAutoInstall}
            onChange={(checked) => {
              void onSave({ auto_install: checked })
            }}
          />
        </div>
      </div>
    </div>
  )
}
