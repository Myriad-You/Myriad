/**
 * AI 配置区块
 * 使用通用设置组件重构
 */

import type { OnboardingPageChrome } from '../agent/onboarding/onboardingTypes'
import type { SettingOption } from '../settings/types'
import type { VendorUsageId, VendorUsageMap } from './AiVendorSources'
import {
  FaMicrophone,
  FaVolumeUp,
  LuChevronLeft,
  LuNotebookPen,
  LuPalette,
  LuRefreshCw,
  LuSearch,
  LuSparkles,
  LuStore,
  SiGooglegemini,
  SiOpenai,
  SiOpenrouter,
} from '@lib/icons'

import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  FACE_UPDATED_EVENT,
} from '../../features/merope/events'
import SiteMotionWorkbench from '../../features/merope/SiteMotionWorkbench'
import { agentService } from '../../services/agent'
import { invalidatePublicConfigCache } from '../../utils/requestDedup'
import { userFacingError } from '../../utils/userFacingError'
import {
  activityKey,
  ADDRESSEE_UPDATED_EVENT,
  moodBand,
} from '../agent/meropeVitals'
import { parseFlattenedPersona } from '../agent/onboarding/onboardingTypes'
import PersonaOnboardingPage from '../agent/onboarding/PersonaOnboardingPage'
import {
  AutoHeight,
  guideDomProps,
  InfoActionCard,
  InputItem,
  ProviderItem,
  SettingGroup,
  SettingsButton,
  SettingSection,
  SettingTitleGuideEntry,
  SettingTitleTag,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import AgentOptionsPanel, {
  AgentNestedSection,
} from './AgentOptionsPanel'
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
import { useAiSubpage } from './usePersonaPage'
import { TencentCloudMark, VolcengineMark } from './vendorIcons'

interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

const OPENAI_BASE_URL = 'https://api.openai.com/v1'
const OPENROUTER_BASE_URL = 'https://openrouter.ai/api/v1'

/**
 * 推断展示用的 Provider。
 * OpenRouter 是 OpenAI 兼容服务，后端仍以 provider=openai + openai_base_url 处理，
 * 因此这里根据 base_url 反推该高亮 OpenAI 还是 OpenRouter。
 */
function resolveProvider(rawProvider: string, openaiBaseUrl: string): string {
  if (
    rawProvider === 'openai' &&
    openaiBaseUrl.trim().toLowerCase().includes('openrouter.ai')
  ) {
    return 'openrouter'
  }
  return rawProvider
}

interface AiConfigSectionProps {
  /** AI 配置字段数组 */
  configFields: ConfigField[]
  /** 更新配置字段值 */
  updateValue: (key: string, value: string) => void
  /** ui bag：Agent 人设总开关存在这里，控件挂在 Lite / Pro 旁边 */
  uiConfigFields: Array<{ key: string; value: string }>
  updateUiFieldValue: (key: string, value: string) => void
  /** 语音测试回调 */
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
                field.key.endsWith('model')
                  ? t.config.openaiModelLabel
                  : field.label
              }
              required={field.required}
              value={field.value}
              onChange={(value) => updateValue(field.key, value)}
              guide={fieldGuide?.guide}
              guidePath={fieldGuide?.guidePath}
              placeholder={field.placeholder}
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

function fieldsForModelTier(
  configFields: ConfigField[],
  prefix: '' | 'lite_' | 'pro_',
  provider: string,
): ConfigField[] {
  const providerKey = `${prefix}provider`
  const geminiPrefix = `${prefix}gemini_`
  return configFields.filter((field) => {
    if (field.key === providerKey) return false
    if (field.key.includes('api_key') || field.key.endsWith('base_url')) {
      return false
    }
    if (provider === 'gemini') {
      return field.key.startsWith(geminiPrefix) && field.key.endsWith('model')
    }
    if (provider === 'openai' || provider === 'openrouter') {
      return field.key === `${prefix}openai_model`
    }
    return false
  })
}

export const AiConfigSection: React.FC<AiConfigSectionProps> = ({
  configFields,
  updateValue,
  uiConfigFields,
  updateUiFieldValue,
  onSpeechTest,
  title,
  icon,
  description,
  sectionId,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [speechTesting, setSpeechTesting] = useState(false)
  const [personaChrome, setPersonaChrome] = useState<OnboardingPageChrome | null>(
    null,
  )
  const {
    page: aiSubpage,
    navDir: aiPaneNav,
    openPage: openAiSubpage,
    closePage: closeAiSubpage,
  } = useAiSubpage((page) => {
    if (page === 'merope-setup') setPersonaChrome(null)
  })
  const setupPage = aiSubpage === 'merope-setup'
  const meropePage = aiSubpage === 'merope'
  const subpageOpen = aiSubpage != null

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

  // 辅助函数：获取配置字段值
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

  // 当前 AI Provider (标准模型)。OpenRouter 依据 base_url 从 openai 中区分出来，未配置时默认 OpenRouter
  const currentProvider = useMemo(() => {
    const raw = getFieldValue('provider')
    if (!raw) return 'openrouter'
    return resolveProvider(raw, getFieldValue('openai_base_url'))
  }, [getFieldValue])

  // Lite 模型是否启用（关闭时回退 Standard）
  const liteEnabled = useMemo(() => {
    const val = getFieldValue('lite_enabled', 'false')
    return val === 'true' || val === '1'
  }, [getFieldValue])

  const agentPersonaEnabled = useMemo(
    () =>
      uiConfigFields.find((field) => field.key === 'merope_enabled')
        ?.value === 'true',
    [uiConfigFields],
  )

  const agentPersonaSpeechEnabled = useMemo(
    () =>
      uiConfigFields.find((field) => field.key === 'merope_speech_enabled')
        ?.value === 'true',
    [uiConfigFields],
  )

  const currentLiteProvider = useMemo(() => {
    const raw = getFieldValue('lite_provider')
    if (!raw) return 'openrouter'
    return resolveProvider(raw, getFieldValue('lite_openai_base_url'))
  }, [getFieldValue])

  // Pro 模型是否启用（关闭时回退 Standard）
  const proEnabled = useMemo(() => {
    const val = getFieldValue('pro_enabled', 'false')
    return val === 'true' || val === '1'
  }, [getFieldValue])

  // 当前 Pro AI Provider（同样未配置时默认 OpenRouter）
  const currentProProvider = useMemo(() => {
    const raw = getFieldValue('pro_provider')
    if (!raw) return 'openrouter'
    return resolveProvider(raw, getFieldValue('pro_openai_base_url'))
  }, [getFieldValue])

  /**
   * 切换 Provider。OpenRouter 落到 provider=openai，并切 base_url。
   * 用户手填的自定义地址不覆盖。模型名不代填。
   */
  const handleProviderChange = useCallback(
    (providerKey: string, baseUrlKey: string, next: string) => {
      if (next === 'openrouter') {
        updateValue(providerKey, 'openai')
        updateValue(baseUrlKey, OPENROUTER_BASE_URL)
        return
      }
      if (next === 'openai') {
        updateValue(providerKey, 'openai')
        const base = getFieldValue(baseUrlKey).trim().toLowerCase()
        // OpenRouter → 官方；空地址和自定义兼容端点都不代填
        if (base.includes('openrouter.ai')) {
          updateValue(baseUrlKey, OPENAI_BASE_URL)
        }
        return
      }
      updateValue(providerKey, next)
    },
    [getFieldValue, updateValue],
  )

  const applyTextSource = useCallback(
    (
      sourceKey: string,
      providerKey: string,
      baseUrlKey: string,
      slug: string,
    ) => {
      if (!slug) {
        updateValue(sourceKey, '')
        const hasVendorText = vendorSources.some(
          (item) => item.enabled && vendorSupports(item, 'text'),
        )
        if (!hasVendorText) updateValue(providerKey, '')
        return
      }
      updateValue(sourceKey, slug)
      const source = vendorSources.find((item) => item.slug === slug)
      if (!source) {
        handleProviderChange(providerKey, baseUrlKey, slug)
        return
      }
      if (source.kind === 'gemini') {
        updateValue(providerKey, 'gemini')
        return
      }
      updateValue(providerKey, 'openai')
      const base =
        source.kind === 'openrouter' ||
        (source.base_url || '').includes('openrouter.ai')
          ? OPENROUTER_BASE_URL
          : (source.base_url || '').trim()
      if (base) updateValue(baseUrlKey, base)
    },
    [handleProviderChange, updateValue, vendorSources],
  )

  const standardSourceValue = textSourceOptions
    ? getFieldValue('ai_source')
    : getFieldValue('provider')
      ? currentProvider
      : ''
  const liteSourceValue = textSourceOptions
    ? getFieldValue('lite_ai_source')
    : getFieldValue('lite_provider')
      ? currentLiteProvider
      : ''
  const proSourceValue = textSourceOptions
    ? getFieldValue('pro_ai_source')
    : getFieldValue('pro_provider')
      ? currentProProvider
      : ''

  // 当前图片生成 Provider
  const currentImageProvider = useMemo(
    () => getFieldValue('ai_image_provider', 'openrouter'),
    [getFieldValue],
  )

  const currentSpeechProvider = useMemo(
    () => getFieldValue('speech_provider', 'tencent'),
    [getFieldValue],
  )

  const vendorUsages = useMemo(() => {
    const map: VendorUsageMap = {}
    const add = (raw: string, id: VendorUsageId) => {
      const slug = resolveUsedVendorSlug(raw, vendorSources)
      if (!slug) return
      const next = map[slug] ?? []
      if (!next.includes(id)) next.push(id)
      map[slug] = next
    }
    add(getFieldValue('ai_source') || currentProvider, 'standard')
    if (liteEnabled) {
      add(getFieldValue('lite_ai_source') || currentLiteProvider, 'lite')
    }
    if (proEnabled) {
      add(getFieldValue('pro_ai_source') || currentProProvider, 'pro')
    }
    add(getFieldValue('ai_image_source') || currentImageProvider, 'image')
    add(getFieldValue('speech_source') || currentSpeechProvider, 'speech')
    for (const source of vendorSources) {
      if (source.enabled && vendorSupports(source, 'realtime')) {
        add(source.slug, 'realtime')
      }
    }
    return map
  }, [
    currentImageProvider,
    currentLiteProvider,
    currentProProvider,
    currentProvider,
    currentSpeechProvider,
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

  const handleSpeechProviderChange = useCallback(
    (slug: string) => {
      if (!slug) {
        updateValue('speech_source', '')
        const hasVendorSpeech = vendorSources.some(
          (item) => item.enabled && vendorSupports(item, 'speech'),
        )
        if (!hasVendorSpeech) updateValue('speech_provider', '')
        return
      }
      // 只改源。转写/播报/音色不代填。
      updateValue('speech_source', slug)
      const source = vendorSources.find((item) => item.slug === slug)
      updateValue(
        'speech_provider',
        speechProviderKindFromSource(source, slug),
      )
    },
    [updateValue, vendorSources],
  )

  // AI Provider 选项。OpenRouter 默认在前，其次 OpenAI 兼容，最后 Gemini
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

  // 图片生成 Provider 选项（OpenAI 兼容复用文本侧同名文案）
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

  const handleImageProviderChange = useCallback(
    (slug: string) => {
      if (!slug) {
        updateValue('ai_image_source', '')
        const hasVendorImage = vendorSources.some(
          (item) => item.enabled && vendorSupports(item, 'image'),
        )
        if (!hasVendorImage) updateValue('ai_image_provider', '')
        return
      }
      updateValue('ai_image_source', slug)
      const source = vendorSources.find((item) => item.slug === slug)
      const kind = source?.kind || slug
      const mapped =
        kind === 'volcengine'
          ? 'volcengine'
          : kind === 'openrouter'
            ? 'openrouter'
            : kind === 'gemini'
              ? 'gemini'
              : 'openai'
      updateValue('ai_image_provider', mapped)
    },
    [updateValue, vendorSources],
  )

  const liteProviderFields = useMemo(
    () => fieldsForModelTier(configFields, 'lite_', currentLiteProvider),
    [configFields, currentLiteProvider],
  )
  const providerFields = useMemo(
    () => fieldsForModelTier(configFields, '', currentProvider),
    [configFields, currentProvider],
  )
  const proProviderFields = useMemo(
    () => fieldsForModelTier(configFields, 'pro_', currentProProvider),
    [configFields, currentProProvider],
  )

  // 处理语音测试
  const handleSpeechTest = useCallback(async () => {
    setSpeechTesting(true)
    setSpeechTestResult(null)
    try {
      const result = await onSpeechTest()
      setSpeechTestResult(result)
    } catch (error) {
      setSpeechTestResult({
        success: false,
        message:
          userFacingError(error, t.config.speechTestFailed),
      })
    } finally {
      setSpeechTesting(false)
    }
  }, [onSpeechTest, t.config.speechTestFailed])

  const o = t.agentPersona.onboarding
  const paneKey = aiSubpage ?? 'ai'
  const personaGuide = bindGuide('ai.agentPersona', g.ai.agentPersona)
  const meropeOn = agentPersonaEnabled && proEnabled
  const [savedPersonaName, setSavedPersonaName] = useState('')
  const [hasSavedPersona, setHasSavedPersona] = useState(false)
  const [mood, setMood] = useState(70)
  const [arousal, setArousal] = useState(48)
  const [activity, setActivity] = useState('idle')
  const [personality, setPersonality] = useState('')
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [vitalsReady, setVitalsReady] = useState(false)
  const [personaBusy, setPersonaBusy] = useState(false)
  const [personaError, setPersonaError] = useState<string | null>(null)

  const handleDeletePersona = useCallback(async () => {
    setPersonaBusy(true)
    setPersonaError(null)
    try {
      await agentService.deletePersona()
      invalidatePublicConfigCache()
      window.dispatchEvent(new CustomEvent('arael-persona-updated'))
    } catch (error) {
      setPersonaError(
        userFacingError(error, t.config.agentPersonaDeleteFailed),
      )
    } finally {
      setPersonaBusy(false)
    }
  }, [t.config.agentPersonaDeleteFailed])

  useEffect(() => {
    if (!meropeOn) {
      setSavedPersonaName('')
      setHasSavedPersona(false)
      setMood(70)
      setArousal(48)
      setActivity('idle')
      setPersonality('')
      setPortraitUrl(null)
      setVitalsReady(false)
      return
    }
    let cancelled = false
    const load = () => {
      void agentService
        .getPersona()
        .then((persona) => {
          if (cancelled || !persona) return
          const name = persona.name?.trim() ?? ''
          setSavedPersonaName(name)
          setHasSavedPersona(
            persona.hasCustomPersona === true ||
              name.length > 0 ||
              Boolean(persona.personality?.trim()),
          )
          setMood(typeof persona.mood === 'number' ? persona.mood : 70)
          setArousal(
            typeof persona.arousal === 'number' ? persona.arousal : 48,
          )
          setActivity(persona.activity ?? 'idle')
          setPersonality(persona.personality?.trim() ?? '')
          setPortraitUrl(
            typeof persona.portraitAssetId === 'string' &&
              persona.portraitAssetId.trim()
              ? persona.portraitAssetId
              : null,
          )
          setVitalsReady(true)
        })
        .catch(() => {
          if (!cancelled) {
            setSavedPersonaName('')
            setHasSavedPersona(false)
            setPersonality('')
            setPortraitUrl(null)
            setVitalsReady(false)
          }
        })
    }
    load()
    window.addEventListener('arael-persona-updated', load)
    window.addEventListener(FACE_UPDATED_EVENT, load)
    window.addEventListener(ADDRESSEE_UPDATED_EVENT, load)
    return () => {
      cancelled = true
      window.removeEventListener('arael-persona-updated', load)
      window.removeEventListener(FACE_UPDATED_EVENT, load)
      window.removeEventListener(ADDRESSEE_UPDATED_EVENT, load)
    }
  }, [meropeOn])
  const personaGateLead = !proEnabled
    ? t.config.agentPersonaNeedsPro
    : meropeOn && !liteEnabled
      ? t.config.agentPersonaNeedsLite
      : t.config.agentPersonaHint

  const personaCardCopy = useMemo(() => {
    if (!meropeOn || !hasSavedPersona) return null
    const summary = parseFlattenedPersona(personality).summary.replace(/\s+/g, ' ').trim()
    return {
      summary,
      mood: vitalsReady ? o.mood[moodBand(mood, arousal)] : '—',
      activity: vitalsReady ? o.activity[activityKey(activity)] : '—',
    }
  }, [
    activity,
    arousal,
    hasSavedPersona,
    meropeOn,
    mood,
    o,
    personality,
    vitalsReady,
  ])

  return (
    <SettingSection
      sectionId={sectionId}
      className={setupPage ? 'setting-section--persona' : undefined}
      title={
        setupPage
          ? (personaChrome?.title ?? o.step1Title)
          : meropePage
            ? t.merope.adminTitle
            : title
      }
      icon={subpageOpen ? undefined : icon}
      description={
        setupPage
          ? (personaChrome?.description ?? o.step1Lead)
          : meropePage
            ? t.merope.adminDescription
            : description
      }
      detail={
        setupPage
          ? (personaChrome?.description ?? o.step1Lead)
          : meropePage
            ? t.merope.adminDescription
            : undefined
      }
      detailTone={setupPage ? personaChrome?.detailTone : undefined}
      showResetPage={subpageOpen ? false : undefined}
      {...(setupPage ? personaGuide : {})}
      headerActions={
        setupPage && personaChrome?.action ? (
          <SettingsButton
            variant="secondary"
            size="sm"
            icon={<LuRefreshCw size={14} />}
            loading={personaChrome.action.busy}
            disabled={personaChrome.action.disabled}
            onClick={personaChrome.action.onClick}
          >
            {personaChrome.action.label}
          </SettingsButton>
        ) : null
      }
      headerBetweenPinned={
        subpageOpen ? undefined : (
          <AiVendorAddTrigger
            sources={vendorSources}
            onChange={setVendorSources}
            usages={vendorUsages}
          />
        )
      }
      headerLeading={
        subpageOpen ? (
          <button
            type="button"
            className="section-header-back"
            onClick={() =>
              setupPage
                ? (personaChrome?.onBack ?? closeAiSubpage)()
                : closeAiSubpage()
            }
            disabled={setupPage ? personaChrome?.backDisabled : false}
            aria-label={
              setupPage
                ? (personaChrome?.backAria ?? t.common.back)
                : t.common.back
            }
          >
            <LuChevronLeft size={18} aria-hidden />
            <span>{t.common.back}</span>
          </button>
        ) : undefined
      }
    >
      <AutoHeight contentKey={paneKey} animate={false}>
        <div key={paneKey} data-nav={aiPaneNav} className="ai-pane sm-pane">
          {setupPage ? (
            <PersonaOnboardingPage
              onBack={closeAiSubpage}
              onFinished={() => openAiSubpage('merope')}
              onChromeChange={setPersonaChrome}
              meropeOn={meropeOn}
              gateLead={personaGateLead}
            />
          ) : meropePage ? (
            <SiteMotionWorkbench
              mood={mood}
              arousal={arousal}
              activity={activity}
            />
          ) : (
            <>
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
          providerItemKey="ai_provider"
          providerLabel={t.config.aiProvider}
          provider={standardSourceValue}
          providerOptions={textSourceOptions ?? aiProviderOptions}
          providerHint={t.config.aiProviderHint}
          fields={providerFields}
          onProviderChange={(provider) =>
            applyTextSource(
              'ai_source',
              'provider',
              'openai_base_url',
              provider,
            )
          }
          updateValue={updateValue}
        />

        <ModelTierGroup
          title={t.config.aiLiteModelTitle}
          description={t.config.aiLiteModelDesc}
          {...bindGuide('ai.lite', g.ai.lite)}
          enableGuide={bindGuide('ai.liteEnable', g.ai.liteEnable).guide}
          enableGuidePath="ai.liteEnable"
          providerGuide={providerGuideBinding.guide}
          providerGuidePath={providerGuideBinding.guidePath}
          fieldGuideFor={fieldGuideFor}
          providerItemKey="lite_ai_provider"
          providerLabel={t.config.aiProvider}
          provider={liteSourceValue}
          providerOptions={textSourceOptions ?? aiProviderOptions}
          providerHint={t.config.aiLiteProviderHint}
          fields={liteProviderFields}
          enabled={liteEnabled}
          toggle={{
            checked: liteEnabled,
            onChange: (value) =>
              updateValue('lite_enabled', value ? 'true' : 'false'),
            ariaLabel: t.config.aiLiteEnable,
            title: t.config.aiLiteEnableDesc,
          }}
          onProviderChange={(provider) =>
            applyTextSource(
              'lite_ai_source',
              'lite_provider',
              'lite_openai_base_url',
              provider,
            )
          }
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
          providerItemKey="pro_ai_provider"
          providerLabel={t.config.aiProvider}
          provider={proSourceValue}
          providerOptions={textSourceOptions ?? aiProviderOptions}
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
          onProviderChange={(provider) =>
            applyTextSource(
              'pro_ai_source',
              'pro_provider',
              'pro_openai_base_url',
              provider,
            )
          }
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
        title={t.config.agentOptions}
        icon={<LuNotebookPen />}
        description={t.config.agentOptionsDesc}
        {...bindGuide('ai.agentPersona', g.ai.agentPersona)}
      >
        <AgentNestedSection
          title={t.config.agentPersona}
          description={personaGateLead}
          {...personaGuide}
          toggleTourAnchor="config-ai-persona-toggle"
          badge={
            <SettingTitleTag variant="beta">
              {t.config.agentPersonaBeta}
            </SettingTitleTag>
          }
          toggle={{
            checked: meropeOn,
            onChange: (value) =>
              updateUiFieldValue('merope_enabled', value ? 'true' : 'false'),
            disabled: !proEnabled,
            ariaLabel: t.config.agentPersona,
            title: t.config.agentPersonaHint,
          }}
        >
        {meropeOn ? (
          <div data-tour="config-ai-persona-card">
          <InfoActionCard
            copyable={false}
            tone={!liteEnabled ? 'info' : 'default'}
            title={
              hasSavedPersona
                ? savedPersonaName || 'Arael'
                : t.config.agentPersonaEmpty
            }
            preview={
              portraitUrl && hasSavedPersona ? (
                <img src={portraitUrl} alt={savedPersonaName || 'Arael'} />
              ) : (
                <span className="info-action-card-preview-empty is-mosaic">
                  <img src="/merope/clothing/everyday.png" alt="" />
                  <img src="/merope/clothing/fantasy.png" alt="" />
                  <img src="/merope/clothing/japanese.png" alt="" />
                  <img src="/merope/clothing/sci-fi.png" alt="" />
                </span>
              )
            }
            actions={
              hasSavedPersona
                ? [
                    {
                      key: 'face',
                      label: t.merope.faceOpen,
                      onClick: () => openAiSubpage('merope'),
                    },
                    {
                      key: 'delete',
                      label: t.config.agentPersonaDelete,
                      onClick: () => void handleDeletePersona(),
                      disabled: personaBusy,
                      loading: personaBusy,
                      variant: 'danger' as const,
                      confirm: t.config.agentPersonaDeleteConfirm,
                    },
                  ]
                : [
                    {
                      key: 'setup',
                      label: o.openPage,
                      onClick: () => openAiSubpage('merope-setup'),
                      disabled: personaBusy,
                    },
                  ]
            }
            footer={personaError}
          >
            {personaCardCopy ? (
              <>
                {personaCardCopy.summary ? (
                  <p className="info-action-card-lede">{personaCardCopy.summary}</p>
                ) : null}
                <p className="info-action-card-meta">
                  <span>{personaCardCopy.mood}</span>
                  <span className="info-action-card-meta-dot" aria-hidden>
                    ·
                  </span>
                  <span>{personaCardCopy.activity}</span>
                </p>
              </>
            ) : (
              <p className="info-action-card-lede">
                {t.config.agentPersonaEmptyLead}
              </p>
            )}
          </InfoActionCard>
          </div>
        ) : null}
        </AgentNestedSection>
        <AgentNestedSection
          title={t.config.agentPersonaSpeech}
          description={t.config.agentPersonaSpeechHint}
          {...bindGuide('ai.agentPersonaSpeech', g.ai.agentPersonaSpeech)}
          tourAnchor="config-ai-persona-speech"
          toggle={{
            checked: agentPersonaSpeechEnabled,
            onChange: (value) =>
              updateUiFieldValue(
                'merope_speech_enabled',
                value ? 'true' : 'false',
              ),
            disabled: !meropeOn,
            ariaLabel: t.config.agentPersonaSpeech,
            title: t.config.agentPersonaSpeechHint,
          }}
        />
        <AgentNestedSection
          title={t.config.qqBotTitle}
          description={t.config.qqBotDesc}
          {...bindGuide('ai.qqBot', g.ai.qqBot)}
          toggle={{
            checked: getFieldValue('qq_bot_enabled') === 'true',
            onChange: (value) =>
              updateValue('qq_bot_enabled', value ? 'true' : 'false'),
            ariaLabel: t.config.qqBotTitle,
            title: t.config.qqBotHint,
          }}
        >
          <InputItem
            itemKey="qq_bot_app_id"
            label={t.config.qqBotAppId}
            value={getFieldValue('qq_bot_app_id')}
            onChange={(value) => updateValue('qq_bot_app_id', value)}
            placeholder="102..."
            hint={t.config.qqBotHint}
            layout="vertical"
            {...bindGuide('ai.qqBot', g.ai.qqBot)}
          />
          <InputItem
            itemKey="qq_bot_app_secret"
            label={t.config.qqBotAppSecret}
            value={getFieldValue('qq_bot_app_secret')}
            onChange={(value) => updateValue('qq_bot_app_secret', value)}
            inputType="password"
            autoSelectOnMask
            layout="vertical"
            {...bindGuide('ai.qqBot', g.ai.qqBot)}
          />
        </AgentNestedSection>
        <AgentOptionsPanel />
      </SettingGroup>

      {/* 图片生成模型 */}
      <SettingGroup
        title={t.config.aiImageTitle}
        icon={<LuPalette />}
        description={t.config.aiImageDesc}
        {...bindGuide('ai.image', g.ai.image)}
      >
        <ProviderItem
          itemKey="image_provider"
          label={t.config.aiProvider}
          {...bindGuide('ai.provider', g.ai.provider)}
          value={
            imageSourceOptions
              ? getFieldValue('ai_image_source')
              : getFieldValue('ai_image_provider')
          }
          onChange={handleImageProviderChange}
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
                ? 'doubao-seedream-5-0-260128'
                : currentImageProvider === 'gemini'
                  ? 'gemini-3.1-flash-image'
                  : 'openai/gpt-image-2'
          }
          inputType="text"
          layout="vertical"
        />
      </SettingGroup>

      {/* 语音服务配置 */}
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
          itemKey="speech_provider"
          label={t.config.speechProvider}
          {...bindGuide('ai.provider', g.ai.provider)}
          value={
            speechSourceOptions
              ? getFieldValue('speech_source')
              : getFieldValue('speech_provider')
          }
          onChange={handleSpeechProviderChange}
          options={speechSourceOptions ?? speechProviderOptions}
          layout="horizontal"
        />

        {(() => {
          const speechSelectorValue = speechSourceOptions
            ? getFieldValue('speech_source')
            : getFieldValue('speech_provider')
          if (!speechSelectorValue) return null
          const selectedSource = vendorSources.find(
            (item) => item.slug === speechSelectorValue,
          )
          const selected = speechProviderKindFromSource(
            selectedSource,
            selectedSource?.kind || speechSelectorValue,
          )
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
                        ? 'gemini-3.6-flash'
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
                        ? 'gemini-2.5-flash-preview-tts'
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
            </>
          )}
        </div>
      </AutoHeight>
    </SettingSection>
  )
}

export default AiConfigSection
