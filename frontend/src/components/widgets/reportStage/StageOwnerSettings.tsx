import type { ReportSettings } from '../../../utils/reportSettings'
import { useCallback, useEffect, useId, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import {
  DEFAULT_REPORT_SETTINGS,
  fetchReportSettings,
  updateReportSettings,
} from '../../../utils/reportSettings'
import { showToast } from '../../../utils/toastManager'
import { userFacingError } from '../../../utils/userFacingError'
import { StageStyleSettings } from './StageStyleSettings'
import './stageOwner.css'

function clampDays(value: number): number {
  return Math.min(365, Math.max(1, Math.round(value)))
}

/** 站长区：报告风格，以及有效期与过期后是否重写。有效期改一项存一项，不另设保存按钮。 */
export function StageOwnerSettings({
  platform,
  onStyleChanged,
}: {
  platform: string
  onStyleChanged: () => void
}) {
  const { t } = useI18n()
  const copy = t.reportStage
  const [settings, setSettings] = useState<ReportSettings | null>(null)
  const [daysText, setDaysText] = useState('')
  const [saving, setSaving] = useState(false)
  const savedRef = useRef<ReportSettings>(DEFAULT_REPORT_SETTINGS)
  const id = useId()

  useEffect(() => {
    let cancelled = false
    fetchReportSettings()
      .then((loaded) => {
        if (cancelled) return
        savedRef.current = loaded
        setSettings(loaded)
        setDaysText(String(loaded.expiryDays))
      })
      .catch((err) => {
        if (cancelled) return
        showToast({ message: userFacingError(err, copy.settingsLoadFailed), type: 'error', replaceKey: 'report-stage-settings' })
      })
    return () => {
      cancelled = true
    }
  }, [copy.settingsLoadFailed])

  const save = useCallback(
    async (patch: Partial<ReportSettings>) => {
      const next = { ...savedRef.current, ...patch }
      setSettings(next)
      setSaving(true)
      try {
        const stored = await updateReportSettings(next)
        savedRef.current = stored
        setSettings(stored)
        setDaysText(String(stored.expiryDays))
        showToast({ message: copy.settingsSaved, type: 'success', replaceKey: 'report-stage-settings' })
      } catch (err) {
        setSettings(savedRef.current)
        setDaysText(String(savedRef.current.expiryDays))
        showToast({ message: userFacingError(err, copy.settingsSaveFailed), type: 'error', replaceKey: 'report-stage-settings' })
      } finally {
        setSaving(false)
      }
    },
    [copy.settingsSaved, copy.settingsSaveFailed],
  )

  const commitDays = () => {
    const parsed = Number(daysText)
    if (!Number.isFinite(parsed) || daysText.trim() === '') {
      setDaysText(String(savedRef.current.expiryDays))
      return
    }
    const days = clampDays(parsed)
    setDaysText(String(days))
    if (days !== savedRef.current.expiryDays) void save({ expiryDays: days })
  }

  if (!settings) {
    return <div className="report-stage__settings report-stage__settings--loading">{t.common.loading}</div>
  }

  return (
    <section className="report-stage__settings" aria-labelledby={`${id}-title`} aria-busy={saving}>
      <h2 id={`${id}-title`}>{copy.settings}</h2>
      <StageStyleSettings platform={platform} onStyleChanged={onStyleChanged} />
      <label className="report-stage__setting">
        <span>
          <strong>{copy.expiryEnabled}</strong>
          <small>{copy.expiryEnabledDesc}</small>
        </span>
        <input
          type="checkbox"
          role="switch"
          checked={settings.expiryEnabled}
          disabled={saving}
          onChange={(event) => void save({ expiryEnabled: event.target.checked })}
        />
      </label>
      <label className="report-stage__setting" data-disabled={!settings.expiryEnabled || undefined}>
        <span>
          <strong>{copy.autoRegenerate}</strong>
          <small>{copy.autoRegenerateDesc}</small>
        </span>
        <input
          type="checkbox"
          role="switch"
          checked={settings.autoRegenerate}
          disabled={saving || !settings.expiryEnabled}
          onChange={(event) => void save({ autoRegenerate: event.target.checked })}
        />
      </label>
      <label className="report-stage__setting" data-disabled={!settings.expiryEnabled || undefined}>
        <span>
          <strong>{copy.expiryDays}</strong>
          <small>{copy.expiryDaysHint}</small>
        </span>
        <span className="report-stage__days">
          <input
            type="number"
            inputMode="numeric"
            min={1}
            max={365}
            step={1}
            value={daysText}
            disabled={saving || !settings.expiryEnabled}
            onChange={(event) => setDaysText(event.target.value)}
            onBlur={commitDays}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                commitDays()
              }
            }}
          />
          {copy.expiryDaysUnit}
        </span>
      </label>
    </section>
  )
}
