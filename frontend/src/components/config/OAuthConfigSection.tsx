/**
 * OAuth 配置区块
 * 使用通用设置组件重构
 */

import { FaCheck, FaClipboard } from '@lib/icons'
import React, { useCallback, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  InfoCard,
  InputItem,
  SettingSection,
} from '../settings'

interface ConfigField {
  key: string
  value: string
}

interface OAuthConfigSectionProps {
  /** UI 配置字段数组 */
  configFields: ConfigField[]
  /** 更新配置字段值 */
  updateValue: (key: string, value: string) => void
  title: string
  icon: React.ReactNode
  description: string
}

export const OAuthConfigSection: React.FC<OAuthConfigSectionProps> = ({
  configFields,
  updateValue,
  title,
  icon,
  description,
}) => {
  const { t } = useI18n()
  const [copiedUrl, setCopiedUrl] = useState(false)

  // 辅助函数：获取配置字段值
  const getFieldValue = useCallback((key: string) => {
    return configFields.find(f => f.key === key)?.value || ''
  }, [configFields])

  const baseUrl = getFieldValue('base_url')
  const callbackUrl = baseUrl ? `${baseUrl.replace(/\/$/, '')}/api/auth/github/callback` : null

  const handleCopyUrl = useCallback(async () => {
    if (callbackUrl) {
      await navigator.clipboard.writeText(callbackUrl)
      setCopiedUrl(true)
      setTimeout(() => setCopiedUrl(false), 2000)
    }
  }, [callbackUrl])

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
    >
      {/* OAuth 配置指南 */}
      <InfoCard
        title={t.config.oauthGuideTitle}
        content={(
          <>
            1.
            {' '}
            {t.config.oauthGuideStep1}
            {' '}
            <a href="https://github.com/settings/developers" target="_blank" rel="noopener noreferrer">
              GitHub Developer Settings
            </a>
            <br />
            2.
            {' '}
            {t.config.oauthGuideStep2}
            <br />
            3.
            {' '}
            {t.config.oauthGuideStep3}
            <br />
            4.
            {' '}
            {t.config.oauthGuideStep4}
          </>
        )}
        className="info-card-spaced"
      />

      {/* 当前回调地址显示 */}
      <div className="config-field">
        <label className="field-label">{t.config.currentCallbackUrl}</label>
        {callbackUrl
          ? (
              <div className="callback-url-display">
                <code className="inline-code callback-url-code">{callbackUrl}</code>
                <button
                  type="button"
                  className="copy-btn"
                  onClick={handleCopyUrl}
                  title={copiedUrl ? 'Copied!' : 'Copy'}
                >
                  {copiedUrl ? <FaCheck /> : <FaClipboard />}
                </button>
              </div>
            )
          : (
              <div className="callback-url-not-configured">
                ⚠️
                {' '}
                {t.config.callbackUrlNotConfigured}
              </div>
            )}
        <p className="field-hint">{t.config.currentCallbackUrlHint}</p>
      </div>

      {/* GitHub Client ID */}
      <InputItem
        itemKey="github_client_id"
        label={t.config.githubClientId}
        required
        value={getFieldValue('github_client_id')}
        onChange={v => updateValue('github_client_id', v)}
        placeholder={t.config.githubClientIdPlaceholder}
        layout="vertical"
      />

      {/* GitHub Client Secret */}
      <InputItem
        itemKey="github_client_secret"
        label={t.config.githubClientSecret}
        required
        value={getFieldValue('github_client_secret')}
        onChange={v => updateValue('github_client_secret', v)}
        placeholder={t.config.githubClientSecretPlaceholder}
        inputType="password"
        autoSelectOnMask
        layout="vertical"
      />
    </SettingSection>
  )
}

export default OAuthConfigSection
