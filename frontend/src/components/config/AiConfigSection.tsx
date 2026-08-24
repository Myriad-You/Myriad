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
  LuPalette,
  LuStore,
  LuRefreshCw,
  LuNotebookPen,
  LuSparkles,
  SiGooglegemini,
  SiOpenai,
  SiOpenrouter,
} from '@lib/icons'

import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { userFacingError } from '../../utils/userFacingError'
import { agentService } from '../../services/agent'
import { invalidatePublicConfigCache } from '../../utils/requestDedup'
import {
  ADDRESSEE_UPDATED_EVENT,
  activityKey,
  moodBand,
} from '../agent/meropeVitals'
import {
  FACE_UPDATED_EVENT,
} from '../../features/merope/events'
import SiteMotionWorkbench from '../../features/merope/SiteMotionWorkbench'
import PersonaOnboardingPage from '../agent/onboarding/PersonaOnboardingPage'
import { parseFlattenedPersona } from '../agent/onboarding/onboardingTypes'
import {
  AutoHeight,
  InfoActionCard,
  InputItem,
  ProviderItem,
  SettingGroup,
  SettingsButton,
  SettingSection,
  SettingTitleTag,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import {
  defaultModelsForSource,
  parseVendorSources,
  resolveUsedVendorSlug,
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

// OpenAI 兼容服务的 Base URL / 默认模型预设
const OPENAI_BASE_URL = 'https://api.openai.com/v1'
const OPENROUTER_BASE_URL = 'https://openrouter.ai/api/v1'
// OpenAI 官方 GPT-5.6 三档：Luna（快/省）· Terra（均衡）· Sol（旗舰）
const OPENAI_MODEL_LITE = 'gpt-5.6-luna'
const OPENAI_MODEL_STANDARD = 'gpt-5.6-terra'
const OPENAI_MODEL_PRO = 'gpt-5.6-sol'
// OpenRouter 默认模型：三个文本模型层级共用同一种 Provider 配置协议。
const OPENROUTER_MODEL_LITE = 'openai/gpt-oss-20b:free'
const OPENROUTER_MODEL_STANDARD = 'minimax/minimax-m3'
const OPENROUTER_MODEL_PRO = 'anthropic/claude-opus-5'

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
}) => {
  const { t } = useI18n()
  return (
  <div className="ai-llm-tier">
    <div className="ai-llm-tier-head">
      <div className="ai-llm-tier-copy">
        <h3 className="ai-llm-tier-title">{title}</h3>
        {description ? (
          <p className="ai-llm-tier-desc">{description}</p>
        ) : null}
      </div>
      {toggle ? (
        <div className="ai-llm-tier-switch">
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
   * 切换 Provider。OpenRouter 落到 provider=openai，并把对应的 base_url 与默认模型
   * 在 OpenAI 官方与 OpenRouter 之间切换（用户手填的自定义地址不覆盖）。
   */
  const handleProviderChange = useCallback(
    (
      providerKey: string,
      baseUrlKey: string,
      modelKey: string,
      openrouterModel: string,
      openaiModel: string,
      next: string,
    ) => {
      if (next === 'openrouter') {
        updateValue(providerKey, 'openai')
        updateValue(baseUrlKey, OPENROUTER_BASE_URL)
        updateValue(modelKey, openrouterModel)
        return
      }
      if (next === 'openai') {
        updateValue(providerKey, 'openai')
        const base = getFieldValue(baseUrlKey).trim().toLowerCase()
        // OpenRouter / 空地址 → 官方；自定义兼容端点保留 base（仍切换高亮与 provider）
        if (!base || base.includes('openrouter.ai')) {
          updateValue(baseUrlKey, OPENAI_BASE_URL)
          updateValue(modelKey, openaiModel)
        }
        return
      }
      // gemini 等：只改 provider；展示侧靠 resolveProvider
      updateValue(providerKey, next)
    },
    [getFieldValue, updateValue],
  )

  const applyTextSource = useCallback(
    (
      sourceKey: string,
      providerKey: string,
      baseUrlKey: string,
      modelKey: string,
      openrouterModel: string,
      openaiModel: string,
      slug: string,
    ) => {
      updateValue(sourceKey, slug)
      const source = vendorSources.find((item) => item.slug === slug)
      if (!source) {
        handleProviderChange(
          providerKey,
          baseUrlKey,
          modelKey,
          openrouterModel,
          openaiModel,
          slug,
        )
        return
      }
      if (source.kind === 'gemini') {
        updateValue(providerKey, 'gemini')
        const fallback = defaultModelsForSource(source, 'text').text
        if (fallback && !getFieldValue(modelKey)) {
          updateValue(modelKey, fallback)
        }
        return
      }
      updateValue(providerKey, 'openai')
      const base =
        source.kind === 'openrouter' ||
        (source.base_url || '').includes('openrouter.ai')
          ? OPENROUTER_BASE_URL
          : source.base_url || OPENAI_BASE_URL
      updateValue(baseUrlKey, base)
      if (!getFieldValue(modelKey)) {
        const fallback = defaultModelsForSource(source, 'text').text
        if (fallback) updateValue(modelKey, fallback)
      }
    },
    [getFieldValue, handleProviderChange, updateValue, vendorSources],
  )

  const standardSourceValue =
    getFieldValue('ai_source') || currentProvider
  const liteSourceValue =
    getFieldValue('lite_ai_source') || currentLiteProvider
  const proSourceValue = getFieldValue('pro_ai_source') || currentProProvider

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
      updateValue('speech_source', slug)
      const source = vendorSources.find((item) => item.slug === slug)
      const kind = source?.kind || slug
      const mapped =
        kind === 'tencent'
          ? 'tencent'
          : kind === 'openrouter'
            ? 'openrouter'
            : kind === 'gemini'
              ? 'gemini'
              : 'openai'
      updateValue('speech_provider', mapped)
      const models = defaultModelsForSource(source ?? { kind }, 'speech')
      if (models.stt) updateValue('speech_stt_model', models.stt)
      if (models.tts !== undefined) updateValue('speech_tts_model', models.tts)
      if (models.voice) updateValue('speech_tts_voice', models.voice)
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
      const models = defaultModelsForSource(source ?? { kind }, 'image')
      updateValue('ai_image_model', models.image ?? '')
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
  const [activity, setActivity] = useState('idle')
  const [personality, setPersonality] = useState('')
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [reportCount, setReportCount] = useState(0)
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
      setActivity('idle')
      setPersonality('')
      setPortraitUrl(null)
      setReportCount(0)
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
          setActivity(persona.activity ?? 'idle')
          setPersonality(persona.personality?.trim() ?? '')
          setPortraitUrl(
            typeof persona.portraitAssetId === 'string' &&
              persona.portraitAssetId.trim()
              ? persona.portraitAssetId
              : null,
          )
          setReportCount(
            typeof persona.reportCount === 'number' ? persona.reportCount : 0,
          )
          setVitalsReady(true)
        })
        .catch(() => {
          if (!cancelled) {
            setSavedPersonaName('')
            setHasSavedPersona(false)
            setPersonality('')
            setPortraitUrl(null)
            setReportCount(0)
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
      mood: vitalsReady ? o.mood[moodBand(mood)] : '—',
      activity: vitalsReady ? o.activity[activityKey(activity)] : '—',
    }
  }, [activity, hasSavedPersona, meropeOn, mood, o, personality, vitalsReady])

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
              onChromeChange={setPersonaChrome}
              meropeOn={meropeOn}
              gateLead={personaGateLead}
            />
          ) : meropePage ? (
            <SiteMotionWorkbench mood={mood} activity={activity} />
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
          providerGuide={providerGuideBinding.guide}
          providerGuidePath={providerGuideBinding.guidePath}
          fieldGuideFor={fieldGuideFor}
          providerItemKey="ai_provider"
          providerLabel={t.config.aiProvider}
          provider={textSourceOptions ? standardSourceValue : currentProvider}
          providerOptions={textSourceOptions ?? aiProviderOptions}
          providerHint={t.config.aiProviderHint}
          fields={providerFields}
          onProviderChange={(provider) =>
            applyTextSource(
              'ai_source',
              'provider',
              'openai_base_url',
              'openai_model',
              OPENROUTER_MODEL_STANDARD,
              OPENAI_MODEL_STANDARD,
              provider,
            )
          }
          updateValue={updateValue}
        />

        <ModelTierGroup
          title={t.config.aiLiteModelTitle}
          description={t.config.aiLiteModelDesc}
          providerGuide={providerGuideBinding.guide}
          providerGuidePath={providerGuideBinding.guidePath}
          fieldGuideFor={fieldGuideFor}
          providerItemKey="lite_ai_provider"
          providerLabel={t.config.aiProvider}
          provider={textSourceOptions ? liteSourceValue : currentLiteProvider}
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
              'lite_openai_model',
              OPENROUTER_MODEL_LITE,
              OPENAI_MODEL_LITE,
              provider,
            )
          }
          updateValue={updateValue}
        />

        <ModelTierGroup
          title={t.config.aiProModelTitle}
          description={t.config.aiProModelDesc}
          providerGuide={providerGuideBinding.guide}
          providerGuidePath={providerGuideBinding.guidePath}
          fieldGuideFor={fieldGuideFor}
          providerItemKey="pro_ai_provider"
          providerLabel={t.config.aiProvider}
          provider={textSourceOptions ? proSourceValue : currentProProvider}
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
              'pro_openai_model',
              OPENROUTER_MODEL_PRO,
              OPENAI_MODEL_PRO,
              provider,
            )
          }
          updateValue={updateValue}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.agentPersona}
        icon={<LuNotebookPen />}
        description={personaGateLead}
        titleExtra={
          <SettingTitleTag variant="beta">{t.config.agentPersonaBeta}</SettingTitleTag>
        }
        {...bindGuide('ai.agentPersona', g.ai.agentPersona)}
        switch={{
          checked: meropeOn,
          onChange: (value) =>
            updateUiFieldValue('merope_enabled', value ? 'true' : 'false'),
          disabled: !proEnabled,
          ariaLabel: t.config.agentPersona,
        }}
      >
        {meropeOn ? (
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
                : reportCount >= 3
                  ? [
                      {
                        key: 'setup',
                        label: o.openPage,
                        onClick: () => openAiSubpage('merope-setup'),
                        disabled: personaBusy,
                      },
                    ]
                  : undefined
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
                {reportCount < 3
                  ? t.config.agentPersonaNeedsReports
                      .replace('{count}', String(reportCount))
                      .replace('{need}', '3')
                  : t.config.agentPersonaEmptyLead}
              </p>
            )}
          </InfoActionCard>
        ) : null}
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
              ? getFieldValue('ai_image_source') || currentImageProvider
              : currentImageProvider
          }
          onChange={handleImageProviderChange}
          options={imageSourceOptions ?? imageProviderOptions}
          layout="horizontal"
        />

        <InputItem
          itemKey="ai_image_model"
          label={t.config.openaiModelLabel}
          value={getFieldValue('ai_image_model', 'openai/gpt-image-2')}
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
          value={
            speechSourceOptions
              ? getFieldValue('speech_source') || currentSpeechProvider
              : currentSpeechProvider
          }
          onChange={handleSpeechProviderChange}
          options={speechSourceOptions ?? speechProviderOptions}
          layout="horizontal"
        />

        {(() => {
          const selected =
            vendorSources.find(
              (item) =>
                item.slug ===
                (getFieldValue('speech_source') || currentSpeechProvider),
            )?.kind || currentSpeechProvider
          return (
            selected === 'openai' ||
            selected === 'openrouter' ||
            selected === 'openai_compatible' ||
            selected === 'gemini'
          )
        })() && (
          <>
            <InputItem
              itemKey="speech_stt_model"
              label={t.config.speechSttModel}
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
              value={getFieldValue('speech_tts_voice', 'marin')}
              onChange={(v) => updateValue('speech_tts_voice', v)}
              placeholder={
                currentSpeechProvider === 'gemini' ? 'Kore' : 'marin'
              }
              inputType="text"
              layout="vertical"
            />
            {currentSpeechProvider === 'openrouter' ? (
              <p className="setting-hint">{t.config.speechOpenRouterTtsHint}</p>
            ) : null}
          </>
        )}
      </SettingGroup>
            </>
          )}
        </div>
      </AutoHeight>
    </SettingSection>
  )
}

export default AiConfigSection
