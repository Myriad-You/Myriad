import type { RigCharacterHandle } from './rig/RigCharacter'
import type { CompanionRigManifest } from './rig/types'
import type { CompanionActivity } from './types'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router-dom'
import { ADDRESSEE_UPDATED_EVENT } from '../../components/agent/lifeVitals'
import { useI18n } from '../../contexts/I18nContext'
import { isExlight, useAnimationLevel } from '../../hooks/useAnimationLevel'
import { agentService } from '../../services/agent'
import { getPublicConfigDeduped } from '../../utils/requestDedup'
import { getSiteFace } from './api'
import { FACE_UPDATED_EVENT } from './events'
import {
  COMPANION_LIFE_STATE_EVENT,
  COMPANION_PERFORMANCE_EVENT,
  companionLifeStateEventDetail,
  companionPerformanceEventDetail,
} from './performanceEvents'
import {
  estimateSpeechDurationMs,
  planVisemes,
  visemeAmount,
} from './rig/articulation'
import RigCharacter from './rig/RigCharacter'
import './companion.css'

function toCompanionActivity(raw: string | undefined): CompanionActivity {
  if (raw === 'talking' || raw === 'thinking') return raw
  return 'idle'
}

export default function AraelFaceOverlay() {
  const { t } = useI18n()
  const { pathname } = useLocation()
  const still = isExlight(useAnimationLevel())
  const settingsOpen = pathname === '/config' || pathname.startsWith('/config/')
  const [visible, setVisible] = useState(false)
  const [manifest, setManifest] = useState<CompanionRigManifest | null>(null)
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [mood, setMood] = useState(70)
  const [activity, setActivity] = useState<CompanionActivity>('idle')
  const rigCharacterRef = useRef<RigCharacterHandle>(null)
  const latestMoodRevisionRef = useRef(-1)
  const lastDirectiveKeyRef = useRef('')
  const lastSpeechKeyRef = useRef('')
  const speechTimersRef = useRef<number[]>([])

  const stopSpeech = useCallback(() => {
    for (const timer of speechTimersRef.current) window.clearTimeout(timer)
    speechTimersRef.current = []
    rigCharacterRef.current?.setSpeechArticulation({
      energy: 0,
      viseme: 'rest',
      amount: 0,
    })
  }, [])

  const playSpeech = useCallback((text: string) => {
    stopSpeech()
    const spoken = Array.from(text).slice(0, 220).join('')
    if (!spoken) return
    const duration = Math.min(12_000, estimateSpeechDurationMs(spoken))
    const cues = planVisemes(spoken, duration)
    for (const cue of cues) {
      const timer = window.setTimeout(() => {
        const energy = cue.viseme === 'rest' ? 0 : 0.72
        rigCharacterRef.current?.setSpeechArticulation({
          energy,
          viseme: cue.viseme,
          amount: visemeAmount(cue.viseme, energy),
        })
      }, cue.atMs)
      speechTimersRef.current.push(timer)
    }
    speechTimersRef.current.push(
      window.setTimeout(() => {
        rigCharacterRef.current?.setSpeechArticulation({
          energy: 0,
          viseme: 'rest',
          amount: 0,
        })
      }, duration),
    )
  }, [stopSpeech])

  const refresh = useCallback(async () => {
    const publicConfig = await getPublicConfigDeduped()
    if (!publicConfig?.agentLifeEnabled) {
      setVisible(false)
      return
    }
    const [face, persona] = await Promise.all([
      getSiteFace(),
      agentService.getPersona().catch(() => null),
    ])
    if (!face.manifest && !face.portraitUrl) {
      setVisible(false)
      return
    }
    setManifest(face.manifest)
    setPortraitUrl(face.portraitUrl)
    setMood(typeof persona?.mood === 'number' ? persona.mood : 70)
    setActivity(toCompanionActivity(persona?.activity))
    setVisible(true)
  }, [])

  useEffect(() => {
    let cancelled = false
    void refresh().catch(() => {
      if (!cancelled) setVisible(false)
    })
    const onChange = () => {
      rigCharacterRef.current?.stopMotionPlan()
      stopSpeech()
      latestMoodRevisionRef.current = -1
      lastDirectiveKeyRef.current = ''
      lastSpeechKeyRef.current = ''
      void refresh().catch(() => {
        if (!cancelled) setVisible(false)
      })
    }
    window.addEventListener(ADDRESSEE_UPDATED_EVENT, onChange)
    window.addEventListener(FACE_UPDATED_EVENT, onChange)
    window.addEventListener('arael-persona-updated', onChange)
    return () => {
      cancelled = true
      window.removeEventListener(ADDRESSEE_UPDATED_EVENT, onChange)
      window.removeEventListener(FACE_UPDATED_EVENT, onChange)
      window.removeEventListener('arael-persona-updated', onChange)
    }
  }, [refresh, stopSpeech])

  useEffect(() => {
    const onPerformance = (event: Event) => {
      const detail = companionPerformanceEventDetail(
        (event as CustomEvent<unknown>).detail,
      )
      if (!detail) return
      if (detail.performance) {
        const directiveKey = `${detail.messageId || ''}:${detail.performance.phase}:${detail.performance.moodRevision}`
        if (directiveKey !== lastDirectiveKeyRef.current) {
          lastDirectiveKeyRef.current = directiveKey
          rigCharacterRef.current?.playMotionPlan(detail.performance)
        }
      }
      if (detail.text) {
        const speechKey = `${detail.messageId || ''}:${detail.text}`
        if (speechKey !== lastSpeechKeyRef.current) {
          lastSpeechKeyRef.current = speechKey
          playSpeech(detail.text)
        }
      }
    }
    const onLifeState = (event: Event) => {
      const detail = companionLifeStateEventDetail(
        (event as CustomEvent<unknown>).detail,
      )
      if (!detail || detail.mood.revision < latestMoodRevisionRef.current) return
      if (detail.mood.revision > latestMoodRevisionRef.current) {
        rigCharacterRef.current?.stopMotionPlan()
        latestMoodRevisionRef.current = detail.mood.revision
      }
      setMood(detail.mood.after)
      setActivity(toCompanionActivity(detail.activity))
    }
    window.addEventListener(COMPANION_PERFORMANCE_EVENT, onPerformance)
    window.addEventListener(COMPANION_LIFE_STATE_EVENT, onLifeState)
    return () => {
      window.removeEventListener(COMPANION_PERFORMANCE_EVENT, onPerformance)
      window.removeEventListener(COMPANION_LIFE_STATE_EVENT, onLifeState)
      stopSpeech()
    }
  }, [playSpeech, stopSpeech])

  if (!visible || !portraitUrl || settingsOpen) return null

  return (
    <aside className="dlc-stage" aria-label={t.companion.faceStage}>
      <div className="dlc-character">
        <RigCharacter
          ref={rigCharacterRef}
          activity={activity}
          fallbackUrl={portraitUrl}
          manifest={still ? null : manifest}
          mood={mood}
        />
      </div>
    </aside>
  )
}
