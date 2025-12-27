/**
 * 网络代理配置区块
 * 使用通用设置组件重构
 */

import React, { useCallback } from 'react';
import { useI18n } from '../../contexts/I18nContext';
import { FaLightbulb, FaExchangeAlt } from '@lib/icons';
import {
  SettingSection,
  SettingGroup,
  InfoCard,
  SwitchItem,
  InputItem,
} from '../settings';

interface ConfigField {
  key: string;
  value: string;
}

interface NetworkConfigSectionProps {
  /** UI 配置字段数组 */
  configFields: ConfigField[];
  /** 更新配置字段值 */
  updateValue: (key: string, value: string) => void;
  title: string;
  icon: React.ReactNode;
  description: string;
}

export const NetworkConfigSection: React.FC<NetworkConfigSectionProps> = ({
  configFields,
  updateValue,
  title,
  icon,
  description,
}) => {
  const { t } = useI18n();

  // 辅助函数：获取配置字段值
  const getFieldValue = useCallback((key: string) => {
    return configFields.find(f => f.key === key)?.value || '';
  }, [configFields]);

  const isProxyEnabled = getFieldValue('proxy_enabled') === 'true';

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
    >
      {/* 代理说明 */}
      <InfoCard
        title={t.config.networkProxyInfoTitle || '代理配置说明'}
        icon={<FaLightbulb />}
        content={
          <>
            {t.config.networkProxyInfo || '如果您的服务器位于中国大陆，可能需要配置代理才能正常访问 GitHub OAuth、Gemini AI 等外部服务。您可以选择以下方式：'}
            <br /><br />
            <strong>1. HTTP/SOCKS {t.config.proxyOption || '代理'}</strong>：
            {t.config.proxyOptionDesc || '配置代理服务器地址，所有外部请求将通过代理发送。'}
            <br />
            <strong>2. API {t.config.mirrorOption || '镜像服务'}</strong>：
            {t.config.mirrorOptionDesc || '使用第三方 API 镜像/中转服务，无需配置代理。'}
          </>
        }
        className="info-card-spaced"
      />

      {/* 代理开关 */}
      <SwitchItem
        itemKey="proxy_enabled"
        label={t.config.enableProxy || '启用网络代理'}
        description={t.config.enableProxyHint || '开启后将使用代理访问外部API'}
        value={isProxyEnabled}
        onChange={(v) => updateValue('proxy_enabled', v.toString())}
        layout="horizontal"
      />

      {/* 代理配置（仅在启用时显示） */}
      {isProxyEnabled && (
        <>
          <InputItem
            itemKey="proxy_url"
            label={t.config.proxyUrl || '代理地址'}
            value={getFieldValue('proxy_url')}
            onChange={(v) => updateValue('proxy_url', v)}
            placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
            hint={t.config.proxyUrlHint || '支持 HTTP、HTTPS、SOCKS5 代理协议'}
            layout="vertical"
          />

          <InputItem
            itemKey="proxy_bypass"
            label={t.config.proxyBypass || '代理绕过列表'}
            value={getFieldValue('proxy_bypass')}
            onChange={(v) => updateValue('proxy_bypass', v)}
            placeholder="localhost,127.0.0.1,bilibili.com"
            hint={t.config.proxyBypassHint || '不使用代理的域名，用逗号分隔。国内服务（如 Bilibili）建议添加到绕过列表'}
            layout="vertical"
          />
        </>
      )}

      {/* API 镜像配置 */}
      <SettingGroup
        title={t.config.apiMirrorConfig || 'API 镜像服务'}
        icon={<FaExchangeAlt />}
        description={t.config.apiMirrorConfigHint || '使用第三方 API 镜像服务，可替代代理配置'}
      >
        <InputItem
          itemKey="gemini_base_url"
          label={t.config.geminiBaseUrl || 'Gemini API 基础地址'}
          value={getFieldValue('gemini_base_url')}
          onChange={(v) => updateValue('gemini_base_url', v)}
          placeholder="https://generativelanguage.googleapis.com"
          hint={t.config.geminiBaseUrlHint || '留空使用官方地址，可填写第三方代理服务地址'}
          layout="vertical"
        />

        <InputItem
          itemKey="github_api_base_url"
          label={t.config.githubApiBaseUrl || 'GitHub API 基础地址'}
          value={getFieldValue('github_api_base_url')}
          onChange={(v) => updateValue('github_api_base_url', v)}
          placeholder="https://api.github.com"
          hint={t.config.githubApiBaseUrlHint || '留空使用官方地址，可填写 GitHub API 镜像地址（注意：OAuth 认证仍需使用官方地址）'}
          layout="vertical"
        />
      </SettingGroup>
    </SettingSection>
  );
};

export default NetworkConfigSection;
