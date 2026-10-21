import type { WidgetPopupProps } from '../../widgetGridTypes'
import type { StageReport } from './stageScript'
import {
  LuListOrdered,
  LuPause,
  LuPlay,
  LuRefreshCw,
  LuRotateCw,
  LuSettings,
  LuSkipBack,
  LuSkipForward,
  LuX,
} from '@lib/icons'
import { Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useAuth } from '../../../contexts/AuthContext'
import { useI18n } from '../../../contexts/I18nContext'
import { useAnimationLevel } from '../../../hooks/useAnimationLevel'
import { fetchPlatformData, generatePlatformReports } from '../../../services/platformTasksApi'
import { emitAppEvent } from '../../../utils/appEvents'
import { lazyWithPreload } from '../../../utils/codeSplitting'
import { resolvePlatformId } from '../../../utils/platformId'
import { notifyRecentActivityUpdated } from '../../../utils/recentActivity'
import { resolveReportPlatformId } from '../../../utils/reportCardVisuals'
import { reportUserFacingError } from '../../../utils/reportError'
import {
  getLatestReportDeduped,
  getPublicConfigDeduped,
  invalidateLatestReportCache,
} from '../../../utils/requestDedup'
import { showToast } from '../../../utils/toastManager'
import { PLATFORM_CONFIG } from '../reportCard/platformConfig'
import { ReportCardWidget } from '../reportCard/ReportCardWidget'
import { GlowBackground } from '../shared/GlowBackground'
import { buildStageReport } from './stageScript'
import { useStagePlayer } from './useStagePlayer'
import './reportStage.css'

/** 站长区只有站长用得到，访客不下载它。 */
const StageOwnerSettings = lazyWithPreload(() =>
  import('./StageOwnerSettings').then((module) => ({ default: module.StageOwnerSettings })),
)

/** 字幕区最多留几句：旧句往上退，不堆满屏。 */
const VISIBLE_BEATS = 5
const CARD_BASE = { width: 308, height: 154 }
/** 收回动画放完再卸下内嵌的报告卡，让它的计时器停下。 */
const CARD_UNMOUNT_DELAY_MS = 400
/** 卡片在这几幕显示总览，其余各幕翻到详情。 */
const OVERVIEW_CHAPTERS = new Set(['echo', 'portrait', 'closing'])

/** 站长在平台设置里开着的平台，按拖拽顺序。读不到配置时返回 null，舞台照旧全列。 */
async function loadPlatformOrder(): Promise<string[] | null> {
  try {
    const data = await getPublicConfigDeduped()
    if (!Array.isArray(data?.platforms)) return null
    return data.platforms
      .filter((item: { enabled?: unknown }) => item?.enabled)
      .map((item: { name?: unknown }) => (typeof item?.name === 'string' ? resolvePlatformId(item.name) : null))
      .filter((id: string | null): id is string => Boolean(id))
  } catch {
    return null
  }
}

function readPlatformParam(params: unknown): string | null {
  if (params && typeof params === 'object' && 'platform' in params) {
    const value = (params as { platform?: unknown }).platform
    return typeof value === 'string' ? value : null
  }
  return null
}

