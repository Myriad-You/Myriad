import React from 'react'
import './items/SettingItem.css'

export interface CompactSettingGroupProps {
  children: React.ReactNode
  className?: string
}

export const CompactSettingGroup: React.FC<CompactSettingGroupProps> = ({
  children,
  className = '',
}) => {
  return (
    <div className={`config-compact-group ${className}`}>
      {children}
    </div>
  )
}
