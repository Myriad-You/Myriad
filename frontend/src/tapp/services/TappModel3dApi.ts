/** Runtime Grant 留在宿主传输层。 */

import { apiRequest } from './TappHttpClient'

export interface Model3dStatus {
  enabled: boolean
  configured: boolean
  capabilities: string[]
}

export interface Model3dTaskResponse {
  task: Record<string, unknown>
  asset?: {
    asset_id: string
    model_url: string
    metadata_url: string
    output_index: number
    metrics: Record<string, unknown>
  }
  assets: Array<{
    asset_id: string
    model_url: string
    metadata_url: string
    output_index: number
    metrics: Record<string, unknown>
  }>
}

export async function getModel3dStatus(
  runtimeGrant: string,
): Promise<Model3dStatus> {
  return apiRequest('/api/tapp/3d/status', { runtimeGrant })
}

export async function uploadModel3dFile(
  request: { fileName: string; contentType: string; base64: string },
  runtimeGrant: string,
): Promise<{ file_token: string }> {
  return apiRequest('/api/tapp/3d/files', {
    method: 'POST',
    body: JSON.stringify(request),
    runtimeGrant,
  })
}

export async function createModel3dTask(
  request: { operation: string; payload?: unknown },
  runtimeGrant: string,
): Promise<{ task_id: string }> {
  return apiRequest('/api/tapp/3d/tasks', {
    method: 'POST',
    body: JSON.stringify(request),
    runtimeGrant,
  })
}

export async function getModel3dTask(
  taskId: string,
  runtimeGrant: string,
): Promise<Model3dTaskResponse> {
  return apiRequest(`/api/tapp/3d/tasks/${encodeURIComponent(taskId)}`, {
    runtimeGrant,
  })
}

export async function awaitModel3dTask(
  taskId: string,
  runtimeGrant: string,
): Promise<Model3dTaskResponse> {
  return apiRequest(`/api/tapp/3d/tasks/${encodeURIComponent(taskId)}/await`, {
    method: 'POST',
    runtimeGrant,
  })
}
