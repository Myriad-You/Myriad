/**
 * Reports status bar — one card that carries the whole page state:
 * - Rotating tip carousel; highlight copy scrolls before the next tip
 * - Merged actions (play-all / stage transport)
 * - Large hero title that tracks the active tip
 * Controls follow the home page status bar (Home.tsx user card): rounded-xl
 * glass, round avatar, hairline divider, rounded-lg ghost buttons in the
 * theme color. Styling lives in reportsStatusBar.css.
 */
import type { CSSProperties } from 'react'
import type {
  ReportHighlight,
  ReportsStatusCopy,
} from './reportsDynamicStatus'
import {
  LuPause,
  LuPlay,
  LuRefreshCw,
  LuX,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { Avatar } from '../../components/Avatar'
import {
  buildReportsDynamicTips,
  marqueeDurationMs,
} from './reportsDynamicStatus'
import './reportsStatusBar.css'

const FIT_DWELL_MS = 4200
const MARQUEE_START_HOLD_MS = 1400
const MARQUEE_END_HOLD_MS = 900
const SLIDE_EASE = [0.32, 0.72, 0, 1] as const

/** Signed-in viewer, for the avatar on their own report tips. Null when guest. */
export interface ViewerIdentity {
  name?: string | null
  avatarUrl?: string | null
}

interface Props {
  isPageReady?: boolean
  isStageMode: boolean
  stagePaused: boolean
  stagePlatformId?: string | null
  stagePlatformName?: string | null
  /** Latin platform name for the stage hero — display names get localized to CJK */
  stagePlatformHero?: string | null
  enabledPlatformCount: number
  reportCount: number
  hasEnabledPlatforms: boolean
  isAdmin: boolean
  refreshingStage: boolean
  /** Null for guests — then empty-state tips carry no glyph at all */
  viewer?: ViewerIdentity | null
  highlights?: ReportHighlight[]
  copy: ReportsStatusCopy
  actionTitles: {
    playAll: string
    refreshing: string
    refreshCurrent: string
    continuePlay: string
    pause: string
    closeStage: string
  }
  titleStyle: {
    fontFamily: string
    fontSize: string
    color: string
    webkitTextStroke: string
    top: string
  }
  /** Hide hero on small screens during stage (desktop still shows) */
  stageCompactHero?: boolean
  onPlayAll: () => void
  onRefreshStage: () => void
  onCloseStage: () => void
}

/** Reports are the viewer's own data — their face stays on the bar. */
function TipGlyph({ viewer }: { viewer?: ViewerIdentity | null }) {
  if (!viewer) return null
  return (
    <Avatar
      src={viewer.avatarUrl}
      name={viewer.name}
      decorative
      className="rsb-portrait"
    />
  )
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  )
}

