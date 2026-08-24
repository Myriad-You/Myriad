import React from 'react'

export type MyriadConfigIconKind =
  | 'platforms'
  | 'ai'
  | 'tripo'
  | 'basic'
  | 'music'
  | 'oauth'
  | 'permissions'
  | 'notifications'
  | 'modules'
  | 'advanced'
  | 'about'
  | 'users'
  | 'federation'
  /** Agent — same asset as notification source `agent` */
  | 'agent'

interface MyriadConfigIconProps {
  kind: MyriadConfigIconKind
  className?: string
}

const SHARED_CONFIG_ICON_ASSETS: Partial<Record<MyriadConfigIconKind, string>> =
  {
    basic: '/icons/control-panel/config.webp',
    music: '/icons/dynamic/music.webp',
    tripo: '/icons/config/tripo.svg',
    // Same icon as notification center federation source
    federation: '/icons/notifications/aro.webp',
    // Same icon as notification center Agent / agent source
    agent: '/icons/notifications/arael.webp',
  }

/**
 * Myriad 设置页专用品牌图标资产。
 * 同一资产会在分类导航和分类标题中按容器尺寸显示。
 */
export const MyriadConfigIcon = React.memo<MyriadConfigIconProps>(
  ({ kind, className = '' }) => {
    const src = SHARED_CONFIG_ICON_ASSETS[kind] ?? `/icons/config/${kind}.webp`

    return (
      <img
        className={`myriad-config-icon myriad-config-icon--${kind} ${className}`.trim()}
        src={src}
        alt=""
        aria-hidden="true"
        draggable={false}
      />
    )
  },
)

MyriadConfigIcon.displayName = 'MyriadConfigIcon'

export default MyriadConfigIcon
