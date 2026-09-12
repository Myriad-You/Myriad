import type { ReactNode } from 'react'
import React, { useCallback } from 'react'
import './ChoiceControls.css'

export type ChoiceControlSize = 'sm' | 'md'

export interface ChoiceOption<T extends string = string> {
  value: T
  label: ReactNode
  icon?: ReactNode
  count?: number
  disabled?: boolean
}

interface SegmentedBaseProps<T extends string> {
  options: ChoiceOption<T>[]
  disabled?: boolean
  size?: ChoiceControlSize
  columns?: number | 'auto'
  ariaLabel?: string
  className?: string
}

export type SegmentedControlProps<T extends string = string> =
  | (SegmentedBaseProps<T> & {
      mode?: 'single'
      value: T | null
      onChange: (value: T) => void
    })
  | (SegmentedBaseProps<T> & {
      mode: 'multi'
      value: T[]
      onChange: (value: T[]) => void
    })

export function SegmentedControl<T extends string = string>(
  props: SegmentedControlProps<T>,
) {
  const {
    options,
    disabled = false,
    size = 'sm',
    columns = 'auto',
    ariaLabel,
    className = '',
  } = props
  const mode = props.mode ?? 'single'
  const value = props.value
  const onChange = props.onChange

  const isSelected = useCallback(
    (v: T): boolean => {
      if (mode === 'multi') {
        return (value as T[]).includes(v)
      }
      return (value as T | null) === v
    },
    [mode, value],
  )

  const handleSelect = useCallback(
    (next: T, optionDisabled?: boolean) => {
      if (disabled || optionDisabled) return
      if (mode === 'multi') {
        const current = value as T[]
        const multiOnChange = onChange as (value: T[]) => void
        const set = new Set(current)
        if (set.has(next)) set.delete(next)
        else set.add(next)
        multiOnChange(Iterator.from(set).toArray())
        return
      }
      const singleOnChange = onChange as (value: T) => void
      if (next !== (value as T | null)) singleOnChange(next)
    },
    [disabled, mode, value, onChange],
  )

  const isGrid = columns !== 'auto' && typeof columns === 'number'
  const trackStyle = isGrid
    ? ({
        ['--choice-cols' as string]: String(columns),
        ['--choice-cols-mobile' as string]: String(columns >= 3 ? 2 : columns),
      } as React.CSSProperties)
    : undefined

  return (
    <div
      className={[
        'choice-segmented',
        `choice-segmented--${size}`,
        isGrid ? 'is-grid' : 'is-flex',
        disabled ? 'is-disabled' : '',
        className,
      ]
        .filter(Boolean)
        .join(' ')}
      style={trackStyle}
      data-cols={isGrid ? columns : undefined}
      role={mode === 'multi' ? 'group' : 'radiogroup'}
      aria-label={ariaLabel}
      aria-disabled={disabled || undefined}
    >
      {options.map((opt) => {
        const selected = isSelected(opt.value)
        const itemDisabled = disabled || !!opt.disabled
        return (
          <button
            key={opt.value}
            type="button"
            role={mode === 'multi' ? 'checkbox' : 'radio'}
            aria-checked={selected}
            disabled={itemDisabled}
            className={`choice-segmented-item${selected ? ' is-selected' : ''}`}
            onClick={() => handleSelect(opt.value, opt.disabled)}
          >
            {opt.icon != null && (
              <span className="choice-option-icon">{opt.icon}</span>
            )}
            <span className="choice-option-label">{opt.label}</span>
            {opt.count != null && (
              <span className="choice-option-count">{opt.count}</span>
            )}
          </button>
        )
      })}
    </div>
  )
}

export default SegmentedControl
