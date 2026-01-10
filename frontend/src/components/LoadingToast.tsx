import { useEffect, useState } from 'react'
import { useI18n } from '../contexts/I18nContext'
import './LoadingToast.css'

interface LoadingToastProps {
  message?: string
  show: boolean
}

export default function LoadingToast({ message, show }: LoadingToastProps) {
  const { t } = useI18n()
  const displayMessage = message || t.common.loading
  const [visible, setVisible] = useState(false)

  useEffect(() => {
    if (show) {
      setVisible(true)
    }
    else {
      const timer = setTimeout(() => setVisible(false), 300)
      return () => clearTimeout(timer)
    }
  }, [show])

  if (!visible)
    return null

  return (
    <div className={`pointer-events-auto transition-all duration-300 ${
      show ? 'opacity-100 translate-y-0 animate-fade-in' : 'opacity-0 translate-y-2'
    }`}
    >
      <div className="loading-toast-container glass rounded-xl px-4 py-3 shadow-lg border flex items-center gap-3 backdrop-blur-md">
        {/* 简约圆圈加载环 */}
        <div className="loading-toast-spinner"></div>

        {/* 加载文字 */}
        <span className="text-sm font-medium text-gray-700 dark:text-gray-200">
          {displayMessage}
        </span>
      </div>
    </div>
  )
}