function ScaledCard({ platform, visuals, overview }: { platform: string; visuals: unknown; overview: boolean }) {
  const hostRef = useRef<HTMLDivElement>(null)
  const [scale, setScale] = useState(1)
  useLayoutEffect(() => {
    const host = hostRef.current
    if (!host) return
    // 用布局尺寸，不用 getBoundingClientRect：弹窗展开时整体在缩放，那一刻量到的是缩小后的
    // 大小，展开完布局尺寸不变，ResizeObserver 不会再触发，卡就一直停在缩小的倍数上。
    const fit = () => {
      const width = host.clientWidth
      const height = host.clientHeight
      if (width <= 0 || height <= 0) return
      setScale(Math.min(width / CARD_BASE.width, height / CARD_BASE.height))
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(host)
    return () => observer.disconnect()
  }, [])
  const config = useMemo(
    () => ({ id: `stage-${platform}`, type: `report-${platform}`, size: '4x2' as const, position: { x: 0, y: 0 }, config: { platformId: platform } }),
    [platform],
  )
  return (
    <div ref={hostRef} className="report-stage__card">
      <div
        className="report-stage__card-inner"
        style={{ width: CARD_BASE.width, height: CARD_BASE.height, transform: `scale(${scale})` }}
      >
        <ReportCardWidget config={config} isEditMode={false} bare data={visuals} showOverview={overview} />
      </div>
    </div>
  )
}

export default function ReportStagePopup({ config, params, active, close }: WidgetPopupProps) {
  const { t, format, locale } = useI18n()
  const copy = t.reportStage
  const { isAdmin } = useAuth()
  const anim = useAnimationLevel()
  const [reports, setReports] = useState<Map<string, StageReport>>(new Map())
  const [loaded, setLoaded] = useState(false)
  const [platform, setPlatform] = useState(
    () => readPlatformParam(params) ?? resolveReportPlatformId(config),
  )
  const [playAll, setPlayAll] = useState(false)
  const [generating, setGenerating] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)

  // 卡片再次打开弹窗时跟着切到那张卡的平台。
  const cardPlatform = readPlatformParam(params) ?? resolveReportPlatformId(config)
  useEffect(() => {
    setPlatform(cardPlatform)
    setPlayAll(false)
  }, [cardPlatform])

  useEffect(() => {
    if (!active) return
    void import('../../../utils/analyticsEvents').then(({ trackProductEvent, AnalyticsEvents }) => {
      trackProductEvent(AnalyticsEvents.REPORT_STAGE_OPEN, { target: cardPlatform, throttleMs: 3000 })
    })
  }, [active, cardPlatform])

  const titles = useMemo(() => {
    const numbers = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 })
    return {
      echo: copy.echo,
      insight: copy.insight,
      facts: copy.facts as Record<string, string>,
      factValues: copy.factValues as Record<string, Record<string, string>>,
      chapters: copy.chapters as Record<string, string>,
      formatNumber: (value: number) => numbers.format(value),
    }
  }, [copy.chapters, copy.echo, copy.factValues, copy.facts, copy.insight, locale])

  // 先发后至的请求不能盖掉后发先至的（切换风格后的强制刷新最常见）。
  const loadSeq = useRef(0)
  const load = useCallback(
    async (forceRefresh = false) => {
      const seq = ++loadSeq.current
      try {
        const [data, enabled] = await Promise.all([
          getLatestReportDeduped({ forceRefresh }),
          loadPlatformOrder(),
        ])
        if (seq !== loadSeq.current) return
        const built = new Map<string, StageReport>()
        for (const raw of Array.isArray(data?.platform_reports) ? data.platform_reports : []) {
          const report = buildStageReport(raw, titles)
          if (report && PLATFORM_CONFIG[report.platform]) built.set(report.platform, report)
        }
        // 站长关掉的平台不上舞台；顺序跟平台设置里的拖拽一致。
        const next = enabled
          ? new Map(enabled.flatMap((id) => (built.has(id) ? [[id, built.get(id)!] as const] : [])))
          : built
        setReports(next)
      } catch {
        if (seq === loadSeq.current) setReports(new Map())
      } finally {
        if (seq === loadSeq.current) setLoaded(true)
      }
    },
    [titles],
  )
  useEffect(() => {
    void load()
  }, [load])

  const report = reports.get(platform) ?? null
  const chapters = report?.chapters ?? []
  const order = useMemo(() => Array.from(reports.keys()), [reports])

  const advancePlatform = useCallback(() => {
    if (!playAll) return
    const index = order.indexOf(platform)
    const next = order[index + 1]
    if (next) setPlatform(next)
    else setPlayAll(false)
  }, [order, platform, playAll])

  const player = useStagePlayer(chapters, {
    running: active && report !== null && !settingsOpen,
    resetKey: report,
    onEnded: advancePlatform,
  })
  const { state, toggle, pause, jump } = player
  const chapter = chapters[state.chapter]
  const visibleBeats = chapter ? chapter.beats.slice(0, state.shown).slice(-VISIBLE_BEATS) : []
  const currentBeat = chapter && state.shown > 0 ? chapter.beats[state.shown - 1] : undefined

  // 收回时暂停；重开不自动续播，让人自己按。
  const wasActive = useRef(active)
  useEffect(() => {
    if (wasActive.current && !active) pause()
    wasActive.current = active
  }, [active, pause])

  // 收回后卸下内嵌的报告卡：它自己的轮换、在线状态轮询不该在看不见时继续跑。
  const [cardMounted, setCardMounted] = useState(active)
  useEffect(() => {
    if (active) {
      setCardMounted(true)
      return undefined
    }
    const timer = window.setTimeout(setCardMounted, CARD_UNMOUNT_DELAY_MS, false)
    return () => window.clearTimeout(timer)
  }, [active])

  const rootRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!active || settingsOpen) return
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return
      const target = event.target instanceof Element ? event.target : null
      // 只管落在舞台里（或焦点丢到 body）时的按键，别处的方向键不归它。
      if (target && target !== document.body && !rootRef.current?.contains(target)) return
      if (target?.closest('input, textarea, select, [contenteditable="true"]')) return
      // 焦点在按钮上时空格由按钮自己处理，这里再切一次就抵消了。
      if (event.key === ' ' && target?.closest('button')) return
      if (event.key === ' ' || event.key === 'k') {
        event.preventDefault()
        toggle()
      } else if (event.key === 'ArrowRight') {
        event.preventDefault()
        jump(state.chapter + 1)
      } else if (event.key === 'ArrowLeft') {
        event.preventDefault()
        jump(state.chapter - 1)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [active, settingsOpen, toggle, jump, state.chapter])

  const platformLabel = PLATFORM_CONFIG[platform]?.label ?? platform
  const regenerate = useCallback(async () => {
    if (generating) return
    setGenerating(true)
    try {
      try {
        const fetched = await fetchPlatformData(platform)
        if (fetched.success !== false) notifyRecentActivityUpdated()
      } catch {
        // 拉新数据失败不挡生成：用已有数据照样写。
      }
      const body = await generatePlatformReports<{ platform?: string }>([platform])
      const produced = Array.isArray(body?.reports) && body.reports.some((r) => r?.platform === platform)
      if (!produced) throw new Error(body?.message || 'not generated')
      invalidateLatestReportCache()
      emitAppEvent('reports-updated')
      await load(true)
      showToast({ message: format(copy.generated, { platform: platformLabel }), type: 'success', replaceKey: 'report-stage' })
    } catch (err) {
      showToast({
        message: reportUserFacingError(err, copy.generateFailed, t.reportCard),
        type: 'error',
        replaceKey: 'report-stage',
      })
    } finally {
      setGenerating(false)
    }
  }, [copy.generateFailed, copy.generated, format, generating, load, platform, platformLabel, t.reportCard])

  const accent = PLATFORM_CONFIG[platform]?.color ?? 'var(--color-primary, #6366f1)'
  const overview = !chapter || OVERVIEW_CHAPTERS.has(chapter.key)

  return (
    <div ref={rootRef} className="report-stage" style={{ '--stage-accent': accent } as React.CSSProperties}>
      <div className="report-stage__glow" aria-hidden="true">
        <GlowBackground
          color={accent}
          animLevel={anim.level}
          shouldAnimate={active && anim.loop && anim.widgetGlow}
          variant="dual"
          size="lg"
          opacity={0.55}
        />
      </div>

      <header className="report-stage__head">
        <nav className="report-stage__platforms" aria-label={copy.platforms}>
          {(order.includes(platform) ? order : [platform, ...order]).map((id) => (
            <button
              key={id}
              type="button"
              className="report-stage__platform"
              aria-pressed={id === platform}
              onClick={() => {
                setPlayAll(false)
                setPlatform(id)
              }}
            >
              <span className="report-stage__platform-icon" aria-hidden="true">
                {PLATFORM_CONFIG[id]?.icon}
              </span>
              {PLATFORM_CONFIG[id]?.label ?? id}
            </button>
          ))}
        </nav>
        {chapters.length > 0 ? (
          <div className="report-stage__act">
            <span>{format(copy.chapterOf, { current: state.chapter + 1, total: chapters.length })}</span>
            <strong>{chapter?.title}</strong>
          </div>
        ) : null}
        <button type="button" className="report-stage__icon-btn" aria-label={t.common.close} onClick={close}>
          <LuX aria-hidden="true" />
        </button>
      </header>

      <main className="report-stage__body">
        {settingsOpen && isAdmin ? (
          <Suspense fallback={<div className="report-stage__empty"><p>{t.common.loading}</p></div>}>
            <StageOwnerSettings platform={platform} onStyleChanged={() => void load(true)} />
          </Suspense>
        ) : report ? (
          <>
            <section className="report-stage__subtitles" aria-live="polite">
              {report.headline && state.chapter === 0 ? (
                <p className="report-stage__headline">{report.headline}</p>
              ) : null}
              {visibleBeats.map((beat, index) => (
                <p
                  key={`${state.chapter}-${state.shown - visibleBeats.length + index}`}
                  className="report-stage__line"
                  data-current={index === visibleBeats.length - 1 ? 'true' : undefined}
                >
                  {beat.text}
                </p>
              ))}
              {state.ended ? <p className="report-stage__end">{copy.finished}</p> : null}
            </section>
            <aside className="report-stage__side">
              {cardMounted ? (
                <ScaledCard platform={platform} visuals={report.cardVisuals} overview={overview} />
              ) : null}
              {currentBeat?.fact ? (
                <div className="report-stage__fact" key={currentBeat.fact.id}>
                  {currentBeat.fact.media ? (
                    <img src={currentBeat.fact.media} alt="" loading="lazy" decoding="async" />
                  ) : null}
                  <span>{currentBeat.fact.label}</span>
                  <strong>{currentBeat.fact.value}</strong>
                </div>
              ) : null}
            </aside>
          </>
        ) : (
          <div className="report-stage__empty">
            <p>{loaded ? copy.empty : t.common.loading}</p>
            {loaded && isAdmin ? (
              <button type="button" className="report-stage__primary" disabled={generating} onClick={regenerate}>
                <LuRefreshCw aria-hidden="true" className={generating ? 'is-spinning' : undefined} />
                {generating ? copy.generating : copy.generate}
              </button>
            ) : null}
          </div>
        )}
      </main>

      <footer className="report-stage__controls">
        <div className="report-stage__progress" aria-hidden="true">
          {chapters.map((item, index) => (
            <span key={item.key} data-state={index < state.chapter ? 'done' : index === state.chapter ? 'now' : 'next'} />
          ))}
        </div>
        <div className="report-stage__buttons">
          <button type="button" className="report-stage__icon-btn" aria-label={copy.prevChapter} disabled={!report || state.chapter === 0} onClick={() => jump(state.chapter - 1)}>
            <LuSkipBack aria-hidden="true" />
          </button>
          <button
            type="button"
            className="report-stage__play"
            aria-label={state.ended ? copy.replay : state.playing ? copy.pause : copy.play}
            disabled={!report}
            onClick={toggle}
          >
            {state.ended ? <LuRotateCw aria-hidden="true" /> : state.playing ? <LuPause aria-hidden="true" /> : <LuPlay aria-hidden="true" />}
          </button>
          <button type="button" className="report-stage__icon-btn" aria-label={copy.nextChapter} disabled={!report || state.chapter >= chapters.length - 1} onClick={() => jump(state.chapter + 1)}>
            <LuSkipForward aria-hidden="true" />
          </button>
          <button
            type="button"
            className="report-stage__chip"
            aria-pressed={playAll}
            disabled={order.length < 2}
            onClick={() => {
              if (!playAll) {
                void import('../../../utils/analyticsEvents').then(({ trackProductEvent, AnalyticsEvents }) => {
                  trackProductEvent(AnalyticsEvents.REPORT_PLAY_ALL, { target: platform, throttleMs: 5000 })
                })
              }
              setPlayAll((value) => !value)
            }}
          >
            <LuListOrdered aria-hidden="true" />
            {playAll ? copy.playAllOn : copy.playAll}
          </button>
        </div>
        {isAdmin ? (
          <div className="report-stage__owner">
            {report?.styleStale ? <span className="report-stage__stale">{copy.styleStale}</span> : null}
            {report ? (
              <button type="button" className="report-stage__chip" disabled={generating} onClick={regenerate}>
                <LuRefreshCw aria-hidden="true" className={generating ? 'is-spinning' : undefined} />
                {generating ? copy.generating : copy.regenerate}
              </button>
            ) : null}
            <button
              type="button"
              className="report-stage__icon-btn"
              aria-label={copy.settings}
              aria-pressed={settingsOpen}
              onPointerEnter={() => void StageOwnerSettings.preload().catch(() => {})}
              onFocus={() => void StageOwnerSettings.preload().catch(() => {})}
              onClick={() => setSettingsOpen((open) => !open)}
            >
              <LuSettings aria-hidden="true" />
            </button>
          </div>
        ) : null}
      </footer>
    </div>
  )
}
