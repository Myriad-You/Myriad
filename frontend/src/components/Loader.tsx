/**
 * 统一的简约光效加载组件
 */

import './Loader.css'

interface LoaderProps {
  size?: 'small' | 'medium' | 'large'
  className?: string
}

export default function Loader({ size = 'medium', className = '' }: LoaderProps) {
  const sizeMap = {
    small: 100,
    medium: 200,
    large: 300,
  }

  const sizeValue = sizeMap[size]

  return (
    <div className={`loader-container ${className}`}>
      <div
        className="loader-light"
        style={{
          width: `${sizeValue}px`,
          height: `${sizeValue}px`,
        }}
      />
    </div>
  )
}
