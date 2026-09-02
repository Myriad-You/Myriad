import { getPublicConfigDeduped } from '../../utils/requestDedup'

/** 人设开着但没写名字时的默认名，不是产品名。 */
export const PERSONA_DEFAULT_NAME = 'Arael'
/** 人设关掉时的对外名。产品名，不翻译。 */
export const PERSONA_OFF_NAME = 'Agent'

/** 对齐后端 `public_persona_name`：关 → Agent，开且无名 → Arael。 */
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
