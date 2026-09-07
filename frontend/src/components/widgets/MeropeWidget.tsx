import type { CSSProperties, ReactNode, Ref } from 'react'
import type { RigCharacterHandle } from '../../features/merope/rig/RigCharacter'
import type { MoodBand } from '../agent/meropeVitals'
import type { WidgetComponentProps } from '../widgetGridTypes'
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { agentStatusActivity } from '../../features/merope/activity'
import { isAnime25DPlayback } from '../../features/merope/anime25drig/types'
import { getSiteFace } from '../../features/merope/api'
import {
  FACE_UPDATED_EVENT,
  PERSONA_UPDATED_EVENT,
} from '../../features/merope/events'
import { FacePresence } from '../../features/merope/FacePresence'
import {
  LIVE_FACE_PLAYBACK_PRIORITY,
  notifyLiveFaceUnmounted,
  useLiveFacePlayback,
} from '../../features/merope/liveFacePlayback'
import { semanticRigCapabilities } from '../../features/merope/motion/rigStateSummary'
import { useRigMotionLifecycle } from '../../features/merope/motion/useRigMotionLifecycle'
import {
  MEROPE_STATE_EVENT,
  meropeStateEventDetail,
  resolveLoadedMeropeAffect,
} from '../../features/merope/performanceEvents'
import {
  loadPublicPersonaName,
  PERSONA_DEFAULT_NAME,
  PERSONA_OFF_NAME,
  publicPersonaName,
} from '../../features/merope/publicName'
import RigCharacter from '../../features/merope/rig/RigCharacter'
import { sameLiveFaceRuntime } from '../../features/merope/rig/types'
import { useMeropeWidgetFaceSlot } from '../../features/merope/widgetFaceSlot'
import { agentService } from '../../services/agent'
import { useAgentStatus } from '../agent-panel/agentStatusStore'
import { ADDRESSEE_UPDATED_EVENT, moodBand } from '../agent/meropeVitals'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeletonCover } from './shared/WidgetSkeleton'
import './MeropeWidget.css'

const DEFAULT_MOOD = 70
const DEFAULT_AROUSAL = 48
/** 立绘生成时附上的技法参考图；小组件库预览用同一张。 */
const STYLE_REFERENCE_PREVIEW = '/merope/style-reference.png'
const PREVIEW_MOOD_BAND: MoodBand = 'calm'

/** 四格：很低 1，偏低/烦躁 2，平常 3，轻松 4 */
const MOOD_LEVEL: Record<MoodBand, number> = {
  floor: 1,
  sad: 2,
  tense: 2,
  calm: 3,
  excited: 4,
}

const LEVEL_SLOTS = [0, 1, 2, 3]

function Nameplate({
  name,
  band,
  compact,
  hidden,
}: {
  name: string
  band: MoodBand | null
  compact?: boolean
  hidden?: boolean
}) {
  const { t, format } = useI18n()
  const o = t.agentPersona.onboarding
  const word = band ? o.mood[band] : ''
  return (
    <div
      className="merope-widget__identity glass-surface"
      aria-hidden={hidden || undefined}
    >
      <strong>{name}</strong>
      {!compact && band ? (
        <div
          className="merope-widget__mood"
          aria-label={format(o.moodLine, { band: word })}
        >
          <span className="merope-widget__mood-text">{word}</span>
          <span className="merope-widget__mood-level" aria-hidden>
            {LEVEL_SLOTS.map((slot) => (
              <i key={slot} data-on={slot < MOOD_LEVEL[band] || undefined} />
            ))}
          </span>
        </div>
      ) : null}
    </div>
  )
}

function MeropeWidgetChrome({
  compact,
  surfaceStyle,
  label,
  children,
  surfaceRef,
}: {
  compact?: boolean
  surfaceStyle?: CSSProperties
  label?: string
  children: ReactNode
  surfaceRef?: Ref<HTMLDivElement>
}) {
  return (
    <WidgetShell
      padding={0}
      className={`merope-widget${compact ? ' merope-widget--compact' : ''}`}
    >
      <div
        ref={surfaceRef}
        className="merope-widget__surface"
        style={surfaceStyle}
        role={label ? 'group' : undefined}
        aria-label={label}
      >
        {children}
      </div>
    </WidgetShell>
  )
}

function hasPlayableRig(
  manifest: Awaited<ReturnType<typeof getSiteFace>>['manifest'],
): boolean {
  return Boolean(
    manifest &&
    isAnime25DPlayback(manifest.anime25dPlayback) &&
    manifest.textures[0]?.url,
  )
}

/**
 * 取景框与人物画布同比 → 播放器的等比缩放由宽度决定，人物横向铺满卡片，
 * 纵向溢出的身体被卡片裁掉。没有 rig 时交给 CSS 回落到 master portrait 的 3:4。
 */
