/**
 * Reports status bar — one card that carries the whole page state:
 * - Rotating tip carousel with clickable ticks that double as the timer
 * - Merged actions (play-all / stage transport + life CTA)
 * - Large hero title that tracks the active tip
 * Controls follow the home page status bar (Home.tsx user card): rounded-xl
 * glass, round avatar, hairline divider, rounded-lg ghost buttons in the
 * theme color. Styling lives in reportsStatusBar.css.
 */
import type { ReactNode } from 'react'
import type {
  AgentLifeSnapshot,
  LifeStatusKind,
  ReportsDynamicTip,
  ReportsStatusCopy,
} from './reportsDynamicStatus'
import {
  LuChevronRight,
  LuPause,
  LuPlay,
  LuRefreshCw,
  LuX,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useCallback, useEffect, useMemo, useState } from 'react'
import LifeMark from '../../components/agent/LifeMark'
import { Avatar } from '../../components/Avatar'
import {
  buildReportsDynamicTips,
  resolveLifeActionLabel,
} from './reportsDynamicStatus'
import './reportsStatusBar.css'

const TIP_ROTATE_MS = 7000
const SLIDE_EASE = [0.32, 0.72, 0, 1] as const

interface PlatformIconSource {
  id: string
  name: string
  icon: ReactNode
}

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
  /** Latin platform name for the hero — display names get localized to CJK */
  stagePlatformHero?: string | null
  enabledPlatformCount: number
  reportCount: number
  hasEnabledPlatforms: boolean
  platforms: PlatformIconSource[]
  showLife: boolean
  lifeKind: LifeStatusKind
  lifeSnapshot: AgentLifeSnapshot | null
  isAdmin: boolean
  refreshingStage: boolean
  /** Null for guests — then report tips carry no glyph at all */
  viewer?: ViewerIdentity | null
  copy: ReportsStatusCopy
  lifeActionLabels: {
    create: string
    open: string
    login: string
  }
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
  onOpenLife: () => void
  onLogin: () => void
}

/**
 * Only ever shows *who* this tip is about — the platform mark on stage, the
 * life mark for life tips, and the signed-in user's
 * avatar for their own reports. No document icon: a page glyph next to
 * "9 reports ready" says nothing the sentence doesn't.
 */
function TipGlyph({
  tip,
  platforms,
  viewer,
}: {
  tip: ReportsDynamicTip
  platforms: PlatformIconSource[]
  viewer?: ViewerIdentity | null
}) {
  if (tip.kind === 'stage' && tip.platformId) {
    const platform = platforms.find((p) => p.id === tip.platformId)
    if (platform) {
      return (
        <span className="rsb-glyph" aria-hidden>
          {platform.icon}
        </span>
      )
    }
  }

  if (tip.kind.startsWith('life')) {
    return <LifeMark className="rsb-glyph" />
  }

  // Reports are the viewer's own data — their face belongs on them.
  if (viewer) {
    return (
      <Avatar
        src={viewer.avatarUrl}
        name={viewer.name}
        decorative
        className="rsb-portrait"
      />
    )
  }

  return null
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
  platforms,
  showLife,
  lifeKind,
  lifeSnapshot,
  isAdmin,
  refreshingStage,
  viewer,
  copy,
  lifeActionLabels,
  actionTitles,
  titleStyle,
  stageCompactHero = false,
  onPlayAll,
  onRefreshStage,
  onCloseStage,
  onOpenLife,
  onLogin,
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
        showLife,
        lifeKind,
        lifeSnapshot,
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
      showLife,
      lifeKind,
      lifeSnapshot,
    ],
  )

  const tipsSignature = tips.map((item) => item.id).join('|')
  const [tipIndex, setTipIndex] = useState(0)
  // 指针停在条子上就按住轮播：正在读（或正要点）的时候别把字换掉
  const [held, setHeld] = useState(false)

  useEffect(() => {
    setTipIndex(0)
  }, [tipsSignature])

  useEffect(() => {
    if (tips.length <= 1 || held) return
    const timer = window.setInterval(() => {
      setTipIndex((prev) => (prev + 1) % tips.length)
    }, TIP_ROTATE_MS)
    return () => window.clearInterval(timer)
  }, [tips.length, tipsSignature, held])

  const tip = tips[tipIndex] || tips[0]
  const lifeAction = showLife
    ? resolveLifeActionLabel(lifeKind, lifeActionLabels)
    : null
  const lifeInteractive =
    lifeKind === 'create' ||
    lifeKind === 'ready' ||
    lifeKind === 'guest'

  const openLife = useCallback(() => {
    if (lifeKind === 'guest') {
      onLogin()
      return
    }
    if (lifeKind === 'disabled' || lifeKind === 'loading' || !showLife) {
      return
    }
    onOpenLife()
  }, [lifeKind, onLogin, onOpenLife, showLife])

  const tipClickable = tip?.action === 'open-life' || tip?.action === 'login'

  const handleTipActivate = () => {
    if (!tipClickable || !tip) return
    if (tip.action === 'login') {
      onLogin()
      return
    }
    openLife()
  }

  if (!tip) return null

  // The action follows the tip: play-all belongs to the report tips, the life
  // CTA to the life tip. Showing both at once made the right side a permanent
  // toolbar that had nothing to do with the sentence on the left.
  const isLifeTip = tip.kind.startsWith('life')
  const showCta = !isStageMode && isLifeTip && Boolean(lifeAction && lifeInteractive)
  const showPlay = !isStageMode && !isLifeTip && hasEnabledPlatforms
  const hasActions = isStageMode || showCta || showPlay

  return (
    <div
      className={`rsb ${isStageMode ? 'mb-2 md:mb-0' : ''}`}
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
            className={`rsb-tip ${tipClickable ? 'is-clickable' : ''}`}
            role={tipClickable ? 'button' : 'status'}
            tabIndex={tipClickable ? 0 : undefined}
            aria-label={`${tip.main}. ${tip.sub}`}
            onClick={handleTipActivate}
            onKeyDown={(event) => {
              if (!tipClickable) return
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault()
                handleTipActivate()
              }
            }}
          >
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
                  <TipGlyph tip={tip} platforms={platforms} viewer={viewer} />
                  <div className="rsb-lines">
                    <div className="rsb-main">{tip.main}</div>
                    <div className="rsb-sub">{tip.sub}</div>
                  </div>
                </motion.div>
              </AnimatePresence>
            </div>
            {tipClickable && <LuChevronRight className="rsb-go" aria-hidden />}
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
                {showCta ? (
                  <motion.button
                    key="life-cta"
                    type="button"
                    onClick={openLife}
                    className="rsb-btn"
                    initial={{ opacity: 0, scale: 0.9 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.9 }}
                    transition={{ duration: 0.26, ease: SLIDE_EASE }}
                  >
                    {lifeAction}
                  </motion.button>
                ) : showPlay ? (
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
