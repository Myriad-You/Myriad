/**
 * 系统配置视图组件
 */

import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import AnimatedView from '../components/AnimatedView'

import ConfigForm from '../components/ConfigForm'
import { useAuth } from '../contexts/AuthContext'
import { useConfigScheduler } from '../hooks/animation/pages/simple'
import { hasSessionHint } from '../utils/sessionDetection'

export default function Config() {
  // 🆕 初始化页面级调度器
  useConfigScheduler()

  const navigate = useNavigate()
  const { isAdmin: authIsAdmin, isAuthenticated, checkAuth } = useAuth()
  const [loading, setLoading] = useState(true)
  const [isAdmin, setIsAdmin] = useState(false)

  // 使用 AuthContext 检查管理员权限
  useEffect(() => {
    if (!isAuthenticated) {
      // 智能检测：检查是否有登录迹象
      if (hasSessionHint()) {
        // 有登录迹象，触发认证检查
        checkAuth()
      }
      else {
        // 无登录迹象，直接重定向到登录页
        navigate('/login', { replace: true })
      }
    }
    else {
      if (!authIsAdmin) {
        navigate('/', { replace: true })
        return
      }
      setIsAdmin(true)
      setLoading(false)
    }
  }, [authIsAdmin, isAuthenticated, checkAuth, navigate])

  if (loading) {
    return null
  }

  if (!isAdmin) {
    return null
  }

  return (
    <AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
      <div className="max-w-6xl mx-auto">
        <ConfigForm />
      </div>
    </AnimatedView>
  )
}
