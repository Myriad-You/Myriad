import type { CSSProperties, ReactNode, Ref } from 'react'
import type { RigCharacterHandle } from '../../features/merope/character/RigCharacter'
import type { MoodBand } from '../agent/meropeVitals'
import type { WidgetComponentProps } from '../widgetGridTypes'
import { isAnime25DPlayback } from '@myriad/figure/anime25drig/types'
import { sameLiveFaceRuntime } from '@myriad/figure/rig/manifestValidation'
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { useI18n, withI18nNamespace } from '../../contexts/I18nContext'
import { getSiteFace } from '../../features/merope/api'
import RigCharacter from '../../features/merope/character/RigCharacter'
import { FACE_UPDATED_EVENT } from '../../features/merope/events/updates'
import { useRigMotionLifecycle } from '../../features/merope/motion/useRigMotionLifecycle'
import { agentStatusActivity } from '../../features/merope/persona/activity'
import { PERSONA_DEFAULT_NAME } from '../../features/merope/persona/publicName'
import { FacePresence } from '../../features/merope/presence/FacePresence'
import {
  LIVE_FACE_PLAYBACK_PRIORITY,
  notifyLiveFaceUnmounted,
  useLiveFacePlayback,
} from '../../features/merope/presence/liveFacePlayback'
import { useMeropeWidgetFaceSlot } from '../../features/merope/presence/widgetFaceSlot'
import { useAgentStatusKind } from '../agent-panel/agentStatusStore'
import { moodBand } from '../agent/meropeVitals'
import { MeropeFigureBones } from './MeropeSkeleton'
import {
  nameplateHidden,
  previewMoodBand,
  previewPortraitSrc,
  readyKeyAfterMotionChange,
  shouldShowNameplate,
} from './meropeWidgetPresence'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeletonCover } from './shared/WidgetSkeleton'
import { useFigurePlacement } from './useFigurePlacement'
import { useMeropePersona } from './useMeropePersona'
import './MeropeWidget.css'

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
          <img src={previewPortraitSrc()} alt="" draggable={false} />
        </span>
      </div>
      <Nameplate
        name={PERSONA_DEFAULT_NAME}
        band={previewMoodBand(compact)}
        compact={compact}
      />
    </MeropeWidgetChrome>
  )
}

