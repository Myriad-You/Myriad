/**
 * 配额设置组组件
 * 预设组合：用于批量渲染配额数字输入
 */

import type { QuotaGroupConfig } from '../types'
import React, { useCallback } from 'react'
import { NumberItem } from '../items/NumberItem'
import '../SettingGroup.css'
import './presets.css'

export interface QuotaGroupProps extends QuotaGroupConfig {}

export const QuotaGroup: React.FC<QuotaGroupProps> = ({
  title,
  description,
  quotas,
  values,
  onChange,
  disabled = false,
  loading = false,
}) => {
  const handleChange = useCallback((key: string) => (value: number) => {
    onChange(key, value)
  }, [onChange])

  return (
    <div className="quota-config-section">
      {title && <h4 className="quota-section-title">{title}</h4>}
      {description && <p className="quota-section-desc">{description}</p>}

      <div className="quota-config-grid">
        {quotas.map(quota => (
          <NumberItem
            key={quota.key}
            itemKey={quota.key}
            label={quota.label}
            hint={quota.hint}
            value={values[quota.key] ?? 0}
            onChange={handleChange(quota.key)}
            min={quota.min ?? 0}
            max={quota.max}
            step={quota.step ?? 1}
            unit={quota.unit}
            disabled={disabled}
            loading={loading}
            layout="horizontal"
            className="setting-item-quota"
          />
        ))}
      </div>
    </div>
  )
}

QuotaGroup.displayName = 'QuotaGroup'
