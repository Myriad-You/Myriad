import type { StructuredPersona } from '../../../components/agent/onboarding/onboardingTypes'
import type { AgentPersona } from '../../../services/agent/agentApi'
import type { MeropeActivity } from '../types'
import {
  parseFlattenedPersona,
  personaFromApi,
} from '../../../components/agent/onboarding/onboardingTypes'
import { showStickyToast } from '../../../utils/toastManager'

export function reportMeropeError(message: string) {
  if (!message.trim()) return
  showStickyToast({
    message,
    type: 'error',
    replaceKey: 'merope-workbench',
  })
}

export function toMeropeActivity(raw: string): MeropeActivity {
  if (raw === 'talking' || raw === 'thinking') return raw
  return 'idle'
}

export function structuredFromSnapshot(
  persona: AgentPersona | null,
): StructuredPersona | null {
  if (!persona) return null
  if (persona.persona) {
    const parsed = personaFromApi(persona.persona)
    if (
      parsed.summary ||
      parsed.temperament.length ||
      parsed.likes.length ||
      parsed.drives.length ||
      parsed.socialStyle ||
      parsed.speechStyle
    ) {
      return parsed
    }
  }
  if (persona.personality?.trim()) {
    return parseFlattenedPersona(persona.personality)
  }
  return null
}

/** Save a picture as a file, or open it when the browser refuses the download. */
export async function downloadPicture(source: string): Promise<void> {
  try {
    const response = await fetch(source)
    if (!response.ok) throw new Error(`HTTP ${response.status}`)
    const blob = await response.blob()
    const objectUrl = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = objectUrl
    link.download = 'portrait.png'
    document.body.appendChild(link)
    link.click()
    link.remove()
    window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1_000)
  } catch {
    window.open(source, '_blank', 'noopener,noreferrer')
  }
}
