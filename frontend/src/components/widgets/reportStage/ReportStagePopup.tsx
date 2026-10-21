import type { WidgetPopupProps } from '../../widgetGridTypes'
import type { StageReport } from './stageScript'
import {
  LuListOrdered,
  LuRefreshCw,
  LuRotateCw,
  LuSettings,
  LuX,
} from '@lib/icons'
import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
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
import { GlowBackground } from '../shared/GlowBackground'
import { ProgressRing, RevealLine, StageCovers, StagePauseIcon, StagePlayIcon, useFlipLayout } from './stageParts'
import { buildStageReport } from './stageScript'
import { useStagePlayer } from './useStagePlayer'
import './reportStage.css'

/** 站长区只有站长用得到，访客不下载它。 */
const StageOwnerSettings = lazyWithPreload(() =>
  import('./StageOwnerSettings').then((module) => ({ default: module.StageOwnerSettings })),
)

/** 字幕区最多留几句：旧句往上退、变淡，不堆满屏。 */
const VISIBLE_BEATS = 4
/** 台词还没点名时，书架摆几张。 */
const SHELF_SIZE = 5
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

/** 当前句按长短换字号：短句可以很大，长句大字会排成一整面墙。 */
function lengthClass(text: string): 'short' | 'long' | undefined {
  const length = Array.from(text).length
  if (length <= 18) return 'short'
  if (length > 40) return 'long'
  return undefined
}

function readPlatformParam(params: unknown): string | null {
  if (params && typeof params === 'object' && 'platform' in params) {
    const value = (params as { platform?: unknown }).platform
    return typeof value === 'string' ? value : null
  }
  return null
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
  const { state, toggle, pause, jump, next } = player
  const chapter = chapters[state.chapter]
  const visibleBeats = chapter ? chapter.beats.slice(0, state.shown).slice(-VISIBLE_BEATS) : []

  // 收回时暂停；重开不自动续播，让人自己按。
  const wasActive = useRef(active)
  useEffect(() => {
    if (wasActive.current && !active) pause()
    wasActive.current = active
  }, [active, pause])

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

  // 最近一句点到名的作品留着，直到下一句点到别的；这一幕还没点过名就摆书架。
  const covers = useMemo(() => {
    if (!report || !chapter) return { items: [], mode: 'shelf' as const }
    for (let index = Math.min(state.shown, chapter.beats.length) - 1; index >= 0; index--) {
      const media = chapter.beats[index].media
      if (media?.length) return { items: media, mode: 'mentioned' as const }
    }
    return { items: report.media.slice(0, SHELF_SIZE), mode: 'shelf' as const }
  }, [chapter, report, state.shown])

  // 字幕区变高时，标题、旧句、封面从原来的位置滑到新落点。
  const sceneRef = useRef<HTMLElement>(null)
  useFlipLayout(
    sceneRef,
    `${platform}|${state.chapter}|${state.shown}|${state.ended}|${covers.mode}|${covers.items.length}`,
    active && anim.level !== 'exlight',
  )

  const accent = PLATFORM_CONFIG[platform]?.color ?? 'var(--color-primary, #6366f1)'
  const platforms = order.includes(platform) ? order : [platform, ...order]

  return (
    <div
      ref={rootRef}
      className="report-stage"
      style={{ '--stage-accent': accent } as React.CSSProperties}
    >
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

      <header className="report-stage__top">
        <div className="report-stage__who">
          <span className="report-stage__who-icon" aria-hidden="true">{PLATFORM_CONFIG[platform]?.icon}</span>
          <span className="report-stage__who-text">
            <strong>{platformLabel}</strong>
            {report?.headline ? <span>{report.headline}</span> : null}
          </span>
        </div>
        <div className="report-stage__corner">
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
          <button type="button" className="report-stage__close" aria-label={t.common.close} onClick={close}>
            <LuX aria-hidden="true" />
          </button>
        </div>
      </header>

      <main ref={sceneRef} className="report-stage__scene">
        {settingsOpen && isAdmin ? (
          <div className="report-stage__panel">
            <Suspense fallback={<div className="report-stage__empty"><p>{t.common.loading}</p></div>}>
              <StageOwnerSettings platform={platform} onStyleChanged={() => void load(true)} />
            </Suspense>
          </div>
        ) : report ? (
          <section
            className="report-stage__captions"
            aria-live="polite"
            title={state.ended ? undefined : copy.nextLine}
            onClick={(event) => {
              // 选字时不翻。
              if (window.getSelection()?.toString()) return
              if (event.detail > 1 || state.ended) return
              next()
            }}
          >
            {chapter ? (
              <p
                className="report-stage__kicker"
                key={`${platform}-${state.chapter}`}
                data-flip={`kicker-${platform}-${state.chapter}`}
              >
                <span>{format(copy.chapterOf, { current: state.chapter + 1, total: chapters.length })}</span>
                {chapter.title}
              </p>
            ) : null}
            <div className="report-stage__lines">
              {visibleBeats.map((beat, index) => {
                const current = index === visibleBeats.length - 1
                const id = `${platform}-${state.chapter}-${state.shown - visibleBeats.length + index}`
                return (
                  <p
                    key={id}
                    data-flip={`line-${id}`}
                    className="report-stage__line"
                    data-current={current ? 'true' : undefined}
                    data-age={visibleBeats.length - 1 - index}
                    data-length={lengthClass(beat.text)}
                  >
                    {current ? <RevealLine text={beat.text} animate={anim.level !== 'exlight'} /> : beat.text}
                  </p>
                )
              })}
              {state.ended ? <p className="report-stage__end" data-flip="end">{copy.finished}</p> : null}
            </div>
            <StageCovers items={covers.items} mode={covers.mode} />
          </section>
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

      <footer className="report-stage__dock">
        <nav className="report-stage__platforms" aria-label={copy.platforms}>
          {platforms.map((id) => (
            <button
              key={id}
              type="button"
              className="report-stage__platform"
              aria-pressed={id === platform}
              aria-label={PLATFORM_CONFIG[id]?.label ?? id}
              title={PLATFORM_CONFIG[id]?.label ?? id}
              onClick={() => {
                setPlayAll(false)
                setPlatform(id)
              }}
            >
              <span className="report-stage__platform-icon" aria-hidden="true">
                {PLATFORM_CONFIG[id]?.icon}
              </span>
              <span className="report-stage__platform-label" aria-hidden="true">
                {PLATFORM_CONFIG[id]?.label ?? id}
              </span>
            </button>
          ))}
        </nav>
        <span className="report-stage__dock-sep" aria-hidden="true" />
        <div className="report-stage__buttons">
          <button
            type="button"
            className="report-stage__play"
            aria-label={state.ended ? copy.replay : state.playing ? copy.pause : copy.play}
            disabled={!report}
            onClick={toggle}
          >
            <ProgressRing
              chapters={chapters.map((item) => item.beats.length)}
              chapter={state.ended ? chapters.length : state.chapter}
              fraction={chapter ? state.shown / chapter.beats.length : 0}
            />
            {state.ended ? (
              <LuRotateCw aria-hidden="true" />
            ) : state.playing ? (
              <StagePauseIcon />
            ) : (
              <StagePlayIcon />
            )}
          </button>
          <button
            type="button"
            className="report-stage__icon-btn report-stage__play-all"
            aria-label={playAll ? copy.playAllOn : copy.playAll}
            title={playAll ? copy.playAllOn : copy.playAll}
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
          </button>
        </div>
      </footer>
    </div>
  )
}
