import type { CSSProperties } from 'react'
import { useCallback, useEffect, useState } from 'react'

import { useI18n } from '../contexts/I18nContext'
import { coordinator as animationCoordinator } from '../hooks/animation'
import { usePerfMetrics } from '../hooks/usePerfMetrics'
import { clearLyricsCache, clearPlaylistCache } from '../utils/musicPlayer'
import { globalResourceLoader } from '../utils/resourceLoader'

import './PerformanceMonitor.css'

function fpsClass(fps: number, low: boolean): string {
  if (low || fps < 30) return 'pm-bad'
  if (fps < 55) return 'pm-warn'
  return 'pm-ok'
}

function memClass(pct: number | undefined): string {
  if (pct == null) return 'pm-muted'
  if (pct >= 85) return 'pm-bad'
  if (pct >= 70) return 'pm-warn'
  return 'pm-ok'
}

function clsClass(cls: number): string {
  if (cls > 0.25) return 'pm-bad'
  if (cls > 0.1) return 'pm-warn'
  return 'pm-ok'
}

export default function PerformanceMonitor() {
  const { t } = useI18n()
  const pm = t.perfMonitor
  const [isExpanded, setIsExpanded] = useState(false)
  const [pauseAll, setPauseAll] = useState(false)
  const [toast, setToast] = useState<string | null>(null)

  const {
    snapshot,
    refreshAnimations,
    resetLongTasks,
    resetCls,
    configureCoordinator,
  } = usePerfMetrics(isExpanded)

  const { frame, memory, stability, animations, coordinator, resource } =
    snapshot

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'm') {
        e.preventDefault()
        setIsExpanded((v) => !v)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    const id = 'perf-pause-animations'
    if (!pauseAll) {
      document.getElementById(id)?.remove()
      return
    }
    const style = document.createElement('style')
    style.id = id
    style.textContent = `*,*::before,*::after{animation-play-state:paused!important;transition:none!important}`
    document.head.appendChild(style)
    return () => {
      document.getElementById(id)?.remove()
    }
  }, [pauseAll])

  const showToast = useCallback((msg: string) => {
    setToast(msg)
    window.setTimeout(setToast, 1600, null)
  }, [])

  const hasResourceActivity = resource.queued > 0 || resource.active > 0

  return (
    <div
      data-perf-monitor
      className={`pm-root ${isExpanded ? 'pm-expanded' : ''}`}
    >
      <div
        className="pm-bar"
        role="button"
        tabIndex={0}
        aria-expanded={isExpanded}
        title={isExpanded ? pm.collapseTitle : pm.expandTitle}
        onClick={() => setIsExpanded((v) => !v)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault()
            setIsExpanded((v) => !v)
          }
        }}
      >
        <span className={`pm-value ${fpsClass(frame.fps, frame.isLowFps)}`}>
          {frame.fps}
          <span className="pm-unit">fps</span>
        </span>
        {memory && (
          <span className={`pm-value ${memClass(memory.usedPercent)}`}>
            {memory.usedMB}
            <span className="pm-unit">MB</span>
          </span>
        )}
        {stability.longTaskCount > 0 && (
          <span
            className={`pm-value ${stability.longTaskCount > 5 ? 'pm-bad' : 'pm-warn'}`}
            title={
              stability.lastLongTaskMs
                ? pm.lastLongTask.replace(
                    '{ms}',
                    String(stability.lastLongTaskMs),
                  )
                : pm.longTasks
            }
          >
            LT {stability.longTaskCount}
          </span>
        )}
      </div>

      {isExpanded && (
        <div className="pm-body">
          <section className="pm-section">
            <div className="pm-section-title">{pm.sectionFrame}</div>
            <div className="pm-grid">
              <Row
                label="FPS"
                value={`${frame.fps}`}
                className={fpsClass(frame.fps, frame.isLowFps)}
              />
              <Row
                label={pm.avgFrameTime}
                value={`${frame.avgFrameTime} ms`}
              />
              <Row
                label="P95"
                value={`${frame.p95FrameMs} ms`}
                className={
                  frame.p95FrameMs > frame.jankThresholdMs
                    ? 'pm-bad'
                    : frame.p95FrameMs > frame.jankThresholdMs * 0.6
                      ? 'pm-warn'
                      : undefined
                }
              />
              <Row
                label={pm.worstFrame}
                value={`${frame.maxFrameMs} ms`}
                className={
                  frame.maxFrameMs > frame.jankThresholdMs
                    ? 'pm-bad'
                    : frame.maxFrameMs > frame.jankThresholdMs * 0.75
                      ? 'pm-warn'
                      : undefined
                }
              />
              <Row
                label={pm.jankRate}
                value={`${Math.round(frame.jankRatio * 100)}%`}
                className={
                  frame.jankRatio > 0.1
                    ? 'pm-bad'
                    : frame.jankRatio > 0.02
                      ? 'pm-warn'
                      : 'pm-muted'
                }
              />
              <Row
                label={pm.refreshRate}
                value={
                  frame.refreshRateDetected
                    ? `${frame.detectedRefreshRate} Hz`
                    : pm.detecting
                }
              />
              <Row
                label={pm.lowFpsMode}
                value={frame.isLowFps ? 'ON' : 'OFF'}
                className={frame.isLowFps ? 'pm-warn' : 'pm-muted'}
              />
              <Row
                label={pm.sampling}
                value={frame.isMonitoring ? pm.running : pm.notStarted}
                className={frame.isMonitoring ? 'pm-ok' : 'pm-warn'}
              />
            </div>
          </section>

          <section className="pm-section">
            <div className="pm-section-title">
              {pm.sectionRuntime}
              {!memory && (
                <span className="pm-hint">{pm.memChromiumOnly}</span>
              )}
            </div>
            <div className="pm-grid">
              {memory ? (
                <>
                  <Row
                    label="Heap"
                    value={`${memory.usedMB} / ${memory.limitMB} MB`}
                    className={memClass(memory.usedPercent)}
                  />
                  <div className="pm-row pm-row-full">
                    <span className="pm-muted">{pm.usageRate}</span>
                    <div className="pm-meter" aria-hidden>
                      <div
                        className={`pm-meter-fill ${memClass(memory.usedPercent)}`}
                        style={
                          {
                            '--pm-pct': `${memory.usedPercent}%`,
                          } as CSSProperties
                        }
                      />
                    </div>
                    <span className={memClass(memory.usedPercent)}>
                      {memory.usedPercent}%
                    </span>
                  </div>
                </>
              ) : (
                <Row
                  label="Heap"
                  value={pm.unavailable}
                  className="pm-muted"
                />
              )}
              <Row
                label={pm.longTasks}
                value={
                  stability.longTaskCount > 0
                    ? `${stability.longTaskCount}${
                        stability.lastLongTaskMs
                          ? pm.recentMs.replace(
                              '{ms}',
                              String(stability.lastLongTaskMs),
                            )
                          : ''
                      }`
                    : '0'
                }
                className={
                  stability.longTaskCount > 5
                    ? 'pm-bad'
                    : stability.longTaskCount > 0
                      ? 'pm-warn'
                      : undefined
                }
              />
            </div>
            {stability.longTaskCount > 0 && (
              <button
                type="button"
                className="pm-btn pm-btn-ghost"
                onClick={resetLongTasks}
              >
                {pm.resetLtCount}
              </button>
            )}
          </section>

          <section className="pm-section">
            <div className="pm-section-title">{pm.sectionStability}</div>
            <div className="pm-grid">
              <Row
                label="CLS"
                value={stability.cls.toFixed(4)}
                className={clsClass(stability.cls)}
              />
              <Row
                label="FCP"
                value={
                  stability.fcpMs != null ? `${stability.fcpMs} ms` : '—'
                }
              />
            </div>
            {stability.cls > 0 && (
              <button
                type="button"
                className="pm-btn pm-btn-ghost"
                onClick={resetCls}
              >
                {pm.resetCls}
              </button>
            )}
          </section>

          <section className="pm-section">
            <div className="pm-section-title">
              {pm.sectionAnimation}
              <span className="pm-hint">document.getAnimations()</span>
            </div>
            <div className="pm-grid">
              <Row
                label={pm.runningCount}
                value={`${animations.running}`}
                className={
                  animations.running > 15 ? 'pm-warn' : 'pm-ok'
                }
              />
              <Row label={pm.total} value={`${animations.total}`} />
            </div>
            {animations.items.length > 0 && (
              <ul className="pm-list">
                {animations.items.map((item, i) => (
                  <li key={`${item.target}-${item.name}-${i}`}>
                    <span className="pm-list-target" title={item.target}>
                      {item.target}
                    </span>
                    <span className="pm-muted">
                      {item.name}
                      {item.durationMs != null
                        ? ` · ${Math.round(item.durationMs)}ms`
                        : ''}
                      {item.infinite ? ' · ∞' : ''}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <div className="pm-actions">
              <button
                type="button"
                className="pm-btn"
                onClick={refreshAnimations}
              >
                {pm.refreshList}
              </button>
              <button
                type="button"
                className={`pm-btn ${pauseAll ? 'pm-btn-active' : ''}`}
                onClick={() => setPauseAll((v) => !v)}
              >
                {pauseAll ? pm.resumeAnimations : pm.pauseAll}
              </button>
            </div>
          </section>

          <section className="pm-section">
            <div className="pm-section-title">
              {pm.sectionCoordinator}
              <span className="pm-hint">{pm.coordinatorHint}</span>
            </div>
            <div className="pm-grid">
              <Row
                label={pm.instantActive}
                value={`${coordinator.activeSlots} / ${coordinator.maxConcurrent}`}
                className={
                  coordinator.activeSlots >= coordinator.maxConcurrent
                    ? 'pm-warn'
                    : coordinator.activeSlots > 0
                      ? 'pm-ok'
                      : 'pm-muted'
                }
              />
              <Row
                label={pm.sessionPeak}
                value={`${coordinator.peakActiveSlots}`}
                className={
                  coordinator.peakActiveSlots > 0 ? 'pm-ok' : 'pm-muted'
                }
              />
              <Row
                label={pm.waitingDelayed}
                value={`${coordinator.waitingQueue} / ${coordinator.delayedQueue}`}
                className={
                  coordinator.totalQueued > 5 ? 'pm-warn' : 'pm-muted'
                }
              />
              <Row
                label={pm.totalScheduled}
                value={`${coordinator.totalScheduled}`}
                className={
                  coordinator.totalScheduled > 0 ? 'pm-ok' : 'pm-muted'
                }
              />
              <Row
                label={pm.totalAcquired}
                value={`${coordinator.totalAcquired}`}
                className={
                  coordinator.totalAcquired > 0 ? 'pm-ok' : 'pm-muted'
                }
              />
              <Row
                label={pm.stateRegistry}
                value={`${coordinator.statesSize}`}
              />
              <Row
                label={pm.pageReady}
                value={coordinator.pageReady ? 'YES' : 'NO'}
                className={coordinator.pageReady ? 'pm-ok' : 'pm-warn'}
              />
              <Row
                label="Burst"
                value={coordinator.inBurstMode ? 'ON' : 'OFF'}
                className={
                  coordinator.inBurstMode ? 'pm-ok' : 'pm-muted'
                }
              />
              {coordinator.currentPageId && (
                <div className="pm-row pm-row-full">
                  <span className="pm-muted">{pm.page}</span>
                  <span
                    className="pm-list-target"
                    title={coordinator.currentPageId}
                  >
                    {coordinator.currentPageId}
                  </span>
                </div>
              )}
            </div>
            <div className="pm-actions">
              <button
                type="button"
                className="pm-btn"
                onClick={() =>
                  configureCoordinator({
                    baseConcurrent: 6,
                    burstConcurrent: 16,
                  })
                }
              >
                {pm.ecoMode}
              </button>
              <button
                type="button"
                className="pm-btn"
                onClick={() =>
                  configureCoordinator({
                    baseConcurrent: 16,
                    burstConcurrent: 48,
                  })
                }
              >
                {pm.defaultMode}
              </button>
              <button
                type="button"
                className="pm-btn"
                onClick={() =>
                  configureCoordinator({
                    baseConcurrent: 24,
                    burstConcurrent: 48,
                  })
                }
              >
                {pm.performanceMode}
              </button>
              <button
                type="button"
                className="pm-btn pm-btn-ghost"
                onClick={() => {
                  animationCoordinator.resetConcurrencyStats()
                  showToast(pm.toastSessionStatsReset)
                }}
              >
                {pm.resetPeak}
              </button>
            </div>
          </section>

          <section className="pm-section">
            <div className="pm-section-title">{pm.sectionResource}</div>
            <div className="pm-grid">
              <Row
                label={pm.queued}
                value={`${resource.queued}`}
                className={resource.queued > 0 ? 'pm-warn' : 'pm-muted'}
              />
              <Row
                label={pm.inProgress}
                value={`${resource.active}`}
                className={resource.active > 0 ? 'pm-ok' : 'pm-muted'}
              />
              <Row label={pm.completed} value={`${resource.completed}`} />
              <Row
                label={pm.failed}
                value={`${resource.failed}`}
                className={resource.failed > 0 ? 'pm-bad' : 'pm-muted'}
              />
            </div>
            <div className="pm-actions">
              <button
                type="button"
                className="pm-btn"
                disabled={!hasResourceActivity}
                onClick={() => {
                  globalResourceLoader.clear()
                  showToast(pm.toastQueueCleared)
                }}
              >
                {pm.clearQueue}
              </button>
              <button
                type="button"
                className="pm-btn"
                onClick={() => {
                  globalResourceLoader.reset()
                  showToast(pm.toastStatsReset)
                }}
              >
                {pm.resetStats}
              </button>
              <button
                type="button"
                className="pm-btn"
                onClick={() => {
                  clearPlaylistCache()
                  clearLyricsCache()
                  showToast(pm.toastMusicCacheCleared)
                }}
              >
                {pm.clearMusicCache}
              </button>
            </div>
          </section>

          {toast && <div className="pm-toast">{toast}</div>}
        </div>
      )}
    </div>
  )
}

function Row({
  label,
  value,
  className,
}: {
  label: string
  value: string
  className?: string
}) {
  return (
    <div className="pm-row">
      <span className="pm-muted">{label}</span>
      <span className={className}>{value}</span>
    </div>
  )
}
