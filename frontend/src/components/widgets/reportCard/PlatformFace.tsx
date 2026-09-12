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
  // 预览须关掉在线轮询：fixture 身份是假的，不 gate 会拿去打后端。
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

export {
  preloadPlatformFaces as preloadPlatformFaceBatch,
  preloadPlatformFacesForWidgetTypes,
} from './platformFaceLoaders'
