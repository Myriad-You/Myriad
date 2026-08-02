/**
 * 数据及统计：接入平台子分类（列表/拖拽/开关 + 自动刷新）+ 访客统计子分类
 * ↔ 二级页（凭证 + 数据管理）
 * 导航 / 拖拽状态自包含；父级只提供 platforms 草稿与写入回调。
 */

import type { ReactNode } from 'react'
import type { ToastType } from '../Toast'

import type { PlatformAutoFetchConfig } from './PlatformAutoRefreshSettings'
import { LuChevronLeft, LuDatabase, LuGripVertical } from '@lib/icons'
import React, {

  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { fetchJson } from '../../utils/apiHelper'
import { resolvePlatformId } from '../../utils/platformId'
import PlatformIcon from '../PlatformIcon'
import {
  AutoHeight,
  InputItem,
  SettingGroup,
  SETTINGS_DURATION_MS,
  SettingSection,
  SettingTitleHelp,
  SetupFlow,
  ToggleSwitch, useSettingGuide,
} from '../settings'
import AiUsageSection from './AiUsageSection'
import PlatformAutoRefreshSettings from './PlatformAutoRefreshSettings'
import PlatformDataManagement from './PlatformDataManagement'
import { getPlatformSetupGuide } from './platformSetupGuides'
import SiteAnalyticsSection from './SiteAnalyticsSection'
import './PlatformCardSnapshot.css'

/** @deprecated 从 uiBagOwnership 导入；此处 re-export 保持兼容 */
export { PLATFORMS_UI_RESET_KEYS } from './uiBagOwnership'

export interface PlatformConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

export interface PlatformConfig {
  name: string
  enabled: boolean
  has_token: boolean
  config_fields: PlatformConfigField[]
  description: string
  icon: string
}

export interface PlatformsConfigSectionProps {
  title: string
  icon?: React.ReactNode
  description?: string
  sectionId?: string
  platforms: PlatformConfig[]
  autoFetch: PlatformAutoFetchConfig
  onUpdateField: (
    platformIndex: number,
    fieldKey: string,
    value: string,
  ) => void
  onToggle: (platformIndex: number) => void
  onReorder: (fromIndex: number, toIndex: number) => void
  onAutoFetchChange: (value: PlatformAutoFetchConfig) => void
  showMessage: (
    message: string,
    type?: ToastType,
    duration?: number,
  ) => void
  /** 跳转设置页 OAuth 区块 */
  openOAuthSection: () => void
  /**
   * 外部要求打开某平台二级页（如 Discord OAuth 回调）。
   * 消费后应调用 onFocusPlatformConsumed 清空，避免重复打开。
   */
  focusPlatform?: string | null
  onFocusPlatformConsumed?: () => void
  /** 访客统计总开关（默认开启） */
  analyticsEnabled?: boolean
  onAnalyticsEnabledChange?: (enabled: boolean) => void
}

function isMaskedValue(value: string) {
  return value.includes('••') || value.includes('**') || value === '********'
}

function hasFieldValue(field?: PlatformConfigField) {
  if (!field) return false
  const v = String(field.value ?? '').trim()
  // 空串不算；掩码（•••• / ****）算已配置
  return v.length > 0
}

function findField(platform: PlatformConfig, key: string) {
  return platform.config_fields.find((f) => f.key === key)
}

export function isBangumiPlatform(platform: PlatformConfig) {
  return platform.name.toLowerCase() === 'bangumi'
}

export function hasBangumiCredential(platform: PlatformConfig) {
  return (
    hasFieldValue(findField(platform, 'username')) ||
    hasFieldValue(findField(platform, 'access_token'))
  )
}

/**
 * 指示灯 / 开关可用：是否已配好可同步的凭证。
 * - 掩码密钥算已配置
 * - Discord：仅 access_token / has_token（user_id  alone 不算）
 * - X：username + bearer（或 has_token）
 */
export function isPlatformConfigured(platform: PlatformConfig) {
  if (!platform.config_fields || platform.config_fields.length === 0) {
    return Boolean(platform.has_token)
  }

  const name = platform.name.trim().toLowerCase()

  if (isBangumiPlatform(platform)) {
    return hasBangumiCredential(platform)
  }

  if (name === 'discord') {
    // 只认 access_token（含掩码）。单独 user_id / 空 has_token 不算已配置
    return hasFieldValue(findField(platform, 'access_token'))
  }

  if (name === 'x' || name === 'twitter' || name === 'x (twitter)') {
    // username + bearer；bearer 可为掩码，或 has_token（密钥仅服务端可见时）
    const userOk = hasFieldValue(findField(platform, 'username'))
    const tokenOk =
      hasFieldValue(findField(platform, 'bearer_token')) || platform.has_token
    return userOk && tokenOk
  }

  return platform.config_fields.every((field) => {
    if (!field.required) return true
    return hasFieldValue(field)
  })
}

/** 清理掩码输入，供父级 updateField 复用 */
export function sanitizeMaskedFieldValue(value: string): string {
  if (
    isMaskedValue(value) &&
    value !== '••••••••' &&
    value !== '********'
  ) {
    return value.replace(/[•*]+/g, '')
  }
  return value
}

/** 列表入口卡用的轻量快照（GET /api/cache/previews） */
interface CardPreviewUser {
  username: string
  user_id: string
  level: string | null
  follower_count: number | null
  following_count: number | null
  total_content: number
}

interface CardPreviewMetric {
  key: string
  value: number
}

interface CardPreview {
  exists: boolean
  user: CardPreviewUser | null
  metrics: CardPreviewMetric[]
}

interface CardPreviewsResponse {
  success: boolean
  previews: Record<string, CardPreview>
}

const UNKNOWN_USERNAMES = new Set(['未知用户', 'unknown', 'unknown user', ''])

function formatCardNumber(n: number, locale: string): string {
  if (!Number.isFinite(n)) return '—'
  try {
    return new Intl.NumberFormat(locale, {
      notation: Math.abs(n) >= 10000 ? 'compact' : 'standard',
      maximumFractionDigits: n % 1 === 0 ? 0 : 1,
    }).format(n)
  } catch {
    return String(n)
  }
}

function formatCardPlaytime(minutes: number, hoursTpl: string, minsTpl: string): string {
  if (minutes < 60) {
    return minsTpl.replace('{n}', String(Math.round(minutes)))
  }
  const hours = minutes / 60
  const rounded = hours >= 100 ? Math.round(hours) : Math.round(hours * 10) / 10
  return hoursTpl.replace('{n}', String(rounded))
}

/** 数据行：溢出时无缝横向循环滚动；装得下则静止 */
function PlatformCardMetricsMarquee({
  title,
  items,
  renderItem,
}: {
  title?: string
  items: CardPreviewMetric[]
  renderItem: (m: CardPreviewMetric, keyPrefix: string) => ReactNode
}) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const groupRef = useRef<HTMLSpanElement>(null)
  const [scrolling, setScrolling] = useState(false)
  const [durationSec, setDurationSec] = useState(12)

  const measure = useCallback(() => {
    const viewport = viewportRef.current
    const group = groupRef.current
    if (!viewport || !group) return
    const contentW = group.scrollWidth
    const viewW = viewport.clientWidth
    const needs = contentW > viewW + 1
    setScrolling(needs)
    if (needs) {
      // ~28px/s，下限 8s 上限 28s
      const sec = Math.min(28, Math.max(8, contentW / 28))
      setDurationSec(sec)
    }
  }, [])

  useLayoutEffect(() => {
    measure()
  }, [measure, items])

  useEffect(() => {
    const viewport = viewportRef.current
    if (!viewport || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => measure())
    ro.observe(viewport)
    if (groupRef.current) ro.observe(groupRef.current)
    return () => ro.disconnect()
  }, [measure, items])

  return (
    <div
      ref={viewportRef}
      className={`platform-card-snapshot-metrics${scrolling ? ' is-marquee' : ''}`}
      title={title}
      style={
        scrolling
          ? ({
              '--metrics-marquee-duration': `${durationSec}s`,
            } as React.CSSProperties)
          : undefined
      }
    >
      <div className="platform-card-snapshot-metrics-track">
        <span ref={groupRef} className="platform-card-snapshot-metrics-group">
          {items.map((m) => renderItem(m, 'a'))}
        </span>
        {scrolling ? (
          <span className="platform-card-snapshot-metrics-group" aria-hidden>
            {items.map((m) => renderItem(m, 'b'))}
          </span>
        ) : null}
      </div>
    </div>
  )
}

