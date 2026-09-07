/**
 * 统一开关控件（无 label 壳）
 * 供 SwitchItem、平台卡、OAuth 等复用，避免多处手写 toggle-switch DOM。
 *
 * 可选 `preview`：悬停 / 聚焦时预演开启样子；浮层走 SettingTitleHelp 同一套 hover tooltip。
 */

import type { ToggleSwitchPreview } from './toggleSwitchPreview'
import React, { useMemo } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { useSettingHoverTooltip } from '../useSettingHoverTooltip'
import { resolveToggleSwitchPreview } from './toggleSwitchPreview'
import './SettingItem.css'

export type { ToggleSwitchPreview }

export interface ToggleSwitchProps {
  id?: string
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  /** 无障碍名；有关联 label 时可不传 */
  'aria-label'?: string
  className?: string
  title?: string
  /**
   * 悬停 / 聚焦预告。有内容时不再使用原生 title，避免叠两层。
   */
  preview?: ToggleSwitchPreview
}

export const ToggleSwitch = React.memo<ToggleSwitchProps>(
  ({
    id,
    checked,
    onChange,
    disabled = false,
    'aria-label': ariaLabel,
    className = '',
    title,
    preview,
  }) => {
    const { t } = useI18n()
    const { body, showKicker } = resolveToggleSwitchPreview(
      preview,
      checked,
      disabled,
    )
    const hasPreview =
      showKicker || (body != null && body !== false && body !== '')
    const kicker = showKicker
      ? checked
        ? t.config.switchPreviewOffKicker
        : t.config.switchPreviewOnKicker
      : null
    const content = useMemo(
      () =>
        hasPreview ? (
          <span className="toggle-preview-tip">
            {kicker ? (
              <span className="toggle-preview-kicker">{kicker}</span>
            ) : null}
            <span className="toggle-preview-body">{body}</span>
          </span>
        ) : null,
      [hasPreview, kicker, body],
    )

    const { triggerRef, tooltip, open, show, hide, tooltipId } =
      useSettingHoverTooltip<HTMLLabelElement>({
        content,
        enabled: hasPreview,
      })

    const previewing = open && hasPreview
    const nativeTitle = hasPreview ? undefined : title

    const classNames = useMemo(
      () =>
        [
          'toggle-switch',
          disabled ? 'disabled' : '',
          previewing ? 'is-previewing' : '',
          className,
        ]
          .filter(Boolean)
          .join(' '),
      [disabled, previewing, className],
    )

    return (
      <>
        <label
          ref={triggerRef}
          className={classNames}
          title={nativeTitle}
          onClick={(e) => e.stopPropagation()}
          onMouseEnter={show}
          onMouseLeave={hide}
          onFocus={show}
          onBlur={hide}
        >
          <input
            id={id}
            type="checkbox"
            checked={checked}
            disabled={disabled}
            aria-label={ariaLabel}
            aria-describedby={previewing ? tooltipId : undefined}
            onChange={(e) => {
              if (!disabled) onChange(e.target.checked)
            }}
          />
          <span className="toggle-slider" aria-hidden="true" />
        </label>
        {tooltip}
      </>
    )
  },
)

ToggleSwitch.displayName = 'ToggleSwitch'
