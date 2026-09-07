import type { AiVendorCapability, AiVendorSource } from './aiVendorPresets'
import {
  FaPlus,
  FaTrash,
  LuBookOpen,
  SiCloudflare,
  SiGooglegemini,
  SiOpenai,
  SiOpenrouter,
  SiX,
} from '@lib/icons'
import React, { useCallback, useMemo } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  CheckboxCard,
  CollapseRegion,
  GitHubProjectBadge,
  InputItem,
  isGithubRepoUrl,
  SelectItem,
  SettingsButton,
  SettingTitleGuideEntry,
  SettingTitleTag,
  SetupFlow,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import {
  AI_VENDOR_PRESETS,
  findVendorPreset,
  isAgoraSource,
  sourceFromPreset,
} from './aiVendorPresets'
import { useAddedCardOpen, useAddedSlug } from './useAddedCard'
import {
  AnthropicMark,
  AzureMark,
  CohereMark,
  DashscopeMark,
  DeepseekMark,
  FireworksMark,
  GroqMark,
  MinimaxMark,
  MistralMark,
  MoonshotMark,
  NvidiaMark,
  OllamaMark,
  PerplexityMark,
  ShengwangMark,
  SiliconFlowMark,
  TencentCloudMark,
  TogetherMark,
  VolcengineMark,
  ZhipuMark,
} from './vendorIcons'
import { getVendorSetupGuide } from './vendorSetupGuides'
import './AiVendorAdd.css'

export function VendorKindIcon({
  kind,
  slug,
  preset,
}: {
  kind: string
  slug?: string
  preset?: string | null
}) {
  const resolved = findVendorPreset({ kind, slug, preset })
  const iconId = resolved?.id ?? kind
  switch (iconId) {
    case 'openrouter':
      return <SiOpenrouter />
    case 'openai':
    case 'openaiCompatible':
      return <SiOpenai />
    case 'azureOpenAI':
      return <AzureMark />
    case 'gemini':
      return <SiGooglegemini />
    case 'anthropic':
      return <AnthropicMark />
    case 'deepseek':
      return <DeepseekMark />
    case 'volcengine':
      return <VolcengineMark />
    case 'dashscope':
      return <DashscopeMark />
    case 'moonshot':
      return <MoonshotMark />
    case 'zhipu':
      return <ZhipuMark />
    case 'siliconflow':
      return <SiliconFlowMark />
    case 'groq':
      return <GroqMark />
    case 'xai':
      return <SiX />
    case 'mistral':
      return <MistralMark />
    case 'together':
      return <TogetherMark />
    case 'fireworks':
      return <FireworksMark />
    case 'perplexity':
      return <PerplexityMark />
    case 'minimax':
      return <MinimaxMark />
    case 'agora':
      return <ShengwangMark />
    case 'ollama':
      return <OllamaMark />
    case 'cloudflare':
      return <SiCloudflare />
    case 'cohere':
      return <CohereMark />
    case 'nvidia':
      return <NvidiaMark />
    case 'tencentHunyuan':
    case 'tencent':
      return <TencentCloudMark />
    default:
      break
  }
  if (kind === 'openai' || kind === 'openai_compatible') return <SiOpenai />
  if (kind === 'gemini') return <SiGooglegemini />
  if (kind === 'volcengine') return <VolcengineMark />
  if (kind === 'tencent') return <TencentCloudMark />
  if (kind === 'agora') return <ShengwangMark />
  const letter = (resolved?.display_name || slug || kind).trim().charAt(0) || '?'
  return (
    <span className="oidc-preset-icon-placeholder" aria-hidden>
      {letter.toUpperCase()}
    </span>
  )
}

export type VendorUsageId = 'standard' | 'lite' | 'pro' | 'image' | 'speech' | 'realtime'

export type VendorUsageMap = Partial<Record<string, VendorUsageId[]>>

interface AiVendorSourcesProps {
  sources: AiVendorSource[]
  onChange: (sources: AiVendorSource[]) => void
  usages?: VendorUsageMap
}

function usageLabel(id: VendorUsageId, t: ReturnType<typeof useI18n>['t']): string {
  switch (id) {
    case 'lite':
      return t.config.aiVendorUsedLite
    case 'pro':
      return t.config.aiVendorUsedPro
    case 'image':
      return t.config.aiVendorUsedImage
    case 'speech':
      return t.config.aiVendorUsedSpeech
    case 'realtime':
      return t.config.aiVendorUsedRealtime
    default:
      return t.config.aiVendorUsedStandard
  }
}

