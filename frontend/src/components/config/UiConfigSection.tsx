/**
 * UI 基础配置区块
 * 使用通用设置组件重构
 */

import type { FooterCustomItem } from '../../utils/footerCustomLogic'
import {
  FaGlobe,
  FaInfoCircle,
  FaLink,
  FaMagic,
  FaPlus,
  FaSearch,
  FaTrash,
  LuPalette,
  SiCloudflare,
} from '@lib/icons'
import React, { useCallback, useMemo, useState } from 'react'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { getCSRFToken } from '../../utils/csrf'
import {
  emptyFooterCustomItem,
  FOOTER_CUSTOM_MAX,

  parseFooterCustomSlots,
  serializeFooterCustom,
} from '../../utils/footerCustomLogic'
import { httpStatusMessage, userFacingError } from '../../utils/userFacingError'
import {
  CheckboxGroupItem,
  InputItem,
  SegmentedControl,
  SettingAnchoredPanel,
  SettingGroup,
  SettingsButton,
  SettingSection,
  SliderItem,
  SwitchItem,
  useSettingGuide,
} from '../settings'
import { SettingItemWrapper } from '../settings/items/SettingItemWrapper'
import { SettingTitleTag } from '../settings/SettingTitleTag'
import { SiteUrlField } from './SiteUrlField'
import './UiConfigSection.css'

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

/** @deprecated 从 uiBagOwnership 导入；此处 re-export 保持兼容 */
export { UI_RESET_KEYS } from './uiBagOwnership'

interface UiConfigSectionProps {
  /** UI 配置字段数组 */
  configFields: ConfigField[]
  /** 更新配置字段值（域名应用成功后请用 silent，避免触发统一保存 dirty） */
  updateValue: (
    key: string,
    value: string,
    options?: { silent?: boolean },
  ) => void
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
}

