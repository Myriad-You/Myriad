/**
 * Tapp 运行视图包装器
 * 解析 URL 参数并传递给 TappRunPage
 * 支持多任务模式（无需指定 tappId）
 */

import React from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { useBreakpoints } from '../hooks/useSharedEventListener'
import { TappWindowManager } from '../tapp/components/TappWindowManager'
import { TappRunPage } from '../tapp/pages'

const TappRunView: React.FC = () => {
  // 从路由参数中获取 tappId
  const { id } = useParams<{ id: string }>()
  const [searchParams] = useSearchParams()
  const navigate = useNavigate()
  const { isMobile } = useBreakpoints()
  const tappId = id ? decodeURIComponent(id) : ''

  // 检查是否是多任务模式（无 tappId 但有 multi=true 参数）
  const isMultiWindowMode = !tappId && searchParams.get('multi') === 'true' && !isMobile

  // 多任务模式 - 直接进入窗口管理器
  if (isMultiWindowMode) {
    return (
      <TappWindowManager
        onBack={() => navigate('/tapp')}
        onNotification={(options) => {
          console.log('[MultiWindow] Notification:', options)
        }}
      />
    )
  }

  if (!tappId) {
    return (
      <div className="min-h-screen bg-gray-50 dark:bg-neutral-900 flex items-center justify-center">
        <p className="text-gray-500 dark:text-gray-400">无效的 Tapp ID</p>
      </div>
    )
  }

  return <TappRunPage tappId={tappId} />
}

export default TappRunView
