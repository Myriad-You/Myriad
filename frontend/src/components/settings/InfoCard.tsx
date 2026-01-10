/**
 * 信息卡片组件
 * 用于显示提示、警告等信息
 */

import type { InfoCardConfig } from './types'
import React from 'react'
import './InfoCard.css'

export interface InfoCardProps extends InfoCardConfig {}

export const InfoCard: React.FC<InfoCardProps> = ({
  title,
  content,
  variant = 'default',
  icon,
  className = '',
}) => {
  const renderIcon = () => {
    if (!icon)
      return null
    if (typeof icon === 'string') {
      return <span className="info-card-icon">{icon}</span>
    }
    return <span className="info-card-icon">{icon}</span>
  }

  return (
    <div className={`info-card info-card-${variant} ${className}`}>
      {(title || icon) && (
        <p className="info-title">
          {renderIcon()}
          {title}
        </p>
      )}
      <div className="info-text">{content}</div>
    </div>
  )
}

InfoCard.displayName = 'InfoCard'
