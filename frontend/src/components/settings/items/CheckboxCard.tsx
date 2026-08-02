/**
 * Toggle / press chip — same visual language as 权限下放 / CheckboxGroupItem.
 * (title + optional description / icon; switch keeps status dot)
 *
 * Variants:
 * - `switch` (default) — click flips `checked`; sticky active chrome
 * - `action` — press control (搜索 / 刷新 / 新建); flash on press, no sticky
 *              toggle unless parent sets `checked` (e.g. filter active)
 *
 * Sizes:
 * - `default` — full card (permissions / notifications)
 * - `sm` / `stat` — dense list chip (ManagedList top row)
 */

import type { ReactNode } from 'react'
import React, { useCallback, useRef } from 'react'
import { Spinner } from '../../Spinner'
import './SettingItem.css'

export type CheckboxCardVariant = 'switch' | 'action'
export type CheckboxCardSize = 'default' | 'sm' | 'stat'
/** Soft accent for action chips (bulk retry / clear). */
export type CheckboxCardTone = 'default' | 'primary' | 'danger'

export interface CheckboxCardProps {
  label: ReactNode
  checked: boolean
  /**
   * `switch`: called with the next boolean.
   * `action`: called with `true` on press (checked is not flipped by the card).
   */
  onChange: (checked: boolean) => void
  description?: ReactNode
  icon?: ReactNode
  disabled?: boolean
  loading?: boolean
  /** Native tooltip */
  title?: string
  className?: string
  variant?: CheckboxCardVariant
  size?: CheckboxCardSize
  /** Action accent (primary / danger bulk tools). Default neutral. */
  tone?: CheckboxCardTone
  /**
   * Dot indicator. Default: on for `switch`, off for `action`.
   */
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
    // Force reflow so re-click restarts the animation
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

  // Loading leading slot: spin icon in place (refresh), else spinner ring.
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
      // Keep focusable during load so layout doesn't jump; block via pointer-events.
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
        {/*
          Text column: title + desc share the same start edge (after leading),
          so multi-line chips align with short action chips like 刷新.
        */}
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
