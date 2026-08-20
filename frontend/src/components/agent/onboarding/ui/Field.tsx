import type {
  InputHTMLAttributes,
  ReactNode,
  TextareaHTMLAttributes,
} from 'react'

export function Field({
  label,
  hint,
  optional,
  optionalLabel,
  children,
}: {
  label: string
  hint?: string
  optional?: boolean
  optionalLabel?: string
  children: ReactNode
}) {
  return (
    <label className="life-ob-field">
      <span className="life-ob-field__label">
        {label}
        {optional && optionalLabel && (
          <i className="life-ob-field__optional">{optionalLabel}</i>
        )}
      </span>
      {children}
      {hint && <small className="life-ob-field__hint">{hint}</small>}
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
    <fieldset className="life-ob-field">
      <legend className="life-ob-field__label">{label}</legend>
      {children}
    </fieldset>
  )
}

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input className="life-ob-input" {...props} />
}

export function TextArea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className="life-ob-input life-ob-input--area" {...props} />
}
