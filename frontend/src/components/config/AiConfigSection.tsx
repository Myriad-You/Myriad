/**
 * AI 配置区块
 * 使用通用设置组件重构
 */

import type { SettingOption } from '../settings/types'
import { FaFreeCodeCamp, FaMagic, FaMicrophone, FaPalette, FaVolumeUp, SiGooglegemini, SiOpenai } from '@lib/icons'
import React, { useCallback, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  ButtonItem,
  CompactSettingGroup,
  InfoCard,
  InputItem,
  NumberItem,
  ProviderItem,
  SelectItem,
  SettingGroup,
  SettingSection,
} from '../settings'

interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

interface AiConfigSectionProps {
  /** AI 配置字段数组 */
  configFields: ConfigField[]
  /** 更新配置字段值 */
  updateValue: (key: string, value: string) => void
  /** 语音测试回调 */
  onSpeechTest: () => Promise<{ success: boolean, message: string }>
  title: string
  icon: React.ReactNode
  description: string
}

export const AiConfigSection: React.FC<AiConfigSectionProps> = ({
  configFields,
  updateValue,
  onSpeechTest,
  title,
  icon,
  description,
}) => {
  const { t } = useI18n()
  const [speechTesting, setSpeechTesting] = useState(false)
  const [speechTestResult, setSpeechTestResult] = useState<{ success: boolean, message: string } | null>(null)

  // 辅助函数：获取配置字段值
  const getFieldValue = useCallback((key: string, defaultValue = '') => {
    return configFields.find(f => f.key === key)?.value || defaultValue
  }, [configFields])

  // 当前 AI Provider
  const currentProvider = useMemo(() =>
    getFieldValue('provider', 'gemini'), [getFieldValue])

  // 当前图片生成 Provider
  const currentImageProvider = useMemo(() =>
    getFieldValue('ai_image_provider', 'pollinations'), [getFieldValue])

  // AI Provider 选项
  const aiProviderOptions: SettingOption<string>[] = useMemo(() => [
    { value: 'gemini', label: 'Gemini', icon: <SiGooglegemini /> },
    { value: 'openai', label: 'OpenAI', icon: <SiOpenai /> },
  ], [])

  // 图片生成 Provider 选项
  const imageProviderOptions: SettingOption<string>[] = useMemo(() => [
    { value: 'pollinations', label: 'Pollinations', icon: <FaFreeCodeCamp />, badge: t.config.pollinationsFree },
    { value: 'imaginepro', label: 'ImaginePro', icon: <FaMagic />, badge: 'MJ' },
  ], [t.config.pollinationsFree])

  // Pollinations 模型选项
  const pollinationsModelOptions: SettingOption<string>[] = useMemo(() => [
    { value: 'flux-anime', label: t.config.fluxAnimeRecommend },
    { value: 'flux', label: t.config.fluxDefault },
    { value: 'flux-realism', label: t.config.fluxRealism },
    { value: 'flux-3d', label: t.config.flux3D },
  ], [t.config.fluxAnimeRecommend, t.config.fluxDefault, t.config.fluxRealism, t.config.flux3D])

  // 腾讯云区域选项
  const tencentRegionOptions: SettingOption<string>[] = useMemo(() => [
    { value: 'ap-guangzhou', label: t.config.tencentRegionGuangzhou },
    { value: 'ap-shanghai', label: t.config.tencentRegionShanghai },
    { value: 'ap-beijing', label: t.config.tencentRegionBeijing },
    { value: 'ap-chengdu', label: t.config.tencentRegionChengdu },
    { value: 'ap-chongqing', label: t.config.tencentRegionChongqing },
    { value: 'ap-nanjing', label: t.config.tencentRegionNanjing },
  ], [t.config.tencentRegionGuangzhou, t.config.tencentRegionShanghai, t.config.tencentRegionBeijing, t.config.tencentRegionChengdu, t.config.tencentRegionChongqing, t.config.tencentRegionNanjing])

  // Provider 对应的配置字段
  const providerFields = useMemo(() => {
    return configFields.filter((field) => {
      if (field.key === 'provider')
        return false
      if (field.key.startsWith('ai_image_') || field.key.startsWith('imaginepro_'))
        return false
      if (field.key.startsWith('tencent_'))
        return false

      if (currentProvider === 'gemini') {
        return field.key.startsWith('gemini_')
      }
      else if (currentProvider === 'openai') {
        return field.key.startsWith('openai_')
      }
      return false
    })
  }, [configFields, currentProvider])

  // 处理语音测试
  const handleSpeechTest = useCallback(async () => {
    setSpeechTesting(true)
    setSpeechTestResult(null)
    try {
      const result = await onSpeechTest()
      setSpeechTestResult(result)
    }
    catch (error) {
      setSpeechTestResult({
        success: false,
        message: error instanceof Error ? error.message : 'Test failed',
      })
    }
    finally {
      setSpeechTesting(false)
    }
  }, [onSpeechTest])

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
    >
      {/* AI 服务介绍 */}
      <InfoCard
        title={t.config.aiServiceInfoTitle}
        content={(
          <>
            {t.config.aiServiceInfoDescription}
            <br />
            <strong>Google Gemini</strong>
            :
            {t.config.geminiDescription}
            <a href="https://makersuite.google.com/app/apikey" target="_blank" rel="noopener noreferrer">
              {t.config.getApiKey}
            </a>
            <br />
            <strong>{t.config.openaiCompatible}</strong>
            :
            {t.config.openaiDescription}
          </>
        )}
      />

      {/* AI Provider 选择 */}
      <ProviderItem
        itemKey="ai_provider"
        label={t.config.aiProvider}
        value={currentProvider}
        onChange={v => updateValue('provider', v)}
        options={aiProviderOptions}
        hint={t.config.aiProviderHint}
        layout="horizontal"
      />

      {/* Provider 配置字段 */}
      {providerFields.map(field => (
        <InputItem
          key={field.key}
          itemKey={field.key}
          label={field.label}
          required={field.required}
          value={field.value}
          onChange={v => updateValue(field.key, v)}
          placeholder={field.placeholder}
          inputType={field.field_type as 'text' | 'password'}
          autoSelectOnMask
          layout="vertical"
        />
      ))}

      {/* AI 图片生成配置 */}
      <InfoCard
        title={t.config.aiImageTitle}
        icon={<FaPalette />}
        content={(
          <>
            <strong>Pollinations AI</strong>
            ：
            {t.config.pollinationsDescription}
            <br />
            <strong>ImaginePro</strong>
            ：
            {t.config.imagineproDescription}
          </>
        )}
        className="mt-6"
      />

      {/* 图片生成 Provider 选择 */}
      <ProviderItem
        itemKey="image_provider"
        label={t.config.imageGenService}
        value={currentImageProvider}
        onChange={v => updateValue('ai_image_provider', v)}
        options={imageProviderOptions}
        layout="horizontal"
      />

      {/* Pollinations 配置 */}
      {currentImageProvider === 'pollinations' && (
        <>
          <SelectItem
            itemKey="ai_image_model"
            label={t.config.aiModel}
            value={getFieldValue('ai_image_model', 'flux-anime')}
            onChange={v => updateValue('ai_image_model', v)}
            options={pollinationsModelOptions}
            layout="vertical"
          />
          <CompactSettingGroup>
            <NumberItem
              itemKey="ai_image_width_poll"
              label={t.config.width}
              value={Number.parseInt(getFieldValue('ai_image_width', '512'), 10)}
              onChange={v => updateValue('ai_image_width', String(v))}
              min={256}
              max={1024}
              step={64}
              layout="vertical"
            />
            <NumberItem
              itemKey="ai_image_height_poll"
              label={t.config.height}
              value={Number.parseInt(getFieldValue('ai_image_height', '768'), 10)}
              onChange={v => updateValue('ai_image_height', String(v))}
              min={256}
              max={1024}
              step={64}
              layout="vertical"
            />
          </CompactSettingGroup>
        </>
      )}

      {/* ImaginePro 配置 */}
      {currentImageProvider === 'imaginepro' && (
        <>
          <InputItem
            itemKey="imaginepro_api_key"
            label="API Key"
            required
            value={getFieldValue('imaginepro_api_key')}
            onChange={v => updateValue('imaginepro_api_key', v)}
            placeholder={t.config.imagineproPlaceholder}
            inputType="password"
            autoSelectOnMask
            layout="vertical"
          />
          <CompactSettingGroup>
            <NumberItem
              itemKey="ai_image_width_mj"
              label={t.config.width}
              value={Number.parseInt(getFieldValue('ai_image_width', '1024'), 10)}
              onChange={v => updateValue('ai_image_width', String(v))}
              min={512}
              max={2048}
              step={128}
              layout="vertical"
            />
            <NumberItem
              itemKey="ai_image_height_mj"
              label={t.config.height}
              value={Number.parseInt(getFieldValue('ai_image_height', '1536'), 10)}
              onChange={v => updateValue('ai_image_height', String(v))}
              min={512}
              max={2048}
              step={128}
              layout="vertical"
            />
          </CompactSettingGroup>
        </>
      )}

      {/* 语音服务配置 */}
      <SettingGroup
        title={t.config.speechServiceTitle}
        icon={<FaMicrophone />}
        description={t.config.speechServiceDesc}
      >
        <InputItem
          itemKey="tencent_secret_id"
          label={t.config.tencentSecretId}
          value={getFieldValue('tencent_secret_id')}
          onChange={v => updateValue('tencent_secret_id', v)}
          placeholder={t.config.tencentSecretIdPlaceholder}
          inputType="password"
          autoSelectOnMask
          layout="vertical"
        />

        <InputItem
          itemKey="tencent_secret_key"
          label={t.config.tencentSecretKey}
          value={getFieldValue('tencent_secret_key')}
          onChange={v => updateValue('tencent_secret_key', v)}
          placeholder={t.config.tencentSecretKeyPlaceholder}
          inputType="password"
          autoSelectOnMask
          layout="vertical"
        />

        <SelectItem
          itemKey="tencent_region"
          label={t.config.tencentRegion}
          value={getFieldValue('tencent_region', 'ap-guangzhou')}
          onChange={v => updateValue('tencent_region', v)}
          options={tencentRegionOptions}
          layout="vertical"
        />

        <ButtonItem
          itemKey="speech_test"
          label=""
          buttonText={t.config.speechTestAvailability}
          buttonIcon={<FaVolumeUp />}
          onClick={handleSpeechTest}
          loading={speechTesting}
          loadingText={t.config.speechTestTesting}
          result={speechTestResult}
          variant="secondary"
          layout="vertical"
        />
      </SettingGroup>
    </SettingSection>
  )
}

export default AiConfigSection
