import type { ReactNode } from 'react'
import { LuX } from '@lib/icons'
import React, { useCallback } from 'react'
import { SettingTitleHelp } from './SettingTitleHelp'
import './SettingTitleTag.css'

export type SettingTitleTagVariant = 'default' | 'muted' | 'danger' | 'beta'

export interface SettingTitleTagProps {
  children: ReactNode
  icon?: ReactNode
  onClick?: () => void
  title?: string
  detail?: ReactNode
  disabled?: boolean
  variant?: SettingTitleTagVariant
  className?: string
  role?: string
  onDismiss?: () => void
  dismissAriaLabel?: string
}

export const SettingTitleTag: React.FC<SettingTitleTagProps> = ({
  children,
  icon,
  onClick,
  title,
  detail,
  disabled = false,
  variant = 'default',
  className = '',
  role,
  onDismiss,
  dismissAriaLabel = 'Dismiss',
}) => {
  const classes = [
    'setting-title-tag',
    variant === 'muted' ? 'setting-title-tag--muted' : '',
    variant === 'danger' ? 'setting-title-tag--danger' : '',
    variant === 'beta' ? 'setting-title-tag--beta' : '',
    onDismiss ? 'setting-title-tag--dismissible' : '',
    onClick ? 'setting-title-tag--actionable' : '',
    className,
  ]
    .filter(Boolean)
    .join(' ')
  const a11yRole = role ?? (variant === 'danger' ? 'alert' : undefined)

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      e.preventDefault()
      if (!disabled) onClick?.()
    },
    [disabled, onClick],
  )

  const handleDismiss = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      e.preventDefault()
      if (!disabled) onDismiss?.()
    },
    [disabled, onDismiss],
  )

  const dismissBtn = onDismiss ? (
    <button
      type="button"
      className="setting-title-tag-dismiss"
      aria-label={dismissAriaLabel}
      disabled={disabled}
      onClick={handleDismiss}
    >
      <LuX aria-hidden />
    </button>
  ) : null

  const mainInner = (
    <>
      {icon && (
        <span className="setting-title-tag-icon" aria-hidden>
          {icon}
        </span>
      )}
      <span className="setting-title-tag-label">{children}</span>
    </>
  )

  // clickable+dismiss: button + separate ×; no nested buttons
  if (onClick && onDismiss) {
    return (
      <span className={classes} title={title} role={a11yRole}>
        <button
          type="button"
          className="setting-title-tag-main"
          disabled={disabled}
          onClick={handleClick}
        >
          {mainInner}
        </button>
        {detail != null && detail !== '' && (
          <SettingTitleHelp>{detail}</SettingTitleHelp>
        )}
        {dismissBtn}
      </span>
    )
  }

  if (onClick) {
    return (
      <button
        type="button"
        className={classes}
        title={title}
        disabled={disabled}
        onClick={handleClick}
        role={a11yRole}
      >
        {mainInner}
        {detail != null && detail !== '' && (
          <SettingTitleHelp>{detail}</SettingTitleHelp>
        )}
      </button>
    )
  }

  return (
    <span className={classes} title={title} role={a11yRole}>
      {mainInner}
      {detail != null && detail !== '' && (
        <SettingTitleHelp>{detail}</SettingTitleHelp>
      )}
      {dismissBtn}
    </span>
  )
}

SettingTitleTag.displayName = 'SettingTitleTag'
