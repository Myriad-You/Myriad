/**
 * SPA 路由加载指示器
 * 左上角光效提示 - 与导航岛对齐
 */

import { useEffect, useState } from 'react'
import { useLocation } from 'react-router-dom'
import './RouteLoader.css'

export default function RouteLoader() {
  const [loading, setLoading] = useState(false)
  const [visible, setVisible] = useState(false)
  const location = useLocation()

  useEffect(() => {
    setLoading(true)
    setVisible(true)

    const timer = setTimeout(() => {
      setLoading(false)
      // 等待退出动画完成后再隐藏
      setTimeout(() => setVisible(false), 400)
    }, 600)

    return () => clearTimeout(timer)
  }, [location.pathname])

  if (!visible)
    return null

  return (
    <div className="route-loader-indicator">
      <div className={`route-loader-light ${loading ? 'entering' : 'exiting'}`} />
    </div>
  )
}
