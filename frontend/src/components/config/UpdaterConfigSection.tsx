/**
 * Updater 内联面板 — 以「用户一眼能看懂」为核心的重设计。
 *
 * 结构（自上而下）：
 *   1. 状态卡（hero）：一句话状态 + 一行解释 + 当前版本/通道/上次检查 + 唯一主按钮
 *   2. 新版本卡：解释这次更新是什么、更新时会发生什么（纯说明，不放按钮）
 *   3. 更新通道：三张单选卡片（稳定版 / 预览版 / 开发版·跟随提交），点选即保存
 *   4. 维护与恢复：仅在更新出问题时出现
 *   5. 安装指定版本（高级，折叠）
 *   6. 备份与回退（折叠）
 *   7. 高级与诊断（折叠）
 *
 * 原「更新模式 × 频道」两个下拉合并成单一通道选择（stable / preview /
 * preview+commit），「应用频道设置」按钮被移除——点选即保存，
 * 避免草稿态与服务器态不一致。
 */

import type {
  CompareResult,
  Job,
  ReleaseListItem,
  ReleaseManifest,
  SnapshotMeta,
  TransportMode,
  UpdateMode,
  UpdaterStatus,
} from '../../services/updaterApi'
import { LuRefreshCw } from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  detectVersionDrift,
  makeUpdaterApi,
  UpdaterError,
} from '../../services/updaterApi'
import { ButtonItem, SettingGroup } from '../settings'
import './UpdaterConfigSection.css'

/** Formal release tags look like v0.2.6 (`v`-prefixed semver, matching DeployTag). */
function isReleaseTag(tag: string): boolean {
  return /^v\d+\.\d+\.\d+([.-][0-9A-Za-z.]+)?$/.test(tag.trim())
}

function modeForTarget(target: string, fallback: UpdateMode): UpdateMode {
  return isReleaseTag(target)
    ? 'release'
    : fallback === 'commit'
      ? 'commit'
      : 'release'
}

const POLL_INTERVAL = 4_000
const TEMPLATE_RE = /\{(\w+)\}/g
const COMMIT_URL = 'https://github.com/Myriad-You/Myriad/commit/'

type U = ReturnType<typeof useI18n>['t']['config']

function format(template: string, params: Record<string, string>): string {
  return template.replace(TEMPLATE_RE, (_, k) => params[k] ?? `{${k}}`)
}

/** 从 upstream 转发的错误体里提取一句人能读的话（剥掉嵌套 JSON）。 */
function upstreamDetail(message: string): string {
  const brace = message.indexOf('{')
  if (brace >= 0) {
    try {
      const parsed = JSON.parse(message.slice(brace)) as {
        error?: string
        message?: string
      }
      const inner = parsed.error ?? parsed.message
      if (inner) {
        const cut = inner.indexOf(' {')
        return (cut > 0 ? inner.slice(0, cut) : inner).trim()
      }
    } catch {
      /* fall through to raw message */
    }
  }
  return message.length > 160 ? `${message.slice(0, 160)}…` : message
}

// ===== 通道模型：三个扁平选项，取代「模式 × 频道」矩阵 =====
// 产品轨道只有 stable / preview；commit 模式仅在 preview 下有效。

type ChannelKey = 'stable' | 'preview' | 'dev'

interface ChannelOption {
  key: ChannelKey
  mode: UpdateMode
  channel: string
  badge: 'recommended' | 'dev' | null
}

const CHANNEL_OPTIONS: ChannelOption[] = [
  { key: 'stable', mode: 'release', channel: 'stable', badge: 'recommended' },
  { key: 'preview', mode: 'release', channel: 'preview', badge: null },
  { key: 'dev', mode: 'commit', channel: 'preview', badge: 'dev' },
]

function channelLabel(key: ChannelKey, u: U): string {
  switch (key) {
    case 'stable':
      return u.updaterChannelStable
    case 'preview':
      return u.updaterChannelPreview
    case 'dev':
      return u.updaterChannelDev
  }
}

function channelDesc(key: ChannelKey, u: U): string {
  switch (key) {
    case 'stable':
      return u.updaterChannelStableDesc
    case 'preview':
      return u.updaterChannelPreviewDesc
    case 'dev':
      return u.updaterChannelDevDesc
  }
}

/** 服务器保存的 (mode, channel) → 三选项之一。兼容旧命名。 */
function deriveSelection(status: UpdaterStatus | null): ChannelKey {
  if (!status) return 'stable'
  if (status.update_mode === 'commit') return 'dev'
  return status.channel === 'preview' ? 'preview' : 'stable'
}

// ===== 状态推导 =====

type Mood =
  | 'healthy'
  | 'available'
  | 'downgrade'
  | 'updating'
  | 'maintenance'
  | 'needsManual'
  | 'offline'
  | 'firstRun'

function deriveMood(status: UpdaterStatus | null): Mood {
  if (!status) return 'offline'
  if (status.job_in_flight) return 'updating'
  if (status.maintenance_phase === 'needs_manual') return 'needsManual'
  if (status.maintenance_active) return 'maintenance'
  if (!status.current_version) return 'firstRun'
  if (status.update_available) return 'available'
  if (status.downgrade_available) return 'downgrade'
  return 'healthy'
}

type Tone = 'ok' | 'info' | 'warn' | 'danger' | 'muted'

