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
  configuredPlatformCount: number
  onChange: (value: PlatformAutoFetchConfig) => void
  toc?: boolean
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
  const { t, format } = useI18n()
  const { catalog: g, bindGuide, renderGuide } = useSettingGuide()
  const helpCtx = useSettingsHelp()
  const expandHelp = Boolean(helpCtx?.showDetails)
  const interval = INTERVAL_OPTIONS.includes(value.interval_hours)
    ? value.interval_hours
    : 24
  const selectedValue = value.enabled ? String(interval) : 'off'

  const status = value.enabled
    ? configuredPlatformCount > 0
      ? format(t.config.autoRefreshSummary, {
          count: configuredPlatformCount,
          hours: interval,
        })
      : t.config.autoRefreshNoPlatforms
    : t.config.autoRefreshDisabledHint

  const options = useMemo((): ChoiceOption[] => {
    return [
      { value: 'off', label: t.config.autoRefreshOff },
      ...INTERVAL_OPTIONS.map((hours) => ({
        value: String(hours),
        label: format(t.config.autoRefreshEveryHours, { hours }),
      })),
    ]
  }, [format, t.config.autoRefreshOff, t.config.autoRefreshEveryHours])

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
                ariaLabel={format(t.config.detailHelpAriaNamed, { title })}
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
