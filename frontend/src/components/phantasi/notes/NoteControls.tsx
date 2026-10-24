/**
 * 编辑器自己的控件：墨色、够大、不借设置页的皮。
 * 颜色只用纸色和墨色，壁纸强调色不进来。
 */

import type { ButtonHTMLAttributes, CSSProperties, ReactNode } from 'react'
import { LuChevronDown as ChevronDown, LuX as X } from '@lib/icons'
import { useId } from 'react'
import { ButtonSpinner } from '../../Spinner'
import { fromDatetimeLocal, toDatetimeLocal } from './noteFields'

type NoteButtonVariant = 'solid' | 'tint' | 'outline' | 'quiet' | 'danger'
type NoteButtonSize = 'md' | 'lg'

interface NoteButtonProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  variant?: NoteButtonVariant
  size?: NoteButtonSize
  icon?: ReactNode
  loading?: boolean
  active?: boolean
  children?: ReactNode
}

export function NoteButton({
  variant = 'outline',
  size = 'md',
  icon,
  loading = false,
  active = false,
  disabled = false,
  children,
  className = '',
  type = 'button',
  ...rest
}: NoteButtonProps) {
  const hasLabel = children != null && children !== false && children !== ''
  const classes = [
    'note-btn',
    `note-btn--${variant}`,
    `note-btn--${size}`,
    hasLabel ? '' : 'note-btn--icon',
    active ? 'is-active' : '',
    loading ? 'is-loading' : '',
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
      {...rest}
    >
      {loading ? (
        <span className="note-btn__icon">
          <ButtonSpinner size="xs" />
        </span>
      ) : icon ? (
        <span className="note-btn__icon">{icon}</span>
      ) : null}
      {hasLabel ? <span className="note-btn__label">{children}</span> : null}
    </button>
  )
}

/* ------------------------------------------------------------------ */

interface NoteSwitchOption<T extends string> {
  value: T
  label: ReactNode
}

interface NoteSwitchProps<T extends string> {
  disabled?: boolean
  value: T | null
  options: NoteSwitchOption<T>[]
  onChange: (value: T) => void
  ariaLabel?: string
  className?: string
}

/** 两三个互斥选项的滑块开关。 */
export function NoteSwitch<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
  className = '',
  disabled = false,
}: NoteSwitchProps<T>) {
  const index = options.findIndex((option) => option.value === value)
  return (
    <div
      className={`note-switch ${className}`.trim()}
      role="radiogroup"
      aria-label={ariaLabel}
      style={
        {
          '--note-switch-count': options.length,
          '--note-switch-index': index < 0 ? 0 : index,
        } as CSSProperties
      }
    >
      {index >= 0 ? <span className="note-switch__thumb" aria-hidden="true" /> : null}
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          disabled={disabled}
          aria-checked={option.value === value}
          className={`note-switch__item${option.value === value ? ' is-on' : ''}`}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  )
}

/* ------------------------------------------------------------------ */

interface NoteFieldProps {
  label: ReactNode
  hint?: ReactNode
  htmlFor?: string
  children: ReactNode
}

export function NoteField({ label, hint, htmlFor, children }: NoteFieldProps) {
  return (
    <div className="note-field">
      <label className="note-field__label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {hint ? <p className="note-field__hint">{hint}</p> : null}
    </div>
  )
}

interface NoteSectionProps {
  title: ReactNode
  hint?: ReactNode
  children: ReactNode
}

export function NoteSection({ title, hint, children }: NoteSectionProps) {
  return (
    <section className="note-section">
      <h3 className="note-section__title">{title}</h3>
      {hint ? <p className="note-field__hint">{hint}</p> : null}
      <div className="note-section__body">{children}</div>
    </section>
  )
}

interface NoteChipProps {
  children: ReactNode
  active?: boolean
  muted?: boolean
  onClick?: () => void
  onDismiss?: () => void
  dismissLabel?: string
}

export function NoteChip({
  children,
  active = false,
  muted = false,
  onClick,
  onDismiss,
  dismissLabel,
}: NoteChipProps) {
  const className = [
    'note-chip',
    active ? 'is-on' : '',
    muted ? 'is-muted' : '',
  ]
    .filter(Boolean)
    .join(' ')
  const label = <span className="note-chip__label">{children}</span>
  const dismiss = onDismiss ? (
    <button
      type="button"
      className="note-chip__dismiss"
      aria-label={dismissLabel}
      onClick={onDismiss}
    >
      <X />
    </button>
  ) : null
  if (onClick) {
    return (
      <span className={className}>
        <button type="button" className="note-chip__face" onClick={onClick}>
          {label}
        </button>
        {dismiss}
      </span>
    )
  }
  return (
    <span className={className}>
      {label}
      {dismiss}
    </span>
  )
}

interface NoteSelectProps<T extends string> {
  disabled?: boolean
  id?: string
  value: T
  options: { value: T; label: string }[]
  onChange: (value: T) => void
  'aria-label'?: string
}

export function NoteSelect<T extends string>({
  id,
  value,
  options,
  onChange,
  ...rest
}: NoteSelectProps<T>) {
  const fallbackId = useId()
  return (
    <span className="note-select">
      <select
        id={id ?? fallbackId}
        className="note-input note-select__native"
        value={value}
        onChange={(event) => onChange(event.target.value as T)}
        aria-label={rest['aria-label']}
        disabled={rest.disabled}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <ChevronDown className="note-select__chevron" aria-hidden="true" />
    </span>
  )
}

interface NoteDateInputProps {
  id?: string
  value: number | null
  onChange: (ms: number | null) => void
  'aria-label'?: string
}

export function NoteDateInput({ id, value, onChange, ...rest }: NoteDateInputProps) {
  return (
    <span className="note-date">
      <input
        id={id}
        type="datetime-local"
        className="note-input note-date__native"
        value={toDatetimeLocal(value ?? 0)}
        onChange={(event) => onChange(fromDatetimeLocal(event.target.value))}
        aria-label={rest['aria-label']}
      />
    </span>
  )
}