function moodText(
  mood: Mood,
  u: U,
): { title: string; hint: string | null; tone: Tone } {
  switch (mood) {
    case 'healthy':
      return {
        title: u.updaterStatusHealthy,
        hint: u.updaterHintHealthy,
        tone: 'ok',
      }
    case 'available':
      return { title: u.updaterStatusAvailable, hint: null, tone: 'info' }
    case 'downgrade':
      return { title: u.updaterStatusDowngrade, hint: null, tone: 'muted' }
    case 'updating':
      return {
        title: u.updaterStatusUpdating,
        hint: u.updaterHintUpdating,
        tone: 'warn',
      }
    case 'maintenance':
      return {
        title: u.updaterStatusMaintenance,
        hint: u.updaterHintMaintenance,
        tone: 'warn',
      }
    case 'needsManual':
      return {
        title: u.updaterStatusNeedsManual,
        hint: u.updaterHintNeedsManual,
        tone: 'danger',
      }
    case 'offline':
      return {
        title: u.updaterStatusOffline,
        hint: u.updaterHintOffline,
        tone: 'danger',
      }
    case 'firstRun':
      return {
        title: u.updaterStatusFirstRun,
        hint: u.updaterHintFirstRun,
        tone: 'muted',
      }
  }
}

export interface UpdaterInlinePanelProps {
  heading?: string
}

type Toast = { kind: 'ok' | 'error'; text: string } | null

