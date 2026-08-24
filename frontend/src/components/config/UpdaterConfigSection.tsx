/**
 * Updater 内联面板 — 以「用户一眼能看懂」为核心的重设计。
 *
 * 结构（自上而下）：
 *   1. 状态卡（hero）：一句话状态 + 一行解释 + 当前版本/通道/上次检查 + 唯一主按钮
 *   2. 新版本卡：解释这次更新是什么、更新时会发生什么（纯说明，不放按钮）
 *   3. 更新通道：三张单选卡片（稳定版 / 预览版 / 开发版·跟随提交），点选即保存
 *   4. 维护与恢复：仅在更新出问题时出现
 *   5. 更新器 / 边缘（两列卡片）
 *   6. 安装指定版本（折叠）
 *   7. 备份与回退（折叠）
 *   8. 高级与诊断（折叠）
 *
 * 原「更新模式 × 频道」两个下拉合并成单一通道选择（stable / preview /
 * preview+commit），「应用频道设置」按钮被移除——点选即保存，
 * 避免草稿态与服务器态不一致。
 *
 * 子 UI 拆在 ./updater/*（helpers / StatusHero / TargetPicker / AdvancedPanel）。
 */

import type {
  Job,
  ReleaseManifest,
  SnapshotMeta,
  SnapshotsResponse,
  TransportMode,
  UpdateMode,
  UpdaterStatus,
} from '../../services/updaterApi'
import type { ManagedListItem } from '../settings/ManagedList'
import type { ChannelKey, Mood, Toast } from './updater/helpers'
import type { MaintenancePollStop } from './updaterMaintenanceNav'
import {
  FaCog,
  FaHistory,
  FaRocket,
  FaServer,
  FaTools,
  LuDownload,
} from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  httpStatusMessage,
  isUselessErrorText,
  userFacingError,
} from '../../utils/userFacingError'
import {
  detectVersionDrift,
  makeUpdaterApi,
  UpdaterError,
} from '../../services/updaterApi'
import {
  ButtonItem,
  ManagedList,
  SettingGroup,
  SettingGroupGrid,
  SettingsButton,
  SettingTitleTag,
  useSettingGuide,
} from '../settings'
import { AdvancedPanel } from './updater/AdvancedPanel'
import {
  CHANNEL_OPTIONS,
  channelDesc,
  channelLabel,
  deriveMood,
  deriveSelection,
  format,
  formatBytes,
  infraComponentBehind,
  infraLatestTip,
  isDismissedLastFailed,
  rememberDismissedLastFailed,
  INFRA_OUTCOME_MAX_TRIES,
  INFRA_OUTCOME_POLL_MS,
  isFreshInfraOutcome,
  isTransientUpdaterError,
  modeForTarget,
  POLL_INTERVAL,
  sleep,
  snapshotDeleteBlockReason,
  upstreamDetail,
} from './updater/helpers'
import { SnapshotLimitPrefs } from './updater/SnapshotLimitPrefs'
import { ProgressCard, StatusHero } from './updater/StatusHero'
import { TargetPicker } from './updater/TargetPicker'
import {
  AGO_TICK_MS,
  isCheckStale,
  planLatestUpdate,
} from './updaterCheckFreshness'
import {
  hasNavigatedForJob,
  isLikelyMaintenanceHtml,
  navigateToMaintenancePage,
  startMaintenancePoll,
} from './updaterMaintenanceNav'
import './UpdaterConfigSection.css'

export interface UpdaterInlinePanelProps {
  heading?: string
}

