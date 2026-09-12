import { getPublicConfigDeduped } from '../../utils/requestDedup'

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
