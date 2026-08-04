import React, { useCallback } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  InputItem,
  NumberItem,
  SettingGroup,
  SettingSection,
  SwitchItem,
  useSettingGuide,
} from '../settings'

interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

interface TripoConfigSectionProps {
  configFields: ConfigField[]
  updateValue: (key: string, value: string) => void
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
}

export const TripoConfigSection: React.FC<TripoConfigSectionProps> = ({
  configFields,
  updateValue,
  title,
  icon,
  description,
  sectionId,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const value = useCallback(
    (key: string, fallback = '') =>
      configFields.find((field) => field.key === key)?.value || fallback,
    [configFields],
  )
  const enabled = ['true', '1'].includes(value('tripo_enabled', 'false'))

  return (
    <SettingSection
      sectionId={sectionId}
      title={title}
      icon={icon}
      description={description}
    >
      <SettingGroup
        title={t.config.tripoConnection}
        description={t.config.tripoConnectionDesc}
        {...bindGuide('tripo.connection', g.tripo.connection)}
      >
        <SwitchItem
          itemKey="tripo_enabled"
          label={t.config.tripoEnabled}
          description={t.config.tripoEnabledDesc}
          value={enabled}
          onChange={(checked) =>
            updateValue('tripo_enabled', checked ? 'true' : 'false')
          }
          {...bindGuide('tripo.enabled', g.tripo.enabled)}
        />
        <InputItem
          itemKey="tripo_api_key"
          label={t.config.tripoApiKey}
          description={t.config.tripoApiKeyDesc}
          value={value('tripo_api_key')}
          onChange={(next) => updateValue('tripo_api_key', next)}
          inputType="password"
          placeholder="Get from platform.tripo3d.ai"
          disabled={!enabled}
          {...bindGuide('tripo.apiKey', g.tripo.apiKey)}
        />
        <InputItem
          itemKey="tripo_base_url"
          label={t.config.tripoBaseUrl}
          value={value('tripo_base_url', 'https://openapi.tripo3d.ai/v3')}
          onChange={(next) => updateValue('tripo_base_url', next)}
          placeholder="https://openapi.tripo3d.ai/v3"
          disabled={!enabled}
          {...bindGuide('tripo.baseUrl', g.tripo.baseUrl)}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.tripoWebBudget}
        description={t.config.tripoWebBudgetDesc}
        {...bindGuide('tripo.webBudget', g.tripo.webBudget)}
      >
        <InputItem
          itemKey="tripo_model"
          label={t.config.tripoModel}
          value={value('tripo_model', 'P1-20260311')}
          onChange={(next) => updateValue('tripo_model', next)}
          placeholder="P1-20260311"
          disabled={!enabled}
          {...bindGuide('tripo.model', g.tripo.model)}
        />
        <NumberItem
          itemKey="tripo_face_limit"
          label={t.config.tripoFaceLimit}
          description={t.config.tripoFaceLimitDesc}
          value={Number(value('tripo_face_limit', '5000'))}
          onChange={(next) => updateValue('tripo_face_limit', String(next))}
          min={50}
          max={20000}
          step={250}
          disabled={!enabled}
          {...bindGuide('tripo.faceLimit', g.tripo.faceLimit)}
        />
        <NumberItem
          itemKey="tripo_max_download_mb"
          label={t.config.tripoMaxDownload}
          value={Number(value('tripo_max_download_mb', '64'))}
          onChange={(next) =>
            updateValue('tripo_max_download_mb', String(next))
          }
          min={1}
          max={150}
          unit="MB"
          disabled={!enabled}
          {...bindGuide('tripo.maxDownload', g.tripo.maxDownload)}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.tripoTaskControl}
        description={t.config.tripoTaskControlDesc}
        {...bindGuide('tripo.taskControl', g.tripo.taskControl)}
      >
        <NumberItem
          itemKey="tripo_poll_interval_seconds"
          label={t.config.tripoPollInterval}
          value={Number(value('tripo_poll_interval_seconds', '2'))}
          onChange={(next) =>
            updateValue('tripo_poll_interval_seconds', String(next))
          }
          min={2}
          max={60}
          unit="s"
          disabled={!enabled}
          {...bindGuide('tripo.pollInterval', g.tripo.pollInterval)}
        />
        <NumberItem
          itemKey="tripo_task_timeout_seconds"
          label={t.config.tripoTaskTimeout}
          value={Number(value('tripo_task_timeout_seconds', '900'))}
          onChange={(next) =>
            updateValue('tripo_task_timeout_seconds', String(next))
          }
          min={60}
          max={3600}
          unit="s"
          disabled={!enabled}
          {...bindGuide('tripo.taskTimeout', g.tripo.taskTimeout)}
        />
      </SettingGroup>
    </SettingSection>
  )
}

export default TripoConfigSection