function LiveMeropeWidget({
  compact,
  ownerId,
  playbackId,
  isEditMode,
  openPopup,
}: {
  compact: boolean
  /** 卡片实例 id：弹窗按它把舞台借给这张卡。 */
  ownerId: string
  playbackId: string
  isEditMode: boolean
  openPopup?: () => void
}) {
  const { t, format } = useI18n()
  const [manifest, setManifest] =
    useState<Awaited<ReturnType<typeof getSiteFace>>['manifest']>(null)
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const { name: agentName, mood, arousal, vitalsReady } = useMeropePersona()
  const status = useAgentStatusKind()
  const activity = agentStatusActivity(status)
  const [loading, setLoading] = useState(true)
  const [failed, setFailed] = useState(false)
  const [rigFailed, setRigFailed] = useState(false)
  const [readyKey, setReadyKey] = useState('')
  const faceRequestRef = useRef(0)
  const surfaceRef = useRef<HTMLDivElement>(null)
  const figureHomeRef = useRef<HTMLDivElement>(null)
  // 她只有一份播放器：开弹窗时整个挪到弹窗舞台上，收回时挪回来，不重建。
  const { container: figureContainer, onStage } = useFigurePlacement(ownerId, figureHomeRef)
  const rigRef = useRef<RigCharacterHandle>(null)
  const manifestRef = useRef(manifest)
  manifestRef.current = manifest
  const [capabilities, setCapabilities] = useState<readonly string[]>([])
  // 在弹窗里时按弹窗的优先级占着：这时打开 Agent 面板也不把她从弹窗里抢走。
  const playsLive = useLiveFacePlayback(
    playbackId,
    true,
    onStage ? LIVE_FACE_PLAYBACK_PRIORITY.popup : LIVE_FACE_PLAYBACK_PRIORITY.widget,
  )
  const playableRig = hasPlayableRig(manifest)
  const motionReady = playsLive && playableRig && !rigFailed
  const packageKey = motionReady
    ? 'live'
    : !playableRig && portraitUrl && !rigFailed
      ? portraitUrl
      : ''
  const wantLive = Boolean(packageKey)
  const ready = readyKey === packageKey && packageKey !== ''
  const handlePlaybackReady = useCallback((nextCapabilities: readonly string[]) => {
    setCapabilities(nextCapabilities)
    setReadyKey(packageKey)
  }, [packageKey])
  const handleRigPlaybackError = useCallback(() => setRigFailed(true), [])
  useEffect(() => {
    if (loading || !wantLive || ready) return
    const timeout = window.setTimeout(setRigFailed, 12_000, true)
    return () => window.clearTimeout(timeout)
  }, [loading, packageKey, ready, wantLive])
  useLayoutEffect(() => {
    setReadyKey((current) => readyKeyAfterMotionChange(motionReady, current))
  }, [motionReady])
  useRigMotionLifecycle(rigRef, {
    mood,
    arousal,
    activity,
    capabilities: motionReady && ready ? capabilities : [],
    ready: motionReady && ready,
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

  const retryFace = useCallback(() => {
    setLoading(true)
    setReadyKey('')
    setRigFailed(false)
    loadFace()
  }, [loadFace])

  useEffect(() => {
    loadFace()
    window.addEventListener(FACE_UPDATED_EVENT, loadFace)
    return () => {
      window.removeEventListener(FACE_UPDATED_EVENT, loadFace)
      faceRequestRef.current += 1
    }
  }, [loadFace])

  const stateClass = `merope-widget__rig merope-widget__rig--${activity}`
  const band = vitalsReady ? moodBand(mood, arousal) : null
  const surfaceStyle = useMemo(
    () => portraitFrameStyle(manifest?.anime25dPlayback?.pixelCanvas),
    [manifest],
  )

  return (
    <MeropeWidgetChrome
      compact={compact}
      label={`${t.widgets.agentPersona}: ${agentName ?? t.merope.title}`}
      surfaceRef={surfaceRef}
    >
      <div ref={figureHomeRef} className="merope-widget__figure-home" />
      {createPortal(
        <div className="merope-widget__figure-frame" style={surfaceStyle}>
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
                  <div className="merope-widget__empty" role="status">
                    <span>
                      {failed || rigFailed
                        ? t.merope.loadFailed
                        : t.merope.assetEmpty}
                    </span>
                    {failed || rigFailed ? (
                      <button type="button" onClick={retryFace}>
                        {t.common.retry}
                      </button>
                    ) : null}
                  </div>
                ) : null
              }
            >
              {(mounted) =>
                mounted ? (
                  <RigCharacter
                    ref={rigRef}
                    touchEnabled={motionReady && ready && !isEditMode && (onStage || !openPopup)}
                    activity={activity}
                    fallbackUrl={playableRig ? null : portraitUrl}
                    manifest={playsLive || mounted ? manifest : null}
                    mood={mood}
                    onPlaybackError={handleRigPlaybackError}
                    onPlaybackReady={handlePlaybackReady}
                  />
                ) : null
              }
            </FacePresence>
          </div>
          <WidgetSkeletonCover
            active={loading || (wantLive && !ready)}
            label={t.common.loading}
          >
            <MeropeFigureBones nameplate={!onStage} compact={compact} />
          </WidgetSkeletonCover>
        </div>,
        figureContainer,
      )}
      {shouldShowNameplate(agentName) ? (
        <Nameplate
          name={agentName ?? ''}
          band={band}
          compact={compact}
          hidden={nameplateHidden(wantLive)}
        />
      ) : null}
      {openPopup && !isEditMode && !failed && !rigFailed ? (
        // 整张卡都能点开人设弹窗；盖在角色上面，卡片上就不再接触摸。
        <button
          type="button"
          className="merope-widget__open"
          aria-haspopup="dialog"
          aria-label={format(t.merope.widgetOpenProfile, {
            name: agentName ?? t.merope.title,
          })}
          onClick={(event) => {
            event.stopPropagation()
            openPopup()
          }}
        />
      ) : null}

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

const MeropeWidgetBody = memo(
  ({ isPreview = false, config, isEditMode, openPopup }: WidgetComponentProps) => {
    const compact = config.size === '2x2'
    const holdsFace = useMeropeWidgetFaceSlot(config.id, !isPreview)
    if (isPreview) return <MeropeWidgetPreview compact={compact} />
    if (!holdsFace) return <MeropeWidgetDuplicate compact={compact} />
    return (
      <LiveMeropeWidget
        compact={compact}
        ownerId={config.id}
        isEditMode={isEditMode}
        playbackId={`widget:${config.id}`}
        openPopup={openPopup}
      />
    )
  },
)

MeropeWidgetBody.displayName = 'MeropeWidgetBody'

export const MeropeWidget = withI18nNamespace(['merope'], MeropeWidgetBody)
