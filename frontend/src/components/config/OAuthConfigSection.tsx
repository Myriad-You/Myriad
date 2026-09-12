import type { OAuthProviderEntry } from '../../utils/oauthSettings'

import {
  FaCheck,
  FaClipboard,
  FaGithub,
  FaPlus,
  FaTrash,
  LuBookOpen,
  LuChevronDown,
} from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useState } from 'react'

import { useI18n } from '../../contexts/I18nContext'
import {
  normalizeOAuthIconUrl,
  preloadOAuthIcons,
} from '../../utils/oauthIcons'
import OAuthIconImage from '../OAuthIconImage'
import {
  CheckboxCard,
  CollapseRegion,
  guideDomProps,
  InputItem,
  SettingsButton,
  SettingSection,
  SettingTitleGuideEntry,
  SettingTitleTag,
  SetupFlow,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import { Spinner } from '../Spinner'
import {
  entryFromPreset,
  findPreset,
  hasOAuthCredential,
  OAUTH_PRESETS,
} from './oauthPresets'
import {
  getOAuthSetupGuideForEntry,
  resolveOAuthPresetId,
} from './oauthSetupGuides'
import { useAddedCardOpen, useAddedSlug } from './useAddedCard'
import './AiVendorAdd.css'

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

function availableOAuthPresets(providers: OAuthProviderEntry[]) {
  const hasGithub = providers.some(
    (provider) => provider.slug === 'github' && provider.kind === 'github',
  )
  return OAUTH_PRESETS.filter((preset) => !(preset.id === 'github' && hasGithub))
}

export function OAuthAddTrigger({
  providers,
  onProvidersChange,
}: {
  providers: OAuthProviderEntry[]
  onProvidersChange: (providers: OAuthProviderEntry[]) => void
}) {
  const { t } = useI18n()
  const presets = useMemo(() => availableOAuthPresets(providers), [providers])

  const addFromPreset = useCallback(
    (presetId: string) => {
      const preset = findPreset(presetId)
      if (!preset) return
      onProvidersChange([...providers, entryFromPreset(preset, providers)])
    },
    [onProvidersChange, providers],
  )
  const guide = useMemo(
    () => (
      <OAuthPresetGrid
        presets={presets}
        providers={providers}
        onPick={addFromPreset}
      />
    ),
    [addFromPreset, presets, providers],
  )

  return (
    <SettingTitleGuideEntry
      title={t.config.oauthAddLoginMethod}
      requireShowDetails={false}
      className="ai-vendor-add-entry"
      panelClassName="ai-vendor-add-float"
      guide={guide}
      renderTrigger={({ open, closing, toggle, ariaLabel }) => (
        <CheckboxCard
          variant="switch"
          label={t.config.oauthAddLoginMethod}
          description={t.config.oauthPickPreset}
          icon={<FaPlus />}
          showIndicator={false}
          checked={open || closing}
          onChange={() => toggle()}
          title={t.config.oauthPickPreset}
          aria-label={ariaLabel}
          aria-expanded={open}
          className="ai-vendor-add-toggle settings-help-toggle"
        />
      )}
    />
  )
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
  const addedSlug = useAddedSlug(providers.map((provider) => provider.slug))

  const baseUrl = (
    configFields.find((field) => field.key === 'base_url')?.value || ''
  ).replaceAll(/\/$/g, '')

  const updateProvider = (idx: number, patch: Partial<OAuthProviderEntry>) => {
    onProvidersChange(
      providers.map((provider, i) =>
        i === idx ? { ...provider, ...patch } : provider,
      ),
    )
  }

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
      headerBetweenPinned={
        <OAuthAddTrigger
          providers={providers}
          onProvidersChange={onProvidersChange}
        />
      }
    >
      <div className="oidc-section">
        {loading && (
          <div className="oidc-loading flex justify-center" role="status">
            <Spinner size="sm" color="primary" />
          </div>
        )}

        {!loading && providers.length === 0 && (
          <div className="oidc-empty ai-vendor-empty">
            <span>{t.config.oauthProvidersEmpty}</span>
          </div>
        )}

        {providers.map((entry, idx) => (
          <ProviderCard
            key={entry.slug}
            entry={entry}
            baseUrl={baseUrl}
            justAdded={entry.slug === addedSlug}
            onChange={(patch) => updateProvider(idx, patch)}
            onRemove={() =>
              onProvidersChange(providers.filter((_, i) => i !== idx))
            }
          />
        ))}
      </div>
    </SettingSection>
  )
}

export default OAuthConfigSection

