/**
 * 当前说话对象的心情档。人设关掉或未登录时没有档，聊天那枚贴就不画。
 */

import type { MoodBand } from '../agent/meropeVitals'
import { useEffect, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { PERSONA_UPDATED_EVENT } from '../../features/merope/events'
import {
  MEROPE_STATE_EVENT,
  meropeStateEventDetail,
} from '../../features/merope/performanceEvents'
import { agentService } from '../../services/agent'
import { ADDRESSEE_UPDATED_EVENT, moodBand } from '../agent/meropeVitals'

export function useAddresseeMoodBand(): MoodBand | null {
  const { hasChecked, isAuthenticated } = useAuth()
  const [band, setBand] = useState<MoodBand | null>(null)

  useEffect(() => {
    if (!hasChecked || !isAuthenticated) {
      setBand(null)
      return undefined
    }
    let active = true
    const apply = (
      value: number | undefined,
      arousal: number | undefined,
    ) => {
      if (active) setBand(moodBand(value, arousal))
    }
    const load = () => {
      void agentService
        .getPersona()
        .then((persona) => {
          if (!active) return
          if (!persona) {
            setBand(null)
            return
          }
          apply(
            typeof persona.mood === 'number' ? persona.mood : 70,
            typeof persona.arousal === 'number' ? persona.arousal : 48,
          )
        })
        .catch(() => {
          if (active) setBand(null)
        })
    }
    load()
    const onState = (event: Event) => {
      const detail = meropeStateEventDetail(
        (event as CustomEvent<unknown>).detail,
      )
      if (detail) apply(detail.mood.after, detail.mood.arousalAfter)
    }
    window.addEventListener(MEROPE_STATE_EVENT, onState)
    window.addEventListener(PERSONA_UPDATED_EVENT, load)
    window.addEventListener(ADDRESSEE_UPDATED_EVENT, load)
    return () => {
      active = false
      window.removeEventListener(MEROPE_STATE_EVENT, onState)
      window.removeEventListener(PERSONA_UPDATED_EVENT, load)
      window.removeEventListener(ADDRESSEE_UPDATED_EVENT, load)
    }
  }, [hasChecked, isAuthenticated])

  return band
}
