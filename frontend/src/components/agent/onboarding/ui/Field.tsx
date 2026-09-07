import type {
  InputHTMLAttributes,
  ReactNode,
  TextareaHTMLAttributes,
} from 'react'

export function charCount(value: string): number {
  return [...value].length
}

export function CharMeter({
  value,
  max,
}: {
  value: string
  max: number
}) {
  const count = charCount(value)
  return (
    <small
      className={`merope-ob-field__count${count >= max ? ' is-max' : ''}`}
      aria-live="polite"
    >
      {count}/{max}
    </small>
  )
}

export function Field({
  label,
  hint,
  optional,
  optionalLabel,
  value,
  max,
  children,
}: {
  label: string
  hint?: string
  optional?: boolean
  optionalLabel?: string
  value?: string
  max?: number
  children: ReactNode
}) {
  return (
    <label className="merope-ob-field">
      <span className="merope-ob-field__label">
        {label}
        {optional && optionalLabel && (
          <i className="merope-ob-field__optional">{optionalLabel}</i>
        )}
        {typeof max === 'number' && (
          <CharMeter value={value ?? ''} max={max} />
        )}
      </span>
      {children}
      {hint && <small className="merope-ob-field__hint">{hint}</small>}
    </label>
  )
}

export function FieldGroup({
  label,
  children,
}: {
  label: string
  children: ReactNode
}) {
  return (
    <fieldset className="merope-ob-field">
      <legend className="merope-ob-field__label">{label}</legend>
      {children}
    </fieldset>
  )
}

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input className="merope-ob-input" {...props} />
}

export function TextArea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className="merope-ob-input merope-ob-input--area" {...props} />
}
