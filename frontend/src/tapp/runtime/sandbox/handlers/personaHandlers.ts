/**
 * Read-only Agent 人设 card for TAPPs.
 *
 * Public name and enabled flag come from site config. Mood is the addressee
 * band when the caller is logged in; guests get the baseline. Portrait is a
 * host-origin path for `<img src>` — bytes never cross the bridge.
 */

import type { TappBridge } from '../../TappBridge'
import { getSiteFace } from '../../../../features/merope/api'
import { publicPersonaNameFromConfig } from '../../../../features/merope/publicName'
import { agentService } from '../../../../services/agent'
import { getPublicConfigDeduped } from '../../../../utils/requestDedup'
import { userFacingError } from '../../../../utils/userFacingError'
import { projectPersonaCard, sameOriginPortraitUrl } from '../personaCard'

async function loadPortraitUrl(): Promise<string | null> {
  try {
    const face = await getSiteFace()
    return sameOriginPortraitUrl(face.portraitUrl)
  } catch {
    return null
  }
}

async function loadVitals(): Promise<{
  mood?: number
  arousal?: number
  activity?: string
}> {
  try {
    const persona = await agentService.getPersona()
    if (!persona) return {}
    return {
      mood: persona.mood,
      arousal: persona.arousal,
      activity: persona.activity,
    }
  } catch {
    // Guests have no addressee state; merope-off returns null above.
    return {}
  }
}

export function registerPersonaHandlers(bridge: TappBridge): void {
  bridge.registerHandler('persona.get', async () => {
    try {
      const config = await getPublicConfigDeduped()
      const [portraitUrl, vitals] = await Promise.all([
        loadPortraitUrl(),
        loadVitals(),
      ])
      return {
        success: true,
        data: projectPersonaCard({
          enabled: config?.meropeEnabled === true,
          name: publicPersonaNameFromConfig(config),
          mood: vitals.mood,
          arousal: vitals.arousal,
          activity: vitals.activity,
          portraitUrl,
        }),
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })
}
