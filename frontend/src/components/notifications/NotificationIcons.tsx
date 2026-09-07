import type { NotificationSourceKey } from '../../services/notificationPreferencesApi'
import { useSyncExternalStore } from 'react'
import {
  onPersonaStickerAvatar,
  personaStickerAvatarUrl,
} from '../../features/merope/personaAvatar'

const NOTIFICATION_SOURCE_ICON_ASSETS = {
  agent: '/icons/notifications/arael.webp',
  heartbeat: '/icons/notifications/heartbeat.webp',
  mcp: '/icons/notifications/mcp.webp',
  brew: '/icons/notifications/brew.webp',
  tapp: '/icons/notifications/tapp.webp',
  updater: '/icons/notifications/updater.webp',
  federation: '/icons/notifications/aro.webp',
  system: '/icons/notifications/system.webp',
} satisfies Record<NotificationSourceKey, string>

/**
 * Agent 那一路用人设自己的贴纸头像；没生成过、或人设关掉了，才退回内置的
 * arael 图标。其余来源是产品图标，不跟人设走。
 *
 * 同步返回是硬要求：`new Notification({ icon })` 那一瞬间来不及 await，所以
 * 地址由 `personaAvatar` 那份内存缓存供给。
 */
export function notificationSourceIconAsset(source: NotificationSourceKey) {
  if (source === 'agent') {
    return personaStickerAvatarUrl() ?? NOTIFICATION_SOURCE_ICON_ASSETS.agent
  }
  return NOTIFICATION_SOURCE_ICON_ASSETS[source]
}

/**
 * 贴纸地址是异步拉回来的。已经画出来的图标要在拉到之后自己换掉，否则本次
 * 会话里通知中心一直挂着内置图标。
 */
function useNotificationSourceIcon(source: NotificationSourceKey): string {
  return useSyncExternalStore(
    onPersonaStickerAvatar,
    () => notificationSourceIconAsset(source),
    () => NOTIFICATION_SOURCE_ICON_ASSETS[source],
  )
}

function RasterNotificationIcon({
  src,
  className,
}: {
  src: string
  className?: string
}) {
  return (
    <img
      src={src}
      alt=""
      aria-hidden="true"
      className={className}
      width={16}
      height={16}
      draggable={false}
      decoding="async"
    />
  )
}

export function NotificationSourceIcon({
  source,
  className = 'h-4 w-4',
}: {
  source: NotificationSourceKey
  className?: string
}) {
  const src = useNotificationSourceIcon(source)
  return <RasterNotificationIcon src={src} className={className} />
}
