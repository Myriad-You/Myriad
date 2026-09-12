/** 不绑订阅源字段。 */
import type {
  ButtonHTMLAttributes,
  FormEventHandler,
  HTMLAttributes,
  InputHTMLAttributes,
  LabelHTMLAttributes,
  ReactNode,
} from 'react'

import { cx } from './cx'
import './brew.css'

export function Sheet({
  children,
  edit,
  className,
}: {
  children: ReactNode
  edit?: boolean
  className?: string
}) {
  return (
    <div
      className={cx(
        'brew-skin brew-sheet',
        edit && 'brew-sheet--edit',
        className,
      )}
    >
      {children}
    </div>
  )
}

export function SheetBody({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__body">{children}</div>
}

export function SheetStack({
  children,
  onSubmit,
}: {
  children: ReactNode
  onSubmit?: FormEventHandler<HTMLFormElement>
}) {
  if (onSubmit) {
    return (
      <form className="brew-sheet__stack" onSubmit={onSubmit}>
        {children}
      </form>
    )
  }
  return <div className="brew-sheet__stack">{children}</div>
}

export function SheetField({
  label,
  required,
  hint,
  htmlFor,
  children,
}: {
  label?: ReactNode
  required?: boolean
  hint?: ReactNode
  htmlFor?: string
  children: ReactNode
}) {
  return (
    <div className="brew-sheet__field">
      {label != null ? (
        htmlFor ? (
          <label className="brew-sheet__label" htmlFor={htmlFor}>
            {label}
            {required ? <span className="brew-sheet__req"> *</span> : null}
          </label>
        ) : (
          <span className="brew-sheet__label">
            {label}
            {required ? <span className="brew-sheet__req"> *</span> : null}
          </span>
        )
      ) : null}
      {children}
      {hint != null ? <p className="brew-sheet__hint">{hint}</p> : null}
    </div>
  )
}

export function SheetHint({ children }: { children: ReactNode }) {
  return <p className="brew-sheet__hint">{children}</p>
}

export function SheetChoices({
  children,
  mode,
}: {
  children: ReactNode
  mode?: boolean
}) {
  return (
    <div
      className={cx(
        'brew-sheet__choices',
        mode && 'brew-sheet__choices--mode',
      )}
    >
      {children}
    </div>
  )
}

export function SheetChoice({
  on,
  disabled,
  onClick,
  children,
}: {
  on?: boolean
  disabled?: boolean
  onClick?: () => void
  children: ReactNode
}) {
  return (
    <button
      type="button"
      className={cx('brew-sheet__choice', on && 'is-on')}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

export function SheetSwitch({
  icon,
  title,
  description,
  on,
  onToggle,
  disabled,
  toggleTitle,
}: {
  icon?: ReactNode
  title: ReactNode
  description?: ReactNode
  on: boolean
  onToggle: () => void
  disabled?: boolean
  toggleTitle?: string
}) {
  return (
    <div className="brew-sheet__switch">
      {icon}
      <div className="brew-sheet__switch-copy">
        <strong>{title}</strong>
        {description != null ? <span>{description}</span> : null}
      </div>
      <button
        type="button"
        className={cx('brew-sheet__toggle', on && 'is-on')}
        onClick={onToggle}
        disabled={disabled}
        title={toggleTitle}
        aria-pressed={on}
      >
        <i />
      </button>
    </div>
  )
}

export function SheetInput({
  withMark,
  className,
  ...rest
}: InputHTMLAttributes<HTMLInputElement> & { withMark?: boolean }) {
  return (
    <input
      className={cx('brew-sheet__input', withMark && 'is-with-mark', className)}
      {...rest}
    />
  )
}

export function SheetTrigger({
  children,
  className,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button type="button" className={cx('brew-sheet__input', className)} {...rest}>
      {children}
    </button>
  )
}

export function SheetRow({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__field-row">{children}</div>
}

export function SheetGrow({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__grow">{children}</div>
}

export function SheetPair({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__pair">{children}</div>
}

export function SheetNotice({
  tone,
  children,
}: {
  tone?: 'ok' | 'bad'
  children: ReactNode
}) {
  return (
    <div
      className={cx(
        'brew-sheet__notice',
        tone === 'ok' && 'is-ok',
        tone === 'bad' && 'is-bad',
      )}
    >
      {children}
    </div>
  )
}

export function SheetMark({ children }: { children: ReactNode }) {
  return <span className="brew-sheet__mark">{children}</span>
}

export function SheetMenu({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__menu">{children}</div>
}

export function SheetMenuBody({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__menu-body">{children}</div>
}

export function SheetMenuItem({
  children,
  on,
  onClick,
}: {
  children: ReactNode
  on?: boolean
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      className={cx('brew-sheet__menu-item', on && 'is-on')}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

export function SheetDrop({
  on,
  children,
  ...rest
}: {
  on?: boolean
  children: ReactNode
} & HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cx('brew-sheet__drop', on && 'is-on')} {...rest}>
      {children}
    </div>
  )
}

export function SheetSubmit({
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button type="submit" className="brew-sheet__submit" {...rest}>
      {children}
    </button>
  )
}

export function SheetGhost({
  fit,
  children,
  className,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { fit?: boolean }) {
  return (
    <button
      type="button"
      className={cx('brew-sheet__ghost', fit && 'is-fit', className)}
      {...rest}
    >
      {children}
    </button>
  )
}

export function SheetGhostLabel({
  fit,
  children,
  className,
  ...rest
}: LabelHTMLAttributes<HTMLLabelElement> & { fit?: boolean }) {
  return (
    <label
      className={cx('brew-sheet__ghost', fit && 'is-fit', className)}
      {...rest}
    >
      {children}
    </label>
  )
}

export function SheetFoot({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__foot">{children}</div>
}

export function SheetTab({
  on,
  children,
  onClick,
}: {
  on?: boolean
  children: ReactNode
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      className={cx('brew-sheet__tab', on && 'is-on')}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

export function SheetPills({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__pills">{children}</div>
}

export function SheetPill({ children }: { children: ReactNode }) {
  return <span className="brew-sheet__pill">{children}</span>
}

export function SheetSwatch({ children }: { children: ReactNode }) {
  return <label className="brew-sheet__swatch">{children}</label>
}

export function SheetCount({ children }: { children: ReactNode }) {
  return <span className="brew-sheet__count">{children}</span>
}

export function SheetKeys({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__keys">{children}</div>
}

export function SheetGroup({ children }: { children: ReactNode }) {
  return <section className="brew-sheet__group">{children}</section>
}

export function SheetGroupHead({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__group-head">{children}</div>
}

export function SheetShortcut({ children }: { children: ReactNode }) {
  return <div className="brew-sheet__shortcut">{children}</div>
}

export function SheetKbd({ children }: { children: ReactNode }) {
  return <kbd className="brew-sheet__kbd">{children}</kbd>
}
