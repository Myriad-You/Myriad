/**
 * 统一开关控件（无 label 壳）
 * 供 SwitchItem、平台卡、OAuth 等复用，避免多处手写 toggle-switch DOM。
 */

import React from 'react'
import './SettingItem.css'

export interface ToggleSwitchProps {
  id?: string
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  /** 无障碍名；有关联 label 时可不传 */
  'aria-label'?: string
  className?: string
  title?: string
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
  }) => (
    <label
      className={`toggle-switch${disabled ? ' disabled' : ''}${className ? ` ${className}` : ''}`}
      title={title}
      onClick={(e) => e.stopPropagation()}
    >
      <input
        id={id}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-label={ariaLabel}
        onChange={(e) => {
          if (!disabled) onChange(e.target.checked)
        }}
      />
      <span className="toggle-slider" aria-hidden="true" />
    </label>
  ),
)

ToggleSwitch.displayName = 'ToggleSwitch'
