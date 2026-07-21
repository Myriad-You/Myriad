/**
 * UI 基础配置区块
 * 使用通用设置组件重构
 */

import {
  FaExchangeAlt,
  FaGlobe,
  FaInfoCircle,
  FaLink,
  FaMagic,
  LuPalette,
  SiCloudflare,
} from '@lib/icons'
import React, { useCallback, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { ApiError } from '../../services/api'
import {
  changeSiteDomain,
  checklistItems,
  type ChangeSiteDomainResponse,
  type DomainChecklistItem,
} from '../../services/siteDomainApi'

import {
  ButtonItem,
  CheckboxGroupItem,
  InputItem,
  SelectItem,
  SettingGroup,
  SettingSection,
} from '../settings'

// EdgeOne Logo
const EdgeOneIcon: React.FC = () => (
  <svg viewBox="0 0 32 32" fill="none">
    <path
      d="M29.8101 18.138C29.9349 17.4442 30 16.7297 30 16C30 15.3831 29.9535 14.7772 29.8637 14.1854C29.829 13.9567 29.6296 13.792 29.3983 13.792H21.6802C21.4277 13.792 21.2439 13.5525 21.3093 13.3086L22.2229 9.89892C22.2904 9.6471 22.5185 9.472 22.7792 9.472H27.3634C27.668 9.472 27.8488 9.13574 27.6682 8.89047C25.4834 5.9244 21.9664 4 18 4C11.3726 4 6 9.37258 6 16C6 19.0173 7.11361 21.7745 8.95224 23.883C9.14804 24.1076 9.5076 24.0146 9.58436 23.7268L12.2394 13.7702C12.2882 13.5874 12.1504 13.408 11.9612 13.408H9.65504C9.40274 13.408 9.21899 13.1689 9.284 12.9251L10.0327 10.1174C10.0889 9.90673 10.28 9.76117 10.498 9.75666C13.0104 9.70465 15.493 9.04698 17.6975 7.84351C17.9253 7.71913 18.2007 7.92739 18.1338 8.1782L13.2499 26.4929C13.177 26.7664 13.313 27.0538 13.5761 27.1582C14.9451 27.7014 16.4377 28 18 28C21.7878 28 25.1656 26.2451 27.3649 23.5039C27.5597 23.2611 27.3809 22.912 27.0696 22.912H19.2365C18.984 22.912 18.8002 22.6725 18.8656 22.4286L19.7792 19.0189C19.8467 18.7671 20.0749 18.592 20.3356 18.592H29.2564C29.5268 18.592 29.7622 18.4042 29.8101 18.138Z"
      fill="#0055D2"
    />
  </svg>
)

// 又拍云 Logo
const UpyunIcon: React.FC = () => (
  <svg viewBox="195 270 100 135">
    <path
      fill="#00a0ff"
      d="M282.639,281.223L282.639,281.223L282.639,281.223L282.639,281.223L282.639,281.223c-2.473-1.861-5.034-3.52-7.664-4.983c-1.904-1.059-4.295-0.564-5.604,1.177l-16.492,21.912l-1.176,1.563c-1.786,2.373-4.638,3.665-7.605,3.529c-1.082-0.049-2.164-0.039-3.242,0.029c-8.289,0.525-16.308,4.525-21.694,11.681c-4.33,5.753-6.229,12.576-5.879,19.245c0.063,1.201,0.757,2.298,1.851,2.796c2.27,1.032,4.017,3.137,4.454,5.83c0.618,3.809-1.722,7.551-5.418,8.659c-4.357,1.306-8.832-1.363-9.814-5.724c-0.532-2.362,0.079-4.711,1.463-6.48c0.768-0.981,1.154-2.203,1.044-3.444c-0.758-8.552,1.52-17.402,7.09-24.802c7.026-9.334,17.717-14.274,28.562-14.337c2.09-0.012,4.052-1.011,5.309-2.681l15.188-20.178c1.189-1.579,0.421-3.848-1.476-4.405c-25.43-7.46-53.905,0.934-70.96,23.146c-22.099,28.781-16.729,70.279,11.987,92.462c2.687,2.076,5.482,3.909,8.36,5.508c1.893,1.052,4.274,0.537,5.577-1.193l16.492-21.911l1.176-1.563c1.786-2.373,4.638-3.665,7.605-3.529c1.082,0.049,2.164,0.039,3.242-0.029c8.289-0.525,16.308-4.525,21.694-11.681c4.33-5.753,6.229-12.576,5.879-19.245c-0.063-1.201-0.757-2.298-1.851-2.796c-2.27-1.032-4.017-3.137-4.454-5.83c-0.618-3.809,1.722-7.551,5.418-8.658c4.357-1.306,8.832,1.363,9.814,5.724c0.532,2.362-0.079,4.711-1.463,6.48c-0.768,0.981-1.154,2.203-1.044,3.444c0.758,8.552-1.52,17.402-7.09,24.802c-7.026,9.334-17.717,14.274-28.562,14.337c-2.09,0.012-4.052,1.011-5.309,2.681l-15.187,20.177c-1.18,1.568-0.439,3.842,1.444,4.396c25.633,7.534,54.368-1.045,71.384-23.652C317.614,344.545,311.773,303.151,282.639,281.223z"
    />
  </svg>
)

interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

interface UiConfigSectionProps {
  /** UI 配置字段数组 */
  configFields: ConfigField[]
  /** 更新配置字段值 */
  updateValue: (key: string, value: string) => void
  /** 获取字段标签（国际化） */
  getFieldLabel: (key: string, originalLabel: string) => string
  /** 获取字段占位符（国际化） */
  getFieldPlaceholder: (key: string, originalPlaceholder: string) => string
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
}

export const UiConfigSection: React.FC<UiConfigSectionProps> = ({
  configFields,
  updateValue,
  getFieldLabel,
  getFieldPlaceholder,
  title,
  icon,
  description,
  sectionId,
}) => {
  const { t } = useI18n()
  const [domainDraft, setDomainDraft] = useState('')
  const [domainLoading, setDomainLoading] = useState(false)
  const [domainResult, setDomainResult] = useState<{
    success: boolean
    message: string
  } | null>(null)
  const [domainChecklist, setDomainChecklist] = useState<
    DomainChecklistItem[]
  >([])
  const [domainApplied, setDomainApplied] = useState<
    ChangeSiteDomainResponse['applied'] | null
  >(null)

  // 辅助函数：获取配置字段值
  const getFieldValue = useCallback(
    (key: string) => {
      return configFields.find((f) => f.key === key)?.value || ''
    },
    [configFields],
  )

  const checklistLabel = useCallback(
    (key: string) => {
      const map = t.config.domainChecklist as Record<string, string> | undefined
      return map?.[key] || key
    },
    [t.config.domainChecklist],
  )

  const handleChangeDomain = useCallback(async () => {
    const next = domainDraft.trim()
    if (!next) {
      setDomainResult({
        success: false,
        message: t.config.domainChangeEmpty,
      })
      return
    }
    const current = getFieldValue('base_url').replace(/\/$/, '')
    if (
      !window.confirm(
        t.config.domainChangeConfirm.replace('{origin}', next),
      )
    ) {
      return
    }

    setDomainLoading(true)
    setDomainResult(null)
    setDomainChecklist([])
    setDomainApplied(null)
    try {
      const res = await changeSiteDomain({
        new_origin: next,
        previous_origin: current || undefined,
      })
      if (res.success && res.applied) {
        updateValue('base_url', res.applied.base_url)
        setDomainDraft(res.applied.base_url)
        setDomainApplied(res.applied)
        setDomainChecklist(checklistItems(res.checklist))
        setDomainResult({
          success: true,
          message: res.message || t.config.domainChangeSuccess,
        })
      } else {
        setDomainResult({
          success: false,
          message: res.message || t.config.domainChangeFailed,
        })
      }
    } catch (err) {
      const message =
        err instanceof ApiError
          ? err.message
          : err instanceof Error
            ? err.message
            : t.config.domainChangeFailed
      setDomainResult({ success: false, message })
    } finally {
      setDomainLoading(false)
    }
  }, [domainDraft, getFieldValue, t, updateValue])

  // 站点元数据字段
  const siteMetadataFields = useMemo(
    () =>
      configFields.filter((f) =>
        ['site_title', 'site_description', 'site_favicon'].includes(f.key),
      ),
    [configFields],
  )

  // 背景主题字段（排除特定前缀和字段）
  const backgroundFields = useMemo(
    () =>
      configFields.filter(
        (f) =>
          !f.key.startsWith('pet_') &&
          !f.key.startsWith('github_') &&
          !f.key.startsWith('music_') &&
          !f.key.startsWith('proxy_') &&
          !f.key.startsWith('evocative_') &&
          !f.key.startsWith('site_') &&
          !f.key.endsWith('_base_url') &&
          !['base_url', 'wallpaper_parallax', 'cloud_sponsors'].includes(f.key),
      ),
    [configFields],
  )

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      {/* 站点 URL 配置 */}
      <SettingGroup title={t.config.siteUrlConfig} icon={<FaLink />}>
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

      {/* 更换域名（站点访问身份，非联邦 Move） */}
      <SettingGroup
        title={t.config.domainChangeTitle}
        icon={<FaExchangeAlt />}
        description={t.config.domainChangeDesc}
      >
        <InputItem
          itemKey="new_site_origin"
          label={t.config.domainChangeNewOrigin}
          value={domainDraft}
          onChange={setDomainDraft}
          placeholder={t.config.domainChangePlaceholder}
          hint={t.config.domainChangeHint}
          layout="vertical"
        />
        <ButtonItem
          label={t.config.domainChangeAction}
          description={t.config.domainChangeActionDesc}
          buttonText={
            domainLoading
              ? t.config.domainChangeApplying
              : t.config.domainChangeApply
          }
          onClick={() => {
            void handleChangeDomain()
          }}
          variant="primary"
          disabled={domainLoading || !domainDraft.trim()}
          loading={domainLoading}
          result={domainResult}
          layout="vertical"
        />
        {domainApplied && (
          <div
            className="setting-item-hint"
            style={{ marginTop: '0.5rem', fontSize: '0.875rem' }}
          >
            <div>
              <strong>BASE_URL / FRONTEND_URL:</strong>{' '}
              {domainApplied.base_url}
            </div>
            <div>
              <strong>CORS_ORIGINS:</strong> {domainApplied.cors_origins}
            </div>
          </div>
        )}
        {domainChecklist.length > 0 && (
          <div style={{ marginTop: '0.75rem' }}>
            <div
              style={{
                fontWeight: 600,
                marginBottom: '0.5rem',
                fontSize: '0.9rem',
              }}
            >
              {t.config.domainChecklistTitle}
            </div>
            <ul
              style={{
                margin: 0,
                paddingLeft: '1.25rem',
                fontSize: '0.875rem',
                lineHeight: 1.55,
              }}
            >
              {domainChecklist.map((item) => (
                <li key={item.key} style={{ marginBottom: '0.35rem' }}>
                  <strong>{checklistLabel(item.key)}</strong>
                  <span style={{ opacity: 0.75 }}> ({item.status})</span>
                  <div style={{ opacity: 0.9 }}>{item.summary}</div>
                </li>
              ))}
            </ul>
            <p
              style={{
                marginTop: '0.75rem',
                fontSize: '0.8125rem',
                opacity: 0.8,
              }}
            >
              {t.config.domainFederationNote}
            </p>
          </div>
        )}
      </SettingGroup>

      {/* 站点元数据 */}
      <SettingGroup title={t.config.siteMetadata} icon={<FaGlobe />}>
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
        <CheckboxGroupItem
          label={t.config.cloudSponsors}
          description={t.config.cloudSponsorsHint}
          options={[
            {
              key: 'cloudflare',
              label: t.config.cloudflare,
              icon: <SiCloudflare style={{ color: '#F38020' }} />,
              value: getFieldValue('cloud_sponsors').includes('cloudflare'),
            },
            {
              key: 'edgeone',
              label: t.config.edgeone,
              icon: <EdgeOneIcon />,
              value: getFieldValue('cloud_sponsors').includes('edgeone'),
            },
            {
              key: 'upyun',
              label: t.config.upyun,
              icon: <UpyunIcon />,
              value: getFieldValue('cloud_sponsors').includes('upyun'),
            },
          ]}
          onChange={(key, checked) => {
            const current = getFieldValue('cloud_sponsors')
              .split(',')
              .map((s) => s.trim())
              .filter(Boolean)
            const newSponsors = checked
              ? [...current.filter((s) => s !== key), key]
              : current.filter((s) => s !== key)
            updateValue('cloud_sponsors', newSponsors.join(','))
          }}
        />
      </SettingGroup>

      {/* 背景与主题 */}
      <SettingGroup title={t.config.backgroundAndTheme} icon={<LuPalette />}>
        {backgroundFields.map((field) => (
          <InputItem
            key={field.key}
            itemKey={field.key}
            label={getFieldLabel(field.key, field.label)}
            required={field.required}
            value={field.value}
            onChange={(v) => updateValue(field.key, v)}
            placeholder={getFieldPlaceholder(field.key, field.placeholder)}
            inputType={
              field.field_type as 'text' | 'password' | 'url' | 'email'
            }
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
        {/* 微动效果 / 动态模糊 / 涟漪效果 */}
        <CheckboxGroupItem
          label={t.config.evocativeEffects || '动效开关'}
          options={[
            {
              key: 'evocative_parallax',
              label: t.config.fieldEvocativeParallax,
              description: t.config.fieldEvocativeParallaxHint,
              value: getFieldValue('evocative_parallax') === 'true',
            },
            {
              key: 'evocative_dynamic_blur',
              label: t.config.fieldEvocativeDynamicBlur,
              description: t.config.fieldEvocativeDynamicBlurHint,
              value: getFieldValue('evocative_dynamic_blur') === 'true',
            },
            {
              key: 'evocative_ripple',
              label: t.config.fieldEvocativeRipple,
              description: t.config.fieldEvocativeRippleHint,
              value: getFieldValue('evocative_ripple') === 'true',
            },
          ]}
          onChange={(key, checked) =>
            updateValue(key, checked ? 'true' : 'false')
          }
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
  )
}

export default UiConfigSection
