import { getPublicConfigDeduped } from '../../../utils/requestDedup'
import { PERSONA_UPDATED_EVENT } from '../events/updates'

/** Default when persona is on but unnamed; not the product name. */
export const PERSONA_DEFAULT_NAME = 'Arael'
/** Product name when persona is off. Do not translate. */
export const PERSONA_OFF_NAME = 'Agent'

/** Matches backend `public_persona_name`: off → Agent, on and unnamed → Arael. */
export function publicPersonaName(
  enabled: boolean,
  storedName?: string | null,
): string {
  if (!enabled) return PERSONA_OFF_NAME
  return storedName?.trim() || PERSONA_DEFAULT_NAME
}

export function publicPersonaNameFromConfig(
  config:
    | {
        meropeEnabled?: unknown
        agentPersonaName?: unknown
      }
    | null
    | undefined,
): string {
  if (
    typeof config?.agentPersonaName === 'string' &&
    config.agentPersonaName.trim()
  ) {
    return config.agentPersonaName.trim()
  }
  return publicPersonaName(config?.meropeEnabled === true)
}

export async function loadPublicPersonaName(): Promise<string> {
  try {
    return publicPersonaNameFromConfig(await getPublicConfigDeduped())
  } catch {
    return PERSONA_OFF_NAME
  }
}

/** Settings category stays the product word until the public face has a name. */
export function settingsAgentLabel(
  productLabel: string,
  publicName: string,
): string {
  const name = publicName.trim()
  if (!name || name === PERSONA_OFF_NAME) return productLabel
  return name
}

type PersonaNameListener = (name: string) => void

let cachedName = PERSONA_OFF_NAME
let nameLoaded = false
let nameInFlight: Promise<string> | null = null
const nameListeners = new Set<PersonaNameListener>()

function publishPersonaPublicName(next: string) {
  if (cachedName === next && nameLoaded) return
  cachedName = next
  nameLoaded = true
  for (const listener of nameListeners) listener(cachedName)
}

export function personaPublicName(): string {
  if (!nameLoaded && !nameInFlight) void refreshPersonaPublicName()
  return cachedName
}

export async function refreshPersonaPublicName(): Promise<string> {
  if (nameInFlight) return nameInFlight
  nameInFlight = (async () => {
    try {
      const next = publicPersonaNameFromConfig(await getPublicConfigDeduped())
      publishPersonaPublicName(next)
      return next
    } catch {
      // 无法读取不代表人设已关闭，保留上一次成功的名字。
      return cachedName
    }
  })()
  try {
    const next = await nameInFlight
    return next
  } finally {
    nameInFlight = null
  }
}

export function onPersonaPublicName(listener: PersonaNameListener): () => void {
  nameListeners.add(listener)
  return () => {
    nameListeners.delete(listener)
  }
}

if (typeof window !== 'undefined') {
  window.addEventListener(PERSONA_UPDATED_EVENT, () => {
    void refreshPersonaPublicName()
  })
}