function usedByText(
  ids: VendorUsageId[] | undefined,
  t: ReturnType<typeof useI18n>['t'],
): string {
  if (!ids?.length) return ''
  return t.config.aiVendorUsedBy.replace(
    '{name}',
    ids.map((id) => usageLabel(id, t)).join(t.config.aiVendorUsedJoin),
  )
}

const CAPABILITY_ORDER: AiVendorCapability[] = ['text', 'image', 'speech', 'realtime']

function capabilityLabel(
  id: AiVendorCapability,
  t: ReturnType<typeof useI18n>['t'],
): string {
  switch (id) {
    case 'image':
      return t.config.aiVendorCapImage
    case 'speech':
      return t.config.aiVendorCapSpeech
    case 'realtime':
      return t.config.aiVendorCapRealtime
    default:
      return t.config.aiVendorCapText
  }
}

function capabilityText(
  caps: AiVendorCapability[],
  t: ReturnType<typeof useI18n>['t'],
): string {
  return CAPABILITY_ORDER.filter((id) => caps.includes(id))
    .map((id) => capabilityLabel(id, t))
    .join(t.config.aiVendorUsedJoin)
}

function useAddVendorSource(
  sources: AiVendorSource[],
  onChange: (sources: AiVendorSource[]) => void,
) {
  return useCallback(
    (presetId: string) => {
      const preset = AI_VENDOR_PRESETS.find((item) => item.id === presetId)
      if (!preset) return
      onChange([...sources, sourceFromPreset(preset, sources)])
    },
    [onChange, sources],
  )
}

export function AiVendorAddTrigger({
  sources,
  onChange,
  usages = {},
}: AiVendorSourcesProps) {
  const { t } = useI18n()
  const addFromPreset = useAddVendorSource(sources, onChange)
  const guide = useMemo(
    () => (
      <div className="oidc-preset-grid">
        {AI_VENDOR_PRESETS.map((preset) => {
          const used = sources
            .filter(
              (source) =>
                source.preset === preset.id ||
                source.slug === preset.defaultSlug,
            )
            .flatMap((source) => usages[source.slug] ?? [])
          const usedHint = usedByText([...new Set(used)], t)
          const capsHint = capabilityText(preset.capabilities, t)
          return (
            <button
              key={preset.id}
              type="button"
              className={`oidc-preset-card${usedHint ? ' is-used' : ''}`}
              onClick={() => addFromPreset(preset.id)}
              title={[preset.display_name, capsHint, usedHint]
                .filter(Boolean)
                .join(' · ')}
            >
              <span className="oidc-preset-icon">
                <VendorKindIcon
                  kind={preset.kind}
                  slug={preset.defaultSlug}
                  preset={preset.id}
                />
              </span>
              <span className="oidc-preset-name">{preset.display_name}</span>
              <span className="ai-vendor-preset-caps">{capsHint}</span>
              {usedHint ? (
                <span className="ai-vendor-preset-used">{usedHint}</span>
              ) : null}
            </button>
          )
        })}
      </div>
    ),
    [addFromPreset, sources, t, usages],
  )

  return (
    <SettingTitleGuideEntry
      title={t.config.aiVendorAdd}
      requireShowDetails={false}
      className="ai-vendor-add-entry"
      panelClassName="ai-vendor-add-float"
      guide={guide}
      renderTrigger={({ open, closing, toggle, ariaLabel }) => (
        <CheckboxCard
          variant="switch"
          label={t.config.aiVendorAdd}
          description={t.config.aiVendorAddDesc}
          icon={<FaPlus />}
          showIndicator={false}
          checked={open || closing}
          onChange={() => toggle()}
          title={t.config.aiVendorAddDesc}
          aria-label={ariaLabel}
          aria-expanded={open}
          className="ai-vendor-add-toggle settings-help-toggle"
        />
      )}
    />
  )
}

export const AiVendorSources: React.FC<AiVendorSourcesProps> = ({
  sources,
  onChange,
  usages = {},
}) => {
  const { t } = useI18n()
  const addedSlug = useAddedSlug(sources.map((source) => source.slug))

  const updateSource = (index: number, patch: Partial<AiVendorSource>) => {
    onChange(sources.map((item, i) => (i === index ? { ...item, ...patch } : item)))
  }

  return (
    <div className="oidc-section">
      {sources.length === 0 && (
        <div className="oidc-empty ai-vendor-empty">
          <span>{t.config.aiVendorEmpty}</span>
        </div>
      )}

      {sources.map((source, index) => (
        <VendorCard
          key={source.slug}
          source={source}
          usedBy={usages[source.slug]}
          justAdded={source.slug === addedSlug}
          onChange={(patch) => updateSource(index, patch)}
          onRemove={() => onChange(sources.filter((_, i) => i !== index))}
        />
      ))}
    </div>
  )
}

