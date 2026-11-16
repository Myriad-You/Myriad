import type { RegisteredWidget, WidgetRegistration } from '@myriad/tapp-runtime'
import { apiRequest } from './TappHttpClient'

export async function getAllWidgets(signal?: AbortSignal): Promise<RegisteredWidget[]> {
  return apiRequest<RegisteredWidget[]>('/api/tapps/widgets', { signal })
}

export async function registerTappWidget(
  tappId: string,
  config: WidgetRegistration,
  runtimeGrant: string,
  signal?: AbortSignal,
): Promise<RegisteredWidget> {
  const requestBody = {
    id: config.id,
    name: config.name,
    description: config.description,
    icon: config.icon,
    default_size: config.defaultSize,
    sizes: config.sizes,
    category: config.category,
    settings: config.settings || [],
    refresh_policy: config.refreshPolicy,
  }
  return apiRequest<RegisteredWidget>(
    `/api/tapps/${encodeURIComponent(tappId)}/widgets`,
    {
      method: 'POST',
      body: JSON.stringify(requestBody),
      runtimeGrant,
      signal,
    },
  )
}

export async function unregisterTappWidget(
  tappId: string,
  widgetId: string,
  runtimeGrant: string,
  signal?: AbortSignal,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/widgets/${encodeURIComponent(widgetId)}`,
    { method: 'DELETE', runtimeGrant, signal },
  )
}
