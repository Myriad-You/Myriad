/**
 * Permission i18n copy keys, independent of icon components.
 *
 * Keep this free of `@lib/icons` so catalog/copy tests can run under node:test.
 */

import type { TappPermission } from '../types'

export const PERMISSION_COPY: Record<
  TappPermission,
  { labelKey: string; descriptionKey: string }
> = {
  'widget:register': {
    labelKey: 'permRegisterWidget',
    descriptionKey: 'permRegisterWidgetDesc',
  },
  'platform:read': {
    labelKey: 'permReadPlatform',
    descriptionKey: 'permReadPlatformDesc',
  },
  'platform:write': {
    labelKey: 'permWritePlatform',
    descriptionKey: 'permWritePlatformDesc',
  },
  'platform:register': {
    labelKey: 'permRegisterPlatform',
    descriptionKey: 'permRegisterPlatformDesc',
  },
  'analytics:read': {
    labelKey: 'permReadAnalytics',
    descriptionKey: 'permReadAnalyticsDesc',
  },
  'ai:generate': {
    labelKey: 'permAiGenerate',
    descriptionKey: 'permAiGenerateDesc',
  },
  'ai:analyze': {
    labelKey: 'permAiAnalyze',
    descriptionKey: 'permAiAnalyzeDesc',
  },
  'ai:chat': {
    labelKey: 'permAiChat',
    descriptionKey: 'permAiChatDesc',
  },
  'ai:image': {
    labelKey: 'permAiImage',
    descriptionKey: 'permAiImageDesc',
  },
  '3d:generate': {
    labelKey: 'perm3dGenerate',
    descriptionKey: 'perm3dGenerateDesc',
  },
  'report:read': {
    labelKey: 'permReadReport',
    descriptionKey: 'permReadReportDesc',
  },
  'report:write': {
    labelKey: 'permWriteReport',
    descriptionKey: 'permWriteReportDesc',
  },
  'storage:read': {
    labelKey: 'permStorageRead',
    descriptionKey: 'permStorageReadDesc',
  },
  'storage:write': {
    labelKey: 'permStorageWrite',
    descriptionKey: 'permStorageWriteDesc',
  },
  'ui:notification': {
    labelKey: 'permNotification',
    descriptionKey: 'permNotificationDesc',
  },
  'ui:fullscreen': {
    labelKey: 'permFullscreen',
    descriptionKey: 'permFullscreenDesc',
  },
  'ui:theme': {
    labelKey: 'permReadTheme',
    descriptionKey: 'permReadThemeDesc',
  },
  'ui:confirm': {
    labelKey: 'permConfirm',
    descriptionKey: 'permConfirmDesc',
  },
  'ui:openUrl': {
    labelKey: 'permOpenUrl',
    descriptionKey: 'permOpenUrlDesc',
  },
  'network:fetch': {
    labelKey: 'permNetworkFetch',
    descriptionKey: 'permNetworkFetchDesc',
  },
  'media:control': {
    labelKey: 'permMediaControl',
    descriptionKey: 'permMediaControlDesc',
  },
  'media:read': {
    labelKey: 'permMediaRead',
    descriptionKey: 'permMediaReadDesc',
  },
  'media:audio': {
    labelKey: 'permMediaAudio',
    descriptionKey: 'permMediaAudioDesc',
  },
  'component:theme': {
    labelKey: 'permRegisterTheme',
    descriptionKey: 'permRegisterThemeDesc',
  },
  'component:agent': {
    labelKey: 'permRegisterAgent',
    descriptionKey: 'permRegisterAgentDesc',
  },
  'shortcut:register': {
    labelKey: 'permRegisterShortcut',
    descriptionKey: 'permRegisterShortcutDesc',
  },
  'event:publish': {
    labelKey: 'permPublishEvent',
    descriptionKey: 'permPublishEventDesc',
  },
  'event:subscribe': {
    labelKey: 'permSubscribeEvent',
    descriptionKey: 'permSubscribeEventDesc',
  },
  'scheduler:register': {
    labelKey: 'permSchedulerRegister',
    descriptionKey: 'permSchedulerRegisterDesc',
  },
  'speech:tts': {
    labelKey: 'permSpeechTts',
    descriptionKey: 'permSpeechTtsDesc',
  },
  'speech:asr': {
    labelKey: 'permSpeechAsr',
    descriptionKey: 'permSpeechAsrDesc',
  },
  'tappList:read': {
    labelKey: 'permReadTappList',
    descriptionKey: 'permReadTappListDesc',
  },
  'tappList:manage': {
    labelKey: 'permManageTappList',
    descriptionKey: 'permManageTappListDesc',
  },
  'brew:read': {
    labelKey: 'permReadBrew',
    descriptionKey: 'permReadBrewDesc',
  },
  'brew:write': {
    labelKey: 'permWriteBrew',
    descriptionKey: 'permWriteBrewDesc',
  },
  'brew:commentWrite': {
    labelKey: 'permCommentWriteBrew',
    descriptionKey: 'permCommentWriteBrewDesc',
  },
  'brew:manage': {
    labelKey: 'permManageBrew',
    descriptionKey: 'permManageBrewDesc',
  },
  'federation:read': {
    labelKey: 'permReadFederation',
    descriptionKey: 'permReadFederationDesc',
  },
  'federation:post': {
    labelKey: 'permPostFederation',
    descriptionKey: 'permPostFederationDesc',
  },
  'federation:interact': {
    labelKey: 'permInteractFederation',
    descriptionKey: 'permInteractFederationDesc',
  },
  'federation:channel': {
    labelKey: 'permChannelFederation',
    descriptionKey: 'permChannelFederationDesc',
  },
  'federation:room': {
    labelKey: 'permRoomFederation',
    descriptionKey: 'permRoomFederationDesc',
  },
  'federation:ring': {
    labelKey: 'permRingFederation',
    descriptionKey: 'permRingFederationDesc',
  },
  'federation:message': {
    labelKey: 'permMessageFederation',
    descriptionKey: 'permMessageFederationDesc',
  },
  'federation:trust': {
    labelKey: 'permTrustFederation',
    descriptionKey: 'permTrustFederationDesc',
  },
  'federation:files': {
    labelKey: 'permFederationFiles',
    descriptionKey: 'permFederationFilesDesc',
  },
  'game:session': {
    labelKey: 'permGameSession',
    descriptionKey: 'permGameSessionDesc',
  },
}
