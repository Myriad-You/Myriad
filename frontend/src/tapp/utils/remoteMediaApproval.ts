/** remoteMedia 批准状态：声明了 media:remote 的域名里，还有哪些没被批准。 */

import type { TappInstance } from '../types'

export function declaredRemoteMedia(instance: Pick<TappInstance, 'manifest'>): string[] {
  const { manifest } = instance
  if (!manifest.permissions?.includes('media:remote')) return []
  return [...new Set(manifest.remoteMedia ?? [])]
}

/** 只有能看到批准集的人（管理员、私装本人）才算得出待批准；其余返回空。 */
export function pendingRemoteMedia(
  instance: Pick<TappInstance, 'manifest' | 'approvedRemoteMedia'>,
): string[] {
  if (!instance.approvedRemoteMedia) return []
  const approved = new Set(instance.approvedRemoteMedia)
  return declaredRemoteMedia(instance).filter((host) => !approved.has(host))
}