export const UpdaterInlinePanel: React.FC<UpdaterInlinePanelProps> = ({
  heading,
}) => {
  const { t } = useI18n()
  const u = t.config

  const [transport, setTransport] = useState<TransportMode>('backend')
  const [token, setToken] = useState('')
  const api = useMemo(
    () =>
      makeUpdaterApi({
        mode: transport,
        token: transport === 'direct' ? token : undefined,
      }),
    [transport, token],
  )
  const tokenRequired = transport === 'direct' && !token

  const [status, setStatus] = useState<UpdaterStatus | null>(null)
  const [available, setAvailable] = useState<ReleaseManifest | null>(null)
  const [snapshots, setSnapshots] = useState<SnapshotMeta[]>([])
  const [activeJob, setActiveJob] = useState<Job | null>(null)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const [toast, setToast] = useState<Toast>(null)
  const [drift, setDrift] = useState<{ build: string; current: string } | null>(
    null,
  )
  const [accessDenied, setAccessDenied] = useState(false)
  const [sel, setSel] = useState<ChannelKey>('stable')
  /** 只在首次加载（或保存偏好后）用服务器值覆盖本地选择。 */
  const selHydratedRef = useRef(false)
  const pollRef = useRef<number | null>(null)

  const explain = useCallback(
    (e: unknown): string => {
      if (e instanceof UpdaterError) {
        if (e.status === 401) {
          // Backend admin session vs updater token are different failures.
          if (/admin|login|authorization|session/i.test(e.message)) {
            return u.updaterErr401Admin
          }
          return u.updaterErr401
        }
        if (e.status === 403) {
          // Do NOT map every 403 to manual-override — CSRF / admin denials also 403.
          if (/csrf/i.test(e.message)) return u.updaterErr403Csrf
          if (/admin|forbidden|permission/i.test(e.message)) {
            return u.updaterErr403Admin
          }
          if (
            /manual|override|exit-maintenance|forget-current|rescue/i.test(
              e.message,
            )
          ) {
            return u.updaterErr403
          }
          return `${u.updaterErr403Generic}: ${e.message}`
        }
        if (e.status === 409) return u.updaterErr409
        if (e.status === 412) return `${u.updaterErr412}: ${e.message}`
        if (e.status >= 500) {
          if (/not configured/i.test(e.message)) return u.updaterErrNotConfigured
          if (e.status === 502 || e.status === 503) return u.updaterErrUpstream
          return format(u.updaterErrServer, { msg: upstreamDetail(e.message) })
        }
        return `${e.status}: ${e.message}`
      }
      return String(e)
    },
    [u],
  )

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      let s: UpdaterStatus | null = null
      try {
        s = await api.status()
        if (transport === 'backend' && accessDenied) setAccessDenied(false)
      } catch (e) {
        if (
          transport === 'backend' &&
          e instanceof UpdaterError &&
          (e.status === 401 || e.status === 403)
        ) {
          setAccessDenied(true)
          setStatus(null)
          setSnapshots([])
          setActiveJob(null)
          return
        }
      }
      const snaps = await api
        .snapshots()
        .catch(() => ({ schema_version: 1, items: [] as SnapshotMeta[] }))
      setStatus(s)
      setSnapshots(snaps.items ?? [])
      if (s && !selHydratedRef.current) {
        setSel(deriveSelection(s))
        selHydratedRef.current = true
      }
      if (s?.job_in_flight) {
        const j = await api.job(s.job_in_flight).catch(() => null)
        setActiveJob(j)
      } else {
        setActiveJob(null)
      }
    } finally {
      setLoading(false)
    }
  }, [api, transport, accessDenied])

  useEffect(() => {
    refresh()
    detectVersionDrift().then((d) => {
      if (d?.drift) setDrift({ build: d.build, current: d.current })
    })
  }, [refresh])

  useEffect(() => {
    if (status?.job_in_flight) {
      pollRef.current = window.setInterval(refresh, POLL_INTERVAL)
    } else if (pollRef.current) {
      window.clearInterval(pollRef.current)
      pollRef.current = null
    }
    return () => {
      if (pollRef.current) window.clearInterval(pollRef.current)
    }
  }, [status?.job_in_flight, refresh])

  const selOption = useMemo(
    () => CHANNEL_OPTIONS.find((o) => o.key === sel) ?? CHANNEL_OPTIONS[0],
    [sel],
  )

  /** 服务器未公布 preview 轨道时，只展示稳定版。 */
  const visibleOptions = useMemo(() => {
    const fromServer = status?.available_channels
    if (!fromServer?.length) return CHANNEL_OPTIONS
    return CHANNEL_OPTIONS.filter((o) => fromServer.includes(o.channel))
  }, [status?.available_channels])

  // ===== 操作 =====

  const checkAvailable = useCallback(
    async (opt: ChannelOption = selOption) => {
      if (tokenRequired) {
        setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
        return
      }
      setBusy('check')
      try {
        const manifest = await api.available({
          channel: opt.channel,
          mode: opt.mode,
        })
        setAvailable(manifest)
        setToast(manifest ? null : { kind: 'ok', text: u.updaterNoAvailable })
        await refresh()
      } catch (e) {
        setToast({ kind: 'error', text: explain(e) })
      } finally {
        setBusy(null)
      }
    },
    [api, selOption, refresh, tokenRequired, explain, u],
  )

  const selectChannel = useCallback(
    async (key: ChannelKey) => {
      if (key === sel || busy) return
      if (tokenRequired) {
        setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
        return
      }
      const opt = CHANNEL_OPTIONS.find((o) => o.key === key)!
      const prev = sel
      setSel(key)
      setBusy('channel')
      setAvailable(null)
      try {
        await api.setPrefs({ channel: opt.channel, mode: opt.mode })
        selHydratedRef.current = true
        setToast({
          kind: 'ok',
          text: format(u.updaterChannelSaved, { label: channelLabel(key, u) }),
        })
        setBusy(null)
        // 切换后立刻按新通道重查，让状态卡马上反映结果。
        await checkAvailable(opt)
      } catch (e) {
        setSel(prev)
        setToast({ kind: 'error', text: explain(e) })
        setBusy(null)
      }
    },
    [api, sel, busy, tokenRequired, explain, u, checkAvailable],
  )

  /** 统一的更新派发：确认 → 触发 → 412 二次确认重试。 */
  const dispatchUpdate = useCallback(
    async (
      target: string,
      mode: UpdateMode,
      opts: { isDowngrade: boolean; needsRisk: boolean },
    ) => {
      if (!target) return
      if (tokenRequired) {
        setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
        return
      }
      const current = status?.current_version ?? '—'
      if (opts.isDowngrade) {
        if (
          !confirm(
            format(u.updaterConfirmDowngrade, { version: target, current }),
          )
        ) {
          return
        }
      } else if (opts.needsRisk) {
        if (!confirm(u.updaterConfirmRisk)) return
      } else if (
        !confirm(format(u.updaterConfirmUpgrade, { version: target }))
      ) {
        return
      }

      setBusy('update')
      setToast(null)
      try {
        const r = await api.triggerUpdate(target, {
          mode,
          commit: mode === 'commit',
          allowDowngrade: opts.isDowngrade,
          allowRisk: opts.needsRisk || opts.isDowngrade,
          idemKey: `update-${target}-${Date.now()}`,
        })
        setToast({
          kind: 'ok',
          text: format(u.updaterDispatched, { jobId: r.job_id }),
        })
        await refresh()
      } catch (e) {
        // 服务端要求 allow_downgrade / allow_risk —— 再确认一次后重试。
        if (
          e instanceof UpdaterError &&
          e.status === 412 &&
          /allow_downgrade|downgrade|allow_risk|diverged|unknown|irreversible/i.test(
            e.message,
          )
        ) {
          const msg = /irreversible|diverged|unknown|allow_risk/i.test(e.message)
            ? u.updaterConfirmRisk
            : format(u.updaterConfirmDowngrade, { version: target, current })
          if (confirm(msg)) {
            try {
              const r = await api.triggerUpdate(target, {
                mode,
                commit: mode === 'commit',
                allowDowngrade: true,
                allowRisk: true,
                idemKey: `update-dl-${target}-${Date.now()}`,
              })
              setToast({
                kind: 'ok',
                text: format(u.updaterDispatched, { jobId: r.job_id }),
              })
              await refresh()
              return
            } catch (e2) {
              setToast({ kind: 'error', text: explain(e2) })
              return
            }
          }
        }
        setToast({ kind: 'error', text: explain(e) })
      } finally {
        setBusy(null)
      }
    },
    [api, status, refresh, tokenRequired, explain, u],
  )

  const updateToLatest = useCallback(() => {
    const la = status?.latest_available
    const target = available?.version ?? la?.version
    if (!target) {
      checkAvailable()
      return
    }
    const mode: UpdateMode =
      (available?.mode ?? la?.mode ?? selOption.mode) === 'commit'
        ? 'commit'
        : 'release'
    const relation = available?.relation ?? la?.relation
    const isUpgrade =
      available?.is_upgrade === true || la?.is_upgrade === true
    const isDowngrade =
      available?.is_downgrade === true ||
      la?.is_downgrade === true ||
      status?.downgrade_available === true
    // Dev/commit: build-time upgrades may report relation=unknown without ancestry;
    // only force risk confirm for diverged, or unknown when not a clear upgrade.
    const needsRisk =
      relation === 'diverged' ||
      (relation === 'unknown' && !isUpgrade)
    dispatchUpdate(target, mode, { isDowngrade, needsRisk })
  }, [available, status, selOption, dispatchUpdate, checkAvailable])

  const triggerSelfUpdate = useCallback(async () => {
    if (tokenRequired) {
      setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
      return
    }
    const target = status?.latest_available?.version ?? ''
    if (!target) return
    if (!confirm(format(u.updaterSelfUpdateConfirm, { version: target }))) {
      return
    }
    setBusy('self-update')
    setToast(null)
    try {
      await api.triggerSelfUpdate()
      setToast({ kind: 'ok', text: u.updaterSelfUpdateDispatched })
    } catch (e) {
      setToast({ kind: 'error', text: explain(e) })
    } finally {
      setBusy(null)
    }
  }, [api, status, tokenRequired, explain, u])

  const rollbackTo = useCallback(
    async (snap: SnapshotMeta) => {
      if (tokenRequired) {
        setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
        return
      }
      if (
        !confirm(
          format(u.updaterConfirmRollback, {
            version: snap.source_version ?? snap.id,
          }),
        )
      ) {
        return
      }
      setBusy(`rollback-${snap.id}`)
      try {
        await api.rollback(snap.id)
        setToast({ kind: 'ok', text: u.updaterRollbackDispatched })
        await refresh()
      } catch (e) {
        setToast({ kind: 'error', text: explain(e) })
      } finally {
        setBusy(null)
      }
    },
    [api, refresh, tokenRequired, explain, u],
  )

  const exitMaintenance = useCallback(async () => {
    if (tokenRequired) {
      setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
      return
    }
    if (!confirm(u.updaterConfirmExitMaintenance)) return
    setBusy('exit-maintenance')
    try {
      await api.exitMaintenance()
      setToast({ kind: 'ok', text: u.updaterMaintenanceExited })
      await refresh()
    } catch (e) {
      setToast({ kind: 'error', text: explain(e) })
    } finally {
      setBusy(null)
    }
  }, [api, refresh, tokenRequired, explain, u])

  const rescueContinue = useCallback(async () => {
    if (tokenRequired) {
      setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
      return
    }
    const snap = status?.rescue_snapshot_id
    if (!snap) return
    if (
      !confirm(
        format(u.updaterConfirmRescueContinue, {
          snapshotId: snap,
          version: status?.rescue_source_version ?? '—',
        }),
      )
    ) {
      return
    }
    setBusy('rescue-continue')
    try {
      const res = await api.rescueContinue()
      setToast({
        kind: 'ok',
        text: `${u.updaterRescueContinueDispatched} · ${res.job_id.slice(0, 8)}`,
      })
      await refresh()
    } catch (e) {
      setToast({ kind: 'error', text: explain(e) })
    } finally {
      setBusy(null)
    }
  }, [api, refresh, tokenRequired, explain, u, status])

  // ===== 渲染 =====

  const mood = useMemo<Mood>(() => deriveMood(status), [status])

  // 非 admin：整段隐藏
  if (accessDenied && transport === 'backend') return null

  // Scheme A: admin ProgressCard only for the brief pre-maintenance window.
  // Once maintenance is active, full progress lives on the maintenance page.
  const jobRunning =
    !!activeJob && !['succeeded', 'failed', 'needs_manual'].includes(activeJob.status)
  const showProgress =
    jobRunning && !status?.maintenance_active && status?.maintenance_phase !== 'needs_manual'
  const showMaintenance =
    (mood === 'maintenance' || mood === 'needsManual') && !showProgress
  const requiresSelfUpdate =
    !!status?.requires_self_update && !!status.latest_available
  const showAvailableCard =
    !showProgress &&
    (mood === 'available' || mood === 'downgrade') &&
    !!(available || status?.latest_available)

  return (
    <div className="updater-panel">
      {heading && <h3 className="updater-panel-heading">{heading}</h3>}

      {drift && (
        <div className="updater-drift">
          {format(u.updaterDriftWarn, drift)}{' '}
          <a
            href="#"
            onClick={(e) => {
              e.preventDefault()
              location.reload()
            }}
          >
            {u.updaterDriftAction}
          </a>
        </div>
      )}

      {showProgress ? (
        <ProgressCard job={activeJob!} u={u} />
      ) : (
        <StatusHero
          mood={mood}
          status={status}
          sel={sel}
          busy={busy}
          loading={loading}
          tokenRequired={tokenRequired}
          requiresSelfUpdate={requiresSelfUpdate}
          u={u}
          onCheck={() => checkAvailable()}
          onUpdate={updateToLatest}
          onSelfUpdate={triggerSelfUpdate}
          onRetry={refresh}
        />
      )}

      {jobRunning && status?.maintenance_active && !showProgress && (
        <p className="updater-progress-hint">{u.updaterProgressOnMaintenance}</p>
      )}

      {showAvailableCard && (
        <AvailableCard
          status={status!}
          available={available}
          downgrade={mood === 'downgrade'}
          requiresSelfUpdate={requiresSelfUpdate}
          u={u}
        />
      )}

      {toast && (
        <div className={`updater-toast ${toast.kind}`}>{toast.text}</div>
      )}

      {/* ===== 更新通道：点选即保存 ===== */}
      {!showProgress && (
        <SettingGroup
          title={u.updaterChannelGroupTitle}
          description={u.updaterChannelGroupDesc}
        >
          <div className="updater-channels" role="radiogroup">
            {visibleOptions.map((opt) => (
              <button
                key={opt.key}
                type="button"
                role="radio"
                aria-checked={sel === opt.key}
                className={`updater-channel-card${sel === opt.key ? ' selected' : ''}`}
                disabled={!!busy || tokenRequired}
                onClick={() => selectChannel(opt.key)}
              >
                <span className="updater-channel-radio" aria-hidden="true" />
                <span className="updater-channel-body">
                  <span className="updater-channel-name">
                    {channelLabel(opt.key, u)}
                    {opt.badge === 'recommended' && (
                      <span className="updater-channel-badge recommended">
                        {u.updaterChannelBadgeRecommended}
                      </span>
                    )}
                    {opt.badge === 'dev' && (
                      <span className="updater-channel-badge dev">
                        {u.updaterChannelBadgeDev}
                      </span>
                    )}
                  </span>
                  <span className="updater-channel-desc">
                    {channelDesc(opt.key, u)}
                  </span>
                </span>
              </button>
            ))}
          </div>
        </SettingGroup>
      )}

      {/* ===== 自动检查 / 自动安装 ===== */}
      {!showProgress && (
        <AutoUpdatePrefs
          status={status}
          disabled={!!busy || tokenRequired}
          u={u}
          onSave={async (prefs) => {
            setBusy('auto-prefs')
            setToast(null)
            try {
              await api.setPrefs(prefs)
              setToast({ kind: 'ok', text: u.updaterAutoPrefsSaved })
              await refresh()
            } catch (e) {
              setToast({ kind: 'error', text: explain(e) })
            } finally {
              setBusy(null)
            }
          }}
        />
      )}

      {/* ===== 维护与恢复（仅出问题时出现）===== */}
      {showMaintenance && (
        <SettingGroup
          title={u.updaterMaintenanceGroup}
          description={u.updaterMaintenanceGroupDesc}
        >
          {mood === 'needsManual' && status?.rescue_snapshot_id && (
            <ButtonItem
              itemKey="rescue_continue"
              label={u.updaterRescueContinue}
              description={
                status.rescue_source_version
                  ? `${u.updaterRescueContinueDesc} · ${status.rescue_source_version}`
                  : u.updaterRescueContinueDesc
              }
              buttonText={
                busy === 'rescue-continue'
                  ? u.updaterProcessing
                  : u.updaterRescueContinue
              }
              onClick={rescueContinue}
              variant="primary"
              layout="horizontal"
              disabled={busy === 'rescue-continue' || tokenRequired}
            />
          )}
          <ButtonItem
            itemKey="exit_maintenance"
            label={u.updaterForceExit}
            description={u.updaterForceExitDesc}
            buttonText={
              busy === 'exit-maintenance'
                ? u.updaterProcessing
                : u.updaterForceExit
            }
            onClick={exitMaintenance}
            variant="danger"
            layout="horizontal"
            disabled={busy === 'exit-maintenance' || tokenRequired}
          />
        </SettingGroup>
      )}

      {/* ===== 安装指定版本（高级，折叠）===== */}
      {!showProgress && (
        <SettingGroup
          title={u.updaterTargetGroupTitle}
          description={u.updaterTargetGroupDesc}
          collapsible
          defaultExpanded={false}
        >
          <TargetPicker
            api={api}
            option={selOption}
            disabled={!!busy || tokenRequired}
            installing={busy === 'update'}
            u={u}
            onInstall={(target, opts) =>
              dispatchUpdate(
                target,
                modeForTarget(target, selOption.mode),
                opts,
              )
            }
          />
        </SettingGroup>
      )}

      {/* ===== 备份与回退（折叠）===== */}
      <SettingGroup
        title={u.updaterSnapshotGroupTitle}
        description={u.updaterSnapshotGroupDesc}
        collapsible
        defaultExpanded={false}
      >
        {snapshots.length === 0 ? (
          <p className="updater-empty">{u.updaterNoSnapshots}</p>
        ) : (
          <div className="updater-snapshot-list">
            {snapshots.map((s) => (
              <SnapshotRow
                key={s.id}
                snapshot={s}
                u={u}
                busy={busy === `rollback-${s.id}`}
                disabled={tokenRequired}
                onRollback={() => rollbackTo(s)}
              />
            ))}
          </div>
        )}
      </SettingGroup>

      {/* ===== 高级与诊断（折叠）===== */}
      <SettingGroup
        title={u.updaterGroupAdvanced}
        description={u.updaterGroupAdvancedDesc}
        collapsible
        defaultExpanded={false}
      >
        <AdvancedPanel
          status={status}
          available={available}
          transport={transport}
          token={token}
          loading={loading}
          u={u}
          onTransportChange={(m) => {
            setTransport(m)
            if (m === 'backend') setToken('')
          }}
          onTokenChange={setToken}
          onRefresh={refresh}
        />
      </SettingGroup>
    </div>
  )
}

