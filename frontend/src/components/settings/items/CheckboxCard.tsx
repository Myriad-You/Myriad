import type { ReactNode } from 'react'
import React, { useCallback, useRef } from 'react'
import { Spinner } from '../../Spinner'
import './SettingItem.css'

export type CheckboxCardVariant = 'switch' | 'action'
export type CheckboxCardSize = 'default' | 'sm' | 'stat'
export type CheckboxCardTone = 'default' | 'primary' | 'danger'

export interface CheckboxCardProps {
  label: ReactNode
  checked: boolean
  onChange: (checked: boolean) => void
  description?: ReactNode
  icon?: ReactNode
  disabled?: boolean
  loading?: boolean
  title?: string
  className?: string
  variant?: CheckboxCardVariant
  size?: CheckboxCardSize
  tone?: CheckboxCardTone
  showIndicator?: boolean
  'aria-label'?: string
  'aria-expanded'?: boolean
}

export const CheckboxCard = React.memo<CheckboxCardProps>(({
  label,
  checked,
  onChange,
  description,
  icon,
  disabled = false,
  loading = false,
  title,
  className = '',
  variant = 'switch',
  size = 'default',
  tone = 'default',
  showIndicator,
  'aria-label': ariaLabel,
  'aria-expanded': ariaExpanded,
}) => {
  const busy = disabled || loading
  const dense = size === 'sm' || size === 'stat'
  const isAction = variant === 'action'
  const showDot = showIndicator ?? !isAction
  const btnRef = useRef<HTMLButtonElement>(null)
  const flashTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const flashPress = useCallback(() => {
    const el = btnRef.current
    if (!el || !isAction) return
    el.classList.remove('is-pressing')
    void el.offsetWidth
    el.classList.add('is-pressing')
    if (flashTimer.current) clearTimeout(flashTimer.current)
    flashTimer.current = setTimeout(() => {
      el.classList.remove('is-pressing')
      flashTimer.current = null
    }, 220)
  }, [isAction])

  const handleClick = useCallback(() => {
    if (busy) return
    if (isAction) {
      flashPress()
      onChange(true)
      return
    }
    onChange(!checked)
  }, [busy, isAction, flashPress, onChange, checked])

  const ariaLabelText =
    ariaLabel ?? (typeof label === 'string' ? label : undefined)

  const hasIcon = icon != null && icon !== false

  let leading: ReactNode = null
  if (loading && hasIcon) {
    leading = (
      <span
        className="checkbox-group-card-icon is-spinning"
        aria-hidden
      >
        {icon}
      </span>
    )
  } else if (loading) {
    leading = <Spinner size="xs" color="primary" />
  } else if (showDot) {
    leading = (
      <span className="checkbox-group-card-indicator" aria-hidden />
    )
  }

  const showStaticIcon = hasIcon && !loading

  return (
    <button
      ref={btnRef}
      type="button"
      className={[
        'checkbox-group-card',
        isAction ? 'checkbox-group-card--action' : '',
        tone !== 'default' ? `checkbox-group-card--tone-${tone}` : '',
        checked ? 'active' : '',
        dense ? 'checkbox-group-card--sm' : '',
        hasIcon ? 'has-icon' : '',
        !showDot ? 'no-indicator' : '',
        loading ? 'is-loading' : '',
        disabled && !loading ? 'is-disabled' : '',
        className,
      ]
        .filter(Boolean)
        .join(' ')}
      onClick={handleClick}
      disabled={disabled && !loading}
      aria-disabled={busy || undefined}
      title={title}
      aria-pressed={isAction ? undefined : checked}
      aria-expanded={ariaExpanded}
      aria-busy={loading || undefined}
      aria-label={ariaLabelText}
    >
      <span className="checkbox-group-card-header">
        {leading}
        {showStaticIcon ? (
          <span className="checkbox-group-card-icon" aria-hidden>
            {icon}
          </span>
        ) : null}
        <span className="checkbox-group-card-text">
          <span className="checkbox-group-card-label">{label}</span>
          {description != null &&
          description !== false &&
          description !== '' ? (
            <span className="checkbox-group-card-desc">{description}</span>
          ) : null}
        </span>
      </span>
    </button>
  )
})

export default CheckboxCard
