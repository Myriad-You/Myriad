/**
 * 设置页自定义下拉（替代原生 select option 列表）。
 * 原生 option 弹层由系统绘制，深色模式几乎不可样式化。
 */

import type { SettingOption } from '../types'
import { useEffect, useId, useRef, useState } from 'react'
import './FieldSelect.css'

export interface FieldSelectProps<T extends string = string> {
  id?: string
  value: T
  options: SettingOption<T>[]
  onChange: (value: T) => void
  disabled?: boolean
  className?: string
  /** 触发按钮 aria-label；缺省用当前选中项文案 */
  'aria-label'?: string
  /** 紧凑模式（列表行内使用） */
  size?: 'md' | 'sm'
}

export function FieldSelect<T extends string = string>({
  id,
  value,
  options: optionsProp,
  onChange,
  disabled = false,
  className = '',
  'aria-label': ariaLabel,
  size = 'md',
}: FieldSelectProps<T>) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLUListElement>(null)
  const autoId = useId()
  // useId() 可能含冒号，querySelector 需 CSS.escape；这里只用 id 绑定，不走 selector 拼接
  const listboxId = `${(id || autoId).replace(/:/g, '')}-listbox`
  const options = optionsProp ?? []

  const selected =
    options.find((o) => o.value === value) ?? options.find((o) => !o.disabled)

  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  useEffect(() => {
    if (!open) return
    const el = listRef.current?.querySelector(
      '[aria-selected="true"]',
    ) as HTMLElement | null
    el?.focus()
  }, [open])

  return (
    <div
      ref={rootRef}
      className={`field-select-wrap field-select-size-${size}${open ? ' is-open' : ''}${className ? ` ${className}` : ''}`}
    >
      <button
        type="button"
        id={id}
        className="field-select field-select-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listboxId}
        aria-label={ariaLabel || selected?.label || undefined}
        onClick={() => {
          if (!disabled) setOpen((v) => !v)
        }}
      >
        <span className="field-select-value">
          {selected?.label ?? String(value)}
        </span>
        <span className="field-select-chevron" aria-hidden="true" />
      </button>
      {open && (
        <ul
          ref={listRef}
          id={listboxId}
          className="field-select-menu"
          role="listbox"
          aria-label={ariaLabel || selected?.label}
        >
          {options.map((option) => {
            const isSelected = option.value === value
            return (
              <li key={String(option.value)} role="presentation">
                <button
                  type="button"
                  role="option"
                  aria-selected={isSelected}
                  disabled={option.disabled}
                  className={`field-select-option${isSelected ? ' is-selected' : ''}`}
                  onClick={() => {
                    setOpen(false)
                    if (!option.disabled && option.value !== value) {
                      onChange(option.value)
                    }
                  }}
                >
                  {option.label}
                </button>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