// ===== 状态卡（hero）=====

function StatusHero({
  mood,
  status,
  sel,
  busy,
  loading,
  tokenRequired,
  requiresSelfUpdate,
  u,
  onCheck,
  onUpdate,
  onSelfUpdate,
  onRetry,
}: {
  mood: Mood
  status: UpdaterStatus | null
  sel: ChannelKey
  busy: string | null
  loading: boolean
  tokenRequired: boolean
  requiresSelfUpdate: boolean
  u: U
  onCheck: () => void
  onUpdate: () => void
  onSelfUpdate: () => void
  onRetry: () => void
}) {
  const { title, hint, tone } = moodText(mood, u)
  const targetVersion = status?.latest_available?.version
  // direct 模式没填 token 时连不上是意料之中——提示填 token，而不是让用户去查 backend 配置。
  const effectiveHint =
    mood === 'offline' && tokenRequired ? u.updaterTokenRequiredDirect : hint

  let action: React.ReactNode = null
  if (mood === 'healthy' || mood === 'firstRun') {
    action = (
      <button
        type="button"
        className="btn-base btn-primary"
        onClick={onCheck}
        disabled={busy === 'check' || loading || tokenRequired}
      >
        <LuRefreshCw size={13} />
        <span>{busy === 'check' ? u.updaterChecking : u.updaterCheckNow}</span>
      </button>
    )
  } else if (mood === 'available' || mood === 'downgrade') {
    action = requiresSelfUpdate ? (
      <button
        type="button"
        className="btn-base btn-primary"
        onClick={onSelfUpdate}
        disabled={busy === 'self-update' || tokenRequired}
      >
        <span>
          {busy === 'self-update'
            ? u.updaterProcessing
            : u.updaterSelfUpdateButton}
        </span>
      </button>
    ) : (
      <button
        type="button"
        className={
          mood === 'downgrade'
            ? 'btn-base btn-secondary'
            : 'btn-base btn-primary'
        }
        onClick={onUpdate}
        disabled={busy === 'update' || tokenRequired}
      >
        <span>
          {busy === 'update'
            ? u.updaterDispatching
            : mood === 'downgrade'
              ? format(u.updaterDowngradeNow, {
                  version: targetVersion ?? '…',
                })
              : u.updaterUpdateNow}
        </span>
      </button>
    )
  } else if (mood === 'offline') {
    action = (
      <button
        type="button"
        className="btn-base btn-secondary"
        onClick={onRetry}
        disabled={loading || tokenRequired}
      >
        <LuRefreshCw size={13} />
        <span>{loading ? u.updaterLoading : u.updaterRetry}</span>
      </button>
    )
  }

  return (
    <div className="updater-hero">
      <div className="updater-hero-main">
        <span className={`updater-hero-dot ${tone}`} aria-hidden="true" />
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
          {effectiveHint && (
            <div className="updater-hero-hint">{effectiveHint}</div>
          )}
        </div>
        {action && <div className="updater-hero-action">{action}</div>}
      </div>
      <div className="updater-hero-meta">
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
        <span>
          {u.updaterChannelLabel} {channelLabel(sel, u)}
        </span>
        {status?.last_checked_at && (
          <span>
            {u.updaterLastChecked} {formatAgo(status.last_checked_at, u)}
          </span>
        )}
        {mood !== 'healthy' && mood !== 'firstRun' && mood !== 'offline' && (
          <button
            type="button"
            className="updater-hero-recheck"
            onClick={onCheck}
            disabled={busy === 'check' || loading || tokenRequired}
          >
            <LuRefreshCw size={12} />
            <span>
              {busy === 'check' ? u.updaterChecking : u.updaterCheckNow}
            </span>
          </button>
        )}
      </div>
    </div>
  )
}

