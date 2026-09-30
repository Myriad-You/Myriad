import { getPublicConfigDeduped } from '../../../utils/requestDedup'
import { siteMediaUrl } from '../../../utils/siteMediaUrl'
import { PERSONA_UPDATED_EVENT } from '../events/updates'

/** No sticker → site logo. Shared by settings and notifications. */
export const PERSONA_STICKER_FALLBACK = '/logo.webp'

type Listener = (url: string | null) => void

let cached: string | null = null
let loaded = false
let inFlight: Promise<string | null> | null = null
const listeners = new Set<Listener>()

/** 从公开配置里读贴纸地址。后端把字段名换了、或者给了空串，都必须落成 `null` 而不是一个假地址——通知图标拿到假地址会画成裂图。 */
export function personaStickerAvatarFromConfig(config: unknown): string | null {
  if (!config || typeof config !== 'object') return null
  const raw = (config as { agentPersonaAvatarUrl?: unknown })
    .agentPersonaAvatarUrl
  if (typeof raw !== 'string') return null
  return siteMediaUrl(raw) || null
}

function publish(next: string | null) {
  if (cached === next && loaded) return
  cached = next
  loaded = true
  for (const listener of listeners) listener(cached)
}

export function personaStickerAvatarUrl(): string | null {
  if (!loaded && !inFlight) void refreshPersonaStickerAvatar()
  return cached
}

export async function refreshPersonaStickerAvatar(): Promise<string | null> {
  if (inFlight) return inFlight
  inFlight = (async () => {
    try {
      return personaStickerAvatarFromConfig(await getPublicConfigDeduped())
    } catch {
      return null
    }
  })()
  try {
    const next = await inFlight
    publish(next)
    return next
  } finally {
    inFlight = null
  }
}

export function onPersonaStickerAvatar(listener: Listener): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function resolvedPersonaStickerAvatar(): string {
  return personaStickerAvatarUrl() ?? PERSONA_STICKER_FALLBACK
}

if (typeof window !== 'undefined') {
  window.addEventListener(PERSONA_UPDATED_EVENT, () => {
    void refreshPersonaStickerAvatar()
  })
}
