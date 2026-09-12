import { ISLAND_BTN, ISLAND_BTN_DANGER, ISLAND_BTN_PRIMARY } from './constants'

type ButtonVariant = 'default' | 'primary' | 'danger'

export interface IslandButtonProps {
  onClick: () => void
  children: React.ReactNode
  variant?: ButtonVariant
  label?: string
  title?: string
  disabled?: boolean
  className?: string
}

const variantClass: Record<ButtonVariant, string> = {
  default: ISLAND_BTN,
  primary: ISLAND_BTN_PRIMARY,
  danger: ISLAND_BTN_DANGER,
}

export function IslandButton({
  onClick,
  children,
  variant = 'default',
  label,
  title,
  disabled = false,
  className = '',
}: IslandButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={title}
      className={`${variantClass[variant]} ${className}`}
    >
      {children}
      {label && <span className="text-sm">{label}</span>}
    </button>
  )
}