// ===== 新版本卡（纯说明，无按钮）=====

function AvailableCard({
  status,
  available,
  downgrade,
  requiresSelfUpdate,
  u,
}: {
  status: UpdaterStatus
  available: ReleaseManifest | null
  downgrade: boolean
  requiresSelfUpdate: boolean
  u: U
}) {
  const la = status.latest_available
  const version = available?.version ?? la?.version ?? '—'
  const notesUrl = available?.notes_url || la?.notes_url || null
  const relation = available?.relation ?? la?.relation
  const aheadBy = available?.ahead_by ?? la?.ahead_by
  const behindBy = available?.behind_by ?? la?.behind_by
  const irreversible = available?.migrations?.irreversible === true

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

  return (
    <div className={`updater-available${downgrade ? ' downgrade' : ''}`}>
      <div className="updater-available-head">
        <span className="updater-available-title">
          {downgrade
            ? u.updaterAvailableDowngradeTitle
            : u.updaterAvailableTitle}
        </span>
        <span className="updater-available-versions">
          <code>{status.current_version ?? '—'}</code>
          <span className="updater-available-arrow" aria-hidden="true">
            →
          </span>
          <code>{version}</code>
        </span>
      </div>
      {freshness && <p className="updater-available-freshness">{freshness}</p>}
      {(available?.source ?? la?.source) === 'dockerhub' && (
        <p className="updater-available-warn">{u.updaterDockerHubFallback}</p>
      )}
      {requiresSelfUpdate && (
        <p className="updater-available-warn">
          {format(u.updaterSelfUpdateNeeded, {
            version,
            minVersion: la?.min_updater_version ?? '—',
          })}
        </p>
      )}
      {irreversible && (
        <p className="updater-available-warn">{u.updaterIrreversibleWarn}</p>
      )}
      <div className="updater-available-steps">
        <div className="updater-available-steps-title">
          {u.updaterWhatHappensTitle}
        </div>
        <ol>
          <li>{u.updaterWhatHappens1}</li>
          <li>{u.updaterWhatHappens2}</li>
          <li>{u.updaterWhatHappens3}</li>
        </ol>
      </div>
      {notesUrl && (
        <a
          className="updater-available-notes"
          href={notesUrl}
          target="_blank"
          rel="noreferrer noopener"
        >
          {u.updaterReleaseNotes} ↗
        </a>
      )}
    </div>
  )
}

