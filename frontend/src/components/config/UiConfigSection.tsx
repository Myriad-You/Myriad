/**
 * UI 基础配置区块
 * 使用通用设置组件重构
 */

import React, { useCallback, useMemo } from 'react';
import { useI18n } from '../../contexts/I18nContext';
import {
  SettingSection,
  SettingGroup,
  InputItem,
  CheckboxItem,
  SelectItem,
} from '../settings';
import { FaLink, FaGlobe, FaInfoCircle, FaPalette, FaMagic } from '@lib/icons';

interface ConfigField {
  key: string;
  label: string;
  field_type: string;
  value: string;
  placeholder: string;
  required: boolean;
}

interface UiConfigSectionProps {
  /** UI 配置字段数组 */
  configFields: ConfigField[];
  /** 更新配置字段值 */
  updateValue: (key: string, value: string) => void;
  /** 获取字段标签（国际化） */
  getFieldLabel: (key: string, originalLabel: string) => string;
  /** 获取字段占位符（国际化） */
  getFieldPlaceholder: (key: string, originalPlaceholder: string) => string;
  title: string;
  icon: React.ReactNode;
  description: string;
}

export const UiConfigSection: React.FC<UiConfigSectionProps> = ({
  configFields,
  updateValue,
  getFieldLabel,
  getFieldPlaceholder,
  title,
  icon,
  description,
}) => {
  const { t } = useI18n();

  // 辅助函数：获取配置字段值
  const getFieldValue = useCallback((key: string) => {
    return configFields.find(f => f.key === key)?.value || '';
  }, [configFields]);

  // 站点元数据字段
  const siteMetadataFields = useMemo(() =>
    configFields.filter(f => ['site_title', 'site_description', 'site_favicon'].includes(f.key)),
    [configFields]
  );

  // 背景主题字段（排除特定前缀和字段）
  const backgroundFields = useMemo(() =>
    configFields.filter(f =>
      !f.key.startsWith('pet_') &&
      !f.key.startsWith('github_') &&
      !f.key.startsWith('music_') &&
      !f.key.startsWith('proxy_') &&
      !f.key.startsWith('evocative_') &&
      !f.key.startsWith('site_') &&
      !f.key.endsWith('_base_url') &&
      !['base_url', 'wallpaper_parallax', 'cloud_sponsors'].includes(f.key)
    ),
    [configFields]
  );

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
    >
      {/* 站点 URL 配置 */}
      <SettingGroup
        title={t.config.siteUrlConfig}
        icon={<FaLink />}
      >
        <InputItem
          itemKey="base_url"
          label={t.config.baseUrl}
          value={getFieldValue('base_url')}
          onChange={(v) => updateValue('base_url', v)}
          placeholder={t.config.baseUrlPlaceholder}
          hint={t.config.baseUrlHint}
          layout="vertical"
        />
      </SettingGroup>

      {/* 站点元数据 */}
      <SettingGroup
        title={t.config.siteMetadata}
        icon={<FaGlobe />}
      >
        {siteMetadataFields.map((field) => (
          <InputItem
            key={field.key}
            itemKey={field.key}
            label={getFieldLabel(field.key, field.label)}
            required={field.required}
            value={field.value}
            onChange={(v) => updateValue(field.key, v)}
            placeholder={getFieldPlaceholder(field.key, field.placeholder)}
            multiline={field.key === 'site_description'}
            rows={2}
            layout="vertical"
          />
        ))}
      </SettingGroup>

      {/* 站点底部信息（备案和云赞助商） */}
      <SettingGroup
        title={t.config.siteFooterTitle}
        icon={<FaInfoCircle />}
        description={t.config.siteFooterDesc}
      >
        <InputItem
          itemKey="site_icp"
          label={t.config.siteIcp}
          value={getFieldValue('site_icp')}
          onChange={(v) => updateValue('site_icp', v)}
          placeholder={t.config.siteIcpPlaceholder}
          hint={t.config.siteIcpHint}
          layout="vertical"
        />
        <InputItem
          itemKey="site_gongan"
          label={t.config.siteGongan}
          value={getFieldValue('site_gongan')}
          onChange={(v) => updateValue('site_gongan', v)}
          placeholder={t.config.siteGonganPlaceholder}
          hint={t.config.siteGonganHint}
          layout="vertical"
        />
        {/* 云赞助商开关 */}
        <div className="setting-item vertical">
          <label className="setting-label">{t.config.cloudSponsors}</label>
          <p className="setting-hint">{t.config.cloudSponsorsHint}</p>
          <div className="cloud-sponsors-toggles">
            <CheckboxItem
              itemKey="sponsor_cloudflare"
              label=""
              checkboxLabel={t.config.cloudflare}
              value={getFieldValue('cloud_sponsors').includes('cloudflare')}
              onChange={(v) => {
                const current = getFieldValue('cloud_sponsors').split(',').map(s => s.trim()).filter(Boolean);
                const newSponsors = v
                  ? [...current.filter(s => s !== 'cloudflare'), 'cloudflare']
                  : current.filter(s => s !== 'cloudflare');
                updateValue('cloud_sponsors', newSponsors.join(','));
              }}
              layout="horizontal"
            />
            <CheckboxItem
              itemKey="sponsor_edgeone"
              label=""
              checkboxLabel={t.config.edgeone}
              value={getFieldValue('cloud_sponsors').includes('edgeone')}
              onChange={(v) => {
                const current = getFieldValue('cloud_sponsors').split(',').map(s => s.trim()).filter(Boolean);
                const newSponsors = v
                  ? [...current.filter(s => s !== 'edgeone'), 'edgeone']
                  : current.filter(s => s !== 'edgeone');
                updateValue('cloud_sponsors', newSponsors.join(','));
              }}
              layout="horizontal"
            />
            <CheckboxItem
              itemKey="sponsor_upyun"
              label=""
              checkboxLabel={t.config.upyun}
              value={getFieldValue('cloud_sponsors').includes('upyun')}
              onChange={(v) => {
                const current = getFieldValue('cloud_sponsors').split(',').map(s => s.trim()).filter(Boolean);
                const newSponsors = v
                  ? [...current.filter(s => s !== 'upyun'), 'upyun']
                  : current.filter(s => s !== 'upyun');
                updateValue('cloud_sponsors', newSponsors.join(','));
              }}
              layout="horizontal"
            />
          </div>
        </div>
      </SettingGroup>

      {/* 背景与主题 */}
      <SettingGroup title={`🎨 ${t.config.backgroundAndTheme}`}>
        {backgroundFields.map((field) => (
          <InputItem
            key={field.key}
            itemKey={field.key}
            label={getFieldLabel(field.key, field.label)}
            required={field.required}
            value={field.value}
            onChange={(v) => updateValue(field.key, v)}
            placeholder={getFieldPlaceholder(field.key, field.placeholder)}
            inputType={field.field_type as 'text' | 'password' | 'url' | 'email'}
            layout="vertical"
          />
        ))}
      </SettingGroup>

      {/* Evocative 壁纸动效 */}
      <SettingGroup
        title={t.config.evocativeTitle}
        icon={<FaMagic />}
        description={t.config.evocativeDesc}
      >
        {/* 微动效果 */}
        <CheckboxItem
          itemKey="evocative_parallax"
          label={t.config.fieldEvocativeParallax}
          checkboxLabel={t.config.fieldEvocativeParallaxHint}
          value={getFieldValue('evocative_parallax') === 'true'}
          onChange={(v) => updateValue('evocative_parallax', v ? 'true' : 'false')}
          layout="vertical"
        />

        {/* 动态模糊 */}
        <CheckboxItem
          itemKey="evocative_dynamic_blur"
          label={t.config.fieldEvocativeDynamicBlur}
          checkboxLabel={t.config.fieldEvocativeDynamicBlurHint}
          value={getFieldValue('evocative_dynamic_blur') === 'true'}
          onChange={(v) => updateValue('evocative_dynamic_blur', v ? 'true' : 'false')}
          layout="vertical"
        />

        {/* 涟漪效果 */}
        <CheckboxItem
          itemKey="evocative_ripple"
          label={t.config.fieldEvocativeRipple}
          checkboxLabel={t.config.fieldEvocativeRippleHint}
          value={getFieldValue('evocative_ripple') === 'true'}
          onChange={(v) => updateValue('evocative_ripple', v ? 'true' : 'false')}
          layout="vertical"
        />

        {/* 动效帧率 */}
        <SelectItem
          itemKey="evocative_fps"
          label={t.config.fieldEvocativeFps}
          value={getFieldValue('evocative_fps') || '30'}
          onChange={(v) => updateValue('evocative_fps', v)}
          options={[
            { value: '30', label: `30 FPS (${t.config.fpsBalanced})` },
            { value: '60', label: `60 FPS (${t.config.fpsSmooth})` },
          ]}
          hint={t.config.fieldEvocativeFpsHint}
          layout="vertical"
        />

        {/* 涟漪画质 */}
        <SelectItem
          itemKey="evocative_ripple_quality"
          label={t.config.fieldEvocativeRippleQuality}
          value={getFieldValue('evocative_ripple_quality') || '0.85'}
          onChange={(v) => updateValue('evocative_ripple_quality', v)}
          options={[
            { value: '0.5', label: `50% (${t.config.qualityLow})` },
            { value: '0.65', label: `65% (${t.config.qualityMedium})` },
            { value: '0.85', label: `85% (${t.config.qualityHigh})` },
            { value: '1', label: `100% (${t.config.qualityUltra})` },
          ]}
          hint={t.config.fieldEvocativeRippleQualityHint}
          layout="vertical"
        />
      </SettingGroup>
    </SettingSection>
  );
};

export default UiConfigSection;
