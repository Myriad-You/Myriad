/** 展示元数据。级别在 permissionConfig PERMISSION_LEVELS。 */

import type { ComponentType } from 'react'
import type { TappPermission } from '../types'
import {
  FaBell,
  FaBrain,
  FaBroadcastTower,
  FaChartBar,
  FaClock,
  FaCog,
  FaComments,
  FaCube,
  FaDatabase,
  FaEdit,
  FaEnvelope,
  FaExchangeAlt,
  FaExpand,
  FaExternalLinkAlt,
  FaFolder,
  FaGlobe,
  FaHdd,
  FaHeart,
  FaImage,
  FaList,
  FaLock,
  FaMagic,
  FaMicrophone,
  FaMusic,
  FaNewspaper,
  FaPaintBrush,
  FaPalette,
  FaPaperPlane,
  FaPlay,
  FaQuestionCircle,
  FaRobot,
  FaSearch,
  FaServer,
  FaSignInAlt,
  FaTh,
  FaTools,
  FaUsers,
  FaVolumeUp,
  LuKeyboard,
} from '@lib/icons'
import { PERMISSION_COPY } from './permissionCopy'

export type PermissionIcon = ComponentType<{
  className?: string
  size?: number | string
}>

const PERMISSION_ICONS: Record<TappPermission, PermissionIcon> = {
  'widget:register': FaTh,
  'platform:read': FaDatabase,
  'platform:write': FaEdit,
  'platform:register': FaServer,
  'analytics:read': FaChartBar,
  'ai:generate': FaMagic,
  'ai:analyze': FaBrain,
  'ai:chat': FaComments,
  'ai:image': FaImage,
  'ai:search': FaSearch,
  '3d:generate': FaCube,
  'report:read': FaChartBar,
  'report:write': FaEdit,
  'storage:read': FaHdd,
  'storage:write': FaHdd,
  'ui:notification': FaBell,
  'ui:fullscreen': FaExpand,
  'ui:theme': FaPalette,
  'ui:confirm': FaQuestionCircle,
  'ui:openUrl': FaExternalLinkAlt,
  'network:fetch': FaGlobe,
  'media:control': FaPlay,
  'media:read': FaMusic,
  'media:audio': FaVolumeUp,
  'component:theme': FaPaintBrush,
  'component:agent': FaRobot,
  'shortcut:register': LuKeyboard,
  'event:publish': FaPaperPlane,
  'event:subscribe': FaBroadcastTower,
  'scheduler:register': FaClock,
  'speech:tts': FaVolumeUp,
  'speech:asr': FaMicrophone,
  'tappList:read': FaList,
  'tappList:manage': FaTools,
  'brew:read': FaNewspaper,
  'brew:write': FaEdit,
  'brew:commentWrite': FaComments,
  'brew:manage': FaCog,
  'federation:read': FaUsers,
  'federation:post': FaEdit,
  'federation:interact': FaHeart,
  'federation:channel': FaComments,
  'federation:room': FaSignInAlt,
  'federation:ring': FaExchangeAlt,
  'federation:message': FaEnvelope,
  'federation:trust': FaLock,
  'federation:files': FaFolder,
  'game:session': FaUsers,
}

export const PERMISSION_CONFIG: Record<
  TappPermission,
  {
    icon: PermissionIcon
    labelKey: string
    descriptionKey: string
  }
> = (Object.keys(PERMISSION_COPY) as TappPermission[]).reduce(
  (acc, name) => {
    acc[name] = { icon: PERMISSION_ICONS[name], ...PERMISSION_COPY[name] }
    return acc
  },
  {} as Record<
    TappPermission,
    { icon: PermissionIcon; labelKey: string; descriptionKey: string }
  >,
)
