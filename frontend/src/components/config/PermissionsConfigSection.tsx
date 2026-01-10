/**
 * 权限配置区块示例
 * 演示如何使用通用设置组件重构 ConfigForm 中的权限配置部分
 *
 * 重构前：约 400 行代码
 * 重构后：约 80 行代码
 */

import type { PermissionItem, QuotaItem } from '../settings'
import { FaLightbulb } from '@lib/icons'
import React from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  InfoCard,
  PermissionGroup,

  QuotaGroup,

  SettingGroup,
  SettingSection,
} from '../settings'

interface PermissionsConfigSectionProps {
  permissionConfig: {
    user_perm_ai_generate: boolean
    user_perm_ai_analyze: boolean
    user_perm_ai_chat: boolean
    user_perm_report_write: boolean
    user_perm_network_fetch: boolean
    user_perm_media_control: boolean
    user_perm_component_theme: boolean
    user_perm_shortcut_register: boolean
    user_perm_event_publish: boolean
    // 游客权限
    guest_perm_ai_generate: boolean
    guest_perm_ai_analyze: boolean
    guest_perm_ai_chat: boolean
    guest_perm_report_write: boolean
    guest_perm_network_fetch: boolean
    guest_perm_media_control: boolean
    guest_perm_component_theme: boolean
    guest_perm_shortcut_register: boolean
    guest_perm_event_publish: boolean
    // AI 配额
    user_ai_daily_calls: number
    user_ai_daily_tokens: number
    user_ai_cooldown_seconds: number
    guest_ai_daily_calls: number
    guest_ai_daily_tokens: number
    guest_ai_cooldown_seconds: number
  }
  updatePermissionConfig: (key: string, value: boolean | number) => void
  loading?: boolean
  title: string
  icon: React.ReactNode
  description: string
}

export const PermissionsConfigSection: React.FC<PermissionsConfigSectionProps> = ({
  permissionConfig,
  updatePermissionConfig,
  loading = false,
  title,
  icon,
  description,
}) => {
  const { t } = useI18n()

  // 定义权限项列表（复用于用户和游客）
  const permissionItems: PermissionItem[] = [
    { key: 'ai_generate', code: 'ai:generate', label: t.config.permAiGenerate, hint: t.config.permAiGenerateHint },
    { key: 'ai_analyze', code: 'ai:analyze', label: t.config.permAiAnalyze, hint: t.config.permAiAnalyzeHint },
    { key: 'ai_chat', code: 'ai:chat', label: t.config.permAiChat, hint: t.config.permAiChatHint },
    { key: 'report_write', code: 'report:write', label: t.config.permReportWrite, hint: t.config.permReportWriteHint },
    { key: 'network_fetch', code: 'network:fetch', label: t.config.permNetworkFetch, hint: t.config.permNetworkFetchHint },
    { key: 'media_control', code: 'media:control', label: t.config.permMediaControl, hint: t.config.permMediaControlHint },
    { key: 'component_theme', code: 'component:theme', label: t.config.permComponentTheme, hint: t.config.permComponentThemeHint },
    { key: 'shortcut_register', code: 'shortcut:register', label: t.config.permShortcutRegister, hint: t.config.permShortcutRegisterHint },
    { key: 'event_publish', code: 'event:publish', label: t.config.permEventPublish, hint: t.config.permEventPublishHint },
  ]

  // 定义配额项列表
  const quotaItems: QuotaItem[] = [
    { key: 'daily_calls', label: t.config.aiDailyCalls, hint: t.config.aiDailyCallsHint, min: 0, max: 10000 },
    { key: 'daily_tokens', label: t.config.aiDailyTokens, hint: t.config.aiDailyTokensHint, min: 0, max: 1000000 },
    { key: 'cooldown_seconds', label: t.config.aiCooldownSeconds, hint: t.config.aiCooldownSecondsHint, min: 0, max: 3600, unit: '秒' },
  ]

  // 转换权限值（添加前缀）
  const getUserPermValues = () => {
    const values: Record<string, boolean> = {}
    permissionItems.forEach((item) => {
      values[item.key] = permissionConfig[`user_perm_${item.key}` as keyof typeof permissionConfig] as boolean
    })
    return values
  }

  const getGuestPermValues = () => {
    const values: Record<string, boolean> = {}
    permissionItems.forEach((item) => {
      values[item.key] = permissionConfig[`guest_perm_${item.key}` as keyof typeof permissionConfig] as boolean
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

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
    >
      {/* 说明卡片 */}
      <InfoCard
        title={t.config.tappPermissionsInfoTitle}
        icon={<FaLightbulb />}
        content={t.config.tappPermissionsInfo}
        className="info-card-spaced"
      />

      {/* 普通用户权限 */}
      <PermissionGroup
        title={t.config.userElevatedPermissions}
        description={t.config.userElevatedPermissionsDesc}
        permissions={permissionItems}
        values={getUserPermValues()}
        onChange={(key, value) => updatePermissionConfig(`user_perm_${key}`, value)}
        loading={loading}
      />

      {/* 游客权限 */}
      <PermissionGroup
        title={t.config.guestElevatedPermissions}
        description={t.config.guestElevatedPermissionsDesc}
        permissions={permissionItems}
        values={getGuestPermValues()}
        onChange={(key, value) => updatePermissionConfig(`guest_perm_${key}`, value)}
        loading={loading}
      />

      {/* AI 使用配额 */}
      <SettingGroup title={t.config.aiQuotaTitle} description={t.config.aiQuotaDesc}>
        <QuotaGroup
          title={t.config.userAiQuota}
          quotas={quotaItems}
          values={getUserQuotaValues()}
          onChange={(key, value) => updatePermissionConfig(`user_ai_${key}`, value)}
          loading={loading}
        />

        <QuotaGroup
          title={t.config.guestAiQuota}
          quotas={quotaItems}
          values={getGuestQuotaValues()}
          onChange={(key, value) => updatePermissionConfig(`guest_ai_${key}`, value)}
          loading={loading}
        />

        <InfoCard
          icon={<FaLightbulb />}
          content={t.config.aiQuotaAdminNote}
          className="info-card-spaced"
        />
      </SettingGroup>
    </SettingSection>
  )
}

export default PermissionsConfigSection