function OAuthPresetGrid({
  presets,
  providers,
  onPick,
}: {
  presets: typeof OAUTH_PRESETS
  providers: OAuthProviderEntry[]
  onPick: (id: string) => void
}) {
  const { t } = useI18n()

  useEffect(() => {
    preloadOAuthIcons(presets.map((preset) => preset.icon_url))
  }, [presets])

  return (
    <div className="oidc-preset-grid">
      {presets.map((preset) => {
        const used = providers.some(
          (provider) => resolveOAuthPresetId(provider) === preset.id,
        )
        const kindHint = preset.kind === 'oidc' ? 'OIDC' : 'GitHub'
        const usedHint = used ? t.config.oauthAdded : ''
        return (
          <button
            key={preset.id}
            type="button"
            className={`oidc-preset-card${usedHint ? ' is-used' : ''}`}
            onClick={() => onPick(preset.id)}
            title={[preset.display_name, kindHint, usedHint]
              .filter(Boolean)
              .join(' · ')}
          >
            <OAuthPresetIcon preset={preset} />
            <span className="oidc-preset-name">{preset.display_name}</span>
            <span className="ai-vendor-preset-caps">{kindHint}</span>
            {usedHint ? (
              <span className="ai-vendor-preset-used">{usedHint}</span>
            ) : null}
          </button>
        )
      })}
    </div>
  )
}

function OAuthPresetIcon({ preset }: { preset: (typeof OAUTH_PRESETS)[number] }) {
  if (preset.id === 'github') {
    return <FaGithub className="oidc-preset-icon" />
  }
  if (preset.icon_url) {
    return (
      <OAuthIconImage
        src={normalizeOAuthIconUrl(preset.icon_url) ?? preset.icon_url}
        size={20}
        className="oidc-preset-icon"
        fetchPriority="low"
      />
    )
  }
  return (
    <span className="oidc-preset-icon oidc-preset-icon-placeholder">
      {preset.display_name[0]}
    </span>
  )
}

function ProviderCard({
  entry,
  baseUrl,
  justAdded = false,
  onChange,
  onRemove,
}: {
  entry: OAuthProviderEntry
  baseUrl: string
  justAdded?: boolean
  onChange: (patch: Partial<OAuthProviderEntry>) => void
  onRemove: () => void
}) {
  const { t, format } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const providerGuide = bindGuide('oauth.provider', g.oauth.provider).guide
  const configured = hasOAuthCredential(entry)
  const title =
    entry.display_name || entry.slug || t.config.oidcNewProvider
  const preset = findPreset(resolveOAuthPresetId(entry))
  const [open, setOpen] = useAddedCardOpen(justAdded, !configured)
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

  useEffect(() => {
    if (!copied) return undefined
    const timer = window.setTimeout(setCopied, 2000, false)
    return () => window.clearTimeout(timer)
  }, [copied])

  useEffect(() => {
    if (!copiedData) return undefined
    const timer = window.setTimeout(setCopiedData, 2000, false)
    return () => window.clearTimeout(timer)
  }, [copiedData])

  const copy = async () => {
    if (!callbackUrl) return
    await navigator.clipboard.writeText(callbackUrl)
    setCopied(true)
  }

  const copyDataCallback = async () => {
    if (!discordDataCallbackUrl) return
    await navigator.clipboard.writeText(discordDataCallbackUrl)
    setCopiedData(true)
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
      className={`oidc-provider-card ai-vendor-card has-guide-anchor${
        open ? ' is-open' : ''
      }${entry.enabled ? '' : ' disabled'}${justAdded ? ' is-added' : ''}`}
    >
      <div className="oidc-provider-header ai-vendor-card-header">
        <button
          type="button"
          className="ai-vendor-card-hit"
          onClick={() => setOpen((value) => !value)}
          aria-expanded={open}
          aria-label={format(
            open ? t.config.collapseGroupAria : t.config.expandGroupAria,
            { title },
          )}
        />
        <div className="oidc-provider-title">
          <ProviderIcon entry={entry} />
          <span className="oidc-provider-title-text">{title}</span>
          <span className="ai-vendor-card-tags">
            <span className="ai-vendor-card-control">
              <SettingTitleGuideEntry title={title} guide={providerGuide} />
            </span>
            <SettingTitleTag
              variant="muted"
              className={configured ? undefined : 'ai-vendor-card-status-missing'}
            >
              {configured
                ? t.config.oauthConfigured
                : t.config.oauthCredsMissing}
            </SettingTitleTag>
            {preset?.docs_url ? (
              <span className="ai-vendor-card-control">
                <SettingTitleTag
                  variant="muted"
                  icon={<LuBookOpen />}
                  title={t.config.oauthDocs}
                  onClick={() =>
                    window.open(preset.docs_url, '_blank', 'noopener,noreferrer')
                  }
                >
                  {t.config.oauthDocs}
                </SettingTitleTag>
              </span>
            ) : null}
            <SettingTitleTag variant="muted">
              {open ? t.config.oauthCollapse : t.config.oauthExpand}
            </SettingTitleTag>
          </span>
        </div>
        <div className="oidc-provider-actions">
          <div className="oidc-enable-toggle">
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

      <CollapseRegion open={open}>
        <div className="ai-vendor-card-body">
          {!configured ? (
            <SetupFlow
              title={setupGuide.title}
              optionalLabel={setupGuide.optionalLabel}
              steps={setupGuide.steps}
              className="oidc-provider-setup-flow ai-vendor-setup-flow"
            />
          ) : null}

          {callbackUrl && (
            <div
              className="oidc-callback-row has-guide-anchor"
              {...guideDomProps('oauth.callback')}
            >
              <span className="oidc-callback-label">
                {t.config.currentCallbackUrl}
                <SettingTitleGuideEntry
                  title={t.config.currentCallbackUrl}
                  guide={bindGuide('oauth.callback', g.oauth.callback).guide}
                />
              </span>
              <code className="inline-code callback-url-code">{callbackUrl}</code>
              <button
                type="button"
                className="copy-btn"
                onClick={() => {
                  void copy()
                }}
                title={copied ? t.common.copied : t.common.copy}
              >
                {copied ? <FaCheck /> : <FaClipboard />}
              </button>
            </div>
          )}
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
                onClick={() => {
                  void copyDataCallback()
                }}
                title={copiedData ? t.common.copied : t.common.copy}
              >
                {copiedData ? <FaCheck /> : <FaClipboard />}
              </button>
            </div>
          )}

          <div className="oidc-provider-fields">
            <InputItem
              itemKey={`provider-${entry.slug}-client-id`}
              label={t.config.oidcClientIdLabel}
              required
              {...bindGuide('oauth.clientId', g.oauth.clientId)}
              value={entry.client_id}
              onChange={(value) => onChange({ client_id: value })}
              placeholder=""
              layout="vertical"
            />
            <InputItem
              itemKey={`provider-${entry.slug}-client-secret`}
              label={t.config.oidcClientSecretLabel}
              required
              {...bindGuide('oauth.clientSecret', g.oauth.clientSecret)}
              value={entry.client_secret}
              onChange={(value) => onChange({ client_secret: value })}
              placeholder={t.config.oidcClientSecretPlaceholder}
              inputType="password"
              autoSelectOnMask
              layout="vertical"
            />

            {entry.kind === 'oidc' && (
              <div className="full-width">
                <InputItem
                  itemKey={`provider-${entry.slug}-discovery`}
                  label={t.config.oidcDiscoveryLabel}
                  required
                  {...bindGuide('oauth.discovery', g.oauth.discovery)}
                  value={entry.discovery_url || ''}
                  onChange={(value) => onChange({ discovery_url: value })}
                  placeholder={t.config.oidcDiscoveryPlaceholder}
                  layout="vertical"
                />
              </div>
            )}

            <AdvancedFields entry={entry} onChange={onChange} />
          </div>
        </div>
      </CollapseRegion>
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

