import type { ReactNode } from 'react'
import { LuArrowRight, LuLoader2 } from '@lib/icons'

export function StepBody({ children }: { children: ReactNode }) {
  return <div className="life-ob-body sm-stagger">{children}</div>
}

export function GhostButton({
  label,
  disabled,
  onClick,
}: {
  label: string
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className="life-ghost-button"
      disabled={disabled}
      onClick={onClick}
    >
      {label}
    </button>
  )
}

export function PrimaryButton({
  label,
  busy = false,
  disabled = false,
  onClick,
}: {
  label: string
  busy?: boolean
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className="life-ob-cta"
      disabled={disabled || busy}
      onClick={onClick}
    >
      <span>{label}</span>
      {busy ? (
        <LuLoader2 className="is-spinning" aria-hidden />
      ) : (
        <LuArrowRight aria-hidden />
      )}
    </button>
  )
}

export function ActionBar({ children }: { children: ReactNode }) {
  return <footer className="life-ob-bar">{children}</footer>
}
