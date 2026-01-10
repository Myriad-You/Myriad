/**
 * 权限设置组组件
 * 预设组合：用于批量渲染权限开关
 */

import type { PermissionGroupConfig, PermissionItem } from '../types'
import React, { useCallback } from 'react'
import { SwitchItem } from '../items/SwitchItem'
import '../SettingGroup.css'
import './presets.css'

export interface PermissionGroupProps extends PermissionGroupConfig {}

export const PermissionGroup: React.FC<PermissionGroupProps> = ({
  title,
  description,
  permissions,
  values,
  onChange,
  disabled = false,
  loading = false,
}) => {
  const handleChange = useCallback((key: string) => (value: boolean) => {
    onChange(key, value)
  }, [onChange])

  const renderPermissionLabel = (permission: PermissionItem) => {
    if (permission.code) {
      return (
        <>
          <code>{permission.code}</code>
          {' '}
          {permission.label}
        </>
      )
    }
    return permission.label
  }

  return (
    <div className="permission-group">
      {title && <h3 className="permission-group-title">{title}</h3>}
      {description && <p className="permission-group-desc">{description}</p>}

      <div className="permission-items">
        {permissions.map(permission => (
          <SwitchItem
            key={permission.key}
            itemKey={permission.key}
            label={renderPermissionLabel(permission) as string}
            description={permission.hint}
            value={values[permission.key] ?? false}
            onChange={handleChange(permission.key)}
            disabled={disabled}
            loading={loading}
            layout="horizontal"
            className="setting-item-permission"
          />
        ))}
      </div>
    </div>
  )
}

PermissionGroup.displayName = 'PermissionGroup'
