import type { SettingOption } from '../settings/types'
import type { VendorUsageId, VendorUsageMap } from './AiVendorSources'

import {
  FaMicrophone,
  FaVolumeUp,
  LuPalette,
  LuSearch,
  LuSparkles,
  LuStore,
  SiGooglegemini,
  SiOpenai,
  SiOpenrouter,
} from '@lib/icons'
import React, { useCallback, useMemo, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { showStickyToast, showToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import {
  guideDomProps,
  InputItem,
  ProviderItem,
  SettingGroup,
  SettingSection,
  SettingTitleGuideEntry,
  SettingTitleTag,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import {
  parseVendorSources,
  resolveUsedVendorSlug,
  speechProviderKindFromSource,
  vendorSupports,
} from './aiVendorPresets'
import {
  AiVendorAddTrigger,
  AiVendorSources,
  VendorKindIcon,
} from './AiVendorSources'
import { liteInUse } from './form/liteTier'
import { TencentCloudMark, VolcengineMark } from './vendorIcons'

interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

function notifyConfigAction(message: string, ok: boolean, replaceKey: string) {
  if (ok) {
    showToast({ message, type: 'success', replaceKey })
    return
  }
  showStickyToast({ message, type: 'error', replaceKey })
}

interface AiConfigSectionProps {
  configFields: ConfigField[]
  updateValue: (key: string, value: string) => void
  onSpeechTest: () => Promise<{ success: boolean; message: string }>
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
}

interface ModelTierGroupProps {
  title: string
  description: React.ReactNode
  providerItemKey: string
  providerLabel: string
  provider: string
  providerOptions: SettingOption<string>[]
  providerHint?: string
  /** No sources to choose from: only the model fields. */
  fields: ConfigField[]
  enabled?: boolean
  toggle?: {
    checked: boolean
    onChange: (checked: boolean) => void
    ariaLabel: string
    title?: string
  }
  onProviderChange: (provider: string) => void
  updateValue: (key: string, value: string) => void
}

const ModelTierGroup: React.FC<
  ModelTierGroupProps & {
    providerGuide?: React.ReactNode
    providerGuidePath?: string
    fieldGuideFor?: (
      fieldKey: string,
    ) => { guide?: React.ReactNode; guidePath?: string } | undefined
    guide?: React.ReactNode
    guidePath?: string
    enableGuide?: React.ReactNode
    enableGuidePath?: string
  }
> = ({
  title,
  description,
  providerGuide,
  providerGuidePath,
  fieldGuideFor,
  providerItemKey,
  providerLabel,
  provider,
  providerOptions,
  providerHint,
  fields,
  enabled = true,
  toggle,
  onProviderChange,
  updateValue,
  guide,
  guidePath,
  enableGuide,
  enableGuidePath,
}) => {
  const { t } = useI18n()
  return (
  <div
    className={`ai-llm-tier${guidePath ? ' has-guide-anchor' : ''}`}
    {...guideDomProps(guidePath)}
  >
    <div className="ai-llm-tier-head">
      <div className="ai-llm-tier-copy">
        <h3 className="ai-llm-tier-title">
          {title}
          <SettingTitleGuideEntry title={title} guide={guide} />
        </h3>
        {description ? (
          <p className="ai-llm-tier-desc">{description}</p>
        ) : null}
      </div>
      {toggle ? (
        <div
          className={`ai-llm-tier-switch${enableGuidePath ? ' has-guide-anchor' : ''}`}
          {...guideDomProps(enableGuidePath)}
        >
          {enableGuide ? (
            <SettingTitleGuideEntry
              title={toggle.ariaLabel}
              guide={enableGuide}
            />
          ) : null}
          <ToggleSwitch
            checked={toggle.checked}
            onChange={toggle.onChange}
            aria-label={toggle.ariaLabel}
            title={toggle.title}
          />
        </div>
      ) : null}
    </div>
    {enabled && (
      <>
        <ProviderItem
          itemKey={providerItemKey}
          label={providerLabel}
          value={provider}
          onChange={onProviderChange}
          options={providerOptions}
          hint={providerHint}
          guide={providerGuide}
          guidePath={providerGuidePath}
          layout="horizontal"
        />
        {fields.map((field) => {
          const fieldGuide = fieldGuideFor?.(field.key)
          return (
            <InputItem
              key={field.key}
              itemKey={field.key}
              label={
                field.key === 'aux_judge_model'
                  ? t.config.aiAuxJudgeModelLabel
                  : field.key === 'aux_embedding_model'
                    ? t.config.aiAuxEmbeddingModelLabel
                    : t.config.openaiModelLabel
              }
              required={field.required}
              value={field.value}
              onChange={(value) => updateValue(field.key, value)}
              guide={fieldGuide?.guide}
              guidePath={fieldGuide?.guidePath}
              placeholder={
                field.key === 'aux_judge_model'
                  ? t.config.aiAuxJudgeModelPlaceholder
                  : field.key === 'aux_embedding_model'
                    ? t.config.aiAuxEmbeddingModelPlaceholder
                    : field.key === 'lite_ai_model'
                      ? t.config.aiLiteModelPlaceholder
                      : field.placeholder
              }
              inputType={field.field_type as 'text' | 'password'}
              autoSelectOnMask
              layout="vertical"
            />
          )
        })}
      </>
    )}
  </div>
  )
}

/** The fields with these keys, in this order. */
function fieldsByKey(configFields: ConfigField[], keys: string[]): ConfigField[] {
  return keys.flatMap((key) => {
    const field = configFields.find((item) => item.key === key)
    return field ? [field] : []
  })
}

export const AiConfigSection: React.FC<AiConfigSectionProps> = ({
  configFields,
  updateValue,
  onSpeechTest,
  title,
  icon,
  description,
  sectionId,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [speechTesting, setSpeechTesting] = useState(false)

  const fieldGuideFor = useCallback(
    (fieldKey: string) => {
      if (fieldKey.includes('api_key') || fieldKey.includes('secret')) {
        return bindGuide('ai.apiKey', g.ai.apiKey)
      }
      if (fieldKey.includes('base_url')) {
        return bindGuide('ai.baseUrl', g.ai.baseUrl)
      }
      if (fieldKey.includes('model')) {
        return bindGuide('ai.model', g.ai.model)
      }
      return bindGuide('ai.provider', g.ai.provider)
    },
    [g, bindGuide],
  )

  const providerGuideBinding = bindGuide('ai.provider', g.ai.provider)
  const [speechTestResult, setSpeechTestResult] = useState<{
    success: boolean
    message: string
  } | null>(null)

  const getFieldValue = useCallback(
    (key: string, defaultValue = '') => {
      return configFields.find((f) => f.key === key)?.value || defaultValue
    },
    [configFields],
  )

  const vendorSources = useMemo(
    () => parseVendorSources(getFieldValue('ai_vendor_sources')),
    [getFieldValue],
  )

  const setVendorSources = useCallback(
    (next: ReturnType<typeof parseVendorSources>) => {
      updateValue('ai_vendor_sources', JSON.stringify(next))
    },
    [updateValue],
  )

  const sourceOptions = useCallback(
    (capability: 'text' | 'image' | 'speech') => {
      const enabled = vendorSources.filter(
        (source) => source.enabled && vendorSupports(source, capability),
      )
      if (enabled.length === 0) return null
      return enabled.map((source) => ({
        value: source.slug,
        label: source.display_name || source.slug,
        icon: (
          <VendorKindIcon
            kind={source.kind}
            slug={source.slug}
            preset={source.preset}
          />
        ),
      }))
    },
    [vendorSources],
  )

  const textSourceOptions = sourceOptions('text')
  const imageSourceOptions = sourceOptions('image')
  const speechSourceOptions = sourceOptions('speech')

  const liteEnabled = useMemo(() => liteInUse(getFieldValue), [getFieldValue])

  const proEnabled = useMemo(() => {
    const val = getFieldValue('pro_enabled', 'false')
    return val === 'true' || val === '1'
  }, [getFieldValue])

  // Images and speech: a source each (the server shows the one in effect);
  // its kind picks the placeholders and which fields speech shows.
  const currentImageProvider = useMemo(() => {
    const slug = getFieldValue('ai_image_source', 'openrouter')
    const kind = vendorSources.find((item) => item.slug === slug)?.kind || slug
    return kind === 'volcengine' || kind === 'openrouter' || kind === 'gemini'
      ? kind
      : 'openai'
  }, [getFieldValue, vendorSources])

  const currentSpeechProvider = useMemo(() => {
    const slug = getFieldValue('speech_source', 'tencent')
    const source = vendorSources.find((item) => item.slug === slug)
    return speechProviderKindFromSource(source, source?.kind || slug)
  }, [getFieldValue, vendorSources])

  const vendorUsages = useMemo(() => {
    const map: VendorUsageMap = {}
    const add = (raw: string, id: VendorUsageId) => {
      const slug = resolveUsedVendorSlug(raw, vendorSources)
      if (!slug) return
      const next = map[slug] ?? []
      if (!next.includes(id)) next.push(id)
      map[slug] = next
    }
    add(getFieldValue('ai_source'), 'standard')
    if (liteEnabled) add(getFieldValue('lite_ai_source'), 'lite')
    if (proEnabled) add(getFieldValue('pro_ai_source'), 'pro')
    if (getFieldValue('aux_judge_model') || getFieldValue('aux_embedding_model')) {
      add(getFieldValue('aux_ai_source'), 'aux')
    }
    add(getFieldValue('ai_image_source'), 'image')
    add(getFieldValue('speech_source'), 'speech')
    for (const source of vendorSources) {
      if (source.enabled && vendorSupports(source, 'realtime')) {
        add(source.slug, 'realtime')
      }
    }
    return map
  }, [
    getFieldValue,
    liteEnabled,
    proEnabled,
    vendorSources,
  ])

  const speechProviderOptions: SettingOption<string>[] = useMemo(
    () => [
      {
        value: 'openrouter',
        label: t.config.providerOpenRouter,
        icon: <SiOpenrouter />,
      },
      {
        value: 'openai',
        label: t.config.openaiCompatible,
        icon: <SiOpenai />,
      },
      {
        value: 'tencent',
        label: t.config.speechProviderTencent,
        icon: <TencentCloudMark />,
      },
      {
        value: 'gemini',
        label: t.config.providerGemini,
        icon: <SiGooglegemini />,
      },
    ],
    [
      t.config.openaiCompatible,
      t.config.providerGemini,
      t.config.providerOpenRouter,
      t.config.speechProviderTencent,
    ],
  )

  const aiProviderOptions: SettingOption<string>[] = useMemo(
    () => [
      {
        value: 'openrouter',
        label: t.config.providerOpenRouter,
        icon: <SiOpenrouter />,
      },
      { value: 'openai', label: t.config.openaiCompatible, icon: <SiOpenai /> },
      {
        value: 'gemini',
        label: t.config.providerGemini,
        icon: <SiGooglegemini />,
      },
    ],
    [
      t.config.openaiCompatible,
      t.config.providerGemini,
      t.config.providerOpenRouter,
    ],
  )

  const imageProviderOptions: SettingOption<string>[] = useMemo(
    () => [
      {
        value: 'openrouter',
        label: t.config.providerOpenRouter,
        icon: <SiOpenrouter />,
      },
      {
        value: 'openai',
        label: t.config.openaiCompatible,
        icon: <SiOpenai />,
      },
      {
        value: 'volcengine',
        label: t.config.providerVolcengine,
        icon: <VolcengineMark />,
      },
      {
        value: 'gemini',
        label: t.config.providerGemini,
        icon: <SiGooglegemini />,
      },
    ],
    [
      t.config.openaiCompatible,
      t.config.providerGemini,
      t.config.providerOpenRouter,
      t.config.providerVolcengine,
    ],
  )

  // Each tier: a source (the server shows the one in effect) and one model.
  const providerFields = useMemo(
    () => fieldsByKey(configFields, ['ai_model']),
    [configFields],
  )
  const liteProviderFields = useMemo(
    () => fieldsByKey(configFields, ['lite_ai_model']),
    [configFields],
  )
  const proProviderFields = useMemo(
    () => fieldsByKey(configFields, ['pro_ai_model']),
    [configFields],
  )
  const auxFields = useMemo(
    () => fieldsByKey(configFields, ['aux_judge_model', 'aux_embedding_model']),
    [configFields],
  )
  const textTierOptions = textSourceOptions ?? aiProviderOptions

  const handleSpeechTest = useCallback(async () => {
    setSpeechTesting(true)
    setSpeechTestResult(null)
    try {
      const result = await onSpeechTest()
      setSpeechTestResult(result)
      notifyConfigAction(result.message, result.success, 'config-speech-test')
    } catch (error) {
      const message = userFacingError(error, t.config.speechTestFailed)
      setSpeechTestResult({
        success: false,
        message,
      })
      notifyConfigAction(message, false, 'config-speech-test')
    } finally {
      setSpeechTesting(false)
    }
  }, [onSpeechTest, t.config.speechTestFailed])

  return (
    <SettingSection
      sectionId={sectionId}
      title={title}
      icon={icon}
      description={description}
      headerBetweenPinned={
        <AiVendorAddTrigger
          sources={vendorSources}
          onChange={setVendorSources}
          usages={vendorUsages}
        />
      }
    >
      <div className="ai-pane">
      <SettingGroup
        title={t.config.aiVendorsTitle}
        icon={<LuStore />}
        description={t.config.aiVendorsDesc}
        {...bindGuide('ai.vendors', g.ai.vendors)}
      >
        <AiVendorSources
          sources={vendorSources}
          onChange={setVendorSources}
          usages={vendorUsages}
          sharedKeyValues={{
            openai: getFieldValue('provider_openai_api_key'),
            openrouter: getFieldValue('provider_openrouter_api_key'),
            gemini: getFieldValue('provider_gemini_api_key'),
            volcengine: getFieldValue('provider_volcengine_api_key'),
          }}
          onSharedKeyChange={(keyRef, value) => updateValue(`provider_${keyRef}_api_key`, value)}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.aiLlmTitle}
        icon={<LuSparkles />}
        description={t.config.aiLlmDesc}
        {...bindGuide('ai.llm', g.ai.llm)}
      >
        <ModelTierGroup
          title={t.config.aiStandardModelTitle}
          description={t.config.aiStandardModelDesc}
          {...bindGuide('ai.standard', g.ai.standard)}
          providerGuide={providerGuideBinding.guide}
          providerGuidePath={providerGuideBinding.guidePath}
          fieldGuideFor={fieldGuideFor}
          providerItemKey="ai_source"
          providerLabel={t.config.aiProvider}
          provider={getFieldValue('ai_source')}
          providerOptions={textTierOptions}
          providerHint={t.config.aiProviderHint}
          fields={providerFields}
          onProviderChange={(slug) => updateValue('ai_source', slug)}
          updateValue={updateValue}
        />

        <ModelTierGroup
          title={t.config.aiLiteModelTitle}
          description={t.config.aiLiteModelDesc}
          {...bindGuide('ai.lite', g.ai.lite)}
          providerGuide={providerGuideBinding.guide}
          providerGuidePath={providerGuideBinding.guidePath}
          fieldGuideFor={fieldGuideFor}
          providerItemKey="lite_ai_source"
          providerLabel={t.config.aiProvider}
          provider={getFieldValue('lite_ai_source')}
          providerOptions={textTierOptions}
          providerHint={t.config.aiLiteProviderHint}
          fields={liteProviderFields}
          onProviderChange={(slug) => updateValue('lite_ai_source', slug)}
          updateValue={updateValue}
        />

        <ModelTierGroup
          title={t.config.aiProModelTitle}
          description={t.config.aiProModelDesc}
          {...bindGuide('ai.pro', g.ai.pro)}
          enableGuide={bindGuide('ai.proEnable', g.ai.proEnable).guide}
          enableGuidePath="ai.proEnable"
          providerGuide={providerGuideBinding.guide}
          providerGuidePath={providerGuideBinding.guidePath}
          fieldGuideFor={fieldGuideFor}
          providerItemKey="pro_ai_source"
          providerLabel={t.config.aiProvider}
          provider={getFieldValue('pro_ai_source')}
          providerOptions={textTierOptions}
          providerHint={t.config.aiProProviderHint}
          fields={proProviderFields}
          enabled={proEnabled}
          toggle={{
            checked: proEnabled,
            onChange: (value) =>
              updateValue('pro_enabled', value ? 'true' : 'false'),
            ariaLabel: t.config.aiProEnable,
            title: t.config.aiProEnableDesc,
          }}
          onProviderChange={(slug) => updateValue('pro_ai_source', slug)}
          updateValue={updateValue}
        />

        <ModelTierGroup
          title={t.config.aiAuxModelTitle}
          description={t.config.aiAuxModelDesc}
          {...bindGuide('ai.aux', g.ai.aux)}
          providerGuide={providerGuideBinding.guide}
          providerGuidePath={providerGuideBinding.guidePath}
          fieldGuideFor={fieldGuideFor}
          providerItemKey="aux_ai_source"
          providerLabel={t.config.aiProvider}
          provider={getFieldValue('aux_ai_source')}
          providerOptions={textTierOptions}
          providerHint={t.config.aiAuxProviderHint}
          fields={auxFields}
          onProviderChange={(slug) => updateValue('aux_ai_source', slug)}
          updateValue={updateValue}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.webSearchTitle}
        icon={<LuSearch />}
        description={t.config.webSearchDesc}
        {...bindGuide('ai.webSearch', g.ai.webSearch)}
      >
        <InputItem
          itemKey="provider_tinyfish_api_key"
          label={t.config.tinyfishApiKey}
          required={false}
          value={getFieldValue('provider_tinyfish_api_key')}
          onChange={(value) => updateValue('provider_tinyfish_api_key', value)}
          placeholder={t.config.tinyfishApiKeyPlaceholder}
          hint={t.config.tinyfishApiKeyHint}
          inputType="password"
          autoSelectOnMask
          layout="vertical"
          {...fieldGuideFor('provider_tinyfish_api_key')}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.aiImageTitle}
        icon={<LuPalette />}
        description={t.config.aiImageDesc}
        {...bindGuide('ai.image', g.ai.image)}
      >
        <ProviderItem
          itemKey="ai_image_source"
          label={t.config.aiProvider}
          {...bindGuide('ai.provider', g.ai.provider)}
          value={getFieldValue('ai_image_source')}
          onChange={(slug) => updateValue('ai_image_source', slug)}
          options={imageSourceOptions ?? imageProviderOptions}
          layout="horizontal"
        />

        <InputItem
          itemKey="ai_image_model"
          label={t.config.openaiModelLabel}
          {...bindGuide('ai.imageModel', g.ai.imageModel)}
          value={getFieldValue('ai_image_model')}
          onChange={(v) => updateValue('ai_image_model', v)}
          placeholder={
            currentImageProvider === 'openai'
              ? 'gpt-image-2'
              : currentImageProvider === 'volcengine'
                ? 'doubao-seedream-5-0-pro-260628'
                : currentImageProvider === 'gemini'
                  ? 'gemini-3.1-flash-image'
                  : 'openai/gpt-image-2.5-sunburst'
          }
          inputType="text"
          layout="vertical"
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.speechServiceTitle}
        icon={<FaMicrophone />}
        description={t.config.speechServiceDesc}
        {...bindGuide('ai.speech', g.ai.speech)}
        titleExtra={
          <SettingTitleTag
            variant={
              speechTestResult && !speechTestResult.success ? 'danger' : 'muted'
            }
            icon={<FaVolumeUp />}
            onClick={() => void handleSpeechTest()}
            disabled={speechTesting}
            title={
              speechTestResult?.message || t.config.speechTestAvailability
            }
          >
            {speechTesting
              ? t.config.speechTestAvailability
              : t.config.speechTestTag}
          </SettingTitleTag>
        }
      >
        <ProviderItem
          itemKey="speech_source"
          label={t.config.speechProvider}
          {...bindGuide('ai.provider', g.ai.provider)}
          value={getFieldValue('speech_source')}
          onChange={(slug) => updateValue('speech_source', slug)}
          options={speechSourceOptions ?? speechProviderOptions}
          layout="horizontal"
        />

        {(() => {
          const selected = currentSpeechProvider
          if (selected === 'minimax') {
            return (
              <>
                <InputItem
                  itemKey="speech_tts_model"
                  label={t.config.speechTtsModel}
                  {...bindGuide('ai.speechTts', g.ai.speechTts)}
                  value={getFieldValue('speech_tts_model')}
                  onChange={(v) => updateValue('speech_tts_model', v)}
                  placeholder="speech-2.8-turbo"
                  inputType="text"
                  layout="vertical"
                />
                <InputItem
                  itemKey="speech_tts_voice"
                  label={t.config.speechTtsVoice}
                  {...bindGuide('ai.speechVoice', g.ai.speechVoice)}
                  value={getFieldValue('speech_tts_voice')}
                  onChange={(v) => updateValue('speech_tts_voice', v)}
                  placeholder="female-shaonv"
                  inputType="text"
                  layout="vertical"
                />
                <p className="setting-hint">{t.config.speechMinimaxAsrHint}</p>
              </>
            )
          }
          if (
            selected === 'openai' ||
            selected === 'openrouter' ||
            selected === 'openai_compatible' ||
            selected === 'gemini'
          ) {
            return (
              <>
                <InputItem
                  itemKey="speech_stt_model"
                  label={t.config.speechSttModel}
                  {...bindGuide('ai.speechStt', g.ai.speechStt)}
                  value={getFieldValue('speech_stt_model')}
                  onChange={(v) => updateValue('speech_stt_model', v)}
                  placeholder={
                    currentSpeechProvider === 'openrouter'
                      ? 'openai/gpt-transcribe'
                      : currentSpeechProvider === 'gemini'
                        ? 'gemini-3.8-flash'
                        : 'gpt-transcribe'
                  }
                  inputType="text"
                  layout="vertical"
                />
                <InputItem
                  itemKey="speech_tts_model"
                  label={t.config.speechTtsModel}
                  {...bindGuide('ai.speechTts', g.ai.speechTts)}
                  value={getFieldValue('speech_tts_model')}
                  onChange={(v) => updateValue('speech_tts_model', v)}
                  placeholder={
                    currentSpeechProvider === 'openai'
                      ? 'gpt-4o-mini-tts'
                      : currentSpeechProvider === 'gemini'
                        ? 'gemini-3.8-flash-tts'
                        : ''
                  }
                  inputType="text"
                  layout="vertical"
                />
                <InputItem
                  itemKey="speech_tts_voice"
                  label={t.config.speechTtsVoice}
                  {...bindGuide('ai.speechVoice', g.ai.speechVoice)}
                  value={getFieldValue('speech_tts_voice')}
                  onChange={(v) => updateValue('speech_tts_voice', v)}
                  placeholder={
                    currentSpeechProvider === 'gemini' ? 'Kore' : 'marin'
                  }
                  inputType="text"
                  layout="vertical"
                />
                {currentSpeechProvider === 'openrouter' ? (
                  <p className="setting-hint">
                    {t.config.speechOpenRouterTtsHint}
                  </p>
                ) : null}
              </>
            )
          }
          return null
        })()}
      </SettingGroup>
      </div>
    </SettingSection>
  )
}

export default AiConfigSection
