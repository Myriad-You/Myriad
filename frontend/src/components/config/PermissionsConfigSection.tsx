import type { PermissionItem, QuotaItem } from '../settings'
import { FaSlidersH, LuSparkles } from '@lib/icons'
import React, { useCallback, useMemo } from 'react'
import { useI18n } from '../../contexts/I18nContext'

import {
  PermissionGroup,
  QuotaGroup,
  SegmentedControl,
  SettingGroup,
  SettingGroupGrid,
  SettingSection,
  useSettingGuide,
} from '../settings'
import { MyriadConfigIcon } from './MyriadConfigIcon'

/** Agent elevated keys only; media/theme unchanged */
const AGENT_PRESET_PERM_KEYS = [
  'ai_chat',
  'ai_analyze',
  'ai_generate',
  'ai_image',
  'ai_search',
  'network_fetch',
  'scheduler_register',
] as const

type AgentPresetKey = (typeof AGENT_PRESET_PERM_KEYS)[number]
const GUEST_AGENT_PRESET_PERM_KEYS = AGENT_PRESET_PERM_KEYS.filter(
  (key) => key !== 'scheduler_register',
)
const GUEST_AUTHENTICATED_PERMISSION_KEYS = new Set([
  'component_theme',
  'shortcut_register',
  'scheduler_register',
  'speech_tts',
  'speech_asr',
  // write-federation needs AuthedClaims; guest delegation is a no-op
  'federation_post',
  'federation_channel',
  'federation_room',
  'brew_comment_write',
])
export type AgentPermissionPreset = 'none' | 'chat' | 'standard' | 'elevated'

const AGENT_PRESET_FLAGS: Record<
  AgentPermissionPreset,
  Record<AgentPresetKey, boolean>
> = {
  none: {
    ai_chat: false,
    ai_analyze: false,
    ai_generate: false,
    ai_image: false,
    ai_search: false,
    network_fetch: false,
    scheduler_register: false,
  },
  chat: {
    ai_chat: true,
    ai_analyze: true,
    ai_generate: false,
    ai_image: false,
    ai_search: false,
    network_fetch: false,
    scheduler_register: false,
  },
  standard: {
    ai_chat: true,
    ai_analyze: true,
    ai_generate: true,
    ai_image: true,
    ai_search: true,
    network_fetch: false,
    scheduler_register: false,
  },
  elevated: {
    ai_chat: true,
    ai_analyze: true,
    ai_generate: true,
    ai_image: true,
    ai_search: true,
    network_fetch: true,
    scheduler_register: true,
  },
}

const AGENT_PRESET_LEVELS: AgentPermissionPreset[] = [
  'none',
  'chat',
  'standard',
  'elevated',
]

function detectAgentPreset(
  values: Record<string, boolean>,
  keys: readonly AgentPresetKey[] = AGENT_PRESET_PERM_KEYS,
): AgentPermissionPreset | 'custom' {
  for (const level of AGENT_PRESET_LEVELS) {
    const flags = AGENT_PRESET_FLAGS[level]
    if (keys.every((k) => values[k] === flags[k])) {
      return level
    }
  }
  return 'custom'
}

export interface PermissionConfigValues extends Record<
  string,
  boolean | number
