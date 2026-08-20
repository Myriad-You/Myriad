/**
 * Unified action button for settings / config surfaces.
 *
 * Uses the shared `btn-base` + variant classes (ConfigForm.css) so every
 * primary / secondary / danger control looks the same — floating save,
 * modal footers, list toolbars, row actions, ButtonItem, etc.
 *
 * Selection chips / segmented radios / row expanders stay domain-specific;
 * only action buttons go through this component.
 */

import type { ButtonHTMLAttributes, ReactNode } from 'react'
import React, { useCallback } from 'react'
import { ButtonSpinner } from '../../Spinner'
import './SettingsButton.css'

export type SettingsButtonVariant =
  | 'primary'
  | 'secondary'
  | 'danger'
  | 'ghost'
  | 'icon'

/**
 * - `sm` — dense (list rows, card headers, compact toolbars)
 * - `md` — default settings / modal actions
 * - `lg` — hero / floating primary
 */
export type SettingsButtonSize = 'sm' | 'md' | 'lg'

export interface SettingsButtonProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  variant?: SettingsButtonVariant
  size?: SettingsButtonSize
  loading?: boolean
  /** Leading icon; hidden while loading. */
  icon?: ReactNode
  children?: ReactNode
  /** Optional window.confirm before invoking onClick. */
  confirm?: string
  /** Stretch to container width. */
  block?: boolean
}

function variantClass(variant: SettingsButtonVariant): string {
  switch (variant) {
    case 'primary':
      return 'btn-primary'
    case 'danger':
      return 'btn-danger'
    case 'ghost':
      return 'settings-btn--ghost'
    case 'icon':
      return 'btn-icon'
    case 'secondary':
    default:
      return 'btn-secondary'
  }
}

/** Size tokens live on `.settings-btn--*`; keep global btn-sm/lg for legacy CSS hooks. */
function sizeClass(size: SettingsButtonSize): string {
  switch (size) {
    case 'sm':
      return 'settings-btn--sm btn-sm'
    case 'lg':
      return 'settings-btn--lg btn-lg'
    case 'md':
    default:
      return 'settings-btn--md'
  }
}

export const SettingsButton = React.memo(({
  variant = 'secondary',
  size = 'md',
  loading = false,
  disabled = false,
  icon,
  children,
  confirm,
  block = false,
  className = '',
  onClick,
  type = 'button',
  ...rest
}: SettingsButtonProps) => {
  const handleClick = useCallback(
    (e: React.MouseEvent<HTMLButtonElement>) => {
      if (disabled || loading) return
      if (confirm && !window.confirm(confirm)) return
      onClick?.(e)
    },
    [disabled, loading, confirm, onClick],
  )

  const showLabel = children != null && children !== false && children !== ''

  const classes = [
    'btn-base',
    'settings-btn',
    variantClass(variant),
    sizeClass(size),
    block ? 'settings-btn--block' : '',
    loading ? 'is-loading' : '',
    variant === 'danger' && !showLabel ? 'settings-btn--danger-quiet' : '',
    className,
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <button
      type={type}
      className={classes}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      onClick={handleClick}
      {...rest}
    >
      {loading ? (
        <ButtonSpinner />
      ) : (
        <>
          {icon != null && icon !== false ? (
            <span className="settings-btn-icon" aria-hidden={showLabel || undefined}>
              {icon}
            </span>
          ) : null}
          {showLabel ? (
            <span className="settings-btn-label">{children}</span>
          ) : null}
        </>
      )}
    </button>
  )
})

export default SettingsButton
