/**
 * Tapp 权限展示元数据
 *
 * 权限 → 图标 / i18n 键的映射，供详情页与商店详情视图共用。
 * 权限级别定义见 runtime/permissionConfig.ts 的 PERMISSION_LEVELS。
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
  FaServer,
  FaSignInAlt,
  FaTh,
  FaTools,
  FaUsers,
  FaVolumeUp,
  LuKeyboard,
} from '@lib/icons'

/** 权限图标组件（Fa / Lu 均可） */
export type PermissionIcon = ComponentType<{
  className?: string
  size?: number | string
}>

/** 权限展示配置 - 使用 i18n 键名（对应 t.tapp 中的扁平键） */
export const PERMISSION_CONFIG: Record<
  TappPermission,
  {
    icon: PermissionIcon
    labelKey: string
    descriptionKey: string
  }
> = {
  // —— 小组件 ——
  'widget:register': {
    icon: FaTh,
    labelKey: 'permRegisterWidget',
    descriptionKey: 'permRegisterWidgetDesc',
  },

  // —— 平台数据 ——
  'platform:read': {
    icon: FaDatabase,
    labelKey: 'permReadPlatform',
    descriptionKey: 'permReadPlatformDesc',
  },
  'platform:write': {
    icon: FaEdit,
    labelKey: 'permWritePlatform',
    descriptionKey: 'permWritePlatformDesc',
  },
  'platform:register': {
    icon: FaServer,
    labelKey: 'permRegisterPlatform',
    descriptionKey: 'permRegisterPlatformDesc',
  },

  // —— 访问统计 ——
  'analytics:read': {
    icon: FaChartBar,
    labelKey: 'permReadAnalytics',
    descriptionKey: 'permReadAnalyticsDesc',
  },

  // —— AI ——
  'ai:generate': {
    icon: FaMagic,
    labelKey: 'permAiGenerate',
    descriptionKey: 'permAiGenerateDesc',
  },
  'ai:analyze': {
    icon: FaBrain,
    labelKey: 'permAiAnalyze',
    descriptionKey: 'permAiAnalyzeDesc',
  },
  'ai:chat': {
    icon: FaComments,
    labelKey: 'permAiChat',
    descriptionKey: 'permAiChatDesc',
  },
  'ai:image': {
    icon: FaImage,
    labelKey: 'permAiImage',
    descriptionKey: 'permAiImageDesc',
  },

  // —— 报告 ——
  'report:read': {
    icon: FaChartBar,
    labelKey: 'permReadReport',
    descriptionKey: 'permReadReportDesc',
  },
  'report:write': {
    icon: FaEdit,
    labelKey: 'permWriteReport',
    descriptionKey: 'permWriteReportDesc',
  },

  // —— 存储 / UI ——
  'storage:read': {
    icon: FaHdd,
    labelKey: 'permStorage',
    descriptionKey: 'permStorageDesc',
  },
  'storage:write': {
    icon: FaHdd,
    labelKey: 'permStorage',
    descriptionKey: 'permStorageDesc',
  },
  'ui:notification': {
    icon: FaBell,
    labelKey: 'permNotification',
    descriptionKey: 'permNotificationDesc',
  },
  'ui:fullscreen': {
    icon: FaExpand,
    labelKey: 'permFullscreen',
    descriptionKey: 'permFullscreenDesc',
  },
  'ui:theme': {
    icon: FaPalette,
    labelKey: 'permReadTheme',
    descriptionKey: 'permReadThemeDesc',
  },
  'ui:confirm': {
    icon: FaQuestionCircle,
    labelKey: 'permConfirm',
    descriptionKey: 'permConfirmDesc',
  },
  'ui:openUrl': {
    icon: FaExternalLinkAlt,
    labelKey: 'permOpenUrl',
    descriptionKey: 'permOpenUrlDesc',
  },

  // —— 网络 / 媒体 ——
  'network:fetch': {
    icon: FaGlobe,
    labelKey: 'permNetworkFetch',
    descriptionKey: 'permNetworkFetchDesc',
  },
  'media:control': {
    icon: FaPlay,
    labelKey: 'permMediaControl',
    descriptionKey: 'permMediaControlDesc',
  },
  'media:read': {
    icon: FaMusic,
    labelKey: 'permMediaRead',
    descriptionKey: 'permMediaReadDesc',
  },
  'media:audio': {
    icon: FaVolumeUp,
    labelKey: 'permMediaAudio',
    descriptionKey: 'permMediaAudioDesc',
  },

  // —— 组件注册 ——
  'component:theme': {
    icon: FaPaintBrush,
    labelKey: 'permRegisterTheme',
    descriptionKey: 'permRegisterThemeDesc',
  },
  'component:agent': {
    icon: FaRobot,
    labelKey: 'permRegisterAgent',
    descriptionKey: 'permRegisterAgentDesc',
  },
  'shortcut:register': {
    icon: LuKeyboard,
    labelKey: 'permRegisterShortcut',
    descriptionKey: 'permRegisterShortcutDesc',
  },

  // —— 事件 / 调度 ——
  'event:publish': {
    icon: FaPaperPlane,
    labelKey: 'permPublishEvent',
    descriptionKey: 'permPublishEventDesc',
  },
  'event:subscribe': {
    icon: FaBroadcastTower,
    labelKey: 'permSubscribeEvent',
    descriptionKey: 'permSubscribeEventDesc',
  },
  'scheduler:register': {
    icon: FaClock,
    labelKey: 'permSchedulerRegister',
    descriptionKey: 'permSchedulerRegisterDesc',
  },

  // —— 语音（TTS 输出 / ASR 输入） ——
  'speech:tts': {
    icon: FaVolumeUp,
    labelKey: 'permSpeechTts',
    descriptionKey: 'permSpeechTtsDesc',
  },
  'speech:asr': {
    icon: FaMicrophone,
    labelKey: 'permSpeechAsr',
    descriptionKey: 'permSpeechAsrDesc',
  },

  // —— Tapp 列表 ——
  'tappList:read': {
    icon: FaList,
    labelKey: 'permReadTappList',
    descriptionKey: 'permReadTappListDesc',
  },
  'tappList:manage': {
    icon: FaTools,
    labelKey: 'permManageTappList',
    descriptionKey: 'permManageTappListDesc',
  },

  // —— Brew ——
  'brew:read': {
    icon: FaNewspaper,
    labelKey: 'permReadBrew',
    descriptionKey: 'permReadBrewDesc',
  },
  'brew:write': {
    icon: FaEdit,
    labelKey: 'permWriteBrew',
    descriptionKey: 'permWriteBrewDesc',
  },
  'brew:comment': {
    icon: FaComments,
    labelKey: 'permCommentBrew',
    descriptionKey: 'permCommentBrewDesc',
  },
  'brew:manage': {
    icon: FaCog,
    labelKey: 'permManageBrew',
    descriptionKey: 'permManageBrewDesc',
  },

  // —— 联邦 ——
  'federation:read': {
    icon: FaUsers,
    labelKey: 'permReadFederation',
    descriptionKey: 'permReadFederationDesc',
  },
  'federation:post': {
    icon: FaEdit,
    labelKey: 'permPostFederation',
    descriptionKey: 'permPostFederationDesc',
  },
  'federation:interact': {
    icon: FaHeart,
    labelKey: 'permInteractFederation',
    descriptionKey: 'permInteractFederationDesc',
  },
  'federation:channel': {
    icon: FaComments,
    labelKey: 'permChannelFederation',
    descriptionKey: 'permChannelFederationDesc',
  },
  'federation:room': {
    icon: FaSignInAlt,
    labelKey: 'permRoomFederation',
    descriptionKey: 'permRoomFederationDesc',
  },
  'federation:ring': {
    icon: FaExchangeAlt,
    labelKey: 'permRingFederation',
    descriptionKey: 'permRingFederationDesc',
  },
  'federation:message': {
    icon: FaEnvelope,
    labelKey: 'permMessageFederation',
    descriptionKey: 'permMessageFederationDesc',
  },
  'federation:trust': {
    icon: FaLock,
    labelKey: 'permTrustFederation',
    descriptionKey: 'permTrustFederationDesc',
  },
  'federation:files': {
    icon: FaFolder,
    labelKey: 'permFederationFiles',
    descriptionKey: 'permFederationFilesDesc',
  },
}
