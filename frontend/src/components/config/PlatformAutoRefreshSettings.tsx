import type { ChoiceOption } from '../settings'
import { LuRefreshCw } from '@lib/icons'

import React, { useMemo } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  guideDomProps,
  SegmentedControl,
  SettingGroup,
  SettingTitleGuideEntry,
  SettingTitleHelp,
  useSettingGuide,
  useSettingsHelp,
} from '../settings'

export interface PlatformAutoFetchConfig {
  enabled: boolean
  interval_hours: number
}

interface PlatformAutoRefreshSettingsProps {
  value: PlatformAutoFetchConfig
  /** 已配置（有凭证）的平台数；与报告页 enabled 无关 */
  configuredPlatformCount: number
  onChange: (value: PlatformAutoFetchConfig) => void
  /**
   * 是否参与页内 TOC。嵌在「接入平台」子分类内时应为 false，
   * 与平台列表共享同一 TOC 芯片；此时标题退化为纯文本（无图标）。
   */
  toc?: boolean
  /** 可选扩展内容 */
  children?: React.ReactNode
}

const INTERVAL_OPTIONS = [6, 12, 24]

const PlatformAutoRefreshSettings: React.FC<
  PlatformAutoRefreshSettingsProps
> = ({
  value,
  configuredPlatformCount,
  onChange,
  toc = true,
  children,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide, renderGuide } = useSettingGuide()
  const helpCtx = useSettingsHelp()
  const expandHelp = Boolean(helpCtx?.showDetails)
  const interval = INTERVAL_OPTIONS.includes(value.interval_hours)
    ? value.interval_hours
    : 24
  const selectedValue = value.enabled ? String(interval) : 'off'

  // 当前状态短文案（标题旁标签）；用途说明走 description → ⓘ tooltip
  const status = value.enabled
    ? configuredPlatformCount > 0
      ? t.config.autoRefreshSummary
          .replace('{count}', String(configuredPlatformCount))
          .replace('{hours}', String(interval))
      : t.config.autoRefreshNoPlatforms
    : t.config.autoRefreshDisabledHint

  const options = useMemo((): ChoiceOption[] => {
    return [
      { value: 'off', label: t.config.autoRefreshOff },
      ...INTERVAL_OPTIONS.map((hours) => ({
        value: String(hours),
        label: t.config.autoRefreshEveryHours.replace(
          '{hours}',
          String(hours),
        ),
      })),
    ]
  }, [t.config.autoRefreshOff, t.config.autoRefreshEveryHours])

  const controls = (
    <div className="settings-stack">
      <SegmentedControl
        size="md"
        columns={4}
        ariaLabel={t.config.autoRefreshTitle}
        value={selectedValue}
        options={options}
        onChange={(next) =>
          next === 'off'
            ? onChange({ ...value, enabled: false })
            : onChange({
                enabled: true,
                interval_hours: Number(next),
              })
        }
      />
      {children}
    </div>
  )

  const description = (
    <>
      {t.config.autoRefreshDescription}
      <br />
      {t.config.autoRefreshFrequencyDesc}
    </>
  )

  // 嵌在「接入平台」内：退化为文本分区，不与带图标子分类抢视觉权重
  if (!toc) {
    const title = t.config.autoRefreshTitle
    return (
      <section
        id="platform-auto-refresh"
        className="platform-auto-refresh platform-auto-refresh--plain"
        {...guideDomProps('platforms.autoRefresh')}
      >
        <h5 className="platform-auto-refresh-title">
          <span className="platform-auto-refresh-title-text">
            {title}
            {!expandHelp ? (
              <SettingTitleHelp
                ariaLabel={t.config.detailHelpAriaNamed.replace(
                  '{title}',
                  title,
                )}
              >
                {description}
              </SettingTitleHelp>
            ) : null}
            <SettingTitleGuideEntry
              title={title}
              guide={renderGuide(g.platforms.autoRefresh)}
            />
          </span>
          <span className="platform-auto-refresh-status">{status}</span>
        </h5>
        {expandHelp ? (
          <p className="platform-auto-refresh-desc">{description}</p>
        ) : null}
        {controls}
      </section>
    )
  }

  return (
    <SettingGroup
      id="platform-auto-refresh"
      toc={toc}
      title={t.config.autoRefreshTitle}
      description={t.config.autoRefreshDescription}
      detail={description}
      {...bindGuide('platforms.autoRefresh', g.platforms.autoRefresh)}
      titleExtra={
        <span className="platform-auto-refresh-status">{status}</span>
      }
      icon={<LuRefreshCw />}
    >
      {controls}
    </SettingGroup>
  )
}

export default React.memo(PlatformAutoRefreshSettings)
