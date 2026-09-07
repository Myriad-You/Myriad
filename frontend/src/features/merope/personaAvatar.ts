/**
 * 人设贴纸头像的全站只读缓存。
 *
 * 通知图标要在构造 `Notification` 的那一瞬间同步拿到一个地址，来不及 await，
 * 所以这里存一份内存缓存：第一次读会顺手去拉公开配置，拉到之后通知订阅者重画。
 * 拉不到就是 `null`，调用方自己决定退回哪张图——这里不编造兜底。
 *
 * 数据来自 `/api/config/public` 的 `agentPersonaAvatarUrl`，和对外名字同进同出：
 * 人设关掉时后端不给这个字段，头像也就跟着收回。
 */

import { getPublicConfigDeduped } from '../../utils/requestDedup'

type Listener = (url: string | null) => void

let cached: string | null = null
let loaded = false
let inFlight: Promise<string | null> | null = null
const listeners = new Set<Listener>()

/**
 * 从公开配置里读贴纸地址。后端把字段名换了、或者给了空串，都必须落成
 * `null` 而不是一个假地址——通知图标拿到假地址会画成裂图。
 */
export function personaStickerAvatarFromConfig(config: unknown): string | null {
  if (!config || typeof config !== 'object') return null
  const raw = (config as { agentPersonaAvatarUrl?: unknown })
    .agentPersonaAvatarUrl
  if (typeof raw !== 'string') return null
  return raw.trim() || null
}

function publish(next: string | null) {
  if (cached === next && loaded) return
  cached = next
  loaded = true
  for (const listener of listeners) listener(cached)
}

/**
 * 已知的贴纸头像地址，没有就是 `null`。
 *
 * 第一次调用时会在后台去拉一次；本次调用不等它。
 */
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
      // 公开配置拉不到不该让通知中心裂开，当作没有头像。
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
