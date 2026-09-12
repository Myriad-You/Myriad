import type { ReactNode } from 'react'
import React from 'react'
import { SettingTitleTag } from './SettingTitleTag'

export interface SettingFieldErrorTagProps {
  children: ReactNode
  title?: string
  className?: string
}

export function SettingFieldErrorTag({
  children,
  title,
  className = '',
}: SettingFieldErrorTagProps) {
  if (children == null || children === false || children === '') {
    return null
  }
  const tip =
    title ??
    (typeof children === 'string' || typeof children === 'number'
      ? String(children)
      : undefined)
  return (
    <SettingTitleTag
      variant="danger"
      title={tip}
      className={className}
    >
      {children}
    </SettingTitleTag>
  )
}

SettingFieldErrorTag.displayName = 'SettingFieldErrorTag'
