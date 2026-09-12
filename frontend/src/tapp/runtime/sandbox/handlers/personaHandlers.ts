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
