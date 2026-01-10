/**
 * 空状态组件
 */

import type { EmptyStateProps } from '../types'
import React from 'react'

export const EmptyState: React.FC<EmptyStateProps> = ({
  icon,
  title,
  description,
  action,
  className = '',
}) => {
  return (
    <div className={`flex flex-col items-center justify-center py-16 text-gray-500 dark:text-gray-400 ${className}`}>
      {icon && (
        <div className="w-20 h-20 rounded-2xl bg-gray-500/10 flex items-center justify-center mb-4">
          {icon}
        </div>
      )}
      <p className="text-lg font-medium text-gray-700 dark:text-gray-300">
        {title}
      </p>
      {description && (
        <p className="text-sm mt-1 opacity-70">
          {description}
        </p>
      )}
      {action && (
        <button
          onClick={action.onClick}
          className="mt-4 px-4 py-2 bg-orange-500 hover:bg-orange-600 text-white rounded-lg text-sm font-medium transition-colors"
        >
          {action.label}
        </button>
      )}
    </div>
  )
}

EmptyState.displayName = 'EmptyState'
