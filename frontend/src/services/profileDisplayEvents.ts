/**
 * 画像源变：同时派发 avatar-changed + profile-display-changed（脸变了文案也要刷新）。
 * 文案源变：只派发 profile-display-changed，避免 img remount。
 * BroadcastChannel 名 `myriad-avatar` 保留；消息体用 `type` 区分。
 */

const AVATAR_CHANGED_EVENT = 'avatar-changed'
const PROFILE_DISPLAY_CHANGED_EVENT = 'profile-display-changed'
const CHANNEL = 'myriad-avatar'

function broadcast(type: string): void {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent(type))
  try {
    const channel = new BroadcastChannel(CHANNEL)
    channel.postMessage({ type })
    channel.close()
  } catch {
    // Safari 隐私模式等：同页 CustomEvent 已派发
  }
}

export function notifyAvatarChanged(): void {
  broadcast(AVATAR_CHANGED_EVENT)
  broadcast(PROFILE_DISPLAY_CHANGED_EVENT)
}

export function notifyProfileDisplayChanged(): void {
  broadcast(PROFILE_DISPLAY_CHANGED_EVENT)
}

function subscribe(type: string, handler: () => void): () => void {
  if (typeof window === 'undefined') return () => {}
  window.addEventListener(type, handler)

  let channel: BroadcastChannel | null = null
  try {
    channel = new BroadcastChannel(CHANNEL)
    channel.onmessage = (event) => {
      if (event.data?.type === type) handler()
    }
  } catch {
    channel = null
  }

  return () => {
    window.removeEventListener(type, handler)
    channel?.close()
  }
}

export function onAvatarChanged(handler: () => void): () => void {
  return subscribe(AVATAR_CHANGED_EVENT, handler)
}

export function onProfileDisplayChanged(handler: () => void): () => void {
  return subscribe(PROFILE_DISPLAY_CHANGED_EVENT, handler)
}