export const UpdaterInlinePanel: React.FC<UpdaterInlinePanelProps> = ({
  heading,
}) => {
  const { t } = useI18n()
  const u = t.config
  const { catalog: g, bindGuide } = useSettingGuide()

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
  /** Retention diagnostics from last GET /snapshots (self-heal + counts). */
  const [snapshotStats, setSnapshotStats] = useState<Pick<
    SnapshotsResponse,
    | 'eligible_count'
    | 'protected_count'
    | 'total_count'
    | 'snapshot_limit'
    | 'snapshot_limit_enabled'
  > | null>(null)
  const [activeJob, setActiveJob] = useState<Job | null>(null)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const [toast, setToast] = useState<Toast>(null)
  /** Edge / updater outcome stays in the infra cards, not the app hero. */
  const [infraFeedback, setInfraFeedback] = useState<
    ({ scope: 'self' | 'proxy' } & NonNullable<Toast>) | null
  >(null)
  /**
   * True while an infra upgrade (updater self-update / proxy recreate) briefly
   * drops the admin→updater path. Keep last-known status so the panel does not
   * flash “offline”; the matching infra card explains the gap.
   */
  const [linkDown, setLinkDown] = useState(false)
  const [drift, setDrift] = useState<{ build: string; current: string } | null>(
    null,
  )
  const [accessDenied, setAccessDenied] = useState(false)
  const [sel, setSel] = useState<ChannelKey>('stable')
  /** 只在首次加载（或保存偏好后）用服务器值覆盖本地选择。 */
  const selHydratedRef = useRef(false)
  const pollRef = useRef<number | null>(null)
  /**
   * One silent availability check per panel mount (avoids StrictMode / refresh
   * loops spamming GitHub). Remounting About starts a new session. Manual
   * “Check now” is unaffected.
   */
  const autoRecheckDoneRef = useRef(false)
  /** Drives live relative “ago” labels without a full status refresh. */
  const [nowTick, setNowTick] = useState(() => Date.now())
  /** True while the open-panel always-once auto-check is in flight. */
  const [autoRechecking, setAutoRechecking] = useState(false)
  /** Local hide of the last-failed banner (survives old updater without dismiss API). */
  const [dismissedFailedJobId, setDismissedFailedJobId] = useState<
    string | null
  >(null)
  const [dismissedSelfLastAt, setDismissedSelfLastAt] = useState<string | null>(
    null,
  )
  const [dismissedProxyLastAt, setDismissedProxyLastAt] = useState<
    string | null
  >(null)

  /** Job we are watching for maintenance → full-page navigate (once). */
  const maintWatchJobRef = useRef<string | null>(null)
  const maintPollStopRef = useRef<MaintenancePollStop | null>(null)
  /** In-memory once-guard (sessionStorage is the cross-reload guard). */
  const maintNavDoneRef = useRef<string | null>(null)
  /** Latest status for the proxy poller fallback without re-subscribing. */
  const statusRef = useRef<UpdaterStatus | null>(null)
  statusRef.current = status

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
          return userFacingError(e, u.updaterErr403Generic)
        }
        if (e.status === 409) return u.updaterErr409
        if (e.status === 412) return userFacingError(e, u.updaterErr412)
        if (e.status >= 500) {
          if (/not configured/i.test(e.message))
            return u.updaterErrNotConfigured
          if (e.status === 502 || e.status === 503) return u.updaterErrUpstream
          const detail = upstreamDetail(e.message)
          if (detail && !isUselessErrorText(detail)) {
            return format(u.updaterErrServer, { msg: detail })
          }
          return userFacingError(e, httpStatusMessage(e.status))
        }
        return userFacingError(e, httpStatusMessage(e.status))
      }
      return userFacingError(e)
    },
    [u],
  )

  const stopMaintPoll = useCallback(() => {
    if (maintPollStopRef.current) {
      maintPollStopRef.current()
      maintPollStopRef.current = null
    }
  }, [])

  /** Full-page leave once per job when maintenance is active (or 503 HTML fallback). */
  const navigateToMaintOnce = useCallback(
    (jobId: string) => {
      if (!jobId) return
      if (maintNavDoneRef.current === jobId || hasNavigatedForJob(jobId)) return
      maintNavDoneRef.current = jobId
      stopMaintPoll()
      navigateToMaintenancePage(jobId)
    },
    [stopMaintPoll],
  )

  /**
   * After a successful triggerUpdate (or when a job is already in flight),
   * poll /_proxy/status until maintenance.active then assign('/').
   */
  const beginMaintWatch = useCallback(
    (jobId: string) => {
      if (!jobId) return
      if (maintNavDoneRef.current === jobId || hasNavigatedForJob(jobId)) return
      if (maintWatchJobRef.current === jobId && maintPollStopRef.current) return
      stopMaintPoll()
      maintWatchJobRef.current = jobId
      maintPollStopRef.current = startMaintenancePoll({
        jobId,
        getStatusMaintenanceActive: () =>
          !!statusRef.current?.maintenance_active,
        onNavigate: () => navigateToMaintOnce(jobId),
      })
    },
    [stopMaintPoll, navigateToMaintOnce],
  )

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      let s: UpdaterStatus | null = null
      try {
        s = await api.status()
        setLinkDown(false)
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
          setSnapshotStats(null)
          setActiveJob(null)
          setLinkDown(false)
          return
        }
        // Fallback: once a job is in flight, non-JSON 503 means proxy is
        // serving maintenance HTML for /api/* — leave the SPA.
        const watchJob = maintWatchJobRef.current
        if (
          watchJob &&
          e instanceof UpdaterError &&
          e.status === 503 &&
          isLikelyMaintenanceHtml(e.message)
        ) {
          navigateToMaintOnce(watchJob)
          return
        }
        // Transient gap (proxy recreate / updater self-replace): keep last good
        // status so About does not flip to offline mid-upgrade.
        if (statusRef.current && isTransientUpdaterError(e)) {
          setLinkDown(true)
          return
        }
      }
      if (!s) {
        // First load truly offline, or non-transient failure without prior data.
        if (!statusRef.current) {
          setStatus(null)
          setSnapshots([])
          setSnapshotStats(null)
          setActiveJob(null)
        }
        return
      }
      setStatus(s)
      // Snapshots are a dependent updater resource. Do not emit another 503/502
      // after status has already established that the updater is unavailable.
      // GET /snapshots also self-heals over-limit piles on modern updaters.
      const snaps: SnapshotsResponse = await api
        .snapshots()
        .catch((): SnapshotsResponse => ({ schema_version: 1, items: [] }))
      setSnapshots(snaps.items ?? [])
      setSnapshotStats({
        eligible_count: snaps.eligible_count,
        protected_count: snaps.protected_count,
        total_count: snaps.total_count,
        snapshot_limit: snaps.snapshot_limit,
        snapshot_limit_enabled: snaps.snapshot_limit_enabled,
      })
      if (!selHydratedRef.current) {
        setSel(deriveSelection(s))
        selHydratedRef.current = true
      }
      if (s.job_in_flight) {
        const j = await api.job(s.job_in_flight).catch(() => null)
        setActiveJob(j)
      } else {
        setActiveJob(null)
      }
    } finally {
      setLoading(false)
    }
  }, [api, transport, accessDenied, navigateToMaintOnce])

  useEffect(() => {
    refresh()
    detectVersionDrift().then((d) => {
      if (d?.drift) setDrift({ build: d.build, current: d.current })
    })
  }, [refresh])

  // Old updater has no dismiss API; keep a local ack and sync it once the
  // endpoint exists so the banner stays gone after a later updater upgrade.
  useEffect(() => {
    const jobId = status?.last_failed_update?.job_id
    if (!jobId || !isDismissedLastFailed(jobId)) return
    setDismissedFailedJobId(jobId)
    void api.dismissLastFailed().then(
      () => {
        void refresh()
      },
      () => {
        /* keep the local hide */
      },
    )
  }, [api, refresh, status?.last_failed_update?.job_id])

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

  // Navigate when maintenance is already active while a job runs; watch otherwise.
  // Clear the proxy poller on unmount or when the job ends without maintenance.
  useEffect(() => {
    const jobId = status?.job_in_flight
    if (!jobId) {
      if (!status?.maintenance_active) {
        stopMaintPoll()
        maintWatchJobRef.current = null
      }
      return
    }
    if (status.maintenance_active) {
      navigateToMaintOnce(jobId)
      return
    }
    beginMaintWatch(jobId)
  }, [
    status?.job_in_flight,
    status?.maintenance_active,
    beginMaintWatch,
    navigateToMaintOnce,
    stopMaintPoll,
  ])

  useEffect(() => () => stopMaintPoll(), [stopMaintPoll])

  // Live “N minutes ago” for last_checked_at (does not hit the network).
  useEffect(() => {
    const id = window.setInterval(() => setNowTick(Date.now()), AGO_TICK_MS)
    return () => window.clearInterval(id)
  }, [])

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

  // 操作

  const checkAvailable = useCallback(
    async (opts?: { silent?: boolean }) => {
      if (tokenRequired) {
        setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
        return
      }
      setBusy('check')
      try {
        // Check the saved updater preferences, without query overrides. The
        // updater intentionally treats parameterized checks as ephemeral and
        // does not write them to /status; using one here would let the fresh
        // response disagree with the cached status shown by the same panel.
        const manifest = await api.available()
        setAvailable(manifest)
        if (!opts?.silent) {
          setToast(manifest ? null : { kind: 'ok', text: u.updaterNoAvailable })
        }
        await refresh()
      } catch (e) {
        setToast({ kind: 'error', text: explain(e) })
      } finally {
        setBusy(null)
      }
    },
    [api, refresh, tokenRequired, explain, u],
  )

  // After first successful status(): always silent recheck once per mount.
  // Do not gate on isCheckStale — opening About should always revalidate.
  useEffect(() => {
    if (autoRecheckDoneRef.current) return
    if (!status) return
    if (accessDenied || tokenRequired) return
    if (status.job_in_flight || status.maintenance_active) return
    // Wait until any in-flight panel action finishes before deciding.
    if (busy) return

    autoRecheckDoneRef.current = true
    setAutoRechecking(true)
    void checkAvailable({ silent: true }).finally(() => {
      setAutoRechecking(false)
    })
  }, [status, accessDenied, tokenRequired, busy, checkAvailable])

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
        // 偏好已保存；按 updater 的当前配置重查并同步 /status 缓存。
        await checkAvailable()
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
        // Start proxy-status watch immediately (before refresh) so we leave the
        // SPA as soon as maintenance.json flips active.
        beginMaintWatch(r.job_id)
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
          const msg = /irreversible|diverged|unknown|allow_risk/i.test(
            e.message,
          )
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
              beginMaintWatch(r.job_id)
              await refresh()
              return
            } catch (e2) {
              // Preflight / trigger failure: stay on admin panel.
              setToast({ kind: 'error', text: explain(e2) })
              return
            }
          }
        }
        // Preflight / trigger failure: stay on admin panel, no navigate.
        setToast({ kind: 'error', text: explain(e) })
      } finally {
        setBusy(null)
      }
    },
    [api, status, refresh, tokenRequired, explain, u, beginMaintWatch],
  )

  /**
   * Update to latest: silent recheck available+status first; abort if no longer
   * needed (identical / same version / no target). On recheck error, do not
   * apply from the previous cache.
   */
  const updateToLatest = useCallback(async () => {
    if (tokenRequired) {
      setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
      return
    }
    if (busy) return

    setBusy('check')
    setToast(null)
    let manifest: ReleaseManifest | null = null
    let freshStatus: UpdaterStatus | null = null
    try {
      // Same path as checkAvailable: no query overrides so /available matches /status.
      manifest = await api.available()
      setAvailable(manifest)
      freshStatus = await api.status()
      setStatus(freshStatus)
    } catch (e) {
      setToast({ kind: 'error', text: explain(e) })
      return
    } finally {
      setBusy(null)
    }

    const plan = planLatestUpdate({
      available: manifest,
      latestAvailable: freshStatus?.latest_available,
      currentVersion: freshStatus?.current_version,
      downgradeAvailable: freshStatus?.downgrade_available,
      channelMode: selOption.mode,
    })
    if (!plan.proceed) {
      setToast({ kind: 'ok', text: u.updaterNoAvailable })
      return
    }
    await dispatchUpdate(plan.target, plan.mode, {
      isDowngrade: plan.isDowngrade,
      needsRisk: plan.needsRisk,
    })
  }, [api, busy, tokenRequired, explain, u, selOption.mode, dispatchUpdate])

  /**
   * After proxy/updater recreate the HTTP path blips. Poll durable
   * `*_update_last` until a new outcome appears (up to ~90 minutes), keep last
   * status on transient errors, and drive infra-card progress via linkDown.
   * Progress / success / failure stay on the component cards — not the hero.
   */
  const waitInfraUpdateOutcome = useCallback(
    async (opts: {
      kind: 'self' | 'proxy'
      beforeAt: string | null | undefined
      targetTag: string
    }): Promise<'succeeded' | 'failed' | 'timeout'> => {
      setLinkDown(false)

      let sawDisconnect = false
      for (let i = 0; i < INFRA_OUTCOME_MAX_TRIES; i++) {
        await sleep(INFRA_OUTCOME_POLL_MS)
        try {
          const s = await api.status()
          if (sawDisconnect) {
            setLinkDown(false)
            sawDisconnect = false
          }
          setStatus(s)
          const last =
            opts.kind === 'self' ? s?.self_update_last : s?.proxy_update_last
          const outcome = isFreshInfraOutcome(last, opts.beforeAt, opts.targetTag)
          if (outcome === 'succeeded' && last) {
            setLinkDown(false)
            setInfraFeedback({
              scope: opts.kind,
              kind: 'ok',
              text:
                opts.kind === 'self'
                  ? format(u.updaterSelfUpdateSucceeded, {
                      version: last.target_tag || opts.targetTag || '—',
                      previous: last.previous_tag || '—',
                    })
                  : format(u.updaterProxyUpdateSucceeded, {
                      version: last.target_tag || opts.targetTag || '—',
                      previous: last.previous_tag || '—',
                    }),
            })
            return 'succeeded'
          }
          if (outcome === 'failed' && last) {
            setLinkDown(false)
            setInfraFeedback({
              scope: opts.kind,
              kind: 'error',
              text:
                opts.kind === 'self'
                  ? format(u.updaterSelfUpdateFailed, {
                      error: last.error || '—',
                    })
                  : format(u.updaterProxyUpdateFailed, {
                      error: last.error || '—',
                    }),
            })
            return 'failed'
          }
        } catch {
          // Updater image replace / proxy recreate: expected brief blip.
          sawDisconnect = true
          setLinkDown(true)
        }
      }
      setLinkDown(false)
      setInfraFeedback({
        scope: opts.kind,
        kind: 'ok',
        text:
          opts.kind === 'self'
            ? u.updaterSelfUpdateStillPending
            : u.updaterProxyUpdateStillPending,
      })
      return 'timeout'
    },
    [api, u],
  )

  /** Full status + silent available recheck so version badges/buttons catch up. */
  const refreshAfterInfra = useCallback(async () => {
    try {
      await refresh()
    } catch {
      /* refresh already soft-fails */
    }
    try {
      const manifest = await api.available()
      setAvailable(manifest)
      await refresh()
    } catch {
      // Tip recheck is best-effort; durable outcome already shown on the card.
    }
  }, [api, refresh])

  const triggerSelfUpdate = useCallback(async () => {
    if (tokenRequired) {
      setInfraFeedback({
        scope: 'self',
        kind: 'error',
        text: u.updaterTokenRequiredDirect,
      })
      return
    }
    // Backend resolves the tip itself (release.json or Docker Hub). App
    // `latest_available` is only a hint — it is cleared when already current.
    const tip = status?.latest_available?.version
    const ok = tip
      ? confirm(format(u.updaterSelfUpdateConfirm, { version: tip }))
      : confirm(u.updaterSelfUpdateConfirmAuto)
    if (!ok) return
    const beforeAt = status?.self_update_last?.at
    setBusy('self-update')
    setInfraFeedback(null)
    setLinkDown(false)
    // Do not fall back to the app tip: POST success kills this process before
    // new_updater_tag returns, and the app version is a different tag space.
    let target = ''
    let shouldWait = true
    try {
      try {
        const report = await api.triggerSelfUpdate()
        target = report.new_updater_tag || ''
      } catch (e) {
        // Schedule may have been accepted then the gateway died on recreate.
        if (!isTransientUpdaterError(e)) {
          setInfraFeedback({
            scope: 'self',
            kind: 'error',
            text: explain(e),
          })
          shouldWait = false
        } else {
          setLinkDown(true)
        }
      }
      if (shouldWait) {
        await waitInfraUpdateOutcome({
          kind: 'self',
          beforeAt,
          targetTag: target,
        })
        await refreshAfterInfra()
      }
    } finally {
      setBusy(null)
      setLinkDown(false)
    }
  }, [
    api,
    status,
    tokenRequired,
    explain,
    u,
    waitInfraUpdateOutcome,
    refreshAfterInfra,
  ])

  const triggerProxyUpdate = useCallback(async () => {
    if (tokenRequired) {
      setInfraFeedback({
        scope: 'proxy',
        kind: 'error',
        text: u.updaterTokenRequiredDirect,
      })
      return
    }
    // Tip is optional. Empty body lets the updater pick
    // the component tip from GitHub/Docker Hub when status has no app tip.
    const tip = status?.latest_available?.version
    const ok = tip
      ? confirm(format(u.updaterInfraProxyConfirm, { version: tip }))
      : confirm(u.updaterInfraProxyConfirmAuto)
    if (!ok) return
    const beforeAt = status?.proxy_update_last?.at
    setBusy('proxy-update')
    setInfraFeedback(null)
    setLinkDown(false)
    let target = ''
    let shouldWait = true
    try {
      try {
        const report = await api.triggerProxyUpdate(tip || undefined)
        target = report.new_proxy_tag || ''
      } catch (e) {
        // Proxy recreate tears down the HTTP path mid-request even when the
        // upgrade succeeds; durable proxy_update_last is the source of truth.
        if (!isTransientUpdaterError(e)) {
          // Hard error before/after work: surface API message, still one
          // refresh so last_failed / version lines catch up if written.
          await refresh().catch(() => {})
          setInfraFeedback({
            scope: 'proxy',
            kind: 'error',
            text: explain(e),
          })
          shouldWait = false
        } else {
          setLinkDown(true)
        }
      }
      if (shouldWait) {
        await waitInfraUpdateOutcome({
          kind: 'proxy',
          beforeAt,
          targetTag: target,
        })
        await refreshAfterInfra()
      }
    } finally {
      setBusy(null)
      setLinkDown(false)
    }
  }, [
    api,
    status,
    refresh,
    tokenRequired,
    explain,
    u,
    waitInfraUpdateOutcome,
    refreshAfterInfra,
  ])

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
        const r = await api.rollback(snap.id)
        setToast({ kind: 'ok', text: u.updaterRollbackDispatched })
        // Same as upgrade: watch job_id immediately for maintenance redirect
        beginMaintWatch(r.job_id)
        await refresh()
      } catch (e) {
        setToast({ kind: 'error', text: explain(e) })
      } finally {
        setBusy(null)
      }
    },
    [api, refresh, tokenRequired, explain, u, beginMaintWatch],
  )

  const deleteSnapshot = useCallback(
    async (snap: SnapshotMeta) => {
      if (tokenRequired) {
        setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
        return
      }
      const blockReason = snapshotDeleteBlockReason(snap, {
        rescueSnapshotId: status?.rescue_snapshot_id,
        totalSnapshots: snapshots.length,
        u,
      })
      if (blockReason) {
        setToast({ kind: 'error', text: blockReason })
        return
      }
      if (
        !confirm(
          format(u.updaterDeleteSnapshotConfirm, {
            version: snap.source_version ?? snap.id,
          }),
        )
      ) {
        return
      }
      const prev = snapshots
      setBusy(`delete-${snap.id}`)
      // Optimistic remove so the row vanishes without a full panel refresh.
      setSnapshots((list) => list.filter((s) => s.id !== snap.id))
      try {
        await api.deleteSnapshot(snap.id)
        setToast({ kind: 'ok', text: u.updaterDeleteSnapshotDispatched })
        await refresh()
      } catch (e) {
        setSnapshots(prev)
        setToast({ kind: 'error', text: explain(e) })
      } finally {
        setBusy(null)
      }
    },
    [
      api,
      refresh,
      tokenRequired,
      explain,
      u,
      status?.rescue_snapshot_id,
      snapshots,
    ],
  )

  /** Persist backup retention prefs; prune may free older snapshots immediately. */
  const saveSnapshotLimitPrefs = useCallback(
    async (prefs: {
      snapshot_limit_enabled?: boolean
      snapshot_limit?: number
    }) => {
      if (tokenRequired) {
        setToast({ kind: 'error', text: u.updaterTokenRequiredDirect })
        return
      }
      setBusy('snapshot-limit')
      setToast(null)
      try {
        const res = await api.setPrefs(prefs)
        const pruned = res.pruned_snapshot_ids?.length ?? 0
        const enabled = res.snapshot_limit_enabled !== false
        const limit =
          typeof res.snapshot_limit === 'number'
            ? res.snapshot_limit
            : (prefs.snapshot_limit ?? status?.snapshot_limit ?? 3)
        const eligible =
          typeof res.eligible_count === 'number' ? res.eligible_count : null
        // 0 pruned while still over limit: protected pins/in_use, disk failure,
        // or an old updater that ignores retention — do not claim silent success.
        if (enabled && eligible != null && eligible > limit && pruned === 0) {
          setToast({
            kind: 'error',
            text: format(u.updaterSnapshotLimitStillOver, {
              eligible: String(eligible),
              n: String(limit),
              protected: String(res.protected_count ?? '—'),
            }),
          })
        } else if (pruned > 0) {
          setToast({
            kind: 'ok',
            text: format(u.updaterSnapshotLimitSavedPruned, {
              n: String(pruned),
            }),
          })
        } else {
          setToast({ kind: 'ok', text: u.updaterSnapshotLimitSaved })
        }
        await refresh()
      } catch (e) {
        setToast({ kind: 'error', text: explain(e) })
      } finally {
        setBusy(null)
      }
    },
    [api, refresh, tokenRequired, explain, u, status?.snapshot_limit],
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
      // Same as upgrade: watch job_id immediately for maintenance redirect
      beginMaintWatch(res.job_id)
      await refresh()
    } catch (e) {
      setToast({ kind: 'error', text: explain(e) })
    } finally {
      setBusy(null)
    }
  }, [api, refresh, tokenRequired, explain, u, status, beginMaintWatch])

  const dismissLastFailed = useCallback(async () => {
    const failed = status?.last_failed_update
    if (!failed) return
    rememberDismissedLastFailed(failed.job_id)
    setDismissedFailedJobId(failed.job_id)
    try {
      await api.dismissLastFailed()
      await refresh()
    } catch {
      /* Old updater: local ack is enough to keep this job's banner closed. */
    }
  }, [api, refresh, status?.last_failed_update])

  const dismissSelfUpdateLast = useCallback(async () => {
    const at = status?.self_update_last?.at
    if (at) setDismissedSelfLastAt(at)
    try {
      await api.dismissSelfUpdateLast()
      await refresh()
    } catch {
      /* Old updater: local ack hides this outcome until a new one is written. */
    }
  }, [api, refresh, status?.self_update_last?.at])

  const dismissProxyUpdateLast = useCallback(async () => {
    const at = status?.proxy_update_last?.at
    if (at) setDismissedProxyLastAt(at)
    try {
      await api.dismissProxyUpdateLast()
      await refresh()
    } catch {
      /* Old updater: local ack hides this outcome until a new one is written. */
    }
  }, [api, refresh, status?.proxy_update_last?.at])

  // 渲染

  const mood = useMemo<Mood>(() => deriveMood(status), [status])

  const stale = useMemo(() => {
    if (!status) return false
    return isCheckStale(
      status.last_checked_at,
      status.check_interval_secs,
      nowTick,
    )
  }, [status, nowTick])

  /** Cached available/downgrade while last check is too old — not fully trusted. */
  const pendingConfirm = stale && (mood === 'available' || mood === 'downgrade')

  // 非 admin：整段隐藏
  if (accessDenied && transport === 'backend') return null

  // Scheme A: admin ProgressCard only for the brief pre-maintenance window.
  // Once maintenance is active, full progress lives on the maintenance page.
  const jobRunning =
    !!activeJob &&
    !['succeeded', 'failed', 'needs_manual'].includes(activeJob.status)
  const showProgress =
    jobRunning &&
    !status?.maintenance_active &&
    status?.maintenance_phase !== 'needs_manual'
  const showMaintenance =
    (mood === 'maintenance' || mood === 'needsManual') && !showProgress
  const requiresSelfUpdate =
    !!status?.requires_self_update && !!status.latest_available
  const infraTip = infraLatestTip(status)
  const updaterHasUpdate =
    requiresSelfUpdate || infraComponentBehind(status?.updater_version, infraTip)
  const proxyHasUpdate = infraComponentBehind(status?.proxy_version, infraTip)
  /** 业务侧有新版本（或强制要求先升更新器）时，在组件区给出提示 */
  const infraUpdateCue = updaterHasUpdate || proxyHasUpdate
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

      {/* Auto-rollback / pre-swap cleanup leaves maintenance idle but records this. */}
      {status?.last_failed_update &&
        mood !== 'updating' &&
        dismissedFailedJobId !== status.last_failed_update.job_id &&
        !isDismissedLastFailed(status.last_failed_update.job_id) && (
          <div className="updater-last-failed" role="status">
            <div className="updater-last-failed-head">
              <strong>{u.updaterLastFailedTitle}</strong>
              <button
                type="button"
                className="updater-last-failed-dismiss"
                onClick={() => void dismissLastFailed()}
                aria-label={u.updaterLastFailedDismissAria}
                title={u.updaterLastFailedDismissAria}
              >
                {u.updaterLastFailedDismiss}
              </button>
            </div>
            <span>
              {format(u.updaterLastFailedBody, {
                from: status.last_failed_update.from_version ?? '—',
                to: status.last_failed_update.to_version ?? '—',
                reason: status.last_failed_update.reason,
              })}
            </span>
          </div>
        )}

      {showProgress ? (
        <ProgressCard job={activeJob!} u={u} />
      ) : (
        <StatusHero
          mood={mood}
          status={status}
          available={available}
          sel={sel}
          busy={busy}
          loading={loading}
          tokenRequired={tokenRequired}
          requiresSelfUpdate={requiresSelfUpdate}
          stale={stale}
          autoRechecking={autoRechecking}
          pendingConfirm={pendingConfirm}
          nowTick={nowTick}
          toast={toast}
          u={u}
          onCheck={() => checkAvailable()}
          onUpdate={updateToLatest}
          onRetry={refresh}
          onSaveAutoPrefs={async (prefs) => {
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

      {jobRunning && status?.maintenance_active && !showProgress && (
        <p className="updater-progress-hint">
          {u.updaterProgressOnMaintenance}
        </p>
      )}

      {/* ===== 更新通道：点选即保存 ===== */}
      {!showProgress && (
        <SettingGroup
          title={u.updaterChannelGroupTitle}
          description={u.updaterChannelGroupDesc}
          {...bindGuide('updater.channel', g.updater.channel)}
          icon={<FaRocket />}
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

      {/* ===== 维护与恢复（仅出问题时出现）===== */}
      {showMaintenance && (
        <SettingGroup
          title={u.updaterMaintenanceGroup}
          description={u.updaterMaintenanceGroupDesc}
          {...bindGuide('updater.maintenance', g.updater.maintenance)}
          icon={<FaTools />}
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
              {...bindGuide('updater.rescue', g.updater.rescue)}
              buttonText={u.updaterRescueContinue}
              loading={busy === 'rescue-continue'}
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
            {...bindGuide('updater.forceExit', g.updater.forceExit)}
            buttonText={u.updaterForceExit}
            loading={busy === 'exit-maintenance'}
            onClick={exitMaintenance}
            variant="danger"
            layout="horizontal"
            disabled={busy === 'exit-maintenance' || tokenRequired}
          />
        </SettingGroup>
      )}

      {/* ===== 更新器 / 边缘：子分类 + 两列卡片 ===== */}
      {!showProgress && (
        <SettingGroup
          title={u.updaterInfraGroupTitle}
          {...bindGuide('updater.infra', g.updater.infra)}
          description={
            requiresSelfUpdate
              ? `${u.updaterInfraGroupDesc} ${format(
                  u.updaterSelfUpdateNeeded,
                  {
                    version: status?.latest_available?.version ?? '—',
                    minVersion:
                      status?.latest_available?.min_updater_version ?? '—',
                  },
                )}`
              : u.updaterInfraGroupDesc
          }
          detailTone={requiresSelfUpdate ? 'warning' : 'default'}
          icon={<FaServer />}
          titleExtra={
            requiresSelfUpdate ? (
              <SettingTitleTag
                title={
                  infraTip
                    ? format(u.updaterSelfUpdateNeeded, {
                        version: infraTip,
                        minVersion:
                          status?.latest_available?.min_updater_version ??
                          infraTip,
                      })
                    : u.updaterInfraRequired
                }
              >
                {infraTip ?? u.updaterInfraRequired}
              </SettingTitleTag>
            ) : infraUpdateCue && infraTip ? (
              <SettingTitleTag
                variant="muted"
                title={u.updaterInfraUpdateAvailableHint}
              >
                {infraTip}
              </SettingTitleTag>
            ) : null
          }
        >
          <SettingGroupGrid
            columns={2}
            variant="card"
            align="stretch"
            minColumnWidth="16rem"
            className="updater-infra-grid"
            ariaLabel={u.updaterInfraGroupTitle}
          >
            <SettingGroup
              title={u.updaterInfraUpdaterTitle}
              description={u.updaterInfraUpdaterDesc}
              icon={<FaCog />}
              className={
                requiresSelfUpdate
                  ? 'updater-infra-card is-attention'
                  : updaterHasUpdate
                    ? 'updater-infra-card is-update-cue'
                    : 'updater-infra-card'
              }
              titleExtra={
                requiresSelfUpdate ? (
                  <SettingTitleTag title={u.updaterInfraRequired}>
                    {infraTip ?? u.updaterInfraRequired}
                  </SettingTitleTag>
                ) : updaterHasUpdate && infraTip ? (
                  <SettingTitleTag
                    variant="muted"
                    title={u.updaterInfraUpdateAvailableHint}
                  >
                    {infraTip}
                  </SettingTitleTag>
                ) : null
              }
            >
              <div className="updater-infra-card-body">
                <p className="updater-infra-current">
                  {u.updaterInfraCurrent}{' '}
                  <code>{status?.updater_version ?? '—'}</code>
                  {status?.latest_available?.min_updater_version && (
                    <>
                      {' '}
                      · {u.updaterMinVersionShort}{' '}
                      <code>{status.latest_available.min_updater_version}</code>
                    </>
                  )}
                </p>
                <InfraCardNotice
                  inProgress={busy === 'self-update'}
                  linkDown={linkDown}
                  waiting={u.updaterSelfUpdateWaiting}
                  reconnecting={u.updaterSelfUpdateReconnecting}
                  feedback={
                    infraFeedback?.scope === 'self' ? infraFeedback : null
                  }
                  lastFail={
                    status?.self_update_last?.status === 'failed' &&
                    dismissedSelfLastAt !== status.self_update_last.at
                      ? format(u.updaterInfraSelfLastFailed, {
                          target: status.self_update_last.target_tag || '—',
                          previous: status.self_update_last.previous_tag || '—',
                          error: status.self_update_last.error || '—',
                        })
                      : null
                  }
                  dismissLabel={u.updaterLastFailedDismiss}
                  dismissAria={u.updaterLastFailedDismissAria}
                  onDismiss={() => void dismissSelfUpdateLast()}
                />
                <div className="updater-infra-card-actions">
                  <SettingsButton
                    size="sm"
                    variant={requiresSelfUpdate ? 'primary' : 'secondary'}
                    disabled={
                      // Do not require latest_available: backend clears it when the
                      // app is already current, but self/proxy update still resolve
                      // their own component tips independently.
                      !!busy || tokenRequired || !status
                    }
                    loading={busy === 'self-update'}
                    onClick={() => triggerSelfUpdate()}
                  >
                    {u.updaterSelfUpdateButton}
                  </SettingsButton>
                </div>
              </div>
            </SettingGroup>

            <SettingGroup
              title={u.updaterInfraProxyTitle}
              description={u.updaterInfraProxyDesc}
              icon={<FaServer />}
              className={
                proxyHasUpdate
                  ? 'updater-infra-card is-update-cue'
                  : 'updater-infra-card'
              }
              titleExtra={
                proxyHasUpdate && infraTip ? (
                  <SettingTitleTag
                    variant="muted"
                    title={u.updaterInfraUpdateAvailableHint}
                  >
                    {infraTip}
                  </SettingTitleTag>
                ) : null
              }
            >
              <div className="updater-infra-card-body">
                <p className="updater-infra-current">
                  {u.updaterInfraCurrent}{' '}
                  <code>{status?.proxy_version ?? '—'}</code>
                </p>
                <InfraCardNotice
                  inProgress={busy === 'proxy-update'}
                  linkDown={linkDown}
                  waiting={u.updaterProxyUpdateWaiting}
                  reconnecting={u.updaterProxyUpdateReconnecting}
                  feedback={
                    infraFeedback?.scope === 'proxy' ? infraFeedback : null
                  }
                  lastFail={
                    status?.proxy_update_last?.status === 'failed' &&
                    dismissedProxyLastAt !== status.proxy_update_last.at
                      ? `${format(u.updaterInfraProxyLastFailed, {
                          target: status.proxy_update_last.target_tag || '—',
                          previous: status.proxy_update_last.previous_tag || '—',
                          error: status.proxy_update_last.error || '—',
                        })}${
                          status.proxy_update_last.rolled_back
                            ? ` ${u.updaterInfraProxyRolledBack}`
                            : ''
                        }`
                      : null
                  }
                  dismissLabel={u.updaterLastFailedDismiss}
                  dismissAria={u.updaterLastFailedDismissAria}
                  onDismiss={() => void dismissProxyUpdateLast()}
                />
                <div className="updater-infra-card-actions">
                  <SettingsButton
                    size="sm"
                    variant="secondary"
                    disabled={!!busy || tokenRequired || !status}
                    loading={busy === 'proxy-update'}
                    onClick={() => triggerProxyUpdate()}
                  >
                    {u.updaterInfraProxyUpdateButton}
                  </SettingsButton>
                </div>
              </div>
            </SettingGroup>
          </SettingGroupGrid>
        </SettingGroup>
      )}

      {/* ===== 安装指定版本（折叠）===== */}
      {!showProgress && (
        <SettingGroup
          title={u.updaterTargetGroupTitle}
          description={u.updaterTargetGroupDesc}
          {...bindGuide('updater.target', g.updater.target)}
          icon={<LuDownload />}
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
        {...bindGuide('updater.snapshot', g.updater.snapshot)}
        icon={<FaHistory />}
        collapsible
        defaultExpanded={false}
      >
        <div className="updater-snapshot-section">
          <SnapshotLimitPrefs
            status={status}
            snapshotStats={snapshotStats}
            disabled={!!busy || tokenRequired}
            saving={busy === 'snapshot-limit'}
            u={u}
            onSave={saveSnapshotLimitPrefs}
          />
          <ManagedList
            className="updater-snapshot-managed"
            emptyText={u.updaterNoSnapshots}
            maxHeight={null}
            items={snapshots.map((s): ManagedListItem => {
              const deleteReason = snapshotDeleteBlockReason(s, {
                rescueSnapshotId: status?.rescue_snapshot_id,
                totalSnapshots: snapshots.length,
                u,
              })
              const rowBusy =
                busy === `rollback-${s.id}` || busy === `delete-${s.id}`
              return {
                id: s.id,
                title: s.source_version ? (
                  <code>{s.source_version}</code>
                ) : (
                  <span className="managed-list-muted">—</span>
                ),
                badge: s.keep
                  ? {
                      label: u.updaterDeleteSnapshotKeptBadge,
                      tone: 'warn',
                    }
                  : undefined,
                subtitle: `${new Date(s.created_at).toLocaleString()} · ${formatBytes(s.size_bytes)}`,
                meta: deleteReason || undefined,
                busy: rowBusy,
                actions: [
                  {
                    key: 'rollback',
                    label: u.updaterRollback,
                    variant: 'secondary',
                    onClick: () => void rollbackTo(s),
                    disabled: tokenRequired,
                    loading: busy === `rollback-${s.id}`,
                  },
                  {
                    key: 'delete',
                    label: u.updaterDeleteSnapshot,
                    variant: 'danger',
                    onClick: () => void deleteSnapshot(s),
                    disabled: tokenRequired || !!deleteReason,
                    loading: busy === `delete-${s.id}`,
                    title: deleteReason ?? u.updaterDeleteSnapshot,
                  },
                ],
              }
            })}
          />
        </div>
      </SettingGroup>

      {/* ===== 高级与诊断（折叠）===== */}
      <SettingGroup
        title={u.updaterGroupAdvanced}
        description={u.updaterGroupAdvancedDesc}
        {...bindGuide('updater.advanced', g.updater.advanced)}
        icon={<FaCog />}
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

function InfraCardNotice({
  inProgress,
  linkDown,
  waiting,
  reconnecting,
  feedback,
  lastFail,
  dismissLabel,
  dismissAria,
  onDismiss,
}: {
  inProgress: boolean
  linkDown: boolean
  waiting: string
  reconnecting: string
  feedback: NonNullable<Toast> | null
  lastFail: string | null
  dismissLabel?: string
  dismissAria?: string
  onDismiss?: () => void
}) {
  if (inProgress) {
    return (
      <p className="updater-infra-progress" role="status">
        {linkDown ? reconnecting : waiting}
      </p>
    )
  }
  if (feedback) {
    return (
      <p
        className={
          feedback.kind === 'error'
            ? 'updater-infra-last-fail'
            : 'updater-infra-success'
        }
        role={feedback.kind === 'error' ? 'alert' : 'status'}
      >
        {feedback.text}
      </p>
    )
  }
  if (lastFail) {
    return (
      <div className="updater-infra-last-fail" role="status">
        <span>{lastFail}</span>
        {onDismiss && dismissLabel ? (
          <button
            type="button"
            className="updater-last-failed-dismiss"
            onClick={onDismiss}
            aria-label={dismissAria || dismissLabel}
            title={dismissAria || dismissLabel}
          >
            {dismissLabel}
          </button>
        ) : null}
      </div>
    )
  }
  return null
}

export default UpdaterInlinePanel
