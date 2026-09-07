import type { RefObject } from 'react'
import type { TappSettingItem } from '../../../tapp/types'
import { useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import {
  InputItem,
  NumberItem,
  SelectItem,
  SwitchItem,
} from '../../settings'
import {
  WidgetSettingsSection,
  WidgetSettingsTip,
} from './WidgetSettingsTip'
import '../../ConfigForm.css'

export function WidgetInstanceSettings({
  open,
  title,
  settings,
  value,
  anchor,
  ignoreRef,
  onSave,
  onClose,
}: {
  open: boolean
  title: string
  settings: TappSettingItem[]
  value: Record<string, unknown>
  anchor: DOMRect | null
  ignoreRef?: RefObject<HTMLElement | null>
  onSave: (value: Record<string, unknown>) => void
  onClose: () => void
}) {
  const { t } = useI18n()
  const [draft, setDraft] = useState<Record<string, unknown>>(() => ({
    ...Object.fromEntries(
      settings
        .filter((setting) => setting.defaultValue !== undefined)
        .map((setting) => [setting.key, setting.defaultValue]),
    ),
    ...value,
  }))

  useEffect(() => {
    if (!open) return
    setDraft({
      ...Object.fromEntries(
        settings
          .filter((setting) => setting.defaultValue !== undefined)
          .map((setting) => [setting.key, setting.defaultValue]),
      ),
      ...value,
    })
  }, [open])

  const update = (key: string, next: unknown) =>
    setDraft((current) => ({ ...current, [key]: next }))

  return (
    <WidgetSettingsTip
      open={open}
      anchor={anchor}
      title={title}
      width={300}
      height={Math.min(
        420,
        132 +
          settings.length * 64 +
          (settings.some((setting) => setting.multiline) ? 88 : 0),
      )}
      onClose={onClose}
      ignoreRef={ignoreRef}
    >
      <WidgetSettingsSection>
        <div className="widget-settings-tip__body widget-settings-tip__fields">
          {settings.map((setting) => {
            const current = draft[setting.key] ?? setting.defaultValue
            if (setting.type === 'toggle') {
              return (
                <SwitchItem
                  key={setting.key}
                  itemKey={setting.key}
                  label={setting.label}
                  description={setting.description}
                  value={current === true}
                  onChange={(checked) => update(setting.key, checked)}
                  size="sm"
                  layout="horizontal"
                />
              )
            }
            if (setting.type === 'select') {
              return (
                <SelectItem
                  key={setting.key}
                  itemKey={setting.key}
                  label={setting.label}
                  description={setting.description}
                  value={String(current ?? '')}
                  onChange={(next) => update(setting.key, next)}
                  options={setting.options ?? []}
                  size="sm"
                  layout="vertical"
                />
              )
            }
            if (setting.type === 'number') {
              const num =
                typeof current === 'number' && Number.isFinite(current)
                  ? current
                  : Number(current)
              return (
                <NumberItem
                  key={setting.key}
                  itemKey={setting.key}
                  label={setting.label}
                  description={setting.description}
                  value={Number.isFinite(num) ? num : 0}
                  onChange={(next) => update(setting.key, next)}
                  min={setting.min}
                  max={setting.max}
                  step={setting.step}
                  size="sm"
                  layout="vertical"
                />
              )
            }
            const text = String(current ?? '')
            return (
              <InputItem
                key={setting.key}
                itemKey={setting.key}
                label={setting.label}
                description={setting.description}
                value={text}
                onChange={(next) => update(setting.key, next)}
                placeholder={
                  setting.type === 'color'
                    ? setting.placeholder || '#8b5cf6'
                    : setting.placeholder
                }
                multiline={setting.multiline}
                rows={setting.rows}
                size="sm"
                layout="vertical"
                labelAccessory={
                  setting.type === 'color' ? (
                    <span
                      className="widget-settings-tip__swatch"
                      style={{
                        background: /^#[0-9a-f]{3,8}$/i.test(text)
                          ? text
                          : 'transparent',
                      }}
                      aria-hidden
                    />
                  ) : undefined
                }
              />
            )
          })}
        </div>
        <button
          type="button"
          className="widget-settings-tip__save"
          onClick={() => onSave(draft)}
        >
          {t.common.save}
        </button>
      </WidgetSettingsSection>
    </WidgetSettingsTip>
  )
}
