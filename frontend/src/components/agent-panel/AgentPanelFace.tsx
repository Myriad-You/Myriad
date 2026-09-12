import type { CSSProperties } from 'react'
import type { RigCharacterHandle } from '../../features/merope/rig/RigCharacter'
import type { MeropeRigManifest } from '../../features/merope/rig/types'
import {
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
import { getSiteFace, getWardrobeFace } from '../../features/merope/api'
import { useChatOutfitOverlay } from '../../features/merope/chatOutfitOverlay'
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
import { agentService } from '../../services/agent'
import { ADDRESSEE_UPDATED_EVENT } from '../agent/meropeVitals'
import { useAgentPanelMode } from './agentPanelMode'
import { useAgentStatus } from './agentStatusStore'

const DEFAULT_MOOD = 70
const DEFAULT_AROUSAL = 48

function hasPlayableRig(manifest: MeropeRigManifest | null): boolean {
  return Boolean(
    manifest &&
    isAnime25DPlayback(manifest.anime25dPlayback) &&
    manifest.textures[0]?.url,
  )
}

function portraitStyle(
  canvas: { width: number; height: number } | undefined,
): CSSProperties | undefined {
  if (!canvas || canvas.width <= 0 || canvas.height <= 0) return undefined
  return {
    '--agent-face-portrait': `${canvas.width} / ${canvas.height}`,
  } as CSSProperties
}

export function AgentPanelFace({
  playbackEnabled = true,
}: {
  playbackEnabled?: boolean
}) {
  const { t } = useI18n()
  const { hasChecked, isAuthenticated, user } = useAuth()
  const panelMode = useAgentPanelMode()
  const overlayOutfitId = useChatOutfitOverlay()
  const liveOverlayId = panelMode === 'chat' ? overlayOutfitId : null
  const [manifest, setManifest] = useState<MeropeRigManifest | null>(null)
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [agentName, setAgentName] = useState(PERSONA_DEFAULT_NAME)
  const [mood, setMood] = useState(DEFAULT_MOOD)
  const [arousal, setArousal] = useState(DEFAULT_AROUSAL)
  const { status } = useAgentStatus()
  const activity = agentStatusActivity(status)
  const [personaOn, setPersonaOn] = useState(true)
  const [loading, setLoading] = useState(true)
  const [failed, setFailed] = useState(false)
  const [rigFailed, setRigFailed] = useState(false)
  const [readyKey, setReadyKey] = useState('')
  const faceRequestRef = useRef(0)
  const rigRef = useRef<RigCharacterHandle>(null)
  const manifestRef = useRef(manifest)
  manifestRef.current = manifest
  const capabilities = useMemo(
    () => semanticRigCapabilities(manifest),
    [manifest],
  )
  const playsLive = useLiveFacePlayback(
    'agent-panel-face',
    playbackEnabled,
    LIVE_FACE_PLAYBACK_PRIORITY.panel,
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
    priority: 2,
  })

  const loadFace = useCallback(() => {
    const request = ++faceRequestRef.current
    setFailed(false)
    const overlayId = liveOverlayId
    void (overlayId ? getWardrobeFace(overlayId) : getSiteFace())
      .then((face) => {
        if (
          overlayId &&
          !face.manifest &&
          !face.portraitUrl
        ) {
          return getSiteFace()
        }
        return face
      })
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
  }, [liveOverlayId])

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
      setPersonaOn(true)
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
            setPersonaOn(false)
            setAgentName(PERSONA_OFF_NAME)
            return
          }
          setPersonaOn(true)
          setAgentName(publicPersonaName(true, persona.name))
          const affect = resolveLoadedMeropeAffect(persona)
          setMood(affect.mood)
          setArousal(affect.arousal)
        })
        .catch(() => {
          if (!active) return
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

  const emptyMessage =
    failed || rigFailed
      ? t.merope.loadFailed
      : !personaOn
        ? t.agentPanel.agentPersonaOff
        : t.merope.assetEmpty
  const surfaceStyle = useMemo(
    () => portraitStyle(manifest?.anime25dPlayback?.pixelCanvas),
    [manifest],
  )

  return (
    <div
      className="agent-panel-face"
      style={surfaceStyle}
      data-activity={activity}
      aria-label={`${t.merope.title}: ${agentName}`}
      aria-busy={loading || undefined}
    >
      <div className="agent-panel-face-rig">
        <FacePresence
          present={wantLive}
          packageKey={packageKey}
          ready={ready}
          onLiveUnmounted={() => notifyLiveFaceUnmounted('agent-panel-face')}
          vacant={
            !loading && playbackEnabled ? (
              <span className="agent-panel-tag" data-block="true" role="status">
                <span className="agent-panel-tag-text">{emptyMessage}</span>
              </span>
            ) : null
          }
        >
          {(mounted) =>
            mounted ? (
              <RigCharacter
                ref={rigRef}
                touchEnabled={motionReady && ready && personaOn}
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
    </div>
  )
}