const PlatformsConfigSection: React.FC<PlatformsConfigSectionProps> = ({
  title,
  icon,
  description,
  sectionId = 'platforms',
  platforms,
  autoFetch,
  onUpdateField,
  onToggle,
  onReorder,
  onAutoFetchChange,
  showMessage,
  openOAuthSection,
  focusPlatform = null,
  onFocusPlatformConsumed,
  analyticsEnabled = true,
  onAnalyticsEnabledChange,
}) => {
  const { t, locale } = useI18n()
  const { catalog: settingGuides, bindGuide } = useSettingGuide()
  const dm = t.dataManagement
  const numberLocale =
    locale === 'zh-CN' ? 'zh-CN' : locale === 'ja-JP' ? 'ja-JP' : 'en-US'

  const [selectedPlatform, setSelectedPlatform] = useState<string | null>(null)
  const [platformNavDir, setPlatformNavDir] = useState<
    'none' | 'forward' | 'back'
  >('none')
  const [dragIndex, setDragIndex] = useState<number | null>(null)
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null)
  const dragIndexRef = useRef<number | null>(null)
  const suppressCardClickRef = useRef(false)
  /** 列表入口：各平台轻量数据快照（一次加载，不轮询） */
  const [cardPreviews, setCardPreviews] = useState<
    Record<string, CardPreview>
  >({})

  const loadCardPreviews = useCallback(async () => {
    try {
      const data = await fetchJson<CardPreviewsResponse>(
        `${API_URL}/api/cache/previews`,
        undefined,
        'Unable to load platform previews',
      )
      if (data?.success && data.previews) {
        setCardPreviews(data.previews)
      }
    } catch (e) {
      console.error('Failed to load platform card previews:', e)
    }
  }, [])

  // 列表可见时加载/刷新快照（从二级页返回也会重载，反映处理结果）
  useEffect(() => {
    if (selectedPlatform !== null) return
    void loadCardPreviews()
  }, [selectedPlatform, loadCardPreviews])

  const clearPlatformDrag = useCallback(() => {
    dragIndexRef.current = null
    setDragIndex(null)
    setDragOverIndex(null)
  }, [])

  const openPlatformDetail = useCallback((name: string) => {
    setPlatformNavDir('forward')
    setSelectedPlatform(name)
  }, [])

  const closePlatformDetail = useCallback(() => {
    setPlatformNavDir('back')
    setSelectedPlatform(null)
  }, [])

  const metricLabel = useCallback(
    (key: string): string => {
      const map = dm.previewMetric as Record<string, string | undefined>
      return map[key] || key
    },
    [dm.previewMetric],
  )

  const formatMetricValue = useCallback(
    (key: string, value: number): string => {
      if (key === 'total_playtime_minutes') {
        return formatCardPlaytime(value, dm.playtimeHours, dm.playtimeMinutes)
      }
      if (key === 'average_completion') {
        return `${formatCardNumber(value, numberLocale)}%`
      }
      return formatCardNumber(value, numberLocale)
    },
    [dm.playtimeHours, dm.playtimeMinutes, numberLocale],
  )

  // 推入动画播完后清掉方向标记（否则平台卡毛玻璃会停在实底）
  useEffect(() => {
    if (platformNavDir === 'none') return undefined
    const id = window.setTimeout(
      setPlatformNavDir,
      SETTINGS_DURATION_MS.slow + 60,
      'none',
    )
    return () => window.clearTimeout(id)
  }, [platformNavDir, selectedPlatform])

  // Discord OAuth 等外部深链：打开指定平台
  useEffect(() => {
    if (!focusPlatform) return
    const exists = platforms.some((p) => p.name === focusPlatform)
    if (exists) {
      setPlatformNavDir('forward')
      setSelectedPlatform(focusPlatform)
    }
    onFocusPlatformConsumed?.()
  }, [focusPlatform, platforms, onFocusPlatformConsumed])

  const getPlatformDescription = useCallback(
    (platform: PlatformConfig) => {
      const descMap: Record<string, string> = {
        github: t.config.platformDescGithub,
        bilibili: t.config.platformDescBilibili,
        bangumi: t.config.platformDescBangumi,
        steam: t.config.platformDescSteam,
        youtube: t.config.platformDescYoutube,
        'netease music': t.config.platformDescNetease,
        netease: t.config.platformDescNetease,
        x: t.config.platformDescX,
        discord: t.config.platformDescDiscord,
        myanimelist: t.config.platformDescMal,
        mal: t.config.platformDescMal,
        xbox: t.config.platformDescXbox,
        playstation: t.config.platformDescPsn,
        psn: t.config.platformDescPsn,
      }
      return descMap[platform.name.toLowerCase()] || platform.description
    },
    [t],
  )

  const getPlatformFieldLabel = useCallback(
    (platform: PlatformConfig, field: PlatformConfigField): string => {
      if (!isBangumiPlatform(platform)) return field.label
      const labels: Record<string, string> = {
        username: t.config.bangumiUsernameLabel,
        access_token: t.config.bangumiAccessTokenLabel,
        user_agent: t.config.bangumiUserAgentLabel,
      }
      return labels[field.key] || field.label
    },
    [t],
  )

  const getPlatformFieldPlaceholder = useCallback(
    (platform: PlatformConfig, field: PlatformConfigField): string => {
      if (!isBangumiPlatform(platform)) return field.placeholder
      const placeholders: Record<string, string> = {
        username: t.config.bangumiUsernamePlaceholder,
        access_token: t.config.bangumiAccessTokenPlaceholder,
        user_agent: t.config.bangumiUserAgentPlaceholder,
      }
      return placeholders[field.key] || field.placeholder
    },
    [t],
  )

  const connectDiscordOAuth = useCallback(() => {
    window.location.href = `${API_URL}/api/platforms/discord/oauth/start`
  }, [])

  const detailIndex = selectedPlatform
    ? platforms.findIndex((p) => p.name === selectedPlatform)
    : -1
  const detailPlatform =
    detailIndex >= 0 ? platforms[detailIndex] : null
  const paneKey = selectedPlatform ?? '__list__'

  if (detailPlatform && detailIndex >= 0) {
    const platformCapability = getPlatformDescription(detailPlatform)
    const setupGuide = getPlatformSetupGuide(detailPlatform.name, t.config, {
      connectDiscordOAuth,
      openOAuthSection,
    })

    return (
      <SettingSection
        sectionId={sectionId}
        title={detailPlatform.name}
        icon={
          <PlatformIcon
            platform={detailPlatform.name}
            className="platform-icon"
          />
        }
        description={platformCapability}
        detail={platformCapability}
        {...bindGuide('platforms.platformFields', settingGuides.platforms.platformFields)}
        headerLeading={
          <button
            type="button"
            className="section-header-back"
            onClick={closePlatformDetail}
            aria-label={t.common.back}
          >
            <LuChevronLeft size={18} aria-hidden />
            <span>{t.common.back}</span>
          </button>
        }
      >
        <AutoHeight contentKey={paneKey} className="platform-pane-height">
          <div
            key={paneKey}
            data-nav={platformNavDir === 'none' ? undefined : platformNavDir}
            className="platforms-pane platforms-pane--detail sm-pane"
          >
            <div className="platform-detail">
              {setupGuide ? (
                <SetupFlow
                  title={setupGuide.title}
                  optionalLabel={setupGuide.optionalLabel}
                  steps={setupGuide.steps}
                  className="platform-setup-flow"
                />
              ) : null}

              <div className="platform-detail-body">
                {detailPlatform.config_fields.map((field) => {
                  const rawType = field.field_type
                  const inputType =
                    rawType === 'password' ||
                    rawType === 'url' ||
                    rawType === 'email'
                      ? rawType
                      : 'text'
                  return (
                    <InputItem
                      key={field.key}
                      itemKey={`platform-${detailIndex}-${field.key}`}
                      label={getPlatformFieldLabel(detailPlatform, field)}
                      value={field.value}
                      onChange={(value) =>
                        onUpdateField(detailIndex, field.key, value)
                      }
                      placeholder={getPlatformFieldPlaceholder(
                        detailPlatform,
                        field,
                      )}
                      inputType={inputType}
                      required={field.required}
                      layout="vertical"
                      size="md"
                      autoSelectOnMask
                    />
                  )
                })}
              </div>

              <PlatformDataManagement
                platformName={detailPlatform.name}
                showMessage={showMessage}
              />
            </div>
          </div>
        </AutoHeight>
      </SettingSection>
    )
  }

  return (
    <SettingSection
      sectionId={sectionId}
      title={title}
      icon={icon}
      description={description}
      {...bindGuide('platforms.list', settingGuides.platforms.list)}
    >
      <AutoHeight contentKey={paneKey} className="platform-pane-height">
        <div
          key={paneKey}
          data-nav={platformNavDir === 'none' ? undefined : platformNavDir}
          className="platforms-pane platforms-pane--list sm-pane"
        >
          {/* 接入平台 + 自动刷新 共享同一 TOC 子分类（带图标） */}
          <SettingGroup
            id="connected-platforms"
            title={t.config.connectedPlatforms}
            description={t.config.connectedPlatformsDesc}
            icon={<LuDatabase size={15} />}
            {...bindGuide('platforms.connected', settingGuides.platforms.connected)}
          >
          <div className="platforms-grid">
            {platforms.map((platform, index) => {
              const platformConfigured = isPlatformConfigured(platform)
              const toggleTitle = !platformConfigured
                ? t.config.notConfigured
                : undefined
              const platformDesc = getPlatformDescription(platform)
              const isDragging = dragIndex === index
              const isDragOver =
                dragOverIndex === index && dragIndex !== index

              const openDetail = () => {
                if (suppressCardClickRef.current) {
                  suppressCardClickRef.current = false
                  return
                }
                openPlatformDetail(platform.name)
              }

              return (
                <div
                  key={platform.name}
                  data-guide-path="platforms.platformCard"
                  className={`platform-card has-guide-anchor${
                    platform.enabled ? ' platform-card--enabled' : ''
                  }${isDragging ? ' dragging' : ''}${
                    isDragOver ? ' drag-over' : ''
                  }`}
                  style={{ cursor: 'pointer' }}
                  role="button"
                  tabIndex={0}
                  aria-label={t.config.platformOpenDetailAria.replace(
                    '{name}',
                    platform.name,
                  )}
                  onClick={openDetail}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault()
                      openDetail()
                    }
                  }}
                  onDragOver={(e) => {
                    const from = dragIndexRef.current
                    if (from === null || from === index) return
                    e.preventDefault()
                    e.dataTransfer.dropEffect = 'move'
                    setDragOverIndex((prev) =>
                      prev === index ? prev : index,
                    )
                  }}
                  onDragLeave={(e) => {
                    const next = e.relatedTarget as Node | null
                    if (next && e.currentTarget.contains(next)) return
                    setDragOverIndex((prev) =>
                      prev === index ? null : prev,
                    )
                  }}
                  onDrop={(e) => {
                    e.preventDefault()
                    e.stopPropagation()
                    const from = dragIndexRef.current
                    if (from !== null && from !== index) {
                      onReorder(from, index)
                    }
                    clearPlatformDrag()
                  }}
                >
                  <div className="platform-header">
                    <div className="platform-info">
                      <div
                        className="platform-drag-handle"
                        role="button"
                        tabIndex={0}
                        aria-label={t.config.dragToReorder}
                        title={t.config.dragToReorder}
                        draggable
                        onClick={(e) => e.stopPropagation()}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' || e.key === ' ') {
                            e.preventDefault()
                            e.stopPropagation()
                          }
                        }}
                        onDragStart={(e) => {
                          e.stopPropagation()
                          suppressCardClickRef.current = true
                          dragIndexRef.current = index
                          setDragIndex(index)
                          e.dataTransfer.effectAllowed = 'move'
                          e.dataTransfer.setData('text/plain', String(index))
                          const card = e.currentTarget.closest(
                            '.platform-card',
                          ) as HTMLElement | null
                          if (card) {
                            try {
                              e.dataTransfer.setDragImage(card, 24, 24)
                            } catch {
                              /* ignore */
                            }
                          }
                        }}
                        onDragEnd={() => {
                          clearPlatformDrag()
                          window.setTimeout(() => {
                            suppressCardClickRef.current = false
                          }, 0)
                        }}
                      >
                        <span className="platform-order-num" aria-hidden>
                          {index + 1}
                        </span>
                        <LuGripVertical
                          className="platform-drag-grip"
                          aria-hidden
                        />
                      </div>
                      <div className="platform-icon-wrapper">
                        <PlatformIcon
                          platform={platform.name}
                          className="platform-icon"
                        />
                      </div>
                      <div className="platform-details">
                        {/* 入口卡不响应「显示说明」：仅固定 ⓘ 短说明 + 状态点 */}
                        <div className="platform-title-row">
                          <h3 className="platform-name">
                            <span className="platform-name-text">
                              {platform.name}
                            </span>
                            {platformDesc ? (
                              <SettingTitleHelp
                                ariaLabel={t.config.platformHelpAria.replace(
                                  '{name}',
                                  platform.name,
                                )}
                              >
                                {platformDesc}
                              </SettingTitleHelp>
                            ) : null}
                          </h3>
                          {(() => {
                            const statusClass = !platformConfigured
                              ? 'is-unconfigured'
                              : platform.enabled
                                ? 'is-enabled'
                                : 'is-configured'
                            const statusLabel = !platformConfigured
                              ? t.config.platformStatusUnconfigured
                              : platform.enabled
                                ? t.config.platformStatusEnabled
                                : t.config.platformStatusConfiguredOff
                            return (
                              <span
                                className={`platform-status-dot ${statusClass}`}
                                title={statusLabel}
                                aria-label={statusLabel}
                                role="status"
                              />
                            )
                          })()}
                        </div>
                        {(() => {
                          const slug = resolvePlatformId(platform.name)
                          const snap = slug ? cardPreviews[slug] : undefined
                          if (!snap?.exists) return null
                          const rawName = snap.user?.username?.trim() || ''
                          const showUser =
                            rawName.length > 0 &&
                            !UNKNOWN_USERNAMES.has(rawName.toLowerCase()) &&
                            rawName !== '未知用户'
                          const level = snap.user?.level?.trim() || ''
                          const metrics = (snap.metrics || []).slice(0, 6)
                          if (!showUser && !level && metrics.length === 0) {
                            return null
                          }
                          const metricsTitle = metrics
                            .map(
                              (m) =>
                                `${formatMetricValue(m.key, m.value)}${metricLabel(m.key)}`,
                            )
                            .join(' · ')
                          return (
                            <div className="platform-card-snapshot">
                              {showUser || level ? (
                                <div className="platform-card-snapshot-user">
                                  {showUser ? (
                                    <span className="platform-card-snapshot-username">
                                      {rawName}
                                    </span>
                                  ) : null}
                                  {level ? (
                                    <span className="platform-card-snapshot-level">
                                      {level}
                                    </span>
                                  ) : null}
                                </div>
                              ) : null}
                              {metrics.length > 0 ? (
                                <PlatformCardMetricsMarquee
                                  title={metricsTitle}
                                  items={metrics}
                                  renderItem={(m, keyPrefix) => (
                                    <span
                                      key={`${keyPrefix}-${m.key}`}
                                      className="platform-card-snapshot-metric"
                                    >
                                      <span className="platform-card-snapshot-metric-value">
                                        {formatMetricValue(m.key, m.value)}
                                      </span>
                                      <span className="platform-card-snapshot-metric-label">
                                        {metricLabel(m.key)}
                                      </span>
                                    </span>
                                  )}
                                />
                              ) : null}
                            </div>
                          )
                        })()}
                      </div>
                    </div>
                    <div className="platform-actions">
                      <ToggleSwitch
                        checked={platform.enabled}
                        onChange={() => onToggle(index)}
                        disabled={!platformConfigured}
                        aria-label={t.config.platformEnableAria.replace(
                          '{name}',
                          platform.name,
                        )}
                        title={toggleTitle}
                      />
                    </div>
                  </div>
                </div>
              )
            })}
          </div>

          <PlatformAutoRefreshSettings
            toc={false}
            value={autoFetch}
            configuredPlatformCount={
              platforms.filter((platform) => isPlatformConfigured(platform))
                .length
            }
            onChange={onAutoFetchChange}
          />
          </SettingGroup>

          <SiteAnalyticsSection
            showMessage={showMessage}
            enabled={analyticsEnabled}
            onEnabledChange={onAnalyticsEnabledChange}
          />

          <AiUsageSection showMessage={showMessage} />
        </div>
      </AutoHeight>
    </SettingSection>
  )
}

PlatformsConfigSection.displayName = 'PlatformsConfigSection'

export default PlatformsConfigSection
