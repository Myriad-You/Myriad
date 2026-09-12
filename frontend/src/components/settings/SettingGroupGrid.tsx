import type { ReactNode } from 'react'
import React, { createContext, useContext, useMemo } from 'react'
import './SettingGroupGrid.css'

export type SettingGroupGridColumns = 1 | 2 | 3
export type SettingGroupGridVariant = 'plain' | 'card'
export type SettingGroupGridAlign = 'stretch' | 'rows' | 'start'

export interface SettingGroupGridContextValue {
  inGrid: boolean
  alignRows: boolean
}

const SettingGroupGridContext =
  createContext<SettingGroupGridContextValue | null>(null)

export function useSettingGroupGrid(): SettingGroupGridContextValue | null {
  return useContext(SettingGroupGridContext)
}

export interface SettingGroupGridProps {
  children: ReactNode
  columns?: SettingGroupGridColumns
  minColumnWidth?: string
  variant?: SettingGroupGridVariant
  align?: SettingGroupGridAlign
  className?: string
  ariaLabel?: string
}

function chunkChildren(children: ReactNode, size: number): ReactNode[][] {
  const items = React.Children.toArray(children).filter(Boolean)
  if (size <= 1) return items.map((item) => [item])
  const rows: ReactNode[][] = []
  for (let i = 0; i < items.length; i += size) {
    rows.push(items.slice(i, i + size))
  }
  return rows
}

export const SettingGroupGrid: React.FC<SettingGroupGridProps> = ({
  children,
  columns = 2,
  minColumnWidth = '17.5rem',
  variant = 'card',
  align = 'stretch',
  className = '',
  ariaLabel,
}) => {
  const ctx = useMemo<SettingGroupGridContextValue>(
    () => ({
      inGrid: true,
      alignRows: align === 'rows',
    }),
    [align],
  )

  const rootClass = [
    'setting-group-grid',
    `setting-group-grid--cols-${columns}`,
    `setting-group-grid--align-${align}`,
    variant === 'card' ? 'is-card' : 'is-plain',
    className,
  ]
    .filter(Boolean)
    .join(' ')

  const rootStyle = {
    '--sg-grid-min': minColumnWidth,
    '--sg-grid-columns': String(columns),
  } as React.CSSProperties

  if (align === 'rows') {
    const rows = chunkChildren(children, columns)
    return (
      <SettingGroupGridContext.Provider value={ctx}>
        <div
          className={rootClass}
          style={rootStyle}
          role="group"
          aria-label={ariaLabel}
        >
          {rows.map((row, index) => (
            <div
              key={index}
              className="setting-group-grid-row"
              style={
                {
                  '--sg-row-cols': String(row.length),
                } as React.CSSProperties
              }
            >
              {row}
            </div>
          ))}
        </div>
      </SettingGroupGridContext.Provider>
    )
  }

  return (
    <SettingGroupGridContext.Provider value={ctx}>
      <div
        className={rootClass}
        style={rootStyle}
        role="group"
        aria-label={ariaLabel}
      >
        {children}
      </div>
    </SettingGroupGridContext.Provider>
  )
}

SettingGroupGrid.displayName = 'SettingGroupGrid'

export default SettingGroupGrid
