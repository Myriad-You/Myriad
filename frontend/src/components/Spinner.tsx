/**
 * 统一的加载动画组件 - 极简设计
 * 只有两种尺寸：sm(8px) 和 md(12px)
 * 只在真正需要的地方使用
 */

import './Spinner.css'

export type SpinnerSize = 'sm' | 'md'
export type SpinnerVariant = 'primary' | 'white'

interface SpinnerProps {
  size?: SpinnerSize
  variant?: SpinnerVariant
  className?: string
  text?: string
}

/**
 * 基础 Spinner - 内联使用
 */
export function Spinner({
  size = 'sm',
  variant = 'primary',
  className = '',
}: SpinnerProps) {
  return (
    <div className={`spinner spinner-${size} spinner-${variant} ${className}`} />
  )
}

/**
 * 按钮内 Spinner - 白色，超小
 */
export function ButtonSpinner({
  size = 'sm',
  className = '',
}: Pick<SpinnerProps, 'size' | 'className'>) {
  return (
    <div className={`spinner spinner-${size} spinner-white ${className}`} />
  )
}

export default Spinner