// ===== 进度卡 =====

function ProgressCard({ job, u }: { job: Job; u: U }) {
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
      <p className="updater-progress-hint muted">{u.updaterProgressOnMaintenance}</p>
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

function AutoUpdatePrefs({
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
  const effectiveInterval = status?.check_interval_secs ?? 3600
  const known = INTERVAL_OPTIONS.some((o) => o.value === effectiveInterval)
  const intervalValue = known ? effectiveInterval : 3600
  const autoInstall = status?.auto_install === true

  return (
    <SettingGroup
      title={u.updaterAutoGroupTitle}
      description={u.updaterAutoGroupDesc}
    >
      <div className="updater-auto-prefs">
        <label className="updater-auto-row">
          <span className="updater-auto-label">
            <span className="updater-auto-title">{u.updaterCheckInterval}</span>
            <span className="updater-auto-desc">{u.updaterCheckIntervalDesc}</span>
          </span>
          <select
            className="updater-select"
            value={intervalValue}
            disabled={disabled || !status}
            onChange={(e) => {
              const secs = Number(e.target.value)
              void onSave({ check_interval_secs: secs })
            }}
          >
            {INTERVAL_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {u[o.labelKey]}
              </option>
            ))}
          </select>
        </label>
        <label className="updater-auto-row">
          <span className="updater-auto-label">
            <span className="updater-auto-title">{u.updaterAutoInstall}</span>
            <span className="updater-auto-desc">{u.updaterAutoInstallDesc}</span>
          </span>
          <input
            type="checkbox"
            checked={autoInstall}
            disabled={disabled || !status}
            onChange={(e) => {
              void onSave({ auto_install: e.target.checked })
            }}
          />
        </label>
      </div>
    </SettingGroup>
  )
}

// ===== 安装指定版本（高级）=====

type PickerItem = {
  key: string
  tag: string
  label: string
  message: string
  date: string | null
  kind: 'commit' | 'release'
  title?: string
}

