import { API_URL } from '../../config'
import { getCSRFToken } from '../../utils/csrf'
import { apiRequest } from './TappHttpClient'

// ============ P0: Data Transform API ============

/** 数据输入源 */
export type DataInput =
  | { source: 'platform'; platform: string }
  | { source: 'storage'; key: string }
  | { source: 'inline'; data: unknown }

/** 数据输出目标 */
export type DataOutput =
  { target: 'platform'; platform: string } | { target: 'storage'; key: string }

/** 处理步骤 */
export type ProcessStep =
  | { type: 'filter'; field: string; operator: string; value: unknown }
  | { type: 'sort'; field: string; order?: 'asc' | 'desc' }
  | { type: 'limit'; count: number }
  | { type: 'offset'; count: number }
  | { type: 'select'; fields: string[] }
  | { type: 'group'; by: string }
  | {
      type: 'aggregate'
      operation: 'count' | 'sum' | 'avg' | 'min' | 'max'
      field?: string
    }
  | { type: 'dedupe'; key: string }
  | { type: 'map'; operations: MapOperation[] }

export type MapOperation =
  | { op: 'rename'; from: string; to: string }
  | { op: 'remove'; field: string }
  | { op: 'set'; field: string; value: unknown }
  | { op: 'copy'; from: string; to: string }
  | { op: 'template'; field: string; template: string }
  | { op: 'lower' | 'upper' | 'to_string' | 'to_number'; field: string }
  | { op: 'default'; field: string; value: unknown }
  | { op: 'concat'; fields: string[]; separator?: string; to: string }
  | { op: 'coalesce'; fields: string[]; to: string }

/** 数据转换请求 */
export interface DataTransformRequest {
  tappId: string
  input: DataInput
  pipeline: ProcessStep[]
  output?: DataOutput
}

/** 数据转换响应 */
export interface DataTransformResponse {
  success: boolean
  count: number
  data: unknown[]
}

/**
 * 执行数据转换管道
 */
export async function dataTransform(
  request: DataTransformRequest,
  runtimeGrant?: string,
): Promise<DataTransformResponse> {
  return apiRequest('/api/tapp/data/transform', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      input: request.input,
      pipeline: request.pipeline,
      output: request.output,
    }),
    runtimeGrant,
  })
}

// ============ P0: Context API ============

/** 应用上下文 */
export interface AppContext {
  version: string
  locale: string
  theme: string
  features: {
    aiEnabled: boolean
    platforms: string[]
  }
}

/** 用户上下文 */
export interface UserContext {
  id: string
  username: string
  display_name?: string | null
  avatar: string | null
  avatar_url?: string | null
  /** 是否为管理员 */
  isAdmin: boolean
  /** 用户角色: "guest" | "user" | "admin" */
  role: 'guest' | 'user' | 'admin'
  connectedPlatforms: string[]
  preferences: {
    language: string
    timezone: string
  }
}

/** 播放器上下文 */
export interface PlayerContext {
  isPlaying: boolean
  isPaused: boolean
  currentTrack: {
    id: string
    title: string
    artist: string
    album?: string
    cover?: string
    duration: number
    source: string
  } | null
  progress: {
    current: number
    duration: number
    percentage: number
  }
  playlist: {
    id: string
    name: string
    tracks: number
  } | null
  mode: 'sequence' | 'loop' | 'shuffle' | 'single'
  volume: number
  muted: boolean
}

/** 导航上下文 */
export interface NavigationContext {
  currentPath: string
  previousPath: string | null
  history: string[]
  availableRoutes: {
    path: string
    name: string
    icon: string
  }[]
  tappPages: {
    id: string
    path: string
    name: string
    tappId: string
  }[]
}

/** 系统上下文 */
export interface SystemContext {
  online: boolean
  serverConnected: boolean
  version: string
  backgroundTasks: {
    id: string
    type: string
    status: 'running' | 'completed' | 'failed'
    progress?: number
  }[]
  lastFetch: Record<string, string | null>
}

/**
 * 获取应用上下文
 */
export async function getContextApp(
  runtimeGrant?: string,
): Promise<AppContext> {
  return apiRequest('/api/tapp/context/app', { runtimeGrant })
}

/**
 * 获取用户上下文
 */