function portraitFrameStyle(
  canvas: { width: number; height: number } | undefined,
): CSSProperties | undefined {
  if (!canvas || canvas.width <= 0 || canvas.height <= 0) return undefined
  return {
    '--merope-widget-portrait': `${canvas.width} / ${canvas.height}`,
  } as CSSProperties
}

function MeropeWidgetPreview({ compact }: { compact?: boolean }) {
  return (
    <MeropeWidgetChrome compact={compact}>
      <div
        className="merope-widget__rig merope-widget__rig--idle"
        data-rig-quality="static"
      >
        <span className="merope-rig is-ready" data-rig-quality="static">
          <img src={STYLE_REFERENCE_PREVIEW} alt="" draggable={false} />
        </span>
      </div>
      <Nameplate
        name={PERSONA_DEFAULT_NAME}
        band={compact ? null : PREVIEW_MOOD_BAND}
        compact={compact}
      />
    </MeropeWidgetChrome>
  )
}

function LiveMeropeWidget({
  compact,
  playbackId,
}: {
  compact: boolean
  playbackId: string
}) {
  const { t } = useI18n()
  const { hasChecked, isAuthenticated, user } = useAuth()
  const [manifest, setManifest] =
    useState<Awaited<ReturnType<typeof getSiteFace>>['manifest']>(null)
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [agentName, setAgentName] = useState<string | null>(null)
  const [mood, setMood] = useState(DEFAULT_MOOD)
  const [arousal, setArousal] = useState(DEFAULT_AROUSAL)
  const { status } = useAgentStatus()
  const activity = agentStatusActivity(status)
  const [loading, setLoading] = useState(true)
  const [failed, setFailed] = useState(false)
  const [rigFailed, setRigFailed] = useState(false)
  const [readyKey, setReadyKey] = useState('')
  const [vitalsReady, setVitalsReady] = useState(false)
  const faceRequestRef = useRef(0)
  const surfaceRef = useRef<HTMLDivElement>(null)
  const rigRef = useRef<RigCharacterHandle>(null)
  const manifestRef = useRef(manifest)
  manifestRef.current = manifest
  const capabilities = useMemo(
    () => semanticRigCapabilities(manifest),
    [manifest],
  )
  const playsLive = useLiveFacePlayback(
    playbackId,
    true,
    LIVE_FACE_PLAYBACK_PRIORITY.widget,
  )
  const playableRig = hasPlayableRig(manifest)
  const motionReady = playsLive && playableRig && !rigFailed
  const liveCapabilities = motionReady ? capabilities : []
  const packageKey = motionReady
    ? 'live'
    : !playableRig && portraitUrl
      ? portraitUrl
      : ''
  const wantLive = Boolean(packageKey)
  const ready = readyKey === packageKey && packageKey !== ''
  const handleRigPlaybackError = useCallback(() => setRigFailed(true), [])
  useLayoutEffect(() => {
    if (!motionReady) setReadyKey('')
  }, [motionReady])
  useRigMotionLifecycle(rigRef, {
    mood,
    arousal,
    activity,
    capabilities: liveCapabilities,
    ready: motionReady,
    priority: 1,
  })

  const loadFace = useCallback(() => {
    const request = ++faceRequestRef.current
    setFailed(false)
    void getSiteFace()
      .then((face) => {
        if (request !== faceRequestRef.current) return
        setManifest((current) => {
          if (sameLiveFaceRuntime(current, face.manifest)) return current
          return face.manifest
        })
        setPortraitUrl(face.portraitUrl)
        if (!sameLiveFaceRuntime(manifestRef.current, face.manifest)) {
          setRigFailed(false)
        }
      })
      .catch(() => {
        if (request !== faceRequestRef.current) return
        if (manifestRef.current) return
        setManifest(null)
        setPortraitUrl(null)
        setFailed(true)
      })
      .finally(() => {
        if (request === faceRequestRef.current) setLoading(false)
      })
  }, [])

  useEffect(() => {
    loadFace()
    window.addEventListener(FACE_UPDATED_EVENT, loadFace)
    return () => {
      window.removeEventListener(FACE_UPDATED_EVENT, loadFace)
      faceRequestRef.current += 1
    }
  }, [loadFace])

  useEffect(() => {
    const onState = (event: Event) => {
      const detail = meropeStateEventDetail(
        (event as CustomEvent<unknown>).detail,
      )
      if (!detail) return
      setMood(detail.mood.after)
      setArousal((current) =>
        typeof detail.mood.arousalAfter === 'number'
          ? detail.mood.arousalAfter
          : current,
      )
    }
    window.addEventListener(MEROPE_STATE_EVENT, onState)
    return () => window.removeEventListener(MEROPE_STATE_EVENT, onState)
  }, [])

  useEffect(() => {
    if (!hasChecked) return undefined
    let active = true
    setMood(DEFAULT_MOOD)
    setArousal(DEFAULT_AROUSAL)
    const applyPublicName = () => {
      void loadPublicPersonaName().then((name) => {
        if (active) setAgentName(name)
      })
      setVitalsReady(false)
    }
    if (!isAuthenticated) {
      applyPublicName()
      window.addEventListener(PERSONA_UPDATED_EVENT, applyPublicName)
      return () => {
        active = false
        window.removeEventListener(PERSONA_UPDATED_EVENT, applyPublicName)
      }
    }
    const loadPersona = () => {
      void agentService
        .getPersona()
        .then((persona) => {
          if (!active) return
          if (!persona) {
            setAgentName(PERSONA_OFF_NAME)
            setVitalsReady(false)
            return
          }
          setAgentName(publicPersonaName(true, persona.name))
          const affect = resolveLoadedMeropeAffect(persona)
          setMood(affect.mood)
          setArousal(affect.arousal)
          setVitalsReady(true)
        })
        .catch(() => {
          if (!active) return
          setVitalsReady(false)
          void loadPublicPersonaName().then((name) => {
            if (active) setAgentName(name)
          })
        })
    }
    loadPersona()
    window.addEventListener(PERSONA_UPDATED_EVENT, loadPersona)
    window.addEventListener(ADDRESSEE_UPDATED_EVENT, loadPersona)
    return () => {
      active = false
      window.removeEventListener(PERSONA_UPDATED_EVENT, loadPersona)
      window.removeEventListener(ADDRESSEE_UPDATED_EVENT, loadPersona)
    }
  }, [hasChecked, isAuthenticated, user?.id])

  const stateClass = `merope-widget__rig merope-widget__rig--${activity}`
  const band = vitalsReady ? moodBand(mood, arousal) : null
  const surfaceStyle = useMemo(
    () => portraitFrameStyle(manifest?.anime25dPlayback?.pixelCanvas),
    [manifest],
  )

  return (
    <MeropeWidgetChrome
      compact={compact}
      surfaceStyle={surfaceStyle}
      label={`${t.widgets.agentPersona}: ${agentName ?? t.merope.title}`}
      surfaceRef={surfaceRef}
    >
      <div
        className={stateClass}
        data-rig-quality={playableRig && !rigFailed ? 'live' : 'static'}
      >
        <FacePresence
          present={wantLive}
          packageKey={packageKey}
          ready={ready}
          hostRef={surfaceRef}
          onLiveUnmounted={() => notifyLiveFaceUnmounted(playbackId)}
          vacant={
            !loading && (failed || rigFailed || !playableRig) ? (
              <p className="merope-widget__empty" role="status">
                {failed || rigFailed
                  ? t.merope.loadFailed
                  : t.merope.assetEmpty}
              </p>
            ) : null
          }
        >
          {(mounted) =>
            mounted ? (
              <RigCharacter
                ref={rigRef}
                activity={activity}
                fallbackUrl={playableRig ? null : portraitUrl}
                manifest={playsLive || mounted ? manifest : null}
                mood={mood}
                onPlaybackError={handleRigPlaybackError}
                onPlaybackReady={() => setReadyKey(packageKey)}
              />
            ) : null
          }
        </FacePresence>
      </div>
      {agentName ? (
        <Nameplate
          name={agentName}
          band={band}
          compact={compact}
          hidden={!wantLive}
        />
      ) : null}

      <WidgetSkeletonCover
        active={loading}
        preset="hero"
        label={t.common.loading}
        accent="var(--color-primary)"
      />
    </MeropeWidgetChrome>
  )
}

function MeropeWidgetDuplicate({ compact }: { compact: boolean }) {
  const { t } = useI18n()
  return (
    <MeropeWidgetChrome compact={compact} label={t.widgets.agentPersona}>
      <p className="merope-widget__empty" role="status">
        {t.merope.widgetFaceSlotTaken}
      </p>
    </MeropeWidgetChrome>
  )
}

export const MeropeWidget = memo(
  ({ isPreview = false, config }: WidgetComponentProps) => {
    const compact = config.size === '2x2'
    const holdsFace = useMeropeWidgetFaceSlot(config.id, !isPreview)
    if (isPreview) return <MeropeWidgetPreview compact={compact} />
    if (!holdsFace) return <MeropeWidgetDuplicate compact={compact} />
    return (
      <LiveMeropeWidget
        compact={compact}
        playbackId={`widget:${config.id}`}
      />
    )
  },
)
