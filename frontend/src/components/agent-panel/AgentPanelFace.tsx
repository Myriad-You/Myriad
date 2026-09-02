/**
 * 聊天档：输入框上面的人设形象。
 *
 * 和首页小组件同一份主立绘 / Rig。放大 2.5，裁掉底下 1/5，底边模糊收。
 * 不套小组件外壳，也不是玻璃。
 * 这块不是玻璃 —— 输入框已经是 .glass，再套一层会糊成乳白带。
 */

import type { CSSProperties } from 'react'
import type { RigCharacterHandle } from '../../features/merope/rig/RigCharacter'
import type { MeropeRigManifest } from '../../features/merope/rig/types'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { agentStatusActivity } from '../../features/merope/activity'
import { isAnime25DPlayback } from '../../features/merope/anime25drig/types'
import { getSiteFace } from '../../features/merope/api'
import {
  FACE_UPDATED_EVENT,
  PERSONA_UPDATED_EVENT,
} from '../../features/merope/events'
import { semanticRigCapabilities } from '../../features/merope/motion/rigStateSummary'
import { useRigMotionLifecycle } from '../../features/merope/motion/useRigMotionLifecycle'
import {
  MEROPE_STATE_EVENT,
  meropeStateEventDetail,
} from '../../features/merope/performanceEvents'
import {
  loadPublicPersonaName,
  PERSONA_DEFAULT_NAME,
  PERSONA_OFF_NAME,
  publicPersonaName,
} from '../../features/merope/publicName'
import RigCharacter from '../../features/merope/rig/RigCharacter'
import { agentService } from '../../services/agent'
import { ADDRESSEE_UPDATED_EVENT } from '../agent/meropeVitals'
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

export function AgentPanelFace() {
  const { t } = useI18n()
  const { hasChecked, isAuthenticated } = useAuth()
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
  const faceRequestRef = useRef(0)
  const rigRef = useRef<RigCharacterHandle>(null)
  const capabilities = useMemo(
    () => semanticRigCapabilities(manifest),
    [manifest],
  )
  const playableRig = hasPlayableRig(manifest)
  const motionReady = playableRig && !rigFailed
  const liveCapabilities = motionReady ? capabilities : []
  const showCharacter = motionReady || Boolean(portraitUrl)
  const handleRigPlaybackError = useCallback(() => setRigFailed(true), [])
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
    setLoading(true)
    setFailed(false)
    setRigFailed(false)
    void getSiteFace()
      .then((face) => {
        if (request !== faceRequestRef.current) return
        setManifest(face.manifest)
        setPortraitUrl(face.portraitUrl)
      })
      .catch(() => {
        if (request !== faceRequestRef.current) return
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
          setMood(
            typeof persona.mood === 'number' ? persona.mood : DEFAULT_MOOD,
          )
          setArousal(
            typeof persona.arousal === 'number'
              ? persona.arousal
              : DEFAULT_AROUSAL,
          )
        })
        .catch(() => {
          if (active) setFailed(true)
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
  }, [hasChecked, isAuthenticated])

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
      {showCharacter ? (
        <div className="agent-panel-face-rig">
          <RigCharacter
            ref={rigRef}
            activity={activity}
            fallbackUrl={portraitUrl}
            manifest={manifest}
            mood={mood}
            onPlaybackError={handleRigPlaybackError}
          />
        </div>
      ) : !loading ? (
        <span className="agent-panel-tag" data-block="true" role="status">
          <span className="agent-panel-tag-text">{emptyMessage}</span>
        </span>
      ) : null}
    </div>
  )
}
