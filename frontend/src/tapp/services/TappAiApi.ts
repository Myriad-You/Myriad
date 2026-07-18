/** Governed AI task APIs. Runtime Grants stay in the host transport layer. */

import type {
  AITaskEvent,
  AITaskRequest,
  AITaskSnapshot,
  AIUsageSnapshot,
} from '../types'
import { apiRequest, streamRuntimeEvents } from './TappHttpClient'

export async function createAITask(
  request: AITaskRequest,
  runtimeGrant: string,
): Promise<AITaskSnapshot> {
  return apiRequest('/api/tapp/ai/v2/tasks', {
    method: 'POST',
    body: JSON.stringify(request),
    runtimeGrant,
  })
}

export async function getAITask(
  taskId: string,
  runtimeGrant: string,
): Promise<AITaskSnapshot> {
  return apiRequest(`/api/tapp/ai/v2/tasks/${encodeURIComponent(taskId)}`, {
    runtimeGrant,
  })
}

export async function cancelAITask(
  taskId: string,
  runtimeGrant: string,
): Promise<{ success: boolean; taskId: string }> {
  return apiRequest(`/api/tapp/ai/v2/tasks/${encodeURIComponent(taskId)}`, {
    method: 'DELETE',
    runtimeGrant,
  })
}

export async function getAIUsage(
  runtimeGrant: string,
): Promise<AIUsageSnapshot> {
  const response = await apiRequest<{ usage: AIUsageSnapshot }>(
    '/api/tapp/ai/v2/usage',
    { runtimeGrant },
  )
  return response.usage
}

export async function streamAITaskEvents(
  taskId: string,
  runtimeGrant: string,
  onEvent: (event: AITaskEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  return streamRuntimeEvents(
    `/api/tapp/ai/v2/tasks/${encodeURIComponent(taskId)}/events`,
    runtimeGrant,
    (event, data) => onEvent({ event: event as AITaskEvent['event'], data }),
    signal,
  )
}