> {
  user_perm_ai_generate: boolean
  user_perm_ai_analyze: boolean
  user_perm_ai_chat: boolean
  user_perm_report_write: boolean
  user_perm_network_fetch: boolean
  user_perm_component_theme: boolean
  user_perm_shortcut_register: boolean
  user_perm_event_publish: boolean
  user_perm_ai_image: boolean
  user_perm_ai_search: boolean
  user_perm_3d_generate: boolean
  user_perm_scheduler_register: boolean
  user_perm_speech_tts: boolean
  user_perm_speech_asr: boolean
  user_perm_storage_write: boolean
  user_perm_federation_post: boolean
  user_perm_federation_channel: boolean
  user_perm_federation_room: boolean
  user_perm_brew_comment_write: boolean
  guest_perm_ai_generate: boolean
  guest_perm_ai_analyze: boolean
  guest_perm_ai_chat: boolean
  guest_perm_report_write: boolean
  guest_perm_network_fetch: boolean
  guest_perm_component_theme: boolean
  guest_perm_shortcut_register: boolean
  guest_perm_event_publish: boolean
  guest_perm_ai_image: boolean
  guest_perm_ai_search: boolean
  guest_perm_3d_generate: boolean
  guest_perm_scheduler_register: boolean
  guest_perm_speech_tts: boolean
  guest_perm_speech_asr: boolean
  guest_perm_storage_write: boolean
  guest_perm_federation_post: boolean
  guest_perm_federation_channel: boolean
  guest_perm_federation_room: boolean
  guest_perm_brew_comment_write: boolean
  user_ai_daily_calls: number
  user_ai_daily_tokens: number
  user_ai_cooldown_seconds: number
  guest_ai_daily_calls: number
  guest_ai_daily_tokens: number
  guest_ai_cooldown_seconds: number
}

interface PermissionsConfigSectionProps {
  permissionConfig: PermissionConfigValues
  updatePermissionConfig: (
    key: string | Record<string, boolean | number>,
    value?: boolean | number,
  ) => void
  loading?: boolean
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
}

export const PermissionsConfigSection: React.FC<
  PermissionsConfigSectionProps
