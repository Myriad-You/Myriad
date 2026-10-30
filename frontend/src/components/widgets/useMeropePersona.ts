import {
  MEROPE_STATE_EVENT,
  meropeStateEventDetail,
  resolveLoadedMeropeAffect,
} from '@myriad/figure/events/performanceEvents'
import { useEffect, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { PERSONA_UPDATED_EVENT } from '../../features/merope/events/updates'
import {
  loadPublicPersonaName,
  PERSONA_OFF_NAME,
  publicPersonaName,
} from '../../features/merope/persona/publicName'
import { agentService } from '../../services/agent'
import { ADDRESSEE_UPDATED_EVENT } from '../agent/meropeVitals'

const DEFAULT_MOOD = 70
const DEFAULT_AROUSAL = 48

export interface MeropePersonaView {
  /** null 是还没读到。 */
  name: string | null
  mood: number
  arousal: number
  /** 心情是她对这个人的；访客、读失败时没有，不显示。 */
  vitalsReady: boolean
}

/**
 * 人设卡片和它的弹窗共用的名字和心情：访客只有公开名字，登录的人多一份她对自己的心情。
 * 人设改了、换了称呼对象、心情变了都跟着更新。
 */
export function useMeropePersona(): MeropePersonaView {
  const { hasChecked, isAuthenticated, user } = useAuth()
  const [name, setName] = useState<string | null>(null)
  const [mood, setMood] = useState(DEFAULT_MOOD)
  const [arousal, setArousal] = useState(DEFAULT_AROUSAL)
  const [vitalsReady, setVitalsReady] = useState(false)

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
      void loadPublicPersonaName().then((next) => {
        if (active) setName(next)
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
        .then((loaded) => {
          if (!active) return
          if (!loaded) {
            setName(PERSONA_OFF_NAME)
            setVitalsReady(false)
            return
          }
          setName(publicPersonaName(true, loaded.name))
          const affect = resolveLoadedMeropeAffect(loaded)
          setMood(affect.mood)
          setArousal(affect.arousal)
          setVitalsReady(true)
        })
        .catch(() => {
          if (!active) return
          setVitalsReady(false)
          void loadPublicPersonaName().then((next) => {
            if (active) setName(next)
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

  return { name, mood, arousal, vitalsReady }
}
