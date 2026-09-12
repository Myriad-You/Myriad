import { apiRequest } from './TappHttpClient'

export interface CreateReportRequest {
  tappId: string
  title: string
  reportType: 'platform' | 'custom'
  content: unknown
  metadata?: unknown
}

export interface TappReport {
  id: string
  title: string
  type: string
  content: unknown
  metadata?: unknown
  createdAt: string
  updatedAt: string
}

export async function createTappReport(
  request: CreateReportRequest,
  runtimeGrant?: string,
): Promise<{ success: boolean; report: TappReport }> {
  return apiRequest('/api/tapp/reports', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      title: request.title,
      report_type: request.reportType,
      content: request.content,
      metadata: request.metadata,
    }),
    runtimeGrant,
  })
}

export async function listTappReports(
  tappId: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; reports: TappReport[] }> {
  return apiRequest(`/api/tapp/reports/tapp/${encodeURIComponent(tappId)}`, {
    runtimeGrant,
  })
}

export async function getTappReport(
  tappId: string,
  reportId: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; report: TappReport }> {
  return apiRequest(
    `/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`,
    { runtimeGrant },
  )
}

export async function updateTappReport(
  tappId: string,
  reportId: string,
  updates: { title?: string; content?: unknown; metadata?: unknown },
  runtimeGrant?: string,
): Promise<{ success: boolean; report: TappReport }> {
  return apiRequest(
    `/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`,
    {
      method: 'PUT',
      body: JSON.stringify(updates),
      runtimeGrant,
    },
  )
}

export async function deleteTappReport(
  tappId: string,
  reportId: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; deleted: string }> {
  return apiRequest(
    `/api/tapp/reports/${encodeURIComponent(tappId)}/${encodeURIComponent(reportId)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

export interface MediaControlRequest {
  tappId: string
  action:
    | 'play'
    | 'pause'
    | 'next'
    | 'prev'
    | 'seek'
    | 'volume'
    | 'mode'
    | 'mute'
    | 'unmute'
  value?: unknown
}

export interface MediaStatus {
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

export async function mediaControl(
  request: MediaControlRequest,
  runtimeGrant?: string,
): Promise<{ success: boolean; action: string; value?: unknown }> {
  return apiRequest('/api/tapp/media/control', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      action: request.action,
      value: request.value,
    }),
    runtimeGrant,
  })
}

export async function mediaStatus(runtimeGrant?: string): Promise<{
  success: boolean
  status: MediaStatus
}> {
  return apiRequest('/api/tapp/media/status', { runtimeGrant })
}

export async function createTappNotification(
  request: {
    tappId: string
    title?: string
    message: string
    notificationType?: 'success' | 'info' | 'warning' | 'error'
  },
  runtimeGrant?: string,
): Promise<string> {
  const response = await apiRequest<{
    success: boolean
    notification_id: string
  }>('/api/tapp/notifications', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: request.tappId,
      title: request.title,
      message: request.message,
      notification_type: request.notificationType,
    }),
    runtimeGrant,
  })
  return response.notification_id
}
