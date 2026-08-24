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
    <label className="merope-ob-field">
      <span className="merope-ob-field__label">
        {label}
        {optional && optionalLabel && (
          <i className="merope-ob-field__optional">{optionalLabel}</i>
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
