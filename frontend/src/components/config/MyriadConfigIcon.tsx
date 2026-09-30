import React, { useSyncExternalStore } from 'react'
import {
  onPersonaStickerAvatar,
  PERSONA_STICKER_FALLBACK,
  resolvedPersonaStickerAvatar,
} from '../../features/merope/persona/personaAvatar'

export type MyriadConfigIconKind =
  | 'platforms'
  | 'ai'
  | 'lab'
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
    federation: '/icons/notifications/aro.webp',
    agent: PERSONA_STICKER_FALLBACK,
  }

export const MyriadConfigIcon = React.memo<MyriadConfigIconProps>(
  ({ kind, className = '' }) => {
    const fallback = SHARED_CONFIG_ICON_ASSETS[kind] ?? `/icons/config/${kind}.webp`
    const src = useSyncExternalStore(
      onPersonaStickerAvatar,
      () => (kind === 'agent' ? resolvedPersonaStickerAvatar() : fallback),
      () => fallback,
    )

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