function TargetPicker({
  api,
  option,
  disabled,
  installing,
  u,
  onInstall,
}: {
  api: ReturnType<typeof makeUpdaterApi>
  option: ChannelOption
  disabled: boolean
  installing: boolean
  u: U
  onInstall: (
    target: string,
    opts: { isDowngrade: boolean; needsRisk: boolean },
  ) => void
}) {
  const [items, setItems] = useState<PickerItem[]>([])
  const [selected, setSelected] = useState('')
  const [input, setInput] = useState('')
  const [compare, setCompare] = useState<CompareResult | null>(null)
  const [listLoading, setListLoading] = useState(true)
  const [targetSource, setTargetSource] = useState<'github' | 'dockerhub'>(
    'github',
  )
  const compareTimerRef = useRef<number | null>(null)

  const isCommit = option.mode === 'commit'
  const target = (isCommit && input.trim()) || selected

  useEffect(() => {
    let cancelled = false
    setListLoading(true)
    setSelected('')
    setInput('')
    setItems([])
    setTargetSource('github')

    const load = async () => {
      if (!isCommit) {
        const response = await api.releases({
          channel: option.channel,
          limit: 25,
        })
        if (cancelled) return
        setItems(
          (response.items ?? []).map((r: ReleaseListItem) => ({
            key: r.tag_name,
            tag: r.tag_name,
            label: r.tag_name,
            message: `${r.name || r.tag_name}${r.prerelease ? ' (pre)' : ''}`,
            date: null,
            kind: 'release' as const,
          })),
        )
        return
      }

      // Dev / commit mode: show formal releases + commit builds.
      // Prefer Docker Hub common builds (includes vX.Y.Z + dev-sha); fall back to
      // GitHub commits + releases when builds are empty.
      try {
        const builds = await api.builds({ limit: 25 })
        const buildItems = builds.items ?? []
        if (buildItems.length > 0) {
          if (cancelled) return
          setTargetSource('dockerhub')
          setItems(
            buildItems.map((build) => {
              const kind: 'commit' | 'release' =
                build.kind === 'release' || isReleaseTag(build.tag)
                  ? 'release'
                  : 'commit'
              return {
                key: build.tag,
                tag: build.tag,
                label: kind === 'release' ? build.tag : build.short_sha,
                message:
                  kind === 'release'
                    ? build.tag
                    : u.updaterDockerHubBuild,
                date: build.pushed_at,
                kind,
                title: build.tag,
              }
            }),
          )
          return
        }
      } catch {
        // fall through to GitHub
      }

      const next: PickerItem[] = []
      try {
        const rel = await api.releases({ channel: 'preview', limit: 15 })
        for (const r of rel.items ?? []) {
          next.push({
            key: `rel-${r.tag_name}`,
            tag: r.tag_name,
            label: r.tag_name,
            message: `${r.name || r.tag_name}${r.prerelease ? ' (pre)' : ''}`,
            date: null,
            kind: 'release',
          })
        }
      } catch {
        /* optional */
      }
      try {
        const response = await api.commits({
          branch: option.channel,
          limit: 25,
        })
        for (const c of response.items ?? []) {
          next.push({
            key: c.sha,
            tag: c.tag,
            label: c.short_sha,
            message: c.message,
            date: c.committed_at,
            kind: 'commit',
            title: c.sha,
          })
        }
      } catch {
        /* optional */
      }
      if (!cancelled) {
        setTargetSource('github')
        setItems(next)
      }
    }

    load()
      .catch(() => {
        if (!cancelled) setItems([])
      })
      .finally(() => {
        if (!cancelled) setListLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [api, option, isCommit, u.updaterDockerHubBuild])

  // 输入/选中目标后，防抖对比新旧关系。
  useEffect(() => {
    if (compareTimerRef.current) {
      window.clearTimeout(compareTimerRef.current)
      compareTimerRef.current = null
    }
    if (!target || target.length < 3) {
      setCompare(null)
      return
    }
    compareTimerRef.current = window.setTimeout(() => {
      api
        .compare(target)
        .then(setCompare)
        .catch(() => setCompare(null))
    }, 450)
    return () => {
      if (compareTimerRef.current) window.clearTimeout(compareTimerRef.current)
    }
  }, [api, target])

  const compareTone = compare?.is_downgrade
    ? 'downgrade'
    : compare?.is_upgrade
      ? 'upgrade'
      : 'neutral'
  let compareText: string | null = null
  if (compare) {
    if (compare.is_upgrade) {
      compareText = format(u.updaterFreshnessAhead, {
        n: String(compare.ahead_by),
      })
    } else if (compare.is_downgrade) {
      compareText = format(u.updaterFreshnessBehind, {
        n: String(compare.behind_by),
      })
    } else if (compare.relation === 'identical') {
      compareText = u.updaterFreshnessIdentical
    } else if (compare.relation === 'diverged') {
      compareText = format(u.updaterFreshnessDiverged, {
        ahead: String(compare.ahead_by),
        behind: String(compare.behind_by),
      })
    } else {
      compareText = u.updaterFreshnessUnknown
    }
  }

  return (
    <div className="updater-target">
      <div className="updater-commit-list">
        <div className="updater-commit-list-head">
          {isCommit
            ? targetSource === 'dockerhub'
              ? u.updaterTargetDockerHubHead
              : u.updaterTargetCommitHead
            : u.updaterTargetReleaseHead}
          {' · '}
          <code>{option.channel}</code>
        </div>
        {isCommit && targetSource === 'dockerhub' && (
          <p className="updater-empty">{u.updaterDockerHubFallback}</p>
        )}
        {listLoading ? (
          <p className="updater-empty">{u.updaterLoading}</p>
        ) : items.length === 0 ? (
          <p className="updater-empty">{u.updaterTargetEmpty}</p>
        ) : (
          <ul>
            {items.map((item) => (
              <li key={item.key}>
                <button
                  type="button"
                  className={
                    selected === item.tag
                      ? 'updater-commit-item selected'
                      : 'updater-commit-item'
                  }
                  disabled={disabled}
                  title={item.title}
                  onClick={() => {
                    setSelected(item.tag)
                    setInput('')
                  }}
                >
                  <code>{item.label}</code>
                  <span className="updater-commit-msg">
                    {item.kind === 'release' && isCommit ? 'release · ' : ''}
                    {item.message}
                  </span>
                  {item.date && (
                    <span className="updater-commit-date">
                      {new Date(item.date).toLocaleString()}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {isCommit && (
        <label className="updater-target-input-row">
          <span>{u.updaterCommitTarget}</span>
          <input
            className="updater-input"
            type="text"
            placeholder={u.updaterCommitPlaceholder}
            value={input}
            disabled={disabled}
            onChange={(e) => {
              setInput(e.target.value)
              if (e.target.value.trim()) setSelected('')
            }}
          />
        </label>
      )}

      {compareText && target && (
        <div className={`updater-compare-preview ${compareTone}`}>
          {compareText}
        </div>
      )}

      <div className="updater-target-actions">
        <button
          type="button"
          className="btn-base btn-secondary"
          disabled={disabled || installing || !target}
          onClick={() => {
            if (!target) return
            const isDowngrade = compare?.is_downgrade === true
            const isUpgrade = compare?.is_upgrade === true
            // Clear upgrades (including time-based / unknown ancestry) skip risk dialog.
            const needsRisk =
              !compare ||
              compare.relation === 'diverged' ||
              (compare.relation === 'unknown' && !isUpgrade)
            onInstall(target, { isDowngrade, needsRisk })
          }}
        >
          <span>
            {installing
              ? u.updaterDispatching
              : format(u.updaterInstallTarget, { version: target || '…' })}
          </span>
        </button>
      </div>
    </div>
  )
}

// ===== 快照行 =====

function SnapshotRow({
  snapshot,
  u,
  busy,
  disabled,
  onRollback,
}: {
  snapshot: SnapshotMeta
  u: U
  busy: boolean
  disabled: boolean
  onRollback: () => void
}) {
  return (
    <div className="updater-snapshot-item">
      <div className="updater-snapshot-meta">
        <div className="updater-snapshot-version">
          {snapshot.source_version ? (
            <code>{snapshot.source_version}</code>
          ) : (
            <span className="muted">—</span>
          )}
        </div>
        <div className="updater-snapshot-info">
          {new Date(snapshot.created_at).toLocaleString()} ·{' '}
          {formatBytes(snapshot.size_bytes)}
        </div>
      </div>
      <button
        type="button"
        className="btn-base btn-secondary"
        onClick={onRollback}
        disabled={busy || disabled}
      >
        {busy ? u.updaterProcessing : u.updaterRollback}
      </button>
    </div>
  )
}

// ===== 高级与诊断 =====

function AdvancedPanel({
  status,
  available,
  transport,
  token,
  loading,
  u,
  onTransportChange,
  onTokenChange,
  onRefresh,
}: {
  status: UpdaterStatus | null
  available: ReleaseManifest | null
  transport: TransportMode
  token: string
  loading: boolean
  u: U
  onTransportChange: (m: TransportMode) => void
  onTokenChange: (s: string) => void
  onRefresh: () => void
}) {
  return (
    <>
      <dl className="updater-detail-grid">
        <dt>{u.updaterUpdaterVersion}</dt>
        <dd>{status?.updater_version ?? '—'}</dd>
        <dt>{u.updaterChannelLabel}</dt>
        <dd>
          {status
            ? `${status.channel} (${status.update_mode ?? 'release'})`
            : '—'}
        </dd>
        <dt>{u.updaterJobInFlight}</dt>
        <dd>{status?.job_in_flight ?? u.updaterNone}</dd>
      </dl>

      {available && (
        <details className="updater-digests">
          <summary>{u.updaterImageDigests}</summary>
          <dl className="updater-detail-grid">
            {Object.entries(available.images).map(([k, v]) => (
              <React.Fragment key={k}>
                <dt>{k}</dt>
                <dd>{v.digest}</dd>
              </React.Fragment>
            ))}
          </dl>
        </details>
      )}

      <div className="updater-transport">
        <div className="updater-transport-label">{u.updaterTransport}</div>
        <label>
          <input
            type="radio"
            name="updater-transport"
            checked={transport === 'backend'}
            onChange={() => onTransportChange('backend')}
          />
          {u.updaterTransportBackend}
        </label>
        <label>
          <input
            type="radio"
            name="updater-transport"
            checked={transport === 'direct'}
            onChange={() => onTransportChange('direct')}
          />
          {u.updaterTransportDirect}
        </label>
        {transport === 'direct' && (
          <>
            <p className="updater-transport-hint">
              {u.updaterTransportDirectHint}
            </p>
            <input
              type="password"
              value={token}
              onChange={(e) => onTokenChange(e.target.value)}
              placeholder="UPDATE_TOKEN"
              className="updater-token-input"
              autoComplete="off"
            />
          </>
        )}
      </div>

      <div className="updater-advanced-actions">
        <button
          type="button"
          className="btn-base btn-secondary"
          onClick={onRefresh}
          disabled={loading}
        >
          <LuRefreshCw size={13} />
          <span>{u.updaterRefresh}</span>
        </button>
      </div>
    </>
  )
}

// ===== 辅助 =====

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`
  return `${(n / 1024 ** 3).toFixed(2)} GB`
}

function formatAgo(iso: string, u: U): string {
  const then = new Date(iso).getTime()
  const diffSec = Math.max(0, Math.round((Date.now() - then) / 1000))
  if (diffSec < 45) return u.updaterAgoJustNow
  const min = Math.round(diffSec / 60)
  if (min < 60) return format(u.updaterAgoMin, { n: String(min) })
  const hr = Math.round(min / 60)
  if (hr < 24) return format(u.updaterAgoHour, { n: String(hr) })
  const d = Math.round(hr / 24)
  return format(u.updaterAgoDay, { n: String(d) })
}

export default UpdaterInlinePanel