> = ({
  permissionConfig,
  updatePermissionConfig,
  loading = false,
  title,
  icon,
  description,
  sectionId,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()

  // capabilities that need a persistent login are hidden from guests
  const permissionItems: PermissionItem[] = [
    {
      key: 'ai_generate',
      code: 'ai:generate',
      label: t.config.permAiGenerate,
      hint: t.config.permAiGenerateHint,
    },
    {
      key: 'ai_analyze',
      code: 'ai:analyze',
      label: t.config.permAiAnalyze,
      hint: t.config.permAiAnalyzeHint,
    },
    {
      key: 'ai_chat',
      code: 'ai:chat',
      label: t.config.permAiChat,
      hint: t.config.permAiChatHint,
    },
    {
      key: 'ai_image',
      code: 'ai:image',
      label: t.config.permAiImage,
      hint: t.config.permAiImageHint,
    },
    {
      key: 'ai_search',
      code: 'ai:search',
      label: t.config.permAiSearch,
      hint: t.config.permAiSearchHint,
    },
    {
      key: '3d_generate',
      code: '3d:generate',
      label: t.config.perm3dGenerate,
      hint: t.config.perm3dGenerateHint,
    },
    {
      key: 'speech_tts',
      code: 'speech:tts',
      label: t.config.permSpeechTts,
      hint: t.config.permSpeechTtsHint,
    },
    {
      key: 'speech_asr',
      code: 'speech:asr',
      label: t.config.permSpeechAsr,
      hint: t.config.permSpeechAsrHint,
    },
    {
      key: 'storage_write',
      code: 'storage:write',
      label: t.config.permStorageWrite,
      hint: t.config.permStorageWriteHint,
    },
    {
      key: 'federation_post',
      code: 'federation:post',
      label: t.tapp.permPostFederation,
      hint: t.tapp.permPostFederationDesc,
    },
    {
      key: 'federation_channel',
      code: 'federation:channel',
      label: t.tapp.permChannelFederation,
      hint: t.tapp.permChannelFederationDesc,
    },
    {
      key: 'federation_room',
      code: 'federation:room',
      label: t.tapp.permRoomFederation,
      hint: t.tapp.permRoomFederationDesc,
    },
    {
      key: 'brew_comment_write',
      code: 'brew:commentWrite',
      label: t.tapp.permCommentWriteBrew,
      hint: t.tapp.permCommentWriteBrewDesc,
    },
    {
      key: 'network_fetch',
      code: 'network:fetch',
      label: t.config.permNetworkFetch,
      hint: t.config.permNetworkFetchHint,
    },
    {
      key: 'event_publish',
      code: 'event:publish',
      label: t.config.permEventPublish,
      hint: t.config.permEventPublishHint,
    },
    {
      key: 'component_theme',
      code: 'component:theme',
      label: t.config.permComponentTheme,
      hint: t.config.permComponentThemeHint,
    },
    {
      key: 'shortcut_register',
      code: 'shortcut:register',
      label: t.config.permShortcutRegister,
      hint: t.config.permShortcutRegisterHint,
    },
    {
      key: 'scheduler_register',
      code: 'scheduler:register',
      label: t.config.permSchedulerRegister,
      hint: t.config.permSchedulerRegisterHint,
    },
  ]
  const guestPermissionItems = permissionItems.filter(
    (item) => !GUEST_AUTHENTICATED_PERMISSION_KEYS.has(item.key),
  )

  const quotaItems: QuotaItem[] = [
    {
      key: 'daily_calls',
      label: t.config.aiDailyCalls,
      hint: t.config.aiDailyCallsHint,
      min: 0,
      max: 10000,
    },
    {
      key: 'daily_tokens',
      label: t.config.aiDailyTokens,
      hint: t.config.aiDailyTokensHint,
      min: 0,
      max: 1000000,
    },
    {
      key: 'cooldown_seconds',
      label: t.config.aiCooldownSeconds,
      hint: t.config.aiCooldownSecondsHint,
      min: 0,
      max: 3600,
      unit: t.config.unitSeconds,
    },
  ]

  const getUserPermValues = () => {
    const values: Record<string, boolean> = {}
    permissionItems.forEach((item) => {
      values[item.key] = permissionConfig[
        `user_perm_${item.key}` as keyof typeof permissionConfig
      ] as boolean
    })
    return values
  }

  const getGuestPermValues = () => {
    const values: Record<string, boolean> = {}
    guestPermissionItems.forEach((item) => {
      values[item.key] = permissionConfig[
        `guest_perm_${item.key}` as keyof typeof permissionConfig
      ] as boolean
    })
    return values
  }

  const getUserQuotaValues = () => ({
    daily_calls: permissionConfig.user_ai_daily_calls,
    daily_tokens: permissionConfig.user_ai_daily_tokens,
    cooldown_seconds: permissionConfig.user_ai_cooldown_seconds,
  })

  const getGuestQuotaValues = () => ({
    daily_calls: permissionConfig.guest_ai_daily_calls,
    daily_tokens: permissionConfig.guest_ai_daily_tokens,
    cooldown_seconds: permissionConfig.guest_ai_cooldown_seconds,
  })

  const userAgentValues = useMemo(() => {
    const values: Record<string, boolean> = {}
    for (const k of AGENT_PRESET_PERM_KEYS) {
      values[k] = permissionConfig[
        `user_perm_${k}` as keyof typeof permissionConfig
      ] as boolean
    }
    return values
  }, [permissionConfig])

  const guestAgentValues = useMemo(() => {
    const values: Record<string, boolean> = {}
    for (const k of GUEST_AGENT_PRESET_PERM_KEYS) {
      values[k] = permissionConfig[
        `guest_perm_${k}` as keyof typeof permissionConfig
      ] as boolean
    }
    return values
  }, [permissionConfig])

  const userPreset = detectAgentPreset(userAgentValues)
  const guestPreset = detectAgentPreset(
    guestAgentValues,
    GUEST_AGENT_PRESET_PERM_KEYS,
  )

  const presetLabels = useMemo(
    () => ({
      none: t.config.agentUsageNone,
      chat: t.config.agentUsageChat,
      standard: t.config.agentUsageStandard,
      elevated: t.config.agentUsageElevated,
      custom: t.config.agentPresetCustom,
    }),
    [t],
  )

  const applyAgentPreset = useCallback(
    (role: 'user' | 'guest', level: AgentPermissionPreset) => {
      if (loading) return
      const flags = AGENT_PRESET_FLAGS[level]
      const patch: Record<string, boolean> = {}
      const keys =
        role === 'guest' ? GUEST_AGENT_PRESET_PERM_KEYS : AGENT_PRESET_PERM_KEYS
      for (const k of keys) {
        patch[`${role}_perm_${k}`] = flags[k]
      }
      updatePermissionConfig(patch)
    },
    [loading, updatePermissionConfig],
  )

  const renderPresetButtons = (
    role: 'user' | 'guest',
    current: AgentPermissionPreset | 'custom',
  ) => (
    <SegmentedControl
      size="sm"
      columns={4}
      disabled={loading}
      value={current === 'custom' ? null : current}
      options={AGENT_PRESET_LEVELS.map((level) => ({
        value: level,
        label: presetLabels[level],
      }))}
      onChange={(level) => applyAgentPreset(role, level)}
      ariaLabel={t.config.agentPresetTitle}
    />
  )

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      <SettingGroup
        title={t.config.agentPresetTitle}
        description={t.config.agentPresetDesc}
        {...bindGuide('permissions.agentPreset', g.permissions.agentPreset)}
        icon={<MyriadConfigIcon kind="agent" />}
      >
        <SettingGroupGrid
          columns={2}
          variant="card"
          align="stretch"
          minColumnWidth="16rem"
          className="agent-preset-grid"
          ariaLabel={t.config.agentPresetTitle}
        >
          <SettingGroup
            title={t.config.agentUsageUser}
            description={t.config.agentPresetUserHint}
            {...bindGuide(
              'permissions.agentPresetUser',
              g.permissions.agentPresetUser,
            )}
          >
            {renderPresetButtons('user', userPreset)}
          </SettingGroup>
          <SettingGroup
            title={t.config.agentUsageGuest}
            description={t.config.agentPresetGuestHint}
            {...bindGuide(
              'permissions.agentPresetGuest',
              g.permissions.agentPresetGuest,
            )}
          >
            {renderPresetButtons('guest', guestPreset)}
          </SettingGroup>
        </SettingGroupGrid>
      </SettingGroup>

      <SettingGroup
        title={t.config.agentFineTuneTitle}
        description={t.config.agentFineTuneDesc}
        {...bindGuide('permissions.fineTune', g.permissions.fineTune)}
        icon={<FaSlidersH aria-hidden />}
      >
        <PermissionGroup
          title={t.config.userElevatedPermissions}
          description={t.config.userElevatedPermissionsDesc}
          {...bindGuide('permissions.userElevated', g.permissions.userElevated)}
          permissions={permissionItems}
          values={getUserPermValues()}
          onChange={(key, value) =>
            updatePermissionConfig(`user_perm_${key}`, value)
          }
          loading={loading}
        />
        <PermissionGroup
          title={t.config.guestElevatedPermissions}
          description={t.config.guestElevatedPermissionsDesc}
          {...bindGuide('permissions.guestElevated', g.permissions.guestElevated)}
          permissions={guestPermissionItems}
          values={getGuestPermValues()}
          onChange={(key, value) =>
            updatePermissionConfig(`guest_perm_${key}`, value)
          }
          loading={loading}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.aiQuotaTitle}
        description={t.config.aiQuotaDesc}
        {...bindGuide('permissions.aiQuota', g.permissions.aiQuota)}
        icon={<LuSparkles aria-hidden />}
      >
        <QuotaGroup
          title={t.config.userAiQuota}
          description={t.config.userAiQuotaDesc}
          {...bindGuide('permissions.userQuota', g.permissions.userQuota)}
          quotas={quotaItems}
          values={getUserQuotaValues()}
          onChange={(key, value) =>
            updatePermissionConfig(`user_ai_${key}`, value)
          }
          loading={loading}
        />
        <QuotaGroup
          title={t.config.guestAiQuota}
          description={t.config.guestAiQuotaDesc}
          {...bindGuide('permissions.guestQuota', g.permissions.guestQuota)}
          quotas={quotaItems}
          values={getGuestQuotaValues()}
          onChange={(key, value) =>
            updatePermissionConfig(`guest_ai_${key}`, value)
          }
          loading={loading}
        />
      </SettingGroup>
    </SettingSection>
  )
}

export default PermissionsConfigSection
