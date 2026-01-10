/**
 * 页面内容包装器
 *
 * 现在页面级进入/退出动画由 App.tsx 中的 PageWrapper 处理
 * AnimatedView 只负责：
 * 1. 通知协调器页面状态
 * 2. 提供容器样式
 *
 * 特性:
 * - 与 AnimationCoordinator 集成
 * - 页面就绪状态管理
 */

import { useEffect, useRef } from 'react'
import { useLocation } from 'react-router-dom'
import { usePageTransition } from '../hooks/animation'

interface AnimatedViewProps {
  children: React.ReactNode
  className?: string
}

export default function AnimatedView({ children, className = '' }: AnimatedViewProps) {
  const location = useLocation()
  const pageId = location.pathname.replace(/\//g, '-') || 'home'

  // 使用统一动画协调器（现在只用于状态管理，不做动画）
  const { onEnterComplete } = usePageTransition({ pageId })

  const hasNotified = useRef(false)

  // 组件挂载后通知协调器页面已就绪
  useEffect(() => {
    if (!hasNotified.current) {
      hasNotified.current = true
      // 延迟一帧通知完成，确保内容已渲染
      requestAnimationFrame(() => {
        onEnterComplete()
      })
    }

    return () => {
      hasNotified.current = false
    }
  }, [location.pathname, onEnterComplete])

  return (
    <div className={`animated-view-container ${className}`}>
      {children}
    </div>
  )
}
