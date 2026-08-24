import type { NameStyle } from '../onboardingTypes'
import { LuCheck, LuChevronDown, LuLoader2, LuShuffle } from '@lib/icons'
import { useEffect, useId, useRef, useState } from 'react'
import { NAME_STYLE_OPTIONS } from '../onboardingTypes'

export default function NameStyleRoll({
  style,
  labels,
  styleLabel,
  rollLabel,
  busyLabel,
  disabled,
  rolling,
  onStyle,
  onRoll,
}: {
  style: NameStyle
  labels: Record<NameStyle, string>
  styleLabel: string
  rollLabel: string
  busyLabel: string
  disabled?: boolean
  rolling?: boolean
  onStyle: (style: NameStyle) => void
  onRoll: () => void
}) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const menuId = useId()

  useEffect(() => {
    if (!open) return
    const onPointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onPointer)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  useEffect(() => {
    if (disabled || rolling) setOpen(false)
  }, [disabled, rolling])

  return (
    <div
      ref={rootRef}
      className={`merope-ob-name-dice${open ? ' is-open' : ''}${rolling ? ' is-rolling' : ''}`}
    >
      <div className="merope-ob-name-dice__face">
        <button
          type="button"
          className="merope-ob-name-dice__style"
          disabled={disabled || rolling}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-controls={menuId}
          aria-label={`${styleLabel}: ${labels[style]}`}
          onClick={() => setOpen((current) => !current)}
        >
          <span className="merope-ob-name-dice__label">{labels[style]}</span>
          <LuChevronDown className="merope-ob-name-dice__chevron" aria-hidden />
        </button>
        <span className="merope-ob-name-dice__split" aria-hidden />
        <button
          type="button"
          className="merope-ob-name-dice__roll"
          disabled={disabled || rolling}
          title={rolling ? busyLabel : rollLabel}
          aria-label={rolling ? busyLabel : rollLabel}
          onClick={onRoll}
        >
          {rolling ? (
            <LuLoader2 className="merope-ob-name-roll__spin" aria-hidden />
          ) : (
            <LuShuffle aria-hidden />
          )}
        </button>
      </div>
      {open ? (
        <ul
          id={menuId}
          className="merope-ob-name-dice__menu"
          role="listbox"
          aria-label={styleLabel}
        >
          {NAME_STYLE_OPTIONS.map((option) => {
            const selected = option === style
            return (
              <li key={option} role="presentation">
                <button
                  type="button"
                  role="option"
                  aria-selected={selected}
                  className={`merope-ob-name-dice__option${selected ? ' is-on' : ''}`}
                  onClick={() => {
                    onStyle(option)
                    setOpen(false)
                  }}
                >
                  <span className="merope-ob-name-dice__mark" aria-hidden>
                    {selected ? <LuCheck /> : null}
                  </span>
                  <span>{labels[option]}</span>
                </button>
              </li>
            )
          })}
        </ul>
      ) : null}
    </div>
  )
}