export async function getContextUser(
  runtimeGrant?: string,
): Promise<UserContext> {
  return apiRequest('/api/tapp/context/user', { runtimeGrant })
}

/**
 * 获取播放器上下文
 */
export async function getContextPlayer(
  runtimeGrant?: string,
): Promise<PlayerContext> {
  return apiRequest('/api/tapp/context/player', { runtimeGrant })
}

/**
 * 获取导航上下文
 */
export async function getContextNavigation(
  runtimeGrant?: string,
): Promise<NavigationContext> {
  return apiRequest('/api/tapp/context/navigation', { runtimeGrant })
}

/**
 * 获取系统上下文
 */
export async function getContextSystem(
  runtimeGrant?: string,
): Promise<SystemContext> {
  return apiRequest('/api/tapp/context/system', { runtimeGrant })
}

// ============ 地理位置 API ============

/** 地理位置信息 */
export interface GeoContext {
  lat: number
  lon: number
  city: string
  region: string
  country: string
  /** 国家代码（如 CN, US） */
  countryCode?: string
}

/**
 * 获取客户端地理位置信息
 * 这是一个公开 API，所有用户（包括游客）都可以调用
 */
export async function getContextGeo(
  runtimeGrant?: string,
): Promise<GeoContext> {
  const result = await apiRequest<{ success: boolean; data: GeoContext }>(
    '/api/tapp/context/geo',
    { runtimeGrant },
  )
  if (result.success && result.data) {
    return result.data
  }
  throw new Error('Failed to get geo info')
}

// ============ Tapp API 声明系统 ============

/** Tapp API 执行请求 */
export interface TappApiExecuteRequest {
  tappId: string
  apiName: string
  params?: Record<string, unknown>
}

/** Tapp API 执行响应 */
export interface TappApiExecuteResponse {
  success: boolean
  data?: unknown
  error?: string
  cached?: boolean
}

/** Tapp API 定义 */
export interface TappApiInfo {
  name: string
  access: 'public' | 'protected'
  type: 'http' | 'builtin'
  description?: string
  cacheTtl?: number
}

/**
 * 执行 Tapp 声明的 API
 *
 * @param tappId - Tapp ID
 * @param apiName - API 名称（在 manifest.apis 中定义的 key）
 * @param params - 可选参数
 * @returns API 执行结果
 */
export async function executeTappApi(
  tappId: string,
  apiName: string,
  params?: Record<string, unknown>,
  runtimeGrant?: string,
): Promise<TappApiExecuteResponse> {
  const execute = async (
    grant: string | undefined,
    retryOnRuntimeGrant: boolean,
  ): Promise<TappApiExecuteResponse> => {
    const csrfToken = (await getCSRFToken()) || ''
    const response = await fetch(
      `${API_URL}/api/tapp/${encodeURIComponent(tappId)}/api/${encodeURIComponent(apiName)}`,
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
          ...(grant ? { 'X-Tapp-Runtime-Grant': grant } : {}),
        },
        body: JSON.stringify({ params }),
        credentials: 'include',
      },
    )

    const result = await response.json().catch(() => ({}))
    if (
      response.status === 401 &&
      retryOnRuntimeGrant &&
      grant &&
      result.code === 'INVALID_RUNTIME_GRANT'
    ) {
      const { TappRuntimeGrant } = await import('../runtime/TappRuntimeGrant')
      const replacement = await TappRuntimeGrant.recoverRejectedToken(grant)
      if (replacement) return execute(replacement, false)
    }

    if (!response.ok) {
      return {
        success: false,
        error:
          result.message ||
          result.error ||
          `Declared API request failed (${response.status})`,
      }
    }

    return {
      success: result.success ?? false,
      data: result.data,
      error: result.error,
      cached: result.cached,
    }
  }

  return execute(runtimeGrant, true)
}

/**
 * 列出 Tapp 可用的 API
 *
 * @param tappId - Tapp ID
 * @returns API 列表
 */
export async function listTappApis(
  tappId: string,
  runtimeGrant?: string,
): Promise<TappApiInfo[]> {
  const result = await apiRequest<{ success: boolean; apis: TappApiInfo[] }>(
    `/api/tapp/${encodeURIComponent(tappId)}/apis`,
    { runtimeGrant },
  )
  return result.apis || []
}
