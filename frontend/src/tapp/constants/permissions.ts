/**
 * Tapp 权限展示元数据
 *
 * 权限 → 图标 / i18n 键的映射，供详情页与商店详情视图共用。
 * 权限级别定义见 runtime/permissionConfig.ts 的 PERMISSION_LEVELS。
 * i18n 键在 permissionCopy.ts，避免展示层依赖图标包。
 *
 * 图标语义（逐项对齐权限含义，避免「万能」图标）：
 * - 小组件 → 宫格；平台数据 → 库/编辑/服务；AI → 能力细分
 * - UI → 全屏/主题/确认；媒体 → 播放/音乐/音量；语音 → 出/入
 * - Brew/联邦/列表 → 阅读/社交/消息，不用 Database 兜底
 */

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
  FaSearch,
  FaMusic,
  FaNewspaper,
  FaPaintBrush,
  FaPalette,
  FaPaperPlane,
  FaPlay,
  FaQuestionCircle,
  FaRobot,
  FaServer,
  FaSignInAlt,
  FaTh,
  FaTools,
  FaUsers,
  FaVolumeUp,
  LuKeyboard,
} from '@lib/icons'
import { PERMISSION_COPY } from './permissionCopy'

/** 权限图标组件（Fa / Lu 均可） */
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

/** 权限展示配置 - 使用 i18n 键名（对应 t.tapp 中的扁平键） */
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
