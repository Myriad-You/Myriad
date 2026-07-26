/**
 * Tapp 权限配置：elevated 下放开关 + Agent 预设模板 + AI 配额
 */

import type { PermissionItem, QuotaItem } from '../settings'
import React, { useCallback, useMemo } from 'react'

import { useI18n } from '../../contexts/I18nContext'
import {
  PermissionGroup,
  QuotaGroup,
  SettingGroup,
  SettingSection,
} from '../settings'

/** Agent 相关 elevated 键（预设只改这些，不碰媒体/主题等） */
const AGENT_PRESET_PERM_KEYS = [
  'ai_chat',
  'ai_analyze',
  'ai_generate',
  'ai_image',
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
    network_fetch: false,
    scheduler_register: false,
  },
  chat: {
    ai_chat: true,
    ai_analyze: true,
    ai_generate: false,
    ai_image: false,
    network_fetch: false,
    scheduler_register: false,
  },
  standard: {
    ai_chat: true,
    ai_analyze: true,
    ai_generate: true,
    ai_image: true,
    network_fetch: false,
    scheduler_register: false,
  },
  elevated: {
    ai_chat: true,
    ai_analyze: true,
    ai_generate: true,
    ai_image: true,
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

const PRESET_CARD_CLASS =
  'rounded-lg border border-gray-100 bg-gray-50/80 p-3 dark:border-white/10 dark:bg-white/[0.03]'

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
  user_perm_scheduler_register: boolean
  user_perm_speech_tts: boolean
  user_perm_speech_asr: boolean
  // 游客权限
  guest_perm_ai_generate: boolean
  guest_perm_ai_analyze: boolean
  guest_perm_ai_chat: boolean
  guest_perm_report_write: boolean
  guest_perm_network_fetch: boolean
  guest_perm_component_theme: boolean
  guest_perm_shortcut_register: boolean
  guest_perm_event_publish: boolean
  guest_perm_ai_image: boolean
  guest_perm_scheduler_register: boolean
  guest_perm_speech_tts: boolean
  guest_perm_speech_asr: boolean
  // AI 配额
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

  // 定义权限项列表。要求持久登录主体的注册类能力不向游客展示。
  const permissionItems: PermissionItem[] = [
    // AI 相关
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
    // 语音相关
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
    // 网络（report:write 已仅管理员，不再展示下放开关）
    {
      key: 'network_fetch',
      code: 'network:fetch',
      label: t.config.permNetworkFetch,
      hint: t.config.permNetworkFetchHint,
    },
    // 界面与交互（media:control 已降 basic，始终开放，不再展示下放开关）
    {
      key: 'event_publish',
      code: 'event:publish',
      label: t.config.permEventPublish,
      hint: t.config.permEventPublishHint,
    },
    // 注册类
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

  // 定义配额项列表
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
      unit: '秒',
    },
  ]

  // 转换权限值（添加前缀）
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
    <div className="grid grid-cols-2 gap-1 rounded-lg border border-gray-100 bg-gray-50/80 p-1 dark:border-white/10 dark:bg-white/[0.03] sm:grid-cols-4">
      {AGENT_PRESET_LEVELS.map((level) => {
        const checked = current === level
        return (
          <button
            key={level}
            type="button"
            disabled={loading}
            aria-pressed={checked}
            onClick={() => applyAgentPreset(role, level)}
            className={`min-h-8 rounded-md px-2 text-xs font-medium transition-colors ${
              checked
                ? 'text-[var(--color-primary)]'
                : 'text-gray-500 hover:text-gray-800 dark:text-gray-400 dark:hover:text-gray-100'
            } disabled:opacity-50`}
            style={
              checked
                ? {
                    backgroundColor:
                      'color-mix(in srgb, var(--color-primary, #3b82f6) 12%, transparent)',
                    boxShadow:
                      '0 0 0 1px color-mix(in srgb, var(--color-primary, #3b82f6) 18%, transparent)',
                  }
                : undefined
            }
          >
            {presetLabels[level]}
          </button>
        )
      })}
    </div>
  )

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      {/* Agent 预设：批量开关下方 elevated 项 */}
      <SettingGroup
        title={t.config.agentPresetTitle}
        description={t.config.agentPresetDesc}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <div className={PRESET_CARD_CLASS}>
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="text-xs font-semibold text-gray-700 dark:text-gray-200">
                {t.config.agentUsageUser}
              </span>
              <span className="text-[11px] font-medium text-gray-500 dark:text-gray-400">
                {presetLabels[userPreset]}
              </span>
            </div>
            <p className="mb-2 text-[11px] leading-relaxed text-gray-500 dark:text-gray-400">
              {t.config.agentPresetUserHint}
            </p>
            {renderPresetButtons('user', userPreset)}
          </div>
          <div className={PRESET_CARD_CLASS}>
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="text-xs font-semibold text-gray-700 dark:text-gray-200">
                {t.config.agentUsageGuest}
              </span>
              <span className="text-[11px] font-medium text-gray-500 dark:text-gray-400">
                {presetLabels[guestPreset]}
              </span>
            </div>
            <p className="mb-2 text-[11px] leading-relaxed text-gray-500 dark:text-gray-400">
              {t.config.agentPresetGuestHint}
            </p>
            {renderPresetButtons('guest', guestPreset)}
          </div>
        </div>
      </SettingGroup>

      {/* 普通用户权限 */}
      <PermissionGroup
        title={t.config.userElevatedPermissions}
        description={t.config.userElevatedPermissionsDesc}
        permissions={permissionItems}
        values={getUserPermValues()}
        onChange={(key, value) =>
          updatePermissionConfig(`user_perm_${key}`, value)
        }
        loading={loading}
      />

      {/* 游客权限 */}
      <PermissionGroup
        title={t.config.guestElevatedPermissions}
        description={t.config.guestElevatedPermissionsDesc}
        permissions={guestPermissionItems}
        values={getGuestPermValues()}
        onChange={(key, value) =>
          updatePermissionConfig(`guest_perm_${key}`, value)
        }
        loading={loading}
      />

      {/* 普通用户 AI 配额 */}
      <QuotaGroup
        title={t.config.userAiQuota}
        description={t.config.userAiQuotaDesc}
        quotas={quotaItems}
        values={getUserQuotaValues()}
        onChange={(key, value) =>
          updatePermissionConfig(`user_ai_${key}`, value)
        }
        loading={loading}
      />

      {/* 游客 AI 配额 */}
      <QuotaGroup
        title={t.config.guestAiQuota}
        description={t.config.guestAiQuotaDesc}
        quotas={quotaItems}
        values={getGuestQuotaValues()}
        onChange={(key, value) =>
          updatePermissionConfig(`guest_ai_${key}`, value)
        }
        loading={loading}
      />
    </SettingSection>
  )
}

export default PermissionsConfigSection