function AdvancedFields({
  entry,
  onChange,
}: {
  entry: OAuthProviderEntry
  onChange: (patch: Partial<OAuthProviderEntry>) => void
}) {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [open, setOpen] = useState(false)
  const advancedGuide = bindGuide('oauth.advanced', g.oauth.advanced)
  return (
    <div
      className="full-width oidc-advanced has-guide-anchor"
      {...guideDomProps(advancedGuide.guidePath)}
    >
      <button
        type="button"
        className="oidc-advanced-toggle"
        aria-expanded={open}
        aria-controls={`oidc-advanced-${entry.slug}`}
        onClick={() => setOpen((value) => !value)}
      >
        <LuChevronDown aria-hidden className="oidc-advanced-chevron" />
        {t.config.oauthAdvanced}
      </button>
      <CollapseRegion open={open}>
        <div
          id={`oidc-advanced-${entry.slug}`}
          className="oidc-advanced-content"
        >
          <InputItem
            itemKey={`provider-${entry.slug}-slug`}
            label={t.config.oidcSlugLabel}
            {...advancedGuide}
            value={entry.slug}
            onChange={(value) => onChange({ slug: value })}
            placeholder={t.config.oidcSlugPlaceholder}
            layout="vertical"
          />
          <InputItem
            itemKey={`provider-${entry.slug}-display`}
            label={t.config.oidcDisplayNameLabel}
            value={entry.display_name}
            onChange={(value) => onChange({ display_name: value })}
            placeholder={t.config.oidcDisplayNamePlaceholder}
            layout="vertical"
          />
          {entry.kind === 'oidc' && (
            <InputItem
              itemKey={`provider-${entry.slug}-scopes`}
              label={t.config.oidcScopesLabel}
              value={entry.scopes.join(' ')}
              onChange={(value) =>
                onChange({ scopes: value.split(/\s+/).filter(Boolean) })
              }
              placeholder={t.config.oidcScopesPlaceholder}
              layout="vertical"
            />
          )}
          <InputItem
            itemKey={`provider-${entry.slug}-icon`}
            label={t.config.oidcIconLabel}
            value={entry.icon_url || ''}
            onChange={(value) => onChange({ icon_url: value })}
            placeholder={t.config.oidcIconPlaceholder}
            layout="vertical"
          />
        </div>
      </CollapseRegion>
    </div>
  )
}
