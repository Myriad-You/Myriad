import type { NumberGroupOption } from '../items/NumberGroupItem'
import type { QuotaGroupConfig } from '../types'
import React, { useCallback, useMemo } from 'react'
import { NumberGroupItem } from '../items/NumberGroupItem'
import './presets.css'

export interface QuotaGroupProps extends QuotaGroupConfig {}

export const QuotaGroup: React.FC<QuotaGroupProps> = ({
  title,
  description,
  guide,
  guidePath,
  quotas,
  values,
  onChange,
  disabled = false,
  loading = false,
}) => {
  const handleChange = useCallback(
    (key: string, value: number) => {
      onChange(key, value)
    },
    [onChange],
  )

  const options: NumberGroupOption[] = useMemo(
    () =>
      quotas.map((quota) => ({
        key: quota.key,
        label: quota.label,
        description: quota.hint,
        value: values[quota.key] ?? 0,
        min: quota.min,
        max: quota.max,
        step: quota.step,
        unit: quota.unit,
      })),
    [quotas, values],
  )

  return (
    <NumberGroupItem
      label={title || ''}
      description={description}
      guide={guide}
      guidePath={guidePath}
      options={options}
      onChange={handleChange}
      disabled={disabled || loading}
      className="quota-number-group"
    />
  )
}

QuotaGroup.displayName = 'QuotaGroup'
