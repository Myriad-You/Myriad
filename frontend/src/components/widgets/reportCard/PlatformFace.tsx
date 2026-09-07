/**
 * Mount the platform-specific report face inside the card shell.
 *
 * Face 按平台分包，但禁止 React.lazy + Suspense 渲染期挂起。
 * 首页在 setWidgets 前 preloadPlatformFaces；此处只做同步 registry 读取。
 * 未预热时（组件库漏预热等）补拉一次再同步画，避免错峰入场。
 */
import { createElement, useEffect, useReducer } from 'react'
import { isKnownReportPlatformId } from '../../../utils/reportCardVisuals'
import { Spinner } from '../../Spinner'
import { PLATFORM_CONFIG } from './platformConfig'
import {
  getPlatformFace,
  isPlatformFaceReady,
  preloadPlatformFaces,
} from './platformFaceLoaders'

interface PlatformFaceProps {
  platformId: string
  data: any
  showOverview: boolean
  onContentChange: (content: any) => void
  allowLoop: boolean
  /**
   * 小组件库预览。face 必须据此关掉在线状态轮询：预览数据里的
   * gamertag / online_id 是 'PreviewGamer' / 'PreviewPSN' 这类假身份，
   * 不 gate 的话一开编辑模式就会拿它们去打后端，并每 120s 重复一次。
   */
  isPreview?: boolean
}

export function PlatformFace({
  platformId,
  data,
  showOverview,
  onContentChange,
  allowLoop,
  isPreview,
}: PlatformFaceProps) {
  const platformConfig =
    PLATFORM_CONFIG[platformId] || PLATFORM_CONFIG.bilibili
  const [, rerender] = useReducer((n: number) => n + 1, 0)
  const ready = isPlatformFaceReady(platformId)
  const Face = getPlatformFace(platformId)

  useEffect(() => {
    if (ready || !isKnownReportPlatformId(platformId)) return
    let alive = true
    void preloadPlatformFaces([platformId])
      .then(() => {
        if (alive) rerender()
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [platformId, ready])

  if (!isKnownReportPlatformId(platformId)) {
    return (
      <div className="flex h-full flex-col justify-center gap-1 p-3 text-xs text-gray-600 dark:text-gray-300">
        <div className="font-bold text-gray-800 dark:text-gray-100">
          {platformConfig.label}
        </div>
        {typeof data?.hardcore_score === 'number' && (
          <div>Score {data.hardcore_score}</div>
        )}
        {typeof data?.games_count === 'number' && (
          <div>Games {data.games_count}</div>
        )}
        {typeof data?.player_type === 'string' && <div>{data.player_type}</div>}
        {typeof data?.vibe === 'string' && (
          <div className="line-clamp-2">{data.vibe}</div>
        )}
        {typeof data?.contribution_level === 'string' && (
          <div>{data.contribution_level}</div>
        )}
      </div>
    )
  }

  if (!Face) {
    return (
      <div className="flex h-full w-full items-center justify-center">
        <Spinner size="lg" color="primary" />
      </div>
    )
  }

  return createElement(Face, {
    data,
    showOverview,
    onContentChange,
    allowLoop,
    isPreview,
  })
}

// 兼容旧 import 点
export {
  preloadPlatformFaces as preloadPlatformFaceBatch,
  preloadPlatformFacesForWidgetTypes,
} from './platformFaceLoaders'
