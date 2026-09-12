import type { CheckboxGroupOption } from '../items/CheckboxGroupItem'
import type { PermissionGroupConfig } from '../types'
import React, { useCallback, useMemo } from 'react'
import { CheckboxGroupItem } from '../items/CheckboxGroupItem'
import './presets.css'

export interface PermissionGroupProps extends PermissionGroupConfig {}

export const PermissionGroup: React.FC<PermissionGroupProps> = ({
  title,
  description,
  guide,
  guidePath,
  permissions,
  values,
  onChange,
  disabled = false,
  loading = false,
}) => {
  const handleChange = useCallback(
    (key: string, value: boolean) => {
      onChange(key, value)
    },
    [onChange],
  )

  const options: CheckboxGroupOption[] = useMemo(
    () =>
      permissions.map((permission) => ({
        key: permission.key,
        label: permission.code
          ? `${permission.label} ${permission.code}`
          : permission.label,
        description: permission.hint,
        value: values[permission.key] ?? false,
      })),
    [permissions, values],
  )

  return (
    <CheckboxGroupItem
      label={title || ''}
      description={description}
      guide={guide}
      guidePath={guidePath}
      options={options}
      onChange={handleChange}
      disabled={disabled || loading}
      className="permission-checkbox-group"
    />
  )
}

PermissionGroup.displayName = 'PermissionGroup'
