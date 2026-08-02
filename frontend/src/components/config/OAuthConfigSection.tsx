/**
 * OAuth 配置区块（重构版）
 *
 * 设计：
 * - 右上角页级动作：「添加登录方式」→ preset 选择器
 * - 列表：已配置的 providers 卡片（GitHub 也是其中一种，kind="github"）
 * - 选 preset 后自动填 discovery/scopes/icon，用户只补 client_id/secret
 *
 * 详见 docs/development/OAUTH.md + oauthPresets.ts
 */

import type { OAuthProviderEntry } from '../../utils/oauthSettings'

import {
  FaCheck,
  FaClipboard,
  FaGithub,
  FaPlus,
  FaTrash,
} from '@lib/icons'
import React, { useCallback, useMemo, useState } from 'react'

import { useI18n } from '../../contexts/I18nContext'
import {
  normalizeOAuthIconUrl,
  preloadOAuthIcons,
} from '../../utils/oauthIcons'
import OAuthIconImage from '../OAuthIconImage'
import {
  CheckboxCard,
  InputItem,
  SettingsButton,
  SettingSection,
  SettingTitleGuideEntry,
  SetupFlow,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import { Spinner } from '../Spinner'
import { findPreset, OAUTH_PRESETS } from './oauthPresets'
import { getOAuthSetupGuideForEntry } from './oauthSetupGuides'

interface ConfigField {
  key: string
  value: string
}

interface OAuthConfigSectionProps {
  configFields: ConfigField[]
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
  providers: OAuthProviderEntry[]
  loading?: boolean
  onProvidersChange: (providers: OAuthProviderEntry[]) => void
}

function openPresetPicker(
  setPicker: React.Dispatch<React.SetStateAction<boolean>>,
  iconUrls: Array<string | null | undefined>,
) {
  preloadOAuthIcons(iconUrls)
  setPicker(true)
}

export const OAuthConfigSection: React.FC<OAuthConfigSectionProps> = ({
  configFields,
  title,
  icon,
  description,
  sectionId,
  providers,
  loading = false,
  onProvidersChange,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()

  const getFieldValue = useCallback(
    (key: string) => {
      return configFields.find((f) => f.key === key)?.value || ''
    },
    [configFields],
  )

  const baseUrl = getFieldValue('base_url').replace(/\/$/, '')

  // ---- providers + 开关 ----
  // 选 preset 的弹层状态
  const [picker, setPicker] = useState(false)

  const updateProvider = (idx: number, patch: Partial<OAuthProviderEntry>) => {
    onProvidersChange(
      providers.map((p, i) => (i === idx ? { ...p, ...patch } : p)),
    )
  }
  const removeProvider = (idx: number) => {
    onProvidersChange(providers.filter((_, i) => i !== idx))
  }

  const addFromPreset = (presetId: string) => {
    const preset = findPreset(presetId)
    if (!preset) return
    // 生成唯一 slug
    let slug = preset.defaultSlug || preset.id
    if (slug && providers.some((p) => p.slug === slug)) {
      let n = 2
      while (providers.some((p) => p.slug === `${slug}-${n}`)) n++
      slug = `${slug}-${n}`
    }
    const entry: OAuthProviderEntry = {
      slug,
      kind: preset.kind,
      display_name: preset.display_name,
      enabled: true,
      client_id: '',
      client_secret: '',
      scopes: [...preset.scopes],
      discovery_url: preset.discovery_url || '',
      icon_url: preset.icon_url || null,
    }
    onProvidersChange([...providers, entry])
    setPicker(false)
  }

  // 可用 preset = 全部 - 已用 GitHub 的（一个实例足够）
  const availablePresets = useMemo(() => {
    const hasGithub = providers.some(
      (p) => p.slug === 'github' && p.kind === 'github',
    )
    return OAUTH_PRESETS.filter((p) => !(p.id === 'github' && hasGithub))
  }, [providers])

  const handleOpenPicker = useCallback(() => {
    openPresetPicker(
      setPicker,
      availablePresets.map((preset) => preset.icon_url),
    )
  }, [availablePresets])

  const handleTogglePicker = useCallback(() => {
    if (picker) {
      setPicker(false)
      return
    }
    handleOpenPicker()
  }, [handleOpenPicker, picker])

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      detail={
        !baseUrl ? (
          <>
            <strong>{t.config.callbackUrlNotConfigured}</strong>
            <br />
            {t.config.oauthHowToHint}
            {description ? (
              <>
                <br />
                <br />
                {description}
              </>
            ) : null}
          </>
        ) : undefined
      }
      {...bindGuide('oauth.section', g.oauth.section)}
      detailTone={!baseUrl ? 'warning' : 'default'}
      sectionId={sectionId}
      headerActions={
        <CheckboxCard
          variant="action"
          tone="primary"
          label={t.config.oauthAddLoginMethod}
          description={t.config.oauthPickPreset}
          icon={<FaPlus />}
          checked={picker}
          onChange={() => handleTogglePicker()}
          title={t.config.oauthPickPreset}
          className="setting-section-header-action"
          aria-expanded={picker}
        />
      }
    >
      {/* 登录方式列表（与 SettingSection 标题重复，不再套一层子分类） */}
      <div className="oidc-section">
        {picker && (
          <PresetPicker
            presets={availablePresets}
            onPick={addFromPreset}
            onClose={() => setPicker(false)}
            t={t}
          />
        )}

        {loading && (
          <div className="oidc-loading flex justify-center" role="status">
            <Spinner size="sm" color="primary" />
          </div>
        )}

        {!loading && providers.length === 0 && !picker && (
          <button
            type="button"
            className="oidc-empty oidc-empty-clickable"
            onClick={handleOpenPicker}
          >
            <FaPlus />
            <span>{t.config.oauthProvidersEmpty}</span>
          </button>
        )}

        {providers.map((p, idx) => (
          <ProviderCard
            key={`${p.slug}-${idx}`}
            entry={p}
            baseUrl={baseUrl}
            onChange={(patch) => updateProvider(idx, patch)}
            onRemove={() => removeProvider(idx)}
            t={t}
          />
        ))}
      </div>
    </SettingSection>
  )
}

export default OAuthConfigSection

// ============================================================================
// Subcomponents
// ============================================================================

interface ProviderCardProps {
  entry: OAuthProviderEntry
  baseUrl: string
  onChange: (patch: Partial<OAuthProviderEntry>) => void
  onRemove: () => void
  t: any
}

const ProviderCard: React.FC<ProviderCardProps> = ({
  entry,
  baseUrl,
  onChange,
  onRemove,
  t,
}) => {
  const { catalog: g, bindGuide } = useSettingGuide()
  const providerGuideBinding = bindGuide('oauth.provider', g.oauth.provider)
  const providerGuide = providerGuideBinding.guide
  const [copied, setCopied] = useState(false)
  const [copiedData, setCopiedData] = useState(false)
  const callbackUrl =
    baseUrl && entry.slug
      ? `${baseUrl}/api/auth/oauth/${entry.slug}/callback`
      : null
  const isDiscordProvider =
    entry.slug?.toLowerCase().includes('discord') ||
    entry.display_name?.toLowerCase().includes('discord') ||
    entry.discovery_url?.includes('discord.com')
  const discordDataCallbackUrl =
    baseUrl && isDiscordProvider
      ? `${baseUrl}/api/platforms/discord/oauth/callback`
      : null

  const copy = async () => {
    if (!callbackUrl) return
    await navigator.clipboard.writeText(callbackUrl)
    setCopied(true)
    setTimeout(setCopied, 2000, false)
  }

  const copyDataCallback = async () => {
    if (!discordDataCallbackUrl) return
    await navigator.clipboard.writeText(discordDataCallbackUrl)
    setCopiedData(true)
    setTimeout(setCopiedData, 2000, false)
  }

  const setupGuide = getOAuthSetupGuideForEntry(entry, t.config, {
    hasCallback: Boolean(callbackUrl),
    copyCallback: () => {
      void copy()
    },
  })

  return (
    <div
      data-guide-path="oauth.provider"
      className={`oidc-provider-card has-guide-anchor${entry.enabled ? '' : ' disabled'}`}
    >
      <div className="oidc-provider-header">
        <span className="oidc-provider-title">
          <ProviderIcon entry={entry} />
          <span className="oidc-provider-title-text">
            {entry.display_name || entry.slug || t.config.oidcNewProvider}
            <SettingTitleGuideEntry
              title={
                entry.display_name || entry.slug || t.config.oidcNewProvider
              }
              guide={providerGuide}
            />
          </span>
        </span>
        <div className="oidc-provider-actions">
          <div
            className="oidc-enable-toggle"
            role="presentation"
            onClick={() => onChange({ enabled: !entry.enabled })}
          >
            <span className="oidc-enable-toggle-label">
              {t.config.oidcEnabled}
            </span>
            <ToggleSwitch
              checked={entry.enabled}
              onChange={(checked) => onChange({ enabled: checked })}
              aria-label={t.config.oidcEnabled}
            />
          </div>
          <SettingsButton
            variant="danger"
            size="sm"
            icon={<FaTrash />}
            onClick={onRemove}
            aria-label={t.config.oidcDelete}
          />
        </div>
      </div>

      <SetupFlow
        title={setupGuide.title}
        optionalLabel={setupGuide.optionalLabel}
        steps={setupGuide.steps}
        className="oidc-provider-setup-flow"
      />

      {/* 回调 URL — 用户复制粘到 provider 后台 */}
      {callbackUrl && (
        <div className="oidc-callback-row">
          <span className="oidc-callback-label">
            {t.config.currentCallbackUrl}
          </span>
          <code className="inline-code callback-url-code">{callbackUrl}</code>
          <button
            type="button"
            className="copy-btn"
            onClick={copy}
            title={copied ? t.common.copied : t.common.copy}
          >
            {copied ? <FaCheck /> : <FaClipboard />}
          </button>
        </div>
      )}
      {/* Discord 数据平台一键授权 callback（与登录 callback 分开登记） */}
      {discordDataCallbackUrl && (
        <div className="oidc-callback-row">
          <span className="oidc-callback-label">
            {t.config.discordDataCallbackUrl}
          </span>
          <code className="inline-code callback-url-code">
            {discordDataCallbackUrl}
          </code>
          <button
            type="button"
            className="copy-btn"
            onClick={copyDataCallback}
            title={copiedData ? t.common.copied : t.common.copy}
          >
            {copiedData ? <FaCheck /> : <FaClipboard />}
          </button>
        </div>
      )}

      <div className="oidc-provider-fields">
        {/* 必填：client_id / client_secret */}
        <InputItem
          itemKey={`provider-${entry.slug}-client-id`}
          label={t.config.oidcClientIdLabel}
          required
          value={entry.client_id}
          onChange={(v) => onChange({ client_id: v })}
          placeholder=""
          layout="vertical"
        />
        <InputItem
          itemKey={`provider-${entry.slug}-client-secret`}
          label={t.config.oidcClientSecretLabel}
          required
          value={entry.client_secret}
          onChange={(v) => onChange({ client_secret: v })}
          placeholder={t.config.oidcClientSecretPlaceholder}
          inputType="password"
          autoSelectOnMask
          layout="vertical"
        />

        {/* OIDC 额外字段：discovery_url（必填） */}
        {entry.kind === 'oidc' && (
          <div className="full-width">
            <InputItem
              itemKey={`provider-${entry.slug}-discovery`}
              label={t.config.oidcDiscoveryLabel}
              required
              value={entry.discovery_url || ''}
              onChange={(v) => onChange({ discovery_url: v })}
              placeholder={t.config.oidcDiscoveryPlaceholder}
              layout="vertical"
            />
          </div>
        )}

        {/* 高级（折叠） */}
        <AdvancedFields entry={entry} onChange={onChange} t={t} />
      </div>
    </div>
  )
}

const ProviderIcon: React.FC<{ entry: OAuthProviderEntry }> = ({ entry }) => {
  const iconSrc = normalizeOAuthIconUrl(entry.icon_url)
  if (entry.kind === 'github' || entry.slug === 'github') {
    return <FaGithub className="oidc-provider-icon-img" />
  }
  if (iconSrc) {
    return (
      <OAuthIconImage
        src={iconSrc}
        size={20}
        className="oidc-provider-icon-img"
        fetchPriority="low"
      />
    )
  }
  return (
    <span className="oidc-provider-icon-img oidc-provider-icon-placeholder">
      {(entry.display_name || entry.slug || '?')[0]?.toUpperCase()}
    </span>
  )
}

interface AdvancedFieldsProps {
  entry: OAuthProviderEntry
  onChange: (patch: Partial<OAuthProviderEntry>) => void
  t: any
}

const AdvancedFields: React.FC<AdvancedFieldsProps> = ({
  entry,
  onChange,
  t,
}) => {
  const [open, setOpen] = useState(false)
  return (
    <div className="full-width oidc-advanced">
      <button
        type="button"
        className="oidc-advanced-toggle"
        aria-expanded={open}
        aria-controls={`oidc-advanced-${entry.slug}`}
        onClick={() => setOpen((v) => !v)}
      >
        {open ? '▼' : '▶'} {t.config.oauthAdvanced}
      </button>
      {open && (
        <div
          id={`oidc-advanced-${entry.slug}`}
          className="oidc-advanced-content"
        >
          <InputItem
            itemKey={`provider-${entry.slug}-slug`}
            label={t.config.oidcSlugLabel}
            value={entry.slug}
            onChange={(v) => onChange({ slug: v })}
            placeholder={t.config.oidcSlugPlaceholder}
            layout="vertical"
          />
          <InputItem
            itemKey={`provider-${entry.slug}-display`}
            label={t.config.oidcDisplayNameLabel}
            value={entry.display_name}
            onChange={(v) => onChange({ display_name: v })}
            placeholder={t.config.oidcDisplayNamePlaceholder}
            layout="vertical"
          />
          {entry.kind === 'oidc' && (
            <InputItem
              itemKey={`provider-${entry.slug}-scopes`}
              label={t.config.oidcScopesLabel}
              value={entry.scopes.join(' ')}
              onChange={(v) =>
                onChange({ scopes: v.split(/\s+/).filter(Boolean) })
              }
              placeholder={t.config.oidcScopesPlaceholder}
              layout="vertical"
            />
          )}
          <InputItem
            itemKey={`provider-${entry.slug}-icon`}
            label={t.config.oidcIconLabel}
            value={entry.icon_url || ''}
            onChange={(v) => onChange({ icon_url: v })}
            placeholder={t.config.oidcIconPlaceholder}
            layout="vertical"
          />
        </div>
      )}
    </div>
  )
}

interface PresetPickerProps {
  presets: typeof OAUTH_PRESETS
  onPick: (id: string) => void
  onClose: () => void
  t: any
}

const PresetPicker: React.FC<PresetPickerProps> = ({
  presets,
  onPick,
  onClose,
  t,
}) => {
  return (
    <div className="oidc-preset-picker">
      <div className="oidc-preset-picker-header">
        <span>{t.config.oauthPickPreset}</span>
        <button
          type="button"
          className="oidc-preset-picker-close"
          onClick={onClose}
          aria-label={t.common.close}
        >
          ✕
        </button>
      </div>
      <div className="oidc-preset-grid">
        {presets.map((p) => (
          <button
            key={p.id}
            type="button"
            className="oidc-preset-card"
            onClick={() => onPick(p.id)}
          >
            {p.id === 'github' ? (
              <FaGithub className="oidc-preset-icon" />
            ) : p.icon_url ? (
              <OAuthIconImage
                src={normalizeOAuthIconUrl(p.icon_url) ?? p.icon_url}
                size={20}
                className="oidc-preset-icon"
                fetchPriority="low"
              />
            ) : (
              <span className="oidc-preset-icon oidc-preset-icon-placeholder">
                {p.display_name[0]}
              </span>
            )}
            <span className="oidc-preset-name">{p.display_name}</span>
            {p.kind === 'oidc' && (
              <span className="oidc-preset-badge">OIDC</span>
            )}
          </button>
        ))}
      </div>
    </div>
  )
}