function MarqueeSub({
  text,
  paused,
  canAdvance,
  onFinished,
}: {
  text: string
  paused: boolean
  canAdvance: boolean
  onFinished: () => void
}) {
  const trackRef = useRef<HTMLDivElement>(null)
  const textRef = useRef<HTMLSpanElement>(null)
  const [overflow, setOverflow] = useState(0)
  const [phase, setPhase] = useState<'hold-start' | 'scroll' | 'hold-end'>(
    'hold-start',
  )

  useLayoutEffect(() => {
    const track = trackRef.current
    const node = textRef.current
    if (!track || !node) return

    const measure = () => {
      setOverflow(Math.max(0, node.scrollWidth - track.clientWidth))
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(track)
    observer.observe(node)
    return () => observer.disconnect()
  }, [text])

  useEffect(() => {
    setPhase('hold-start')
  }, [text])

  const durationMs = marqueeDurationMs(overflow)
  const shouldScroll = overflow > 0 && !prefersReducedMotion()

  useEffect(() => {
    if (paused) return

    if (phase === 'hold-start') {
      if (!shouldScroll && !canAdvance) return
      const wait = shouldScroll ? MARQUEE_START_HOLD_MS : FIT_DWELL_MS
      const timer = window.setTimeout(() => {
        if (shouldScroll) setPhase('scroll')
        else if (canAdvance) onFinished()
      }, wait)
      return () => window.clearTimeout(timer)
    }

    if (phase === 'hold-end') {
      if (!canAdvance) return
      const timer = window.setTimeout(onFinished, MARQUEE_END_HOLD_MS)
      return () => window.clearTimeout(timer)
    }
  }, [phase, paused, canAdvance, shouldScroll, onFinished])

  return (
    <div
      ref={trackRef}
      className={`rsb-sub rsb-sub-track${overflow > 0 ? ' is-overflow' : ''}`}
    >
      <span
        ref={textRef}
        className={`rsb-sub-text${phase === 'scroll' ? ' is-marquee' : ''}`}
        style={
          {
            '--rsb-marquee-shift': `${overflow}px`,
            '--rsb-marquee-ms': `${durationMs}ms`,
            transform:
              phase === 'hold-end' ? `translateX(-${overflow}px)` : undefined,
          } as CSSProperties
        }
        onAnimationEnd={(event) => {
          if (event.animationName === 'rsb-marquee' && phase === 'scroll') {
            setPhase('hold-end')
          }
        }}
      >
        {text}
      </span>
    </div>
  )
}

export default function ReportsStatusBar({
  isPageReady = true,
  isStageMode,
  stagePaused,
  stagePlatformId,
  stagePlatformName,
  stagePlatformHero,
  enabledPlatformCount,
  reportCount,
  hasEnabledPlatforms,
  isAdmin,
  refreshingStage,
  viewer,
  highlights,
  copy,
  actionTitles,
  titleStyle,
  stageCompactHero = false,
  onPlayAll,
  onRefreshStage,
  onCloseStage,
}: Props) {
  const tips = useMemo(
    () =>
      buildReportsDynamicTips({
        copy,
        isStageMode,
        stagePaused,
        stagePlatformId,
        stagePlatformName,
        stagePlatformHero,
        enabledPlatformCount,
        reportCount,
        highlights,
      }),
    [
      copy,
      isStageMode,
      stagePaused,
      stagePlatformId,
      stagePlatformName,
      stagePlatformHero,
      enabledPlatformCount,
      reportCount,
      highlights,
    ],
  )

  const tipsSignature = tips.map((item) => item.id).join('|')
  const [tipIndex, setTipIndex] = useState(0)
  // 指针停在条子上就按住轮播：正在读（或正要点）的时候别把字换掉
  const [held, setHeld] = useState(false)

  useEffect(() => {
    setTipIndex(0)
  }, [tipsSignature])

  const advanceTip = useCallback(() => {
    setTipIndex((prev) => (prev + 1) % Math.max(tips.length, 1))
  }, [tips.length])

  const tip = tips[tipIndex] || tips[0]

  if (!tip) return null

  const showPlay = !isStageMode && hasEnabledPlatforms
  const hasActions = isStageMode || showPlay

  return (
    <div
      className={`rsb mb-2${held ? ' is-held' : ''}`}
      onMouseEnter={() => setHeld(true)}
      onMouseLeave={() => setHeld(false)}
    >
      {/* Dynamic hero title tracks active tip */}
      <div
        className={`rsb-hero ${stageCompactHero ? 'hidden md:block' : ''}`}
        style={{
          top: titleStyle.top,
          fontFamily: titleStyle.fontFamily,
          fontSize: titleStyle.fontSize,
          color: titleStyle.color,
          WebkitTextStroke: titleStyle.webkitTextStroke,
        }}
        aria-hidden
      >
        <AnimatePresence mode="wait">
          <motion.span
            key={tip.hero}
            className="inline-block"
            initial={{ opacity: 0, y: 10, filter: 'blur(6px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
            exit={{ opacity: 0, y: -8, filter: 'blur(6px)' }}
            transition={{ duration: 0.35, ease: 'easeOut' }}
          >
            {tip.hero}
          </motion.span>
        </AnimatePresence>
      </div>

      <motion.div
        className="rsb-rail"
        initial={{ opacity: 0, x: -16 }}
        animate={isPageReady ? { opacity: 1, x: 0 } : { opacity: 0, x: -16 }}
        transition={{
          duration: 0.3,
          ease: 'easeOut',
          delay: isPageReady ? 0.1 : 0,
        }}
      >
        {/* layout: the bar morphs to each tip's width instead of reserving a
            fixed slab with dead space in the middle. It only stays smooth
            because popLayout below pulls the outgoing tip out of flow —
            otherwise the two tips fight over the width mid-swap. */}
        <motion.div
          className="rsb-bar glass"
          layout
          transition={{ duration: 0.42, ease: SLIDE_EASE }}
        >
          <div
            className="rsb-tip"
            role="status"
            aria-label={tip.sub ? `${tip.main}. ${tip.sub}` : tip.main}
          >
            <TipGlyph viewer={viewer} />
            <div className="rsb-stack">
              <AnimatePresence initial={false} mode="popLayout">
                <motion.div
                  key={tip.id}
                  className="rsb-slide"
                  layout="position"
                  initial={{ opacity: 0, y: 8, filter: 'blur(5px)' }}
                  animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
                  exit={{ opacity: 0, y: -8, filter: 'blur(5px)' }}
                  transition={{ duration: 0.34, ease: SLIDE_EASE }}
                >
                  <div
                    className={`rsb-lines${tip.scrollSub ? ' is-ticker' : ''}`}
                  >
                    <div className="rsb-main">{tip.main}</div>
                    {tip.scrollSub && tip.sub ? (
                      <MarqueeSub
                        key={tip.id}
                        text={tip.sub}
                        paused={held}
                        canAdvance={tips.length > 1}
                        onFinished={advanceTip}
                      />
                    ) : tip.sub ? (
                      <div className="rsb-sub">{tip.sub}</div>
                    ) : null}
                  </div>
                </motion.div>
              </AnimatePresence>
            </div>
          </div>

          {hasActions && <span className="rsb-sep" aria-hidden />}

          <div className="rsb-actions">
            {isStageMode ? (
              <>
                {isAdmin && (
                  <button
                    type="button"
                    onClick={onRefreshStage}
                    disabled={refreshingStage}
                    className={`rsb-btn is-icon ${
                      refreshingStage ? 'is-spinning' : ''
                    }`}
                    title={
                      refreshingStage
                        ? actionTitles.refreshing
                        : actionTitles.refreshCurrent
                    }
                  >
                    <LuRefreshCw />
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => {
                    window.dispatchEvent(new CustomEvent('stage-toggle-pause'))
                  }}
                  className={`rsb-btn is-icon ${stagePaused ? 'is-key' : ''}`}
                  title={
                    stagePaused ? actionTitles.continuePlay : actionTitles.pause
                  }
                >
                  {stagePaused ? (
                    <LuPlay className="rsb-glyph-solid" />
                  ) : (
                    <LuPause className="rsb-glyph-solid" />
                  )}
                </button>
                <button
                  type="button"
                  onClick={onCloseStage}
                  className="rsb-btn is-icon"
                  title={actionTitles.closeStage}
                >
                  <LuX />
                </button>
              </>
            ) : (
              <AnimatePresence initial={false} mode="popLayout">
                {showPlay ? (
                  <motion.button
                    key="play-all"
                    type="button"
                    onClick={onPlayAll}
                    className="rsb-btn is-icon"
                    title={actionTitles.playAll}
                    initial={{ opacity: 0, scale: 0.9 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.9 }}
                    transition={{ duration: 0.26, ease: SLIDE_EASE }}
                  >
                    <LuPlay className="rsb-glyph-solid" />
                  </motion.button>
                ) : null}
              </AnimatePresence>
            )}
          </div>
        </motion.div>
      </motion.div>
    </div>
  )
}