function VendorSetupSteps({ source }: { source: AiVendorSource }) {
  const { t } = useI18n()
  const guide = getVendorSetupGuide(source, t.config)
  if (!guide) return null
  return (
    <SetupFlow
      title={guide.title}
      steps={guide.steps}
      className="ai-vendor-setup-flow"
    />
  )
}

function hasVendorCredential(source: AiVendorSource): boolean {
  if (isAgoraSource(source)) {
    return Boolean(
      source.app_id?.trim()
      && source.api_key?.trim()
      && source.secret_id?.trim()
      && source.secret_key?.trim(),
    )
  }
  if (source.kind === 'tencent') {
    return Boolean(source.secret_id?.trim() || source.secret_key?.trim())
  }
  return Boolean(source.api_key?.trim())
}

function VendorCard({
  source,
  usedBy,
  justAdded = false,
  onChange,
  onRemove,
}: {
  source: AiVendorSource
  usedBy?: VendorUsageId[]
  justAdded?: boolean
  onChange: (patch: Partial<AiVendorSource>) => void
  onRemove: () => void
}) {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const preset = findVendorPreset(source)
  const configured = hasVendorCredential(source)
  const title = source.display_name || source.slug
  const usedHint = usedByText(usedBy, t)
  const [open, setOpen] = useAddedCardOpen(justAdded, !configured)
  const regionOptions = useMemo(
    () => [
      { value: 'ap-guangzhou', label: t.config.tencentRegionGuangzhou },
      { value: 'ap-shanghai', label: t.config.tencentRegionShanghai },
      { value: 'ap-beijing', label: t.config.tencentRegionBeijing },
      { value: 'ap-chengdu', label: t.config.tencentRegionChengdu },
      { value: 'ap-chongqing', label: t.config.tencentRegionChongqing },
      { value: 'ap-nanjing', label: t.config.tencentRegionNanjing },
    ],
    [t.config],
  )

  const toggleOpen = () => setOpen((value) => !value)

  return (
    <div
      className={`oidc-provider-card ai-vendor-card${open ? ' is-open' : ''}${
        source.enabled ? '' : ' disabled'
      }${justAdded ? ' is-added' : ''}`}
    >
      <div className="oidc-provider-header ai-vendor-card-header">
        <button
          type="button"
          className="ai-vendor-card-hit"
          onClick={toggleOpen}
          aria-expanded={open}
          aria-label={(open
            ? t.config.collapseGroupAria
            : t.config.expandGroupAria
          ).replace('{title}', title)}
        />
        <div className="oidc-provider-title">
          <span className="oidc-provider-icon-img" aria-hidden>
            <VendorKindIcon
              kind={source.kind}
              slug={source.slug}
              preset={source.preset}
            />
          </span>
          <span className="oidc-provider-title-text">{title}</span>
          <span className="ai-vendor-card-tags">
            <SettingTitleTag
              variant="muted"
              className={configured ? undefined : 'ai-vendor-card-status-missing'}
            >
              {configured
                ? t.config.aiVendorConfigured
                : t.config.aiVendorKeyMissing}
            </SettingTitleTag>
            {usedHint ? (
              <SettingTitleTag variant="muted">{usedHint}</SettingTitleTag>
            ) : null}
            {preset?.docs_url ? (
              <span className="ai-vendor-card-control">
                {isGithubRepoUrl(preset.docs_url) ? (
                  <GitHubProjectBadge
                    url={preset.docs_url}
                    name={preset.display_name}
                  />
                ) : (
                  <SettingTitleTag
                    variant="muted"
                    icon={<LuBookOpen />}
                    title={t.config.aiVendorDocs}
                    onClick={() =>
                      window.open(
                        preset.docs_url,
                        '_blank',
                        'noopener,noreferrer',
                      )
                    }
                  >
                    {t.config.aiVendorDocs}
                  </SettingTitleTag>
                )}
              </span>
            ) : null}
            <SettingTitleTag variant="muted">
              {open ? t.config.aiVendorCollapse : t.config.aiVendorExpand}
            </SettingTitleTag>
          </span>
        </div>
        <div className="oidc-provider-actions">
          <div className="oidc-enable-toggle">
            <ToggleSwitch
              checked={source.enabled}
              onChange={(checked) => onChange({ enabled: checked })}
              aria-label={t.common.enabled}
            />
          </div>
          <SettingsButton
            variant="danger"
            size="sm"
            icon={<FaTrash />}
            onClick={onRemove}
            aria-label={t.common.delete}
          />
        </div>
      </div>

      <CollapseRegion open={open}>
        <div className="ai-vendor-card-body">
          {!configured ? (
            <VendorSetupSteps source={source} />
          ) : null}
          <InputItem
            itemKey={`${source.slug}-name`}
            label={t.config.aiVendorDisplayName}
            value={source.display_name}
            onChange={(value) => onChange({ display_name: value })}
            layout="vertical"
          />

          {source.kind === 'tencent' ? (
            <>
              <InputItem
                itemKey={`${source.slug}-sid`}
                label={t.config.tencentSecretId}
                {...bindGuide('ai.apiKey', g.ai.apiKey)}
                value={source.secret_id || ''}
                onChange={(value) => onChange({ secret_id: value })}
                placeholder={t.config.tencentSecretIdPlaceholder}
                inputType="password"
                autoSelectOnMask
                layout="vertical"
              />
              <InputItem
                itemKey={`${source.slug}-skey`}
                label={t.config.tencentSecretKey}
                {...bindGuide('ai.apiKey', g.ai.apiKey)}
                value={source.secret_key || ''}
                onChange={(value) => onChange({ secret_key: value })}
                placeholder={t.config.tencentSecretKeyPlaceholder}
                inputType="password"
                autoSelectOnMask
                layout="vertical"
              />
              <SelectItem
                itemKey={`${source.slug}-region`}
                label={t.config.tencentRegion}
                value={source.region || 'ap-guangzhou'}
                onChange={(value) => onChange({ region: value })}
                options={regionOptions}
                layout="vertical"
              />
            </>
          ) : isAgoraSource(source) ? (
            <>
              <InputItem
                itemKey={`${source.slug}-app-id`}
                label={t.config.agoraAppId}
                value={source.app_id || ''}
                onChange={(value) => onChange({ app_id: value })}
                placeholder="App ID"
                inputType="text"
                layout="vertical"
              />
              <InputItem
                itemKey={`${source.slug}-cert`}
                label={t.config.agoraAppCertificate}
                {...bindGuide('ai.apiKey', g.ai.apiKey)}
                value={source.api_key || ''}
                onChange={(value) => onChange({ api_key: value })}
                placeholder=""
                inputType="password"
                autoSelectOnMask
                layout="vertical"
              />
              <InputItem
                itemKey={`${source.slug}-cid`}
                label={t.config.agoraCustomerId}
                {...bindGuide('ai.apiKey', g.ai.apiKey)}
                value={source.secret_id || ''}
                onChange={(value) => onChange({ secret_id: value })}
                placeholder=""
                inputType="password"
                autoSelectOnMask
                layout="vertical"
              />
              <InputItem
                itemKey={`${source.slug}-csec`}
                label={t.config.agoraCustomerSecret}
                {...bindGuide('ai.apiKey', g.ai.apiKey)}
                value={source.secret_key || ''}
                onChange={(value) => onChange({ secret_key: value })}
                placeholder=""
                inputType="password"
                autoSelectOnMask
                layout="vertical"
              />
              <InputItem
                itemKey={`${source.slug}-base`}
                label={t.config.agoraApiBase}
                {...bindGuide('ai.baseUrl', g.ai.baseUrl)}
                value={source.base_url || ''}
                onChange={(value) => onChange({ base_url: value })}
                placeholder="https://api.agora.io/cn"
                inputType="text"
                layout="vertical"
              />
              <p className="setting-hint">{t.config.agoraConvoHint}</p>
            </>
          ) : (
            <>
              <InputItem
                itemKey={`${source.slug}-key`}
                label={t.config.aiVendorApiKey}
                {...bindGuide('ai.apiKey', g.ai.apiKey)}
                value={source.api_key || ''}
                onChange={(value) => onChange({ api_key: value })}
                placeholder={
                  preset?.keyPlaceholder ||
                  (source.kind === 'openrouter'
                    ? 'sk-or-v1-...'
                    : source.kind === 'gemini'
                      ? 'AIza...'
                      : 'sk-...')
                }
                inputType="password"
                autoSelectOnMask
                layout="vertical"
              />
              {source.kind !== 'gemini' && !(preset?.base_url || '').trim() && (
                <InputItem
                  itemKey={`${source.slug}-base`}
                  label={t.config.openaiBaseUrlLabel}
                  {...bindGuide('ai.baseUrl', g.ai.baseUrl)}
                  value={source.base_url || ''}
                  onChange={(value) => onChange({ base_url: value })}
                  placeholder={
                    preset?.id === 'openaiCompatible'
                      ? 'https://api.example.com/v1'
                      : t.config.openaiBaseUrlLabel
                  }
                  layout="vertical"
                />
              )}
            </>
          )}
        </div>
      </CollapseRegion>
    </div>
  )
}