export const UiConfigSection: React.FC<UiConfigSectionProps> = ({
  configFields,
  updateValue,
  title,
  icon,
  description,
  sectionId,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  type SeoAiField = 'site_description' | 'site_keywords' | 'site_ai_intro'

  /** Which SEO field is currently being AI-generated (tag loading state) */
  const [aiGenField, setAiGenField] = useState<SeoAiField | null>(null)
  /** Feedback pinned under the field that was just generated */
  const [aiGenFeedback, setAiGenFeedback] = useState<{
    field: SeoAiField
    message: string
  } | null>(null)
  /** Open anchored panel field + optional owner hint draft */
  const [aiGenTipField, setAiGenTipField] = useState<SeoAiField | null>(null)
  const [aiGenHint, setAiGenHint] = useState('')

  // 标签归属本 Section，壳层不再维护死字段 label map
  const getFieldLabel = useCallback(
    (fieldKey: string, originalLabel: string): string => {
      const labels: Record<string, string> = {
        wallpaper_url: t.config.fieldWallpaperUrl,
        wallpaper_blur: t.config.fieldWallpaperBlur,
        site_title: t.config.fieldSiteTitle,
        site_description: t.config.fieldSiteDescription,
        site_favicon: t.config.fieldSiteFavicon,
        site_keywords: t.config.fieldSiteKeywords,
        site_og_image: t.config.fieldSiteOgImage,
        site_noindex: t.config.fieldSiteNoindex,
        site_visibility_policy: t.config.fieldSiteVisibilityPolicy,
        site_ai_intro: t.config.fieldSiteAiIntro,
      }
      return labels[fieldKey] || originalLabel
    },
    [t],
  )

  const getFieldPlaceholder = useCallback(
    (fieldKey: string, originalPlaceholder: string): string => {
      const placeholders: Record<string, string> = {
        wallpaper_url: t.config.placeholderWallpaperUrl,
        site_title: t.config.placeholderSiteTitle,
        site_description: t.config.placeholderSiteDescription,
        site_favicon: t.config.placeholderSiteFavicon,
        site_keywords: t.config.placeholderSiteKeywords,
        site_og_image: t.config.placeholderSiteOgImage,
      }
      return placeholders[fieldKey] || originalPlaceholder
    },
    [t],
  )

  // 辅助函数：获取配置字段值
  const getFieldValue = useCallback(
    (key: string) => {
      return configFields.find((f) => f.key === key)?.value || ''
    },
    [configFields],
  )

  const visibilityPolicy = useMemo(() => {
    const raw = getFieldValue('site_visibility_policy').trim()
    if (
      raw === 'private' ||
      raw === 'search_only' ||
      raw === 'ai_citation' ||
      raw === 'ai_full'
    ) {
      return raw
    }
    return getFieldValue('site_noindex') === 'true' ? 'private' : 'ai_full'
  }, [getFieldValue])

  /** Per-policy tip with inline code for paths / technical tokens */
  const visibilityPolicyHint = useMemo(() => {
    const code = (text: string, key?: string) => (
      <code key={key ?? text} className="inline-code">
        {text}
      </code>
    )
    switch (visibilityPolicy) {
      case 'private':
        return (
          <>
            {t.config.visibilityPrivateHintBefore}
            {code('noindex')}
            {t.config.visibilityPrivateHintMid}
            {code('sitemap')}
            {t.config.visibilityPrivateHintMid2}
            {code('/llms.txt')}
            {t.config.visibilityPrivateHintAfter}
          </>
        )
      case 'search_only':
        return (
          <>
            {t.config.visibilitySearchOnlyHintBefore}
            {code('Google')}
            {' / '}
            {code('Bing')}
            {t.config.visibilitySearchOnlyHintAfter}
          </>
        )
      case 'ai_citation':
        return (
          <>
            {t.config.visibilityAiCitationHintBefore}
            {code('/llms.txt')}
            {t.config.visibilityAiCitationHintAfter}
          </>
        )
      default:
        return (
          <>
            {t.config.visibilityAiFullHintBefore}
            {code('/llms.txt')}
            {t.config.visibilityAiFullHintAfter}
          </>
        )
    }
  }, [t.config, visibilityPolicy])

  const tryOpenAiGenTip = useCallback(
    (field: SeoAiField) => {
      const title = getFieldValue('site_title').trim()
      if (!title) {
        setAiGenFeedback({
          field,
          message: t.config.siteAiGenerateNeedTitle,
        })
        setAiGenTipField(null)
        return
      }
      setAiGenHint('')
      setAiGenTipField(field)
    },
    [getFieldValue, t.config.siteAiGenerateNeedTitle],
  )

  const handleAiGenerateField = useCallback(
    async (field: SeoAiField, hint: string) => {
      const title = getFieldValue('site_title').trim()
      const description = getFieldValue('site_description').trim()
      if (!title) {
        setAiGenFeedback({
          field,
          message: t.config.siteAiGenerateNeedTitle,
        })
        return
      }
      setAiGenTipField(null)
      setAiGenField(field)
      setAiGenFeedback(null)
      try {
        const base = API_URL || ''
        const csrfToken = await getCSRFToken()
        if (!csrfToken) {
          throw new Error('csrf')
        }
        const res = await fetch(`${base}/api/seo/generate-copy`, {
          method: 'POST',
          credentials: 'include',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': csrfToken,
          },
          body: JSON.stringify({
            site_title: title,
            site_description: description,
            hint: hint.trim(),
            fields: [field],
          }),
        })
        if (!res.ok) {
          throw new Error(httpStatusMessage(res.status))
        }
        const data = (await res.json()) as {
          site_description?: string
          site_keywords?: string
          site_ai_intro?: string
          source?: string
        }
        const generated =
          field === 'site_description'
            ? data.site_description
            : field === 'site_keywords'
              ? data.site_keywords
              : data.site_ai_intro
        if (!generated?.trim()) {
          // HTTP 200 with empty field must not look like success
          setAiGenFeedback({
            field,
            message: t.config.siteAiGenerateError,
          })
          return
        }
        if (field === 'site_description') {
          updateValue('site_description', generated)
        } else if (field === 'site_keywords') {
          updateValue('site_keywords', generated)
        } else {
          updateValue('site_ai_intro', generated)
        }
        setAiGenFeedback({
          field,
          message:
            data.source === 'fallback'
              ? t.config.siteAiGenerateFallback
              : t.config.siteAiGenerateSuccess,
        })
      } catch (err) {
        setAiGenFeedback({
          field,
          message: userFacingError(err, t.config.siteAiGenerateError),
        })
      } finally {
        setAiGenField(null)
      }
    },
    [getFieldValue, t.config, updateValue],
  )

  const renderSeoAiPanel = useCallback(
    (field: SeoAiField) => {
      const busy = aiGenField === field
      const anyBusy = aiGenField !== null
      return (
        <SettingAnchoredPanel
          open={aiGenTipField === field}
          onOpenChange={(next) => {
            if (!next) {
              if (aiGenField === null) setAiGenTipField(null)
              return
            }
            tryOpenAiGenTip(field)
          }}
          preventClose={anyBusy}
          disabled={anyBusy && !busy}
          placement="bottom"
          align="start"
          ariaLabel={t.config.siteAiGenerateDialogTitle}
          className="seo-ai-gen-tag-anchor"
          trigger={({ toggle }) => (
            <SettingTitleTag
              variant="muted"
              icon={<FaMagic />}
              disabled={anyBusy}
              onClick={toggle}
              title={
                busy
                  ? t.config.siteAiGenerating
                  : t.config.siteAiGenerateTagHint
              }
            >
              {busy ? t.config.siteAiGenerating : t.config.siteAiGenerateTag}
            </SettingTitleTag>
          )}
        >
          <div className="setting-anchored-panel-title">
            {t.config.siteAiGenerateDialogTitle}
          </div>
          <p className="setting-anchored-panel-desc">
            {t.config.siteAiGenerateDialogDesc}
          </p>
          <label
            className="setting-anchored-panel-label"
            htmlFor={`seo-ai-gen-hint-${field}`}
          >
            {t.config.siteAiGenerateHintLabel}
            <span className="setting-anchored-panel-label-meta">
              {t.config.siteAiGenerateHintOptional}
            </span>
          </label>
          <textarea
            id={`seo-ai-gen-hint-${field}`}
            className="setting-anchored-panel-input"
            rows={2}
            value={aiGenTipField === field ? aiGenHint : ''}
            onChange={(e) => setAiGenHint(e.target.value)}
            placeholder={t.config.siteAiGenerateHintPlaceholder}
            disabled={anyBusy}
            autoFocus
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                e.preventDefault()
                void handleAiGenerateField(field, aiGenHint)
              }
            }}
          />
          <div className="setting-anchored-panel-actions">
            <SettingsButton
              size="sm"
              variant="secondary"
              disabled={anyBusy}
              onClick={() => setAiGenTipField(null)}
            >
              {t.common.cancel}
            </SettingsButton>
            <SettingsButton
              size="sm"
              variant="primary"
              loading={busy}
              disabled={anyBusy}
              icon={<FaMagic />}
              onClick={() => void handleAiGenerateField(field, aiGenHint)}
            >
              {t.config.siteAiGenerateConfirm}
            </SettingsButton>
          </div>
        </SettingAnchoredPanel>
      )
    },
    [
      aiGenField,
      aiGenHint,
      aiGenTipField,
      handleAiGenerateField,
      t.common.cancel,
      t.config,
      tryOpenAiGenTip,
    ],
  )

  const baseUrlValue = getFieldValue('base_url')

  /** 涟漪画质：配置存 0–1，UI 用 50–100 的整数百分比 */
  const rippleQualityPercent = useMemo(() => {
    const raw = Number.parseFloat(
      getFieldValue('evocative_ripple_quality') || '0.85',
    )
    const pct = Math.round((Number.isFinite(raw) ? raw : 0.85) * 100)
    return Math.min(100, Math.max(50, pct))
  }, [getFieldValue])

  const handleSiteUrlApplied = useCallback(
    (url: string) => {
      updateValue('base_url', url, { silent: true })
    },
    [updateValue],
  )

  const footerCustomItems = useMemo(
    () => parseFooterCustomSlots(getFieldValue('site_footer_custom')),
    [getFieldValue],
  )

  const commitFooterCustom = useCallback(
    (items: FooterCustomItem[]) => {
      updateValue('site_footer_custom', serializeFooterCustom(items))
    },
    [updateValue],
  )

  const updateFooterCustomAt = useCallback(
    (index: number, patch: Partial<FooterCustomItem>) => {
      const next = footerCustomItems.map((it, i) =>
        i === index ? { ...it, ...patch } : it,
      )
      commitFooterCustom(next)
    },
    [commitFooterCustom, footerCustomItems],
  )

  const addFooterCustom = useCallback(() => {
    if (footerCustomItems.length >= FOOTER_CUSTOM_MAX) return
    commitFooterCustom([...footerCustomItems, emptyFooterCustomItem()])
  }, [commitFooterCustom, footerCustomItems])

  const removeFooterCustomAt = useCallback(
    (index: number) => {
      commitFooterCustom(footerCustomItems.filter((_, i) => i !== index))
    },
    [commitFooterCustom, footerCustomItems],
  )

  // 站点元数据字段
  const siteMetadataFields = useMemo(
    () =>
      configFields.filter((f) =>
        ['site_title', 'site_description', 'site_favicon'].includes(f.key),
      ),
    [configFields],
  )

  // 背景主题：只认明确属于本组的字段（勿用排除法——ui_config 是跨页共用的大袋子）
  const backgroundFields = useMemo(() => {
    const order = ['wallpaper_url', 'wallpaper_blur'] as const
    const byKey = new Map(configFields.map((f) => [f.key, f]))
    return order.flatMap((key) => {
      const field = byKey.get(key)
      return field ? [field] : []
    })
  }, [configFields])

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      {/* 站点地址：只读 → 点击编辑 → 框内保存（独立 API + 运维清单） */}
      <SettingGroup
        title={t.config.siteUrlConfig}
        description={t.config.siteUrlFieldDesc}
        {...bindGuide('ui.siteUrl', g.ui.siteUrl)}
        icon={<FaLink />}
      >
        <SiteUrlField value={baseUrlValue} onApplied={handleSiteUrlApplied} />
      </SettingGroup>

      {/* 站点名片 + PWA：标题/简介/图标与可安装应用开关同组 */}
      <SettingGroup
        title={t.config.siteIdentity}
        description={t.config.siteIdentityDesc}
        {...bindGuide('ui.siteIdentity', g.ui.siteIdentity)}
        icon={<FaGlobe />}
      >
        {siteMetadataFields.map((field) =>
          field.key === 'site_favicon' ? (
            <InputItem
              key={field.key}
              itemKey={field.key}
              label={getFieldLabel(field.key, field.label)}
              required={field.required}
              value={field.value}
              onChange={(v) => updateValue(field.key, v)}
              placeholder={getFieldPlaceholder(field.key, field.placeholder)}
              inputType="url"
              variant="imageUpload"
              accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml,image/x-icon,.ico"
              maxImageBytes={512 * 1024}
              uploadLabel={t.config.imageUpload}
              clearImageLabel={t.config.imageUploadClear}
              localImageLabel={t.config.imageUploadLocal}
              previewAlt={getFieldLabel(field.key, field.label)}
              imageTypeError={t.config.imageUploadTypeError}
              imageSizeError={t.config.imageUploadSizeError}
              imageReadError={t.config.imageUploadReadError}
              hint={t.config.imageUploadHint}
              {...bindGuide('ui.siteFavicon', g.ui.siteFavicon)}
              layout="vertical"
            />
          ) : (
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
              labelAccessory={
                field.key === 'site_description'
                  ? renderSeoAiPanel('site_description')
                  : undefined
              }
              hint={
                field.key === 'site_description' &&
                aiGenFeedback?.field === 'site_description'
                  ? aiGenFeedback.message
                  : undefined
              }
              {...(field.key === 'site_title'
                ? bindGuide('ui.siteTitle', g.ui.siteTitle)
                : bindGuide('ui.siteDescription', g.ui.siteDescription))}
              layout="vertical"
            />
          ),
        )}
        <SwitchItem
          itemKey="pwa_enabled"
          label={t.config.fieldPwaEnabled}
          description={t.config.fieldPwaEnabledHint}
          value={getFieldValue('pwa_enabled') !== 'false'}
          onChange={(checked) =>
            updateValue('pwa_enabled', checked ? 'true' : 'false')
          }
          {...bindGuide('ui.pwaEnabled', g.ui.pwaEnabled)}
          layout="horizontal"
        />
      </SettingGroup>

      {/* SEO / GEO：可见性策略、AI 文案、关键词、分享图 */}
      <SettingGroup
        title={t.config.siteSeo}
        description={t.config.siteSeoDesc}
        {...bindGuide('ui.siteSeo', g.ui.siteSeo)}
        icon={<FaSearch />}
      >
        <SettingItemWrapper
          label={t.config.fieldSiteVisibilityPolicy}
          description={t.config.fieldSiteVisibilityPolicyHint}
          layout="vertical"
          {...bindGuide('ui.siteVisibilityPolicy', g.ui.siteVisibilityPolicy)}
        >
          <SegmentedControl
            size="sm"
            columns={4}
            value={visibilityPolicy}
            options={[
              {
                value: 'private',
                label: t.config.visibilityPrivate,
              },
              {
                value: 'search_only',
                label: t.config.visibilitySearchOnly,
              },
              {
                value: 'ai_citation',
                label: t.config.visibilityAiCitation,
              },
              {
                value: 'ai_full',
                label: t.config.visibilityAiFull,
              },
            ]}
            onChange={(v) => {
              updateValue('site_visibility_policy', v)
              updateValue('site_noindex', v === 'private' ? 'true' : 'false')
            }}
            ariaLabel={t.config.fieldSiteVisibilityPolicy}
          />
          <p className="setting-hint" style={{ marginTop: '0.5rem' }}>
            {visibilityPolicyHint}
          </p>
        </SettingItemWrapper>

        <InputItem
          itemKey="site_ai_intro"
          label={t.config.fieldSiteAiIntro}
          value={getFieldValue('site_ai_intro')}
          onChange={(v) => updateValue('site_ai_intro', v)}
          placeholder={t.config.fieldSiteAiIntroHint}
          hint={
            aiGenFeedback?.field === 'site_ai_intro'
              ? aiGenFeedback.message
              : t.config.fieldSiteAiIntroHint
          }
          multiline
          rows={3}
          labelAccessory={renderSeoAiPanel('site_ai_intro')}
          {...bindGuide('ui.siteAiIntro', g.ui.siteAiIntro)}
          layout="vertical"
        />

        <InputItem
          itemKey="site_keywords"
          label={t.config.fieldSiteKeywords}
          value={getFieldValue('site_keywords')}
          onChange={(v) => updateValue('site_keywords', v)}
          placeholder={t.config.placeholderSiteKeywords}
          hint={
            aiGenFeedback?.field === 'site_keywords'
              ? aiGenFeedback.message
              : t.config.fieldSiteKeywordsHint
          }
          labelAccessory={renderSeoAiPanel('site_keywords')}
          {...bindGuide('ui.siteKeywords', g.ui.siteKeywords)}
          layout="vertical"
        />
        <InputItem
          itemKey="site_og_image"
          label={t.config.fieldSiteOgImage}
          value={getFieldValue('site_og_image')}
          onChange={(v) => updateValue('site_og_image', v)}
          placeholder={t.config.placeholderSiteOgImage}
          inputType="url"
          hint={t.config.fieldSiteOgImageHint}
          {...bindGuide('ui.siteOgImage', g.ui.siteOgImage)}
          layout="vertical"
        />
      </SettingGroup>

      {/* 站点底部信息（备案和云赞助商） */}
      <SettingGroup
        title={t.config.siteFooterTitle}
        icon={<FaInfoCircle />}
        description={t.config.siteFooterDesc}
        {...bindGuide('ui.siteFooter', g.ui.siteFooter)}
      >
        <InputItem
          itemKey="site_icp"
          label={t.config.siteIcp}
          value={getFieldValue('site_icp')}
          onChange={(v) => updateValue('site_icp', v)}
          placeholder={t.config.siteIcpPlaceholder}
          hint={t.config.siteIcpHint}
          {...bindGuide('ui.siteIcp', g.ui.siteIcp)}
          layout="vertical"
        />
        <InputItem
          itemKey="site_gongan"
          label={t.config.siteGongan}
          value={getFieldValue('site_gongan')}
          onChange={(v) => updateValue('site_gongan', v)}
          placeholder={t.config.siteGonganPlaceholder}
          hint={t.config.siteGonganHint}
          {...bindGuide('ui.siteGongan', g.ui.siteGongan)}
          layout="vertical"
        />
        <CheckboxGroupItem
          label={t.config.cloudSponsors}
          description={t.config.cloudSponsorsHint}
          {...bindGuide('ui.cloudSponsors', g.ui.cloudSponsors)}
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

        <SettingItemWrapper
          label={t.config.siteFooterCustom}
          hint={t.config.siteFooterCustomHint}
          {...bindGuide('ui.siteFooterCustom', g.ui.siteFooterCustom)}
          layout="vertical"
        >
          <div className="site-footer-custom-editor">
            {footerCustomItems.map((item, index) => (
              <div
                key={`footer-custom-${index}`}
                className="site-footer-custom-card"
              >
                <div className="site-footer-custom-card-head">
                  <span className="site-footer-custom-card-title">
                    {t.config.siteFooterCustomItem.replace(
                      '{n}',
                      String(index + 1),
                    )}
                  </span>
                  <button
                    type="button"
                    className="site-footer-custom-remove"
                    onClick={() => removeFooterCustomAt(index)}
                    aria-label={t.config.siteFooterCustomRemove}
                  >
                    <FaTrash aria-hidden />
                  </button>
                </div>
                <InputItem
                  itemKey={`site_footer_custom_text_${index}`}
                  label={t.config.siteFooterCustomText}
                  value={item.text}
                  onChange={(v) => updateFooterCustomAt(index, { text: v })}
                  placeholder={t.config.siteFooterCustomTextPlaceholder}
                  layout="vertical"
                />
                <InputItem
                  itemKey={`site_footer_custom_icon_${index}`}
                  label={t.config.siteFooterCustomIcon}
                  value={item.icon}
                  onChange={(v) => updateFooterCustomAt(index, { icon: v })}
                  placeholder={t.config.siteFooterCustomIconPlaceholder}
                  inputType="url"
                  variant="imageUpload"
                  accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml"
                  maxImageBytes={256 * 1024}
                  uploadLabel={t.config.imageUpload}
                  clearImageLabel={t.config.imageUploadClear}
                  localImageLabel={t.config.imageUploadLocal}
                  previewAlt={t.config.siteFooterCustomIcon}
                  imageTypeError={t.config.imageUploadTypeError}
                  imageSizeError={t.config.imageUploadSizeError}
                  imageReadError={t.config.imageUploadReadError}
                  hint={t.config.siteFooterCustomIconHint}
                  layout="vertical"
                />
                <InputItem
                  itemKey={`site_footer_custom_url_${index}`}
                  label={t.config.siteFooterCustomUrl}
                  value={item.url}
                  onChange={(v) => updateFooterCustomAt(index, { url: v })}
                  placeholder={t.config.siteFooterCustomUrlPlaceholder}
                  inputType="url"
                  hint={t.config.siteFooterCustomUrlHint}
                  layout="vertical"
                />
              </div>
            ))}
            {footerCustomItems.length < FOOTER_CUSTOM_MAX ? (
              <button
                type="button"
                className="site-footer-custom-add"
                onClick={addFooterCustom}
              >
                <FaPlus aria-hidden />
                {t.config.siteFooterCustomAdd}
              </button>
            ) : null}
          </div>
        </SettingItemWrapper>
      </SettingGroup>

      <SettingGroup
        title={t.config.backgroundAndTheme}
        description={t.config.backgroundAndThemeDesc}
        {...bindGuide('ui.backgroundAndTheme', g.ui.backgroundAndTheme)}
        icon={<LuPalette />}
      >
        {backgroundFields.map((field) =>
          field.key === 'wallpaper_blur' ? (
            <SliderItem
              key={field.key}
              itemKey={field.key}
              label={getFieldLabel(field.key, field.label)}
              required={field.required}
              value={Number.parseFloat(field.value) || 0}
              onChange={(v) => updateValue(field.key, String(v))}
              min={0}
              max={10}
              step={1}
              showValue
              showRangeLabels
              startLabel={t.config.sliderWeak}
              endLabel={t.config.sliderStrong}
              {...bindGuide('ui.wallpaperBlur', g.ui.wallpaperBlur)}
              layout="vertical"
            />
          ) : (
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
              {...bindGuide('ui.wallpaper', g.ui.wallpaper)}
              layout="vertical"
            />
          ),
        )}
      </SettingGroup>

      <SettingGroup
        title={t.config.evocativeTitle}
        icon={<FaMagic />}
        description={t.config.evocativeDesc}
        {...bindGuide('ui.evocative', g.ui.evocative)}
      >
        <CheckboxGroupItem
          label={t.config.evocativeEffects}
          {...bindGuide('ui.evocativeEffects', g.ui.evocativeEffects)}
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

        <SettingItemWrapper
          label={t.config.fieldEvocativeFps}
          hint={t.config.fieldEvocativeFpsHint}
          {...bindGuide('ui.evocativeFps', g.ui.evocativeFps)}
          layout="vertical"
        >
          <SegmentedControl
            size="sm"
            columns={2}
            value={getFieldValue('evocative_fps') || '30'}
            options={[
              {
                value: '30',
                label: `30 FPS (${t.config.fpsBalanced})`,
              },
              {
                value: '60',
                label: `60 FPS (${t.config.fpsSmooth})`,
              },
            ]}
            onChange={(v) => updateValue('evocative_fps', v)}
            ariaLabel={t.config.fieldEvocativeFps}
          />
        </SettingItemWrapper>

        <SliderItem
          itemKey="evocative_ripple_quality"
          label={t.config.fieldEvocativeRippleQuality}
          value={rippleQualityPercent}
          onChange={(v) => {
            const q = Math.min(100, Math.max(50, Math.round(v))) / 100
            updateValue(
              'evocative_ripple_quality',
              q === 1 ? '1' : q.toFixed(2),
            )
          }}
          min={50}
          max={100}
          step={1}
          showValue
          showRangeLabels
          startLabel={t.config.sliderRippleLow}
          endLabel={t.config.sliderRippleUltra}
          formatValue={(v) => `${Math.round(v)}%`}
          recommendedValue={85}
          recommendedLabel={t.config.sliderRecommended}
          {...bindGuide('ui.evocativeRippleQuality', g.ui.evocativeRippleQuality)}
          layout="vertical"
        />
      </SettingGroup>
    </SettingSection>
  )
}

export default UiConfigSection
