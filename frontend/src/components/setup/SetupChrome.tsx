import type {
  ComponentType,
  InputHTMLAttributes,
  ReactNode,
  RefObject,
  SVGProps,
} from 'react'
import {
  LuAlertTriangle,
  LuArrowRight,
  LuCheck,
  LuChevronLeft,
  LuInfo,
  LuLoader2,
  LuSettings,
} from '@lib/icons'
import { useEffect } from 'react'

type Glyph = ComponentType<SVGProps<SVGSVGElement>>

export function Aurora() {
  return (
    <div className="setup-ob__aurora" aria-hidden>
      <i />
      <i />
    </div>
  )
}

export function useTopBarDense(
  cardRef: RefObject<HTMLElement | null>,
  scrollerRef: RefObject<HTMLElement | null>,
  key: string,
): void {
  useEffect(() => {
    const card = cardRef.current
    const scroller = scrollerRef.current
    if (!card) return undefined
    if (!scroller) {
      card.style.setProperty('--sob-top-dense', '0')
      return undefined
    }

    const syncDense = () => {
      const t = Math.min(1, Math.max(0, scroller.scrollTop / 128))
      const dense = t * t * (3 - 2 * t)
      card.style.setProperty('--sob-top-dense', dense.toFixed(3))
    }

    syncDense()
    scroller.addEventListener('scroll', syncDense, { passive: true })
    return () => {
      scroller.removeEventListener('scroll', syncDense)
      card.style.setProperty('--sob-top-dense', '0')
    }
  }, [cardRef, scrollerRef, key])
}

export function StepTopBar({
  back,
  stepName,
  current,
  total,
  progressText,
}: {
  back?: ReactNode
  stepName?: string
  current?: number
  total?: number
  progressText?: string
}) {
  return (
    <div className="setup-ob-top">
      <div className="setup-ob-top__side" key={stepName ?? 'brand'}>
        {back}
      </div>
      {stepName && current && total ? (
        <p
          className="setup-ob-top__step"
          aria-label={progressText}
          key={`${current}-${stepName}`}
        >
          <b>{stepName}</b>
          <span aria-hidden>
            {current}/{total}
          </span>
        </p>
      ) : null}
    </div>
  )
}

export function BackButton({
  label,
  destination,
  disabled,
  onClick,
}: {
  label: string
  destination: string
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <div className="setup-ob-back">
      <button
        type="button"
        className="setup-ob-back__hit"
        disabled={disabled}
        title={label}
        aria-label={label}
        onClick={onClick}
      >
        <LuChevronLeft aria-hidden />
      </button>
      <span className="setup-ob-back__label" aria-hidden>
        {destination}
      </span>
    </div>
  )
}

export function BrandMark({
  label,
  icon: Icon = LuSettings,
}: {
  label: string
  icon?: Glyph
}) {
  return (
    <div className="setup-ob-back is-static">
      <span className="setup-ob-back__hit" aria-hidden>
        <Icon />
      </span>
      <span className="setup-ob-back__label">{label}</span>
    </div>
  )
}

export function BrandTag({
  label,
  showLogo = true,
}: {
  label: string
  showLogo?: boolean
}) {
  return (
    <span className={`setup-ob-brand${showLogo ? '' : ' is-textonly'}`}>
      {showLogo ? <img src="/logo.webp" alt="" aria-hidden /> : null}
      {label}
    </span>
  )
}

export function StepHero({
  eyebrow,
  title,
  lead,
  titleId,
  action,
  notes,
}: {
  eyebrow?: string
  title: ReactNode
  lead: string
  titleId: string
  action?: ReactNode
  notes?: ReactNode
}) {
  return (
    <header className="setup-ob-hero">
      {eyebrow ? <p className="setup-ob-hero__eyebrow">{eyebrow}</p> : null}
      <div className="setup-ob-hero__row">
        <h1 id={titleId}>{title}</h1>
        {action}
      </div>
      <p className="setup-ob-hero__lead">{lead}</p>
      {notes ? <div className="setup-ob-hero__notes">{notes}</div> : null}
    </header>
  )
}

export function StepBody({ children }: { children: ReactNode }) {
  return <div className="setup-ob-body">{children}</div>
}

export function ActionBar({
  children,
  split = false,
}: {
  children: ReactNode
  split?: boolean
}) {
  return (
    <footer className={`setup-ob-bar${split ? ' is-split' : ''}`}>
      {children}
    </footer>
  )
}

export function PrimaryButton({
  label,
  icon: Icon = LuArrowRight,
  busy = false,
  disabled = false,
  type = 'button',
  onClick,
}: {
  label: string
  // 正文动作传 null 表示不要图标。
  icon?: Glyph | null
  busy?: boolean
  disabled?: boolean
  type?: 'button' | 'submit'
  onClick?: () => void
}) {
  return (
    <button
      type={type}
      className="setup-ob-cta"
      disabled={disabled || busy}
      title={label}
      aria-label={label}
      onClick={onClick}
    >
      <span>{label}</span>
      {busy ? (
        <LuLoader2 className="setup-ob-spin" aria-hidden />
      ) : Icon ? (
        <Icon aria-hidden />
      ) : null}
    </button>
  )
}

export function GhostButton({
  label,
  icon: Icon,
  busy = false,
  disabled = false,
  onClick,
}: {
  label: string
  icon?: Glyph
  busy?: boolean
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className="setup-ob-ghost"
      disabled={disabled || busy}
      onClick={onClick}
    >
      {busy ? (
        <LuLoader2 className="setup-ob-spin" aria-hidden />
      ) : Icon ? (
        <Icon aria-hidden />
      ) : null}
      {label}
    </button>
  )
}

export type NoteTone = 'info' | 'warn' | 'success' | 'error' | 'active'

export function Note({
  tone = 'info',
  children,
}: {
  tone?: NoteTone
  children: ReactNode
}) {
  const Icon =
    tone === 'warn' || tone === 'error'
      ? LuAlertTriangle
      : tone === 'success'
        ? LuCheck
        : LuInfo
  return (
    <p
      className={`setup-ob-note is-${tone}`}
      role={tone === 'error' ? 'alert' : 'status'}
      aria-live="polite"
    >
      <Icon aria-hidden />
      <span>{children}</span>
    </p>
  )
}

export function Field({
  label,
  hint,
  optional,
  optionalLabel,
  wide,
  children,
}: {
  label: string
  hint?: ReactNode
  optional?: boolean
  optionalLabel?: string
  wide?: boolean
  children: ReactNode
}) {
  return (
    <label className={`setup-ob-field${wide ? ' is-wide' : ''}`}>
      <span className="setup-ob-field__label">
        {label}
        {optional && optionalLabel && (
          <i className="setup-ob-field__optional">{optionalLabel}</i>
        )}
      </span>
      {children}
      {hint && <small className="setup-ob-field__hint">{hint}</small>}
    </label>
  )
}

export function TextInput({
  mono,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }) {
  return (
    <input className={`setup-ob-input${mono ? ' is-mono' : ''}`} {...props} />
  )
}
