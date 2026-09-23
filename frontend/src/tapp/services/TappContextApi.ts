import { apiRequest, TappHttpError, tappRequest } from './TappHttpClient'

export type DataInput =
  | { source: 'platform'; platform: string }
  | { source: 'storage'; key: string }
  | { source: 'inline'; data: unknown }

export type DataOutput =
  { target: 'platform'; platform: string } | { target: 'storage'; key: string }

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

export interface DataTransformRequest {
  tappId: string
  input: DataInput
  pipeline: ProcessStep[]
  output?: DataOutput
}

export interface DataTransformResponse {
  success: boolean
  count: number
  data: unknown[]
}

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

export interface AppContext {
  version: string
  locale: string
  theme: string
  features: {
    aiEnabled: boolean
    platforms: string[]
  }
}

export interface UserContext {
  id: string
  username: string
  display_name?: string | null
  avatar: string | null
  avatar_url?: string | null
  isAdmin: boolean
  role: 'guest' | 'user' | 'admin'
  connectedPlatforms: string[]
  preferences: {
    language: string
    timezone: string
  }
}

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

export async function getContextApp(
  runtimeGrant?: string,
): Promise<AppContext> {
  return apiRequest('/api/tapp/context/app', { runtimeGrant })
}

export async function getContextUser(
  runtimeGrant?: string,
): Promise<UserContext> {
  return apiRequest('/api/tapp/context/user', { runtimeGrant })
}

export async function getContextPlayer(
  runtimeGrant?: string,
): Promise<PlayerContext> {
  return apiRequest('/api/tapp/context/player', { runtimeGrant })
}

export async function getContextNavigation(
  runtimeGrant?: string,
): Promise<NavigationContext> {
  return apiRequest('/api/tapp/context/navigation', { runtimeGrant })
}

export async function getContextSystem(
  runtimeGrant?: string,
): Promise<SystemContext> {
  return apiRequest('/api/tapp/context/system', { runtimeGrant })
}

export interface GeoContext {
  lat: number
  lon: number
  city: string
  region: string
  country: string
  countryCode?: string
}

export async function getContextGeo(
  runtimeGrant?: string,
): Promise<GeoContext> {
  return apiRequest('/api/tapp/context/geo', { runtimeGrant })
}

export interface TappApiExecuteResponse {
  success: boolean
  data?: unknown
  error?: string
  cached?: boolean
}

export interface TappApiInfo {
  name: string
  access: 'public' | 'protected'
  type: 'http' | 'builtin'
  description?: string
  cacheTtl?: number
}

export async function executeTappApi(
  tappId: string,
  apiName: string,
  params?: Record<string, unknown>,
  runtimeGrant?: string,
): Promise<TappApiExecuteResponse> {
  try {
    const result = await tappRequest<{
      success?: boolean
      data?: unknown
      error?: string
      cached?: boolean
    }>(
      `/api/tapp/${encodeURIComponent(tappId)}/api/${encodeURIComponent(apiName)}`,
      { method: 'POST', body: JSON.stringify({ params }), runtimeGrant },
    )
    return {
      success: result.success ?? false,
      data: result.data,
      error: result.error,
      cached: result.cached,
    }
  } catch (error) {
    // A declared API's refusal is data for the TAPP, not an exception.
    if (!(error instanceof TappHttpError) || error.status === 0) throw error
    const body = error.body as { message?: string, error?: string } | undefined
    return {
      success: false,
      error: body?.message || body?.error || `Declared API request failed (${error.status})`,
    }
  }
}

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
