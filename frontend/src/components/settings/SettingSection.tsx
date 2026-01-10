/**
 * 设置区块组件
 * 带标题、图标和描述的设置容器
 */

import type { SettingSectionConfig } from './types'
import { motionShim as motion } from '@lib/motionShim'
import React from 'react'
import { SettingGroup } from './SettingGroup'
import './SettingSection.css'

export interface SettingSectionProps extends SettingSectionConfig {}

export const SettingSection: React.FC<SettingSectionProps> = ({
  title,
  icon,
  description,
  groups,
  children,
  className = '',
  animated = true,
}) => {
  const renderIcon = () => {
    if (!icon)
      return null
    if (typeof icon === 'string') {
      return <span className="section-icon">{icon}</span>
    }
    return <span className="section-icon">{icon}</span>
  }

  const content = (
    <div className={`config-section setting-section ${className}`}>
      <div className="section-header">
        <div className="section-header-left">
          {renderIcon()}
          <div>
            <h2 className="section-title">{title}</h2>
            {description && <p className="section-description">{description}</p>}
          </div>
        </div>
      </div>

      <div className="config-form">
        {groups?.map((group, index) => (
          <SettingGroup key={group.title || `group-${index}`} {...group} />
        ))}
        {children}
      </div>
    </div>
  )

  if (animated) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3 }}
      >
        {content}
      </motion.div>
    )
  }

  return content
}

SettingSection.displayName = 'SettingSection'
