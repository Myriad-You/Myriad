/**
 * 按钮设置项组件
 */

import type { ReactNode } from 'react'
import type { ButtonSettingConfig } from '../types'
import React, { useCallback, useState } from 'react'
import { ButtonSpinner } from '../../Spinner'
import { SettingItemWrapper } from './SettingItemWrapper'
import './SettingItem.css'

export interface ButtonItemProps extends Omit<ButtonSettingConfig, 'type'> {
  /** 异步操作 */
  asyncAction?: boolean
  /** 操作结果 */
  result?: {
    success: boolean
    message: string
  } | null
  /** 自定义结果渲染 */
  renderResult?: (result: { success: boolean; message: string }) => ReactNode
}

export const ButtonItem = React.memo<ButtonItemProps>(
  ({
    label,
    description,
    hint,
    onClick,
    buttonText,
    buttonIcon,
    variant = 'secondary',
    disabled = false,
    loading: externalLoading = false,
    size = 'md',
    layout = 'vertical',
    asyncAction = false,
    result,
    renderResult,
    className = '',
  }) => {
    const [internalLoading, setInternalLoading] = useState(false)
    const loading = externalLoading || internalLoading

    const handleClick = useCallback(() => {
      if (disabled || loading) return

      if (asyncAction) {
        setInternalLoading(true)
        try {
          onClick()
        } finally {
          setInternalLoading(false)
        }
      } else {
        onClick()
      }
    }, [onClick, disabled, loading, asyncAction])

    const renderIcon = () => {
      if (!buttonIcon) return null
      if (typeof buttonIcon === 'string') {
        return <span>{buttonIcon}</span>
      }
      return buttonIcon
    }

    const variantClass = `btn-${variant}`

    return (
      <SettingItemWrapper
        label={label}
        description={description}
        hint={hint}
        layout={layout}
        size={size}
        className={`setting-item-button ${className}`}
        disabled={disabled}
        contentRight={true}
      >
        <div className="setting-button-row">
          <button
            type="button"
            onClick={handleClick}
            disabled={disabled || loading}
            className={`btn-base ${variantClass}`}
            aria-busy={loading || undefined}
          >
            {loading ? (
              <ButtonSpinner />
            ) : (
              <>
                {renderIcon()}
                <span>{buttonText}</span>
              </>
            )}
          </button>
          {result &&
            (renderResult ? (
              renderResult(result)
            ) : (
              <span
                className={`test-result ${result.success ? 'success' : 'error'}`}
              >
                {result.message}
              </span>
            ))}
        </div>
      </SettingItemWrapper>
    )
  },
)

ButtonItem.displayName = 'ButtonItem'
